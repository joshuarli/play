use crate::sched::{SeekPrecision, 
    AudioChunk, AudioDecoder, AudioOutput, Command, DemuxEvent, Demuxer, Packet, Scheduler,
    StreamKind,
};
use crate::state::State;
use crate::sub::{EngineSub, TrackInfo, TrackKind};
use crate::video::{EngineVideo, EngineVideoOutput, PresentedFrame};
use player_audio::decode::PacketDecoder;
use player_audio::sink::{AudioSink, NullSink, SinkKind};
use player_output::GpuApi;
use player_media::{MediaError, MediaSession, Packet as MediaPacket, StreamInfo, StreamKind as MediaKind};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PRESENTED_CHANNEL_DEPTH: usize = 4;

pub struct EngineDemux {
    session: MediaSession,
    audio_streams: Vec<usize>,
    selected: Option<usize>,
    selected_video: Option<usize>,
    sub_streams: Vec<usize>,
    duration: f64,
}

impl EngineDemux {
    pub fn open(path: &str) -> Result<Self, String> {
        Self::open_with_video(path, true)
    }

    pub fn open_audio_only(path: &str) -> Result<Self, String> {
        Self::open_with_video(path, false)
    }

    fn open_with_video(path: &str, include_video: bool) -> Result<Self, String> {
        let session = MediaSession::open(path).map_err(|err| err.to_string())?;
        let mut audio = Vec::new();
        let mut selected_video = None;
        let mut subs = Vec::new();
        for stream in session.streams() {
            match stream.kind {
                MediaKind::Video
                    if include_video && !stream.attached && selected_video.is_none() =>
                {
                    selected_video = Some(stream.index)
                }
                MediaKind::Audio => audio.push(stream.index),
                MediaKind::Subtitle => subs.push(stream.index),
                _ => {}
            }
        }
        let selected = audio.first().copied();
        if selected.is_none() && !include_video {
            return Err(format!("{path} has no audio stream"));
        }
        if let Some(selected) = selected {
            let info = stream_info(&session, selected)
                .ok_or_else(|| format!("{path} has no usable audio format"))?;
            if info.sample_rate <= 0 || info.channels <= 0 {
                return Err(format!("{path} has no usable audio format"));
            }
        }
        if selected.is_none() && selected_video.is_none() {
            return Err(format!("{path} has no playable streams"));
        }
        let duration = session.duration_seconds().unwrap_or(0.0).max(0.0);
        Ok(EngineDemux {
            session,
            audio_streams: audio,
            selected,
            selected_video,
            sub_streams: subs,
            duration,
        })
    }

    pub fn audio_streams(&self) -> &[usize] {
        &self.audio_streams
    }

    pub fn selected_audio_stream(&self) -> Option<usize> {
        self.selected
    }

    pub fn sub_streams(&self) -> &[usize] {
        &self.sub_streams
    }

    pub fn sub_info(&self, index: usize) -> Option<StreamInfo> {
        self.session
            .streams()
            .into_iter()
            .find(|stream| stream.index == index && stream.kind == MediaKind::Subtitle)
    }

    pub fn stream_info(&self, index: usize) -> Option<StreamInfo> {
        stream_info(&self.session, index)
    }

    pub fn session(&self) -> &MediaSession {
        &self.session
    }

    pub fn selected_video(&self) -> Option<usize> {
        self.selected_video
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }
}

fn stream_info(session: &MediaSession, index: usize) -> Option<StreamInfo> {
    session
        .streams()
        .into_iter()
        .find(|stream| stream.index == index && stream.kind == MediaKind::Audio)
}

impl Demuxer for EngineDemux {
    fn has_audio(&self) -> bool {
        self.selected.is_some()
    }

    fn has_video(&self) -> bool {
        self.selected_video.is_some()
    }

    fn read(&mut self) -> DemuxEvent {
        loop {
            match self.session.read_packet() {
                Err(MediaError::Eof) => return DemuxEvent::Eof,
                Err(other) => return DemuxEvent::Error(other.to_string()),
                Ok(packet) => {
                    let kind = if Some(packet.stream_index) == self.selected {
                        StreamKind::Audio
                    } else if Some(packet.stream_index) == self.selected_video {
                        StreamKind::Video
                    } else if self.sub_streams.contains(&packet.stream_index) {
                        StreamKind::Subtitle
                    } else {
                        continue;
                    };
                    return DemuxEvent::Packet(Packet {
                        stream: packet.stream_index,
                        kind,
                        pts: packet.pts_seconds,
                        duration: packet.duration_seconds,
                        payload: packet.data,
                    });
                }
            }
        }
    }

    fn seek(&mut self, seconds: f64) {
        let _ = self.session.seek_seconds(seconds.max(0.0));
    }

    fn select_audio(&mut self, stream: usize) {
        if self.audio_streams.contains(&stream) {
            self.selected = Some(stream);
        }
    }

    fn deselect_audio(&mut self) {
        self.selected = None;
    }

    fn selected_audio(&self) -> usize {
        self.selected.unwrap_or(usize::MAX)
    }

    fn audio_info(&self, stream: usize) -> Option<StreamInfo> {
        if self.audio_streams.contains(&stream) {
            stream_info(&self.session, stream)
        } else {
            None
        }
    }
}

