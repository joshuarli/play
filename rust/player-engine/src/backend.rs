use crate::sched::{
    AudioChunk, AudioDecoder, AudioOutput, Command, DemuxEvent, Demuxer, Packet, Scheduler,
    StreamKind,
};
use crate::state::State;
use crate::sub::{EngineSub, TrackInfo, TrackKind};
use crate::video::{EngineVideo, EngineVideoOutput, PresentedFrame};
use player_ass::{Library, RenderedImage};
use player_audio::decode::PacketDecoder;
use player_audio::sink::{AudioSink, NullSink, SinkKind};
use player_audio::open_default_sink;
use player_media::{MediaError, MediaSession, Packet as MediaPacket, StreamInfo, StreamKind as MediaKind};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PRESENTED_CHANNEL_DEPTH: usize = 4;

pub struct EngineDemux {
    session: MediaSession,
    audio_streams: Vec<usize>,
    selected: usize,
    selected_video: Option<usize>,
    sub_streams: Vec<usize>,
    duration: f64,
}

impl EngineDemux {
    pub fn open(path: &str) -> Result<Self, String> {
        let session = MediaSession::open(path).map_err(|err| err.to_string())?;
        let mut audio = Vec::new();
        let mut selected_video = None;
        let mut subs = Vec::new();
        for stream in session.streams() {
            match stream.kind {
                MediaKind::Video if !stream.attached && selected_video.is_none() => {
                    selected_video = Some(stream.index)
                }
                MediaKind::Audio => audio.push(stream.index),
                MediaKind::Subtitle => subs.push(stream.index),
                _ => {}
            }
        }
        let selected = *audio
            .first()
            .ok_or_else(|| format!("{path} has no audio stream"))?;
        let info = stream_info(&session, selected).ok_or_else(|| format!("{path} has no usable audio format"))?;
        if info.sample_rate <= 0 || info.channels <= 0 {
            return Err(format!("{path} has no usable audio format"));
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
        true
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
                    let kind = if packet.stream_index == self.selected {
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
            self.selected = stream;
        }
    }

    fn selected_audio(&self) -> usize {
        self.selected
    }
}

pub struct EngineAudio {
    core: PacketDecoder,
    outbox: VecDeque<AudioChunk>,
    stream: usize,
    channels: u16,
    rate: u32,
}

impl EngineAudio {
    pub fn open(info: &StreamInfo, target_rate: u32) -> Result<Self, String> {
        let core = PacketDecoder::open(info, target_rate)?;
        Ok(EngineAudio {
            channels: core.target_layout().channels() as u16,
            rate: core.target_rate(),
            core,
            outbox: VecDeque::new(),
            stream: info.index,
        })
    }
}

impl AudioDecoder for EngineAudio {
    fn send(&mut self, packet: Packet) -> Result<(), String> {
        let media = MediaPacket {
            stream_index: packet.stream,
            pts_seconds: packet.pts,
            dts_seconds: None,
            duration_seconds: packet.duration,
            keyframe: false,
            data: packet.payload,
        };
        let pts = packet.pts.unwrap_or(0.0);
        if let Some(ready) = self.core.feed(&media, pts)? {
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
        if let Some(ready) = self.core.finish()? {
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
        self.core.reset();
        self.outbox.clear();
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
        let _ = self.sink.flush();
    }

    fn drained(&self) -> bool {
        self.sink.queued_frames() == 0
    }
}

#[derive(Debug, Clone, Default)]
pub struct EngineStatus {
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub muted: bool,
    pub paused: bool,
    pub ended: bool,
    pub presented: u64,
    pub video_width: u32,
    pub video_height: u32,
    pub error: Option<String>,
    pub tracks: Vec<TrackInfo>,
}

#[derive(Debug)]
enum WorkerCommand {
    SetPaused(bool),
    Seek(f64),
    SetVolume(f32),
    SetMuted(bool),
    Stop,
    SelectSub(Option<usize>),
    AddSub(String, mpsc::Sender<Result<(), String>>),
    SetSubVisible(bool),
    RenderSubs {
        width: i32,
        height: i32,
        par: f64,
        now_ms: i64,
        reply: mpsc::Sender<Result<Vec<RenderedImage>, String>>,
    },
}

pub struct SchedulerEngine {
    commands: mpsc::Sender<WorkerCommand>,
    state: Arc<Mutex<EngineStatus>>,
    frames: Option<mpsc::Receiver<PresentedFrame>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl SchedulerEngine {
    pub fn load(path: &str, kind: SinkKind) -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel::<WorkerCommand>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        // Bounded: present blocks when the consumer falls behind instead of
        // buffering frames without limit.
        let (frames_tx, frames_rx) = mpsc::sync_channel(PRESENTED_CHANNEL_DEPTH);
        let state = Arc::new(Mutex::new(EngineStatus {
            volume: 1.0,
            ..EngineStatus::default()
        }));
        let worker_state = state.clone();
        let owned_path = path.to_owned();
        let worker = thread::Builder::new()
            .name("player-scheduler".to_string())
            .spawn(move || {
                run_worker(owned_path, kind, command_rx, worker_state, ready_tx, frames_tx);
            })
            .map_err(|err| format!("scheduler worker failed to start: {err}"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(SchedulerEngine {
                commands: command_tx,
                state,
                frames: Some(frames_rx),
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

    pub fn seek(&self, seconds: f64) {
        self.send(WorkerCommand::Seek(seconds.max(0.0)));
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

    pub fn render_subtitles(
        &self,
        width: i32,
        height: i32,
        par: f64,
        now_ms: i64,
    ) -> Result<Vec<RenderedImage>, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send(WorkerCommand::RenderSubs {
            width,
            height,
            par,
            now_ms,
            reply: reply_tx,
        });
        reply_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| "subtitle render timed out".to_owned())?
    }

    pub fn snapshot(&self) -> EngineStatus {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    pub fn drain_frames(&self) -> Vec<PresentedFrame> {
        let mut frames = Vec::new();
        if let Some(rx) = self.frames.as_ref() {
            while let Ok(frame) = rx.try_recv() {
                frames.push(frame);
            }
        }
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
    commands: mpsc::Receiver<WorkerCommand>,
    state: Arc<Mutex<EngineStatus>>,
    ready: mpsc::Sender<Result<(), String>>,
    frames_tx: mpsc::SyncSender<PresentedFrame>,
) {
    let mut guard = PanicGuard {
        state: state.clone(),
        clean: false,
    };
    let build = || -> Result<(EngineScheduler, f64, u32, u32, Library, (i32, i32)), String> {
        let demux = EngineDemux::open(&path)?;
        let info = demux
            .stream_info(demux.selected_audio())
            .ok_or_else(|| format!("{path} has no usable audio format"))?;
        let sink: Box<dyn AudioSink> = match kind {
            SinkKind::Null => Box::new(NullSink::new(
                info.channels.max(1) as usize,
                info.sample_rate.max(1) as u32,
            )),
            SinkKind::Default => open_default_sink(
                info.channels.max(1) as usize,
                info.sample_rate.max(1) as u32,
                info.channel_mask,
            )?,
        };
        let duration = demux.duration();
        let rate = sink.rate();
        let audio = EngineAudio::open(&info, rate)?;
        let output = EngineOutput {
            sink,
            written: 0,
            volume: 1.0,
            muted: false,
        };
        let (video, video_out) = match demux.selected_video() {
            Some(stream) => {
                let decoder = EngineVideo::open(demux.session(), stream)?;
                let shared = decoder.shared();
                let output = EngineVideoOutput::open(shared, frames_tx)?;
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
        let library = Library::new()?;
        let playres = if video_width > 0 && video_height > 0 {
            (video_width as i32, video_height as i32)
        } else {
            (640, 480)
        };
        let mut subs = Vec::new();
        for stream in scheduler.demux().sub_streams().to_vec() {
            let Some(info) = scheduler.demux().sub_info(stream) else {
                continue;
            };
            let Ok(sub) = EngineSub::open_embedded(&library, &info, playres) else {
                continue;
            };
            subs.push(sub);
        }
        scheduler.set_subs(subs);
        scheduler.open();
        Ok((scheduler, duration, video_width, video_height, library, playres))
    };
    let (mut worker, duration, video_width, video_height) = match build() {
        Ok((scheduler, duration, video_width, video_height, library, playres)) => (
            Worker {
                scheduler,
                volume: 1.0,
                muted: false,
                library,
                playres,
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
        while let Ok(command) = commands.try_recv() {
            if matches!(command, WorkerCommand::Stop) {
                stopped = true;
                break;
            }
            worker.apply(command);
        }
        if stopped {
            break;
        }
        let now = start.elapsed().as_secs_f64();
        worker.scheduler.step(now);
        worker.publish(&state, now);
        if parked(worker.scheduler.state()) {
            match commands.recv() {
                Ok(WorkerCommand::Stop) => stopped = true,
                Ok(command) => worker.apply(command),
                Err(_) => stopped = true,
            }
            worker.publish(&state, start.elapsed().as_secs_f64());
        } else if worker.scheduler.audio_output().queued_seconds() > 0.5 {
            thread::sleep(Duration::from_millis(5));
        }
    }
    guard.clean = true;
}

fn parked(state: State) -> bool {
    matches!(state, State::Paused { .. } | State::Ended | State::Error)
}

struct Worker {
    scheduler: EngineScheduler,
    volume: f32,
    muted: bool,
    library: Library,
    playres: (i32, i32),
}

impl Worker {
    fn apply(&mut self, command: WorkerCommand) {
        match command {
            WorkerCommand::SetPaused(paused) => self.scheduler.send(Command::Pause(paused)),
            WorkerCommand::Seek(seconds) => self.scheduler.send(Command::Seek(seconds)),
            WorkerCommand::SetVolume(volume) => {
                self.volume = volume;
                self.scheduler.audio_output_mut().set_volume(volume);
            }
            WorkerCommand::SetMuted(muted) => {
                self.muted = muted;
                self.scheduler.audio_output_mut().set_muted(muted);
            }
            WorkerCommand::Stop => {}
            WorkerCommand::SelectSub(selected) => self.scheduler.select_sub(selected),
            WorkerCommand::AddSub(path, reply) => {
                let result = EngineSub::open_external(&self.library, &path, self.playres)
                    .map(|sub| {
                        self.scheduler.add_sub(sub);
                    });
                let _ = reply.send(result);
            }
            WorkerCommand::SetSubVisible(visible) => self.scheduler.set_sub_visible(visible),
            WorkerCommand::RenderSubs {
                width,
                height,
                par,
                now_ms,
                reply,
            } => {
                let result = self
                    .scheduler
                    .render_subtitles(width, height, par, now_ms);
                let _ = reply.send(result);
            }
        }
    }

    fn publish(&self, state: &Arc<Mutex<EngineStatus>>, now: f64) {
        let position = self.scheduler.clock_position(now);
        let paused = matches!(self.scheduler.state(), State::Paused { .. });
        let ended = matches!(self.scheduler.state(), State::Ended | State::Error);
        let error = self.scheduler.failure().map(str::to_string);
        let presented = self.scheduler.stats().presented;
        let (volume, muted) = (self.volume, self.muted);
        let tracks = track_list(&self.scheduler);
        update(state, |status| {
            status.position = position;
            status.paused = paused;
            status.ended = ended;
            status.error = error;
            status.presented = presented;
            status.volume = volume;
            status.muted = muted;
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
            codec: sub.source().codec.name().to_owned(),
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
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration as StdDuration;

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

    fn fixture(name: &str) -> String {
        fixtures().join(name).to_string_lossy().into_owned()
    }

    fn poll_until(engine: &SchedulerEngine, timeout: StdDuration, ready: impl Fn(&EngineStatus) -> bool) -> EngineStatus {
        let deadline = Instant::now() + timeout;
        loop {
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
        let engine = SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null).expect("engine loads");
        let status = poll_until(&engine, StdDuration::from_secs(10), |status| status.ended);
        assert!(status.error.is_none(), "unexpected error: {:?}", status.error);
        assert!((2.5..3.6).contains(&status.position), "position {}", status.position);
        assert!(status.duration > 0.0);
    }

    #[test]
    fn pause_freezes_position() {
        let engine = SchedulerEngine::load(&fixture("audio-flac.flac"), SinkKind::Null).expect("engine loads");
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
        let engine = SchedulerEngine::load(&fixture("audio-mp3.mp3"), SinkKind::Null).expect("engine loads");
        engine.set_paused(true);
        engine.seek(2.0);
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
            SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null).expect("engine loads video");
        engine.set_paused(true);
        let paused = poll_until(&engine, StdDuration::from_secs(10), |status| status.paused);
        assert!(paused.paused);
        engine.seek(1.0);
        let status = poll_until(&engine, StdDuration::from_secs(10), |status| {
            (status.position - 1.0).abs() < 0.05
        });
        assert!(status.error.is_none(), "unexpected error: {:?}", status.error);
        assert!(!status.ended, "engine ended early: {status:?}");
        assert!((status.position - 1.0).abs() < 0.05);
    }

    fn pause_and_seek(engine: &SchedulerEngine, seconds: f64) {
        engine.set_paused(true);
        poll_until(engine, StdDuration::from_secs(10), |status| status.paused);
        engine.seek(seconds);
        poll_until(engine, StdDuration::from_secs(10), |status| {
            (status.position - seconds).abs() < 0.05
        });
    }

    fn inked(images: &[player_ass::RenderedImage]) -> usize {
        images
            .iter()
            .map(|image| image.bitmap.iter().filter(|pixel| **pixel > 0).count())
            .sum()
    }

    #[test]
    fn external_subs_render_on_audio_only() {
        let engine =
            SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null).expect("engine loads");
        pause_and_seek(&engine, 0.0);
        engine
            .add_sub(&fixture("external.srt"))
            .expect("srt loads");
        let status = engine.snapshot();
        let sub = status
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle)
            .expect("sub track listed");
        assert_eq!(sub.codec, "subrip");
        assert!(sub.selected, "added subs auto-select");
        assert_eq!(sub.sub_index, Some(0));
        let during = engine
            .render_subtitles(640, 480, 1.0, 600)
            .expect("subs render");
        assert!(!during.is_empty(), "no bitmaps during the cue");
        assert!(inked(&during) > 50, "no ink during the cue");
        let outside = engine
            .render_subtitles(640, 480, 1.0, 2900)
            .expect("subs render");
        assert!(outside.is_empty(), "bitmaps outside the cues");
        engine.select_sub(None);
        let deadline = Instant::now() + StdDuration::from_secs(10);
        loop {
            let images = engine
                .render_subtitles(640, 480, 1.0, 600)
                .expect("subs render");
            if images.is_empty() {
                break;
            }
            assert!(Instant::now() < deadline, "deselect never applied");
            thread::sleep(StdDuration::from_millis(5));
        }
        engine.select_sub(Some(0));
        let reselected = engine
            .render_subtitles(640, 480, 1.0, 600)
            .expect("subs render");
        assert!(!reselected.is_empty(), "no bitmaps after reselect");
        engine.set_sub_visible(false);
        let hidden = engine
            .render_subtitles(640, 480, 1.0, 600)
            .expect("subs render");
        assert!(hidden.is_empty(), "bitmaps while hidden");
        engine.set_sub_visible(true);
        engine.seek(0.6);
        let status = poll_until(&engine, StdDuration::from_secs(10), |status| {
            (status.position - 0.6).abs() < 0.05
        });
        assert!(!status.ended, "ended after reseek: {status:?}");
        let reseeked = engine
            .render_subtitles(640, 480, 1.0, 700)
            .expect("subs render");
        assert!(!reseeked.is_empty(), "no bitmaps after seek");
        assert!(engine.add_sub(&fixture("audio-wav.wav")).is_err());
    }

    #[test]
    fn external_ass_switches_tracks() {
        let engine =
            SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null).expect("engine loads");
        pause_and_seek(&engine, 0.0);
        engine
            .add_sub(&fixture("external.srt"))
            .expect("srt loads");
        engine
            .add_sub(&fixture("external.ass"))
            .expect("ass loads");
        let status = engine.snapshot();
        let selected: Vec<usize> = status
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Subtitle && track.selected)
            .map(|track| track.id)
            .collect();
        assert_eq!(selected.len(), 1, "one sub selected: {selected:?}");
        let ass = status
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle && track.codec == "ass")
            .expect("ass listed");
        assert!(ass.selected, "added ass auto-selects");
        engine.select_sub(ass.sub_index);
        let _ = engine.snapshot();
        engine.select_sub(Some(0));
        let srt_cue = engine
            .render_subtitles(640, 480, 1.0, 1700)
            .expect("subs render");
        assert!(!srt_cue.is_empty(), "no bitmaps for the srt cue");
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn embedded_subs_render_at_cue_time() {
        let engine = SchedulerEngine::load(&fixture("h264-embedded-srt.mkv"), SinkKind::Null)
            .expect("engine loads video with subs");
        engine.select_sub(Some(0));
        poll_until(&engine, StdDuration::from_secs(30), |status| status.ended);
        engine.seek(0.5);
        engine.set_paused(true);
        poll_until(&engine, StdDuration::from_secs(10), |status| status.paused);
        let status = engine.snapshot();
        let sub = status
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle)
            .expect("embedded sub listed");
        assert_eq!(sub.codec, "subrip");
        assert!(sub.selected, "embedded subs auto-select");
        let during = engine
            .render_subtitles(320, 240, 1.0, 600)
            .expect("subs render");
        assert!(!during.is_empty(), "no bitmaps during the cue");
        assert!(inked(&during) > 50, "no ink during the cue");
        let outside = engine
            .render_subtitles(320, 240, 1.0, 2900)
            .expect("subs render");
        assert!(outside.is_empty(), "bitmaps outside the cues");
        engine.seek(1.5);
        engine.set_paused(true);
        poll_until(&engine, StdDuration::from_secs(10), |status| status.paused);
        let second = engine
            .render_subtitles(320, 240, 1.0, 1800)
            .expect("subs render");
        assert!(!second.is_empty(), "no bitmaps for the second cue");
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn unsupported_sub_codec_lists_but_never_selects() {
        let engine = SchedulerEngine::load(&fixture("h264-movtext.mp4"), SinkKind::Null)
            .expect("engine loads video with movtext");
        pause_and_seek(&engine, 0.0);
        let status = engine.snapshot();
        let sub = status
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle)
            .expect("movtext listed");
        assert_eq!(sub.codec, "mov_text");
        assert!(!sub.selected, "unsupported codec auto-selects");
        assert_eq!(sub.sub_index, None);
        engine.select_sub(Some(0));
        let still = engine.snapshot();
        assert!(
            still
                .tracks
                .iter()
                .filter(|track| track.kind == TrackKind::Subtitle && track.selected)
                .next()
                .is_none(),
            "unsupported codec selected"
        );
    }

    #[test]
    fn seek_after_end_restarts() {
        let engine = SchedulerEngine::load(&fixture("audio-mono.wav"), SinkKind::Null).expect("engine loads");
        poll_until(&engine, StdDuration::from_secs(10), |status| status.ended);
        engine.seek(0.0);
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
            SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null).expect("engine loads video");
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
        assert!(status.presented > 0, "no frames presented");
        assert!(!frames.is_empty(), "no frames delivered");
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
            status.presented as usize,
            frames.len(),
            "every presented frame is delivered exactly once"
        );
    }

    #[test]
    fn rejects_missing_files() {
        assert!(SchedulerEngine::load("/no/such/file.wav", SinkKind::Null).is_err());
    }
}