pub struct EngineAudio {
    core: Option<PacketDecoder>,
    outbox: VecDeque<AudioChunk>,
    stream: usize,
    channels: u16,
    rate: u32,
    layout: player_audio::mix::Layout,
}

impl EngineAudio {
    pub fn open(info: &StreamInfo, target_rate: u32) -> Result<Self, String> {
        let core = PacketDecoder::open(info, target_rate)?;
        Ok(EngineAudio {
            channels: core.target_layout().channels() as u16,
            layout: core.target_layout(),
            rate: core.target_rate(),
            core: Some(core),
            outbox: VecDeque::new(),
            stream: info.index,
        })
    }

    pub fn silent(rate: u32) -> Self {
        EngineAudio {
            core: None,
            outbox: VecDeque::new(),
            stream: usize::MAX,
            channels: 2,
            rate,
            layout: player_audio::mix::Layout::Stereo,
        }
    }

    pub fn reconfigure(&mut self, info: &StreamInfo) -> Result<(), String> {
        let core = PacketDecoder::open_with_layout(info, self.rate, self.layout)?;
        self.channels = core.target_layout().channels() as u16;
        self.layout = core.target_layout();
        self.stream = info.index;
        self.outbox.clear();
        self.core = Some(core);
        Ok(())
    }
}

impl AudioDecoder for EngineAudio {
    fn send(&mut self, packet: Packet) -> Result<(), String> {
        let Some(core) = self.core.as_mut() else {
            return Ok(());
        };
        let media = MediaPacket {
            stream_index: packet.stream,
            pts_seconds: packet.pts,
            dts_seconds: None,
            duration_seconds: packet.duration,
            keyframe: false,
            data: packet.payload,
        };
        let pts = packet.pts.unwrap_or(0.0);
        if let Some(ready) = core.feed(&media, pts)? {
            self.outbox.push_back(AudioChunk {
                pts: ready.pts,
                stream: self.stream,
                channels: self.channels,
                rate: self.rate,
                samples: ready.samples,
            });
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<Vec<AudioChunk>, String> {
        Ok(self.outbox.drain(..).collect())
    }

    fn finish(&mut self) -> Result<Vec<AudioChunk>, String> {
        if let Some(core) = self.core.as_mut()
            && let Some(ready) = core.finish()?
        {
            self.outbox.push_back(AudioChunk {
                pts: ready.pts,
                stream: self.stream,
                channels: self.channels,
                rate: self.rate,
                samples: ready.samples,
            });
        }
        Ok(self.outbox.drain(..).collect())
    }

    fn flush(&mut self) {
        if let Some(core) = self.core.as_mut() {
            core.reset();
        }
        self.outbox.clear();
    }

    fn reconfigure(&mut self, info: &StreamInfo) -> Result<(), String> {
        EngineAudio::reconfigure(self, info)
    }
}

pub struct EngineOutput {
    sink: Box<dyn AudioSink>,
    written: u64,
    volume: f32,
    muted: bool,
}

impl EngineOutput {
    pub fn queued_seconds(&self) -> f64 {
        seconds(self.sink.queued_frames(), self.sink.rate())
    }

    pub fn device_position(&self) -> f64 {
        (seconds(self.written, self.sink.rate()) - self.queued_seconds()).max(0.0)
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }
}

fn seconds(frames: u64, rate: u32) -> f64 {
    if rate == 0 {
        return 0.0;
    }
    frames as f64 / rate as f64
}

impl AudioOutput for EngineOutput {
    fn push(&mut self, mut chunk: AudioChunk) -> Result<(), String> {
        let gain = if self.muted { 0.0 } else { self.volume };
        if gain != 1.0 {
            for sample in chunk.samples.iter_mut() {
                *sample *= gain;
            }
        }
        self.sink.write(&chunk.samples)?;
        self.written += (chunk.samples.len() / self.sink.channels().max(1)) as u64;
        Ok(())
    }

    fn queued_seconds(&self) -> f64 {
        self.queued_seconds()
    }

    fn device_position(&self) -> f64 {
        self.device_position()
    }

    fn set_paused(&mut self, paused: bool) {
        if paused {
            let _ = self.sink.pause();
        } else {
            let _ = self.sink.resume();
        }
    }

    fn flush(&mut self) {
        self.written = self.written.saturating_sub(self.sink.queued_frames());
        let _ = self.sink.flush();
    }

    fn finish(&mut self) -> Result<(), String> {
        self.sink.finish()
    }

    fn drained(&self) -> bool {
        self.sink.queued_frames() == 0
    }
}

#[derive(Debug, Clone)]
pub struct EngineStatus {
    pub position: f64,
    pub duration: f64,
    pub speed: f64,
    pub volume: f32,
    pub muted: bool,
    pub paused: bool,
    /// Terminal: the scheduler reached `Ended` or `Error`.
    pub ended: bool,
    /// Terminal by failure: the scheduler reached `Error`.
    pub failed: bool,
    pub decoded: u64,
    pub delivered: u64,
    pub dropped_late: u64,
    pub evicted: u64,
    pub missing: u64,
    pub discarded: u64,
    pub flushed: u64,
    pub frames_pending: u64,
    pub drained: u64,
    pub video_width: u32,
    pub video_height: u32,
    pub error: Option<String>,
    pub subtitle_text: String,
    pub tracks: Vec<TrackInfo>,
}

impl Default for EngineStatus {
    fn default() -> Self {
        EngineStatus {
            position: 0.0,
            duration: 0.0,
            speed: 1.0,
            volume: 1.0,
            muted: false,
            paused: false,
            ended: false,
            failed: false,
            decoded: 0,
            delivered: 0,
            dropped_late: 0,
            evicted: 0,
            missing: 0,
            discarded: 0,
            flushed: 0,
            frames_pending: 0,
            drained: 0,
            video_width: 0,
            video_height: 0,
            error: None,
            subtitle_text: String::new(),
            tracks: Vec::new(),
        }
    }
}

#[derive(Debug)]
enum WorkerCommand {
    SetPaused(bool),
    Seek(f64, SeekPrecision),
    SetSpeed(f64),
    SetVolume(f32),
    SetMuted(bool),
    SelectAudio(usize),
    DisableAudio,
    Stop,
    SelectSub(Option<usize>),
    AddSub(String, mpsc::Sender<Result<(), String>>),
    SetSubVisible(bool),
}

pub struct SchedulerEngine {
    commands: mpsc::Sender<WorkerCommand>,
    state: Arc<Mutex<EngineStatus>>,
    frames: Option<mpsc::Receiver<PresentedFrame>>,
    drained: AtomicU64,
    worker: Option<thread::JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartState {
    pub volume: f32,
    pub muted: bool,
    pub paused: bool,
    pub gpu_api: GpuApi,
}

impl Default for StartState {
    fn default() -> Self {
        StartState {
            volume: 1.0,
            muted: false,
            paused: false,
            gpu_api: GpuApi::default(),
        }
    }
}

impl SchedulerEngine {
    pub fn load(path: &str, kind: SinkKind, start: StartState) -> Result<Self, String> {
        Self::start(path, kind, start, true, true)
    }

    pub fn load_null_video(path: &str, kind: SinkKind, start: StartState) -> Result<Self, String> {
        Self::start(path, kind, start, true, false)
    }

    pub fn load_audio_only(path: &str, kind: SinkKind, start: StartState) -> Result<Self, String> {
        Self::start(path, kind, start, false, false)
    }

    fn start(
        path: &str,
        kind: SinkKind,
        start: StartState,
        include_video: bool,
        render_video: bool,
    ) -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel::<WorkerCommand>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        // Bounded: present blocks when the consumer falls behind instead of
        // buffering frames without limit.
        let (frames_tx, frames_rx) = mpsc::sync_channel(PRESENTED_CHANNEL_DEPTH);
        let start = StartState {
            volume: start.volume.clamp(0.0, 2.0),
            ..start
        };
        let state = Arc::new(Mutex::new(EngineStatus {
            volume: start.volume,
            muted: start.muted,
            paused: start.paused,
            ..EngineStatus::default()
        }));
        let worker_state = state.clone();
        let owned_path = path.to_owned();
        let worker = thread::Builder::new()
            .name("player-scheduler".to_string())
            .spawn(move || {
                run_worker(
                    owned_path,
                    kind,
                    start,
                    include_video,
                    render_video,
                    command_rx,
                    worker_state,
                    ready_tx,
                    frames_tx,
                );
            })
            .map_err(|err| format!("scheduler worker failed to start: {err}"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(SchedulerEngine {
                commands: command_tx,
                state,
                frames: Some(frames_rx),
                drained: AtomicU64::new(0),
                worker: Some(worker),
            }),
            Ok(Err(message)) => {
                let _ = worker.join();
                Err(message)
            }
            Err(_) => {
                let _ = worker.join();
                Err("scheduler worker died before reporting readiness".to_string())
            }
        }
    }

    fn send(&self, command: WorkerCommand) {
        let _ = self.commands.send(command);
    }

    pub fn set_paused(&self, paused: bool) {
        self.send(WorkerCommand::SetPaused(paused));
    }

    pub fn seek(&self, seconds: f64, precision: SeekPrecision) {
        self.send(WorkerCommand::Seek(seconds.max(0.0), precision));
    }

    pub fn set_speed(&self, speed: f64) {
        self.send(WorkerCommand::SetSpeed(speed.max(0.0)));
    }

    pub fn set_volume(&self, volume: f32) {
        self.send(WorkerCommand::SetVolume(volume.clamp(0.0, 2.0)));
    }

    pub fn set_muted(&self, muted: bool) {
        self.send(WorkerCommand::SetMuted(muted));
    }

    pub fn select_sub(&self, selected: Option<usize>) {
        self.send(WorkerCommand::SelectSub(selected));
    }

    pub fn select_sub_position(&self, position: usize) {
        self.select_sub(Some(position));
    }

    pub fn add_sub(&self, path: &str) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send(WorkerCommand::AddSub(path.to_owned(), reply_tx));
        reply_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| "subtitle load timed out".to_owned())?
    }

    pub fn set_sub_visible(&self, visible: bool) {
        self.send(WorkerCommand::SetSubVisible(visible));
    }

    pub fn select_audio(&self, stream: usize) {
        self.send(WorkerCommand::SelectAudio(stream));
    }

    pub fn disable_audio(&self) {
        self.send(WorkerCommand::DisableAudio);
    }

    pub fn snapshot(&self) -> EngineStatus {
        let mut status = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        status.drained = self.drained.load(Ordering::Relaxed);
        status
    }

    pub fn drain_frames(&self) -> Vec<PresentedFrame> {
        let mut frames = Vec::new();
        if let Some(rx) = self.frames.as_ref() {
            while let Ok(frame) = rx.try_recv() {
                frames.push(frame);
            }
        }
        self.drained
            .fetch_add(frames.len() as u64, Ordering::Relaxed);
        frames
    }
}

impl Drop for SchedulerEngine {
    fn drop(&mut self) {
        // Drop the receiver first: a worker blocked in present unblocks with
        // a send error instead of hanging the join below.
        drop(self.frames.take());
        let _ = self.commands.send(WorkerCommand::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

type EngineScheduler = Scheduler<EngineDemux, EngineAudio, EngineVideo, EngineOutput, EngineVideoOutput>;

fn run_worker(
    path: String,
    kind: SinkKind,
    start: StartState,
    include_video: bool,
    render_video: bool,
    commands: mpsc::Receiver<WorkerCommand>,
    state: Arc<Mutex<EngineStatus>>,
    ready: mpsc::Sender<Result<(), String>>,
    frames_tx: mpsc::SyncSender<PresentedFrame>,
) {
    let mut guard = PanicGuard {
        state: state.clone(),
        clean: false,
    };
    let build = || -> Result<(EngineScheduler, f64, u32, u32), String> {
        let demux = if include_video {
            EngineDemux::open(&path)?
        } else {
            EngineDemux::open_audio_only(&path)?
        };
        let duration = demux.duration();
        let (sink, audio): (Box<dyn AudioSink>, EngineAudio) =
            if let Some(selected) = demux.selected_audio_stream() {
                let info = demux.stream_info(selected).ok_or_else(|| {
                    format!("{path} has no usable audio format")
                })?;
                let layout = player_audio::decode::target_layout(&info)?;
                let sink: Box<dyn AudioSink> = match &kind {
                    SinkKind::Null => Box::new(NullSink::new(
                        layout.channels(),
                        info.sample_rate.max(1) as u32,
                    )),
                    SinkKind::Default | SinkKind::Device(_) => player_audio::open_sink(
                        &kind,
                        layout.channels(),
                        info.sample_rate.max(1) as u32,
                        layout.channel_mask(),
                    )?,
                };
                let rate = sink.rate();
                let audio = EngineAudio::open(&info, rate)?;
                (sink, audio)
            } else {
                let sink: Box<dyn AudioSink> = Box::new(NullSink::new(2, 48_000));
                let rate = sink.rate();
                (sink, EngineAudio::silent(rate))
            };
        let output = EngineOutput {
            sink,
            written: 0,
            volume: start.volume,
            muted: start.muted,
        };
        let (video, video_out) = match demux.selected_video() {
            Some(stream) => {
                let decoder = EngineVideo::open(demux.session(), stream)?;
                let shared = decoder.shared();
                let output = if render_video {
                    EngineVideoOutput::open(shared, frames_tx, start.gpu_api)?
                } else {
                    EngineVideoOutput::null(shared, frames_tx)
                };
                (decoder, output)
            }
            None => {
                let decoder = EngineVideo::passive();
                let shared = decoder.shared();
                (decoder, EngineVideoOutput::passive(shared, frames_tx))
            }
        };
        let (video_width, video_height) = demux
            .selected_video()
            .and_then(|index| {
                demux
                    .session()
                    .streams()
                    .into_iter()
                    .find(|stream| stream.index == index)
            })
            .map(|info| (info.width.max(0) as u32, info.height.max(0) as u32))
            .unwrap_or((0, 0));
        let mut scheduler = EngineScheduler::new(demux, audio, video, output, video_out);
        scheduler.enable_video_preroll();
        let mut subs = Vec::new();
        for stream in scheduler.demux().sub_streams().to_vec() {
            let Some(info) = scheduler.demux().sub_info(stream) else {
                continue;
            };
            let Ok(sub) = EngineSub::open_embedded(&info) else {
                continue;
            };
            subs.push(sub);
        }
        scheduler.set_subs(subs);
        scheduler.open();
        if start.paused {
            scheduler.send(Command::Pause(true));
            scheduler.send(Command::Seek(0.0, SeekPrecision::Exact));
        }
        Ok((scheduler, duration, video_width, video_height))
    };
    let (mut worker, duration, video_width, video_height) = match build() {
        Ok((scheduler, duration, video_width, video_height)) => (
            Worker {
                scheduler,
                volume: start.volume,
                muted: start.muted,
            },
            duration,
            video_width,
            video_height,
        ),
        Err(message) => {
            update(&state, |status| status.error = Some(message.clone()));
            let _ = ready.send(Err(message));
            guard.clean = true;
            return;
        }
    };
    update(&state, |status| {
        status.duration = duration;
        status.video_width = video_width;
        status.video_height = video_height;
    });
    if ready.send(Ok(())).is_err() {
        guard.clean = true;
        return;
    }
    let start = Instant::now();
    let mut stopped = false;
    while !stopped {
        let now = start.elapsed().as_secs_f64();
        while let Ok(command) = commands.try_recv() {
            if matches!(command, WorkerCommand::Stop) {
                stopped = true;
                break;
            }
            worker.apply(&state, now, command);
        }
        if stopped {
            break;
        }
        worker.scheduler.step(now);
        worker.publish(&state, now);
        if parked(worker.scheduler.state()) {
            match commands.recv() {
                Ok(WorkerCommand::Stop) => stopped = true,
                Ok(command) => worker.apply(&state, start.elapsed().as_secs_f64(), command),
                Err(_) => stopped = true,
            }
            worker.publish(&state, start.elapsed().as_secs_f64());
        } else if worker.scheduler.audio_output().queued_seconds() > 0.005 {
            thread::sleep(Duration::from_millis(2));
        }
    }
    guard.clean = true;
}

fn parked(state: State) -> bool {
    matches!(state, State::Paused { .. } | State::Ended | State::Error)
}

fn terminal_flags(state: State) -> (bool, bool) {
    (
        matches!(state, State::Ended | State::Error),
        matches!(state, State::Error),
    )
}

struct Worker {
    scheduler: EngineScheduler,
    volume: f32,
    muted: bool,
}

impl Worker {
    fn apply(&mut self, state: &Arc<Mutex<EngineStatus>>, now: f64, command: WorkerCommand) {
        match command {
            WorkerCommand::SetPaused(paused) => self.scheduler.send(Command::Pause(paused)),
            WorkerCommand::Seek(seconds, precision) => {
                self.scheduler.send(Command::Seek(seconds, precision))
            }
            WorkerCommand::SetSpeed(speed) => self.scheduler.send(Command::SetSpeed(speed)),
            WorkerCommand::SetVolume(volume) => {
                self.volume = volume;
                self.scheduler.audio_output_mut().set_volume(volume);
            }
            WorkerCommand::SetMuted(muted) => {
                self.muted = muted;
                self.scheduler.audio_output_mut().set_muted(muted);
            }
            WorkerCommand::SelectAudio(stream) => self.scheduler.send(Command::SwitchAudio(stream)),
            WorkerCommand::DisableAudio => self.scheduler.send(Command::DisableAudio),
            WorkerCommand::Stop => {}
            WorkerCommand::SelectSub(selected) => self.scheduler.select_sub(selected),
            WorkerCommand::AddSub(path, reply) => {
                let result = EngineSub::open_external(&path)
                    .map(|sub| {
                        self.scheduler.add_sub(sub);
                    });
                self.publish(state, now);
                let _ = reply.send(result);
            }
            WorkerCommand::SetSubVisible(visible) => self.scheduler.set_sub_visible(visible),
        }
    }

    fn publish(&self, state: &Arc<Mutex<EngineStatus>>, now: f64) {
        let position = self.scheduler.clock_position(now);
        let paused = matches!(self.scheduler.state(), State::Paused { .. });
        let (ended, failed) = terminal_flags(self.scheduler.state());
        let error = self.scheduler.failure().map(str::to_string);
        let stats = self.scheduler.stats();
        let (volume, muted) = (self.volume, self.muted);
        let tracks = track_list(&self.scheduler);
        let subtitle_text = self.scheduler.subtitle_text(now);
        update(state, |status| {
            status.position = position;
            status.speed = self.scheduler.speed();
            status.paused = paused;
            status.ended = ended;
            status.failed = failed;
            status.error = error;
            status.decoded = stats.decoded;
            status.delivered = stats.delivered;
            status.dropped_late = stats.dropped_late;
            status.evicted = stats.evicted;
            status.missing = stats.missing;
            status.discarded = stats.discarded;
            status.flushed = stats.flushed;
            status.frames_pending = stats.frames_pending;
            status.volume = volume;
            status.muted = muted;
            status.subtitle_text = subtitle_text;
            status.tracks = tracks;
        });
    }
}

fn track_list(scheduler: &EngineScheduler) -> Vec<TrackInfo> {
    let demux = scheduler.demux();
    let mut tracks = Vec::new();
    for (position, stream) in demux.audio_streams().iter().enumerate() {
        let info = demux.stream_info(*stream);
        tracks.push(TrackInfo {
            id: position + 1,
            kind: TrackKind::Audio,
            src_id: stream + 1,
            codec: info
                .as_ref()
                .map(|info| info.codec_name.clone())
                .unwrap_or_else(|| "unknown".to_owned()),
            language: info.as_ref().and_then(|info| info.language.clone()),
            title: info.as_ref().and_then(|info| info.title.clone()),
            selected: *stream == demux.selected_audio(),
            external: false,
            sub_index: None,
        });
    }
    if let Some(stream) = demux.selected_video() {
        let codec = demux
            .session()
            .streams()
            .into_iter()
            .find(|candidate| candidate.index == stream)
            .map(|info| info.codec_name)
            .unwrap_or_else(|| "unknown".to_owned());
        tracks.push(TrackInfo {
            id: 1,
            kind: TrackKind::Video,
            src_id: stream + 1,
            codec,
            language: None,
            title: None,
            selected: true,
            external: false,
            sub_index: None,
        });
    }
    let mut sub_id = 0;
    let external_base = demux.session().streams().len() + 1;
    let mut external_order = 0;
    for stream in demux.sub_streams() {
        sub_id += 1;
        let info = demux.sub_info(*stream);
        let position = scheduler
            .subs()
            .iter()
            .position(|sub| sub.source().stream == Some(*stream));
        tracks.push(TrackInfo {
            id: sub_id,
            kind: TrackKind::Subtitle,
            src_id: stream + 1,
            codec: info
                .as_ref()
                .map(|info| info.codec_name.clone())
                .unwrap_or_else(|| "unknown".to_owned()),
            language: info.as_ref().and_then(|info| info.language.clone()),
            title: info.as_ref().and_then(|info| info.title.clone()),
            selected: position.is_some_and(|index| scheduler.selected_sub() == Some(index)),
            external: false,
            sub_index: position,
        });
    }
    for (offset, sub) in scheduler.subs().iter().enumerate() {
        if sub.source().stream.is_some() {
            continue;
        }
        sub_id += 1;
        external_order += 1;
        tracks.push(TrackInfo {
            id: sub_id,
            kind: TrackKind::Subtitle,
            src_id: external_base + external_order,
            codec: crate::sub::SUBRIP_CODEC.to_owned(),
            language: sub.source().language.clone(),
            title: sub.source().title.clone(),
            selected: scheduler.selected_sub() == Some(offset),
            external: true,
            sub_index: Some(offset),
        });
    }
    tracks
}

fn update(state: &Arc<Mutex<EngineStatus>>, apply: impl FnOnce(&mut EngineStatus)) {
    if let Ok(mut guard) = state.lock() {
        apply(&mut guard);
    }
}

struct PanicGuard {
    state: Arc<Mutex<EngineStatus>>,
    clean: bool,
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        if !self.clean {
            update(&self.state, |status| {
                status.error = Some("scheduler worker died".to_string());
                status.ended = true;
                status.failed = true;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration as StdDuration;

    struct QueueSink {
        queued: u64,
    }

    impl AudioSink for QueueSink {
        fn write(&mut self, frames: &[f32]) -> Result<(), String> {
            self.queued += (frames.len() / 2) as u64;
            Ok(())
        }
        fn channels(&self) -> usize {
            2
        }
        fn rate(&self) -> u32 {
            48_000
        }
        fn queued_frames(&self) -> u64 {
            self.queued
        }
        fn pause(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn resume(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn flush(&mut self) -> Result<(), String> {
            self.queued = 0;
            Ok(())
        }
    }

    #[test]
    fn flushing_queued_audio_keeps_the_device_position() {
        let mut output = EngineOutput {
            sink: Box::new(QueueSink { queued: 0 }),
            written: 0,
            volume: 1.0,
            muted: false,
        };
        output
            .push(AudioChunk {
                pts: 0.0,
                stream: 0,
                channels: 2,
                rate: 48_000,
                samples: vec![0.0; 24_000 * 2],
            })
            .expect("chunk queues");
        assert!(output.device_position().abs() < 1e-9);
        AudioOutput::flush(&mut output);
        assert!(
            output.device_position().abs() < 1e-9,
            "flushing unplayed audio moved the device clock to {}",
            output.device_position()
        );
    }

    fn fixtures() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");
        let dir = root.join(".work/fixtures");
        assert!(
            dir.join("audio-wav.wav").is_file(),
            "run tests/fixtures/gen.sh first"
        );
        dir
    }

    #[test]
    fn terminal_flags_keep_failure_apart_from_a_clean_end() {
        assert_eq!(terminal_flags(State::Ended), (true, false));
        assert_eq!(terminal_flags(State::Error), (true, true));
        assert_eq!(terminal_flags(State::Playing), (false, false));
        assert_eq!(terminal_flags(State::Paused { resume: crate::state::PlayResume::Playing }), (false, false));
    }

    fn fixture(name: &str) -> String {
        fixtures().join(name).to_string_lossy().into_owned()
    }

    fn poll_until(engine: &SchedulerEngine, timeout: StdDuration, ready: impl Fn(&EngineStatus) -> bool) -> EngineStatus {
        let deadline = Instant::now() + timeout;
        loop {
            let _ = engine.drain_frames();
            let status = engine.snapshot();
            if ready(&status) {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting: {status:?}"
            );
            thread::sleep(StdDuration::from_millis(5));
        }
    }

    #[test]
    fn plays_to_end_with_null_sink() {
        let engine = SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null, StartState::default()).expect("engine loads");
        let status = poll_until(&engine, StdDuration::from_secs(10), |status| status.ended);
        assert!(status.error.is_none(), "unexpected error: {:?}", status.error);
        assert!((2.5..3.6).contains(&status.position), "position {}", status.position);
        assert!(status.duration > 0.0);
    }

    #[test]
    fn start_state_applies_before_any_audio_plays() {
        let start = StartState {
            volume: 0.25,
            muted: true,
            paused: true,
            ..StartState::default()
        };
        let engine = SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null, start)
            .expect("engine loads");
        let first = engine.snapshot();
        assert!(first.paused && first.muted, "start state is visible at once: {first:?}");
        assert!((first.volume - 0.25).abs() < 1e-6);
        std::thread::sleep(StdDuration::from_millis(300));
        let later = engine.snapshot();
        assert!(later.paused, "engine left the paused start");
        assert!(later.position < 0.01, "paused start advanced to {}", later.position);

        let video = SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null, start)
            .expect("video engine loads");
        let shown = poll_until(&video, StdDuration::from_secs(5), |status| status.delivered > 0);
        assert!(shown.paused && shown.position < 0.05, "paused start shows the first frame: {shown:?}");
    }

    #[test]
    fn audio_only_load_plays_video_files_without_presenting() {
        let engine = SchedulerEngine::load_audio_only(&fixture("h264.mp4"), SinkKind::Null, StartState::default())
            .expect("engine loads video audio");
        let status = poll_until(&engine, StdDuration::from_secs(15), |status| status.ended);
        assert!(status.error.is_none(), "unexpected error: {:?}", status.error);
        assert!(status.duration > 0.0);
        assert_eq!(status.delivered, 0, "audio-only load delivered video");
        assert!(status.position > 2.0, "position {}", status.position);
    }

    #[test]
    fn pause_freezes_position() {
        let engine = SchedulerEngine::load(&fixture("audio-flac.flac"), SinkKind::Null, StartState::default()).expect("engine loads");
        engine.set_paused(true);
        thread::sleep(StdDuration::from_millis(100));
        let frozen = engine.snapshot();
        assert!(frozen.paused);
        let first = frozen.position;
        thread::sleep(StdDuration::from_millis(100));
        let second = engine.snapshot().position;
        assert_eq!(first, second);
        engine.set_paused(false);
        let status = poll_until(&engine, StdDuration::from_secs(10), |status| status.ended);
        assert!(status.position > first);
    }

    #[test]
    fn seek_moves_position() {
        let engine = SchedulerEngine::load(&fixture("audio-mp3.mp3"), SinkKind::Null, StartState::default()).expect("engine loads");
        engine.set_paused(true);
        engine.seek(2.0, SeekPrecision::Exact);
        let status = poll_until(&engine, StdDuration::from_secs(10), |status| {
            (status.position - 2.0).abs() < 0.05
        });
        assert!(status.paused);
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn seek_video_while_paused_lands_exactly() {
        let engine =
            SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null, StartState::default()).expect("engine loads video");
        engine.set_paused(true);
        let paused = poll_until(&engine, StdDuration::from_secs(10), |status| status.paused);
        assert!(paused.paused);
        engine.seek(1.0, SeekPrecision::Exact);
        let deadline = Instant::now() + StdDuration::from_secs(10);
        let mut frames = Vec::new();
        let status = loop {
            frames.extend(engine.drain_frames());
            let status = engine.snapshot();
            if (status.position - 1.0).abs() < 0.05 && !frames.is_empty() {
                break status;
            }
            assert!(Instant::now() < deadline, "timed out waiting: {status:?}");
            thread::sleep(StdDuration::from_millis(5));
        };
        assert!(status.error.is_none(), "unexpected error: {:?}", status.error);
        assert!(!status.ended, "engine ended early: {status:?}");
        assert!((status.position - 1.0).abs() < 0.05);
        let frame = frames.last().expect("paused seek presented a frame");
        assert!(
            (frame.pts - 1.0).abs() < 0.15,
            "paused seek presented frame {}",
            frame.pts
        );
    }

    fn pause_and_seek(engine: &SchedulerEngine, seconds: f64) {
        engine.set_paused(true);
        poll_until(engine, StdDuration::from_secs(10), |status| status.paused);
        engine.seek(seconds, SeekPrecision::Exact);
        poll_until(engine, StdDuration::from_secs(10), |status| {
            (status.position - seconds).abs() < 0.05
        });
    }

    fn subtitle_at(engine: &SchedulerEngine, seconds: f64) -> String {
        pause_and_seek(engine, seconds);
        engine.snapshot().subtitle_text
    }

    fn wait_for_subtitle(engine: &SchedulerEngine, expected: &str) {
        poll_until(engine, StdDuration::from_secs(10), |status| status.subtitle_text == expected);
    }

    #[test]
    fn external_srt_cues_follow_selection_visibility_and_seeks() {
        let engine =
            SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null, StartState::default()).expect("engine loads");
        pause_and_seek(&engine, 0.0);
        engine.add_sub(&fixture("external.srt")).expect("srt loads");
        let status = poll_until(&engine, StdDuration::from_secs(5), |status| {
            status.tracks.iter().any(|track| track.kind == TrackKind::Subtitle)
        });
        let sub = status
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle)
            .expect("sub track listed");
        assert_eq!(sub.codec, "subrip");
        assert!(sub.selected, "added subs auto-select");
        assert_eq!(sub.sub_index, Some(0));
        assert_eq!(subtitle_at(&engine, 0.6), "Hello from SubRip.");
        assert_eq!(subtitle_at(&engine, 2.9), "");
        assert_eq!(subtitle_at(&engine, 1.7), "Second cue.");
        engine.select_sub(None);
        wait_for_subtitle(&engine, "");
        engine.select_sub(Some(0));
        wait_for_subtitle(&engine, "Second cue.");
        engine.set_sub_visible(false);
        wait_for_subtitle(&engine, "");
        engine.set_sub_visible(true);
        assert_eq!(subtitle_at(&engine, 0.6), "Hello from SubRip.");
        assert!(!engine.snapshot().ended);
    }

    #[test]
    fn subtitle_files_other_than_srt_are_rejected() {
        let engine =
            SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null, StartState::default()).expect("engine loads");
        for file in ["external.ass", "external.vtt", "audio-wav.wav"] {
            let error = engine.add_sub(&fixture(file)).expect_err(file);
            assert!(error.contains("only SubRip"), "{file}: {error}");
        }
        assert!(engine.snapshot().tracks.iter().all(|track| track.kind != TrackKind::Subtitle));
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn embedded_subs_prime_on_a_paused_seek_before_playback() {
        let engine = SchedulerEngine::load(&fixture("h264-embedded-srt.mkv"), SinkKind::Null, StartState::default())
            .expect("engine loads video with subs");
        engine.set_paused(true);
        poll_until(&engine, StdDuration::from_secs(10), |status| status.paused);
        engine.select_sub(Some(0));
        poll_until(&engine, StdDuration::from_secs(10), |status| {
            status.tracks.iter().any(|track| track.kind == TrackKind::Subtitle && track.selected)
        });
        let text = subtitle_at(&engine, 1.0);
        assert!(engine.snapshot().error.is_none());
        assert!(!text.is_empty(), "no cue after a paused seek");
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn embedded_subs_show_at_cue_time() {
        let engine = SchedulerEngine::load(&fixture("h264-embedded-srt.mkv"), SinkKind::Null, StartState::default())
            .expect("engine loads video with subs");
        engine.select_sub(Some(0));
        poll_until(&engine, StdDuration::from_secs(30), |status| status.ended);
        let status = engine.snapshot();
        let sub = status
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle)
            .expect("embedded sub listed");
        assert_eq!(sub.codec, "subrip");
        assert!(sub.selected, "embedded subs auto-select");
        let first = subtitle_at(&engine, 0.6);
        assert!(!first.is_empty(), "no cue at 0.6s");
        assert_eq!(subtitle_at(&engine, 2.9), "");
        let second = subtitle_at(&engine, 1.8);
        assert!(!second.is_empty() && second != first, "second cue {second:?} after {first:?}");
    }

    #[test]
    fn embedded_subtitles_in_other_formats_are_listed_but_not_selectable() {
        let engine = SchedulerEngine::load(&fixture("h264-movtext.mp4"), SinkKind::Null, StartState::default())
            .expect("engine loads video with movtext");
        let status = poll_until(&engine, StdDuration::from_secs(10), |status| {
            status.tracks.iter().any(|track| track.kind == TrackKind::Subtitle)
        });
        let sub = status
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle)
            .expect("movtext listed");
        assert_eq!(sub.codec, "mov_text");
        assert_eq!(sub.sub_index, None);
        assert!(!sub.selected);
    }

    #[test]
    fn seek_after_end_restarts() {
        let engine = SchedulerEngine::load(&fixture("audio-mono.wav"), SinkKind::Null, StartState::default()).expect("engine loads");
        poll_until(&engine, StdDuration::from_secs(10), |status| status.ended);
        engine.seek(0.0, SeekPrecision::Exact);
        let status = poll_until(&engine, StdDuration::from_secs(10), |status| {
            !status.ended && status.position > 0.5
        });
        assert!(status.error.is_none());
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn plays_video_through_renderer() {
        let engine =
            SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null, StartState::default()).expect("engine loads video");
        let deadline = Instant::now() + StdDuration::from_secs(30);
        let mut frames = Vec::new();
        let status = loop {
            frames.extend(engine.drain_frames());
            let status = engine.snapshot();
            if status.ended || status.error.is_some() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting: {status:?}"
            );
            thread::sleep(StdDuration::from_millis(5));
        };
        frames.extend(engine.drain_frames());
        assert!(status.error.is_none(), "unexpected error: {:?}", status.error);
        assert_eq!(
            (status.video_width, status.video_height),
            (320, 240),
            "status carries the selected video dims"
        );
        assert!(status.delivered > 0, "no frames delivered");
        assert!(
            frames.len() > 24,
            "only {} frames delivered; delivered {} dropped {} at pos {}",
            frames.len(),
            status.delivered,
            status.dropped_late,
            status.position
        );
        for frame in &frames {
            assert_eq!((frame.width, frame.height), (320, 240));
            assert_eq!(frame.rgba.len(), 320 * 240 * 4);
        }
        let varied = frames.iter().any(|frame| {
            let (min, max) = frame
                .rgba
                .iter()
                .fold((255u8, 0u8), |(lo, hi), &b| (lo.min(b), hi.max(b)));
            max - min > 16
        });
        assert!(varied, "frames carry no picture");
        assert_eq!(
            status.delivered as usize,
            frames.len(),
            "every delivered frame is drained exactly once"
        );
    }

    #[test]
    fn rejects_missing_files() {
        assert!(SchedulerEngine::load("/no/such/file.wav", SinkKind::Null, StartState::default()).is_err());
    }
}
