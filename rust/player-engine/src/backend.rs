use crate::sched::{
    AudioChunk, AudioDecoder, AudioOutput, Command, DecodedFrame, DemuxEvent, Demuxer, Packet,
    Scheduler, StreamKind, VideoDecoder, VideoOutput,
};
use crate::state::State;
use player_audio::decode::PacketDecoder;
use player_audio::engine::SinkKind;
use player_audio::sink::{AudioSink, NullSink};
use player_audio::open_default_sink;
use player_media::{MediaError, MediaSession, Packet as MediaPacket, StreamInfo, StreamKind as MediaKind};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub struct EngineDemux {
    session: MediaSession,
    audio_streams: Vec<usize>,
    selected: usize,
    duration: f64,
}

impl EngineDemux {
    pub fn open(path: &str) -> Result<Self, String> {
        let session = MediaSession::open(path).map_err(|err| err.to_string())?;
        let mut audio = Vec::new();
        for stream in session.streams() {
            match stream.kind {
                MediaKind::Video if !stream.attached => {
                    return Err(format!("{path} has a video stream"));
                }
                MediaKind::Audio => audio.push(stream.index),
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
            duration,
        })
    }

    pub fn stream_info(&self, index: usize) -> Option<StreamInfo> {
        stream_info(&self.session, index)
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
        false
    }

    fn read(&mut self) -> DemuxEvent {
        loop {
            match self.session.read_packet() {
                Err(MediaError::Eof) => return DemuxEvent::Eof,
                Err(other) => return DemuxEvent::Error(other.to_string()),
                Ok(packet) => {
                    if packet.stream_index != self.selected {
                        continue;
                    }
                    return DemuxEvent::Packet(Packet {
                        stream: packet.stream_index,
                        kind: StreamKind::Audio,
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

pub struct NoVideoDecoder;

impl VideoDecoder for NoVideoDecoder {
    fn send(&mut self, _packet: Packet) -> Result<(), String> {
        Err("video packet on the audio-only path".to_string())
    }

    fn drain(&mut self) -> Result<Vec<DecodedFrame>, String> {
        Ok(Vec::new())
    }

    fn finish(&mut self) -> Result<Vec<DecodedFrame>, String> {
        Ok(Vec::new())
    }

    fn flush(&mut self) {}
}

pub struct NullVideoOutput;

impl VideoOutput for NullVideoOutput {
    fn present(&mut self, _pts: f64, _id: u64) -> Result<(), String> {
        Ok(())
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
    pub error: Option<String>,
}

#[derive(Debug)]
enum WorkerCommand {
    SetPaused(bool),
    Seek(f64),
    SetVolume(f32),
    SetMuted(bool),
    Stop,
}

pub struct SchedulerEngine {
    commands: mpsc::Sender<WorkerCommand>,
    state: Arc<Mutex<EngineStatus>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl SchedulerEngine {
    pub fn load(path: &str, kind: SinkKind) -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel::<WorkerCommand>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let state = Arc::new(Mutex::new(EngineStatus {
            volume: 1.0,
            ..EngineStatus::default()
        }));
        let worker_state = state.clone();
        let owned_path = path.to_owned();
        let worker = thread::Builder::new()
            .name("player-scheduler".to_string())
            .spawn(move || {
                run_worker(owned_path, kind, command_rx, worker_state, ready_tx);
            })
            .map_err(|err| format!("scheduler worker failed to start: {err}"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(SchedulerEngine {
                commands: command_tx,
                state,
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

    pub fn snapshot(&self) -> EngineStatus {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
}

impl Drop for SchedulerEngine {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

type EngineScheduler = Scheduler<EngineDemux, EngineAudio, NoVideoDecoder, EngineOutput, NullVideoOutput>;

fn run_worker(
    path: String,
    kind: SinkKind,
    commands: mpsc::Receiver<WorkerCommand>,
    state: Arc<Mutex<EngineStatus>>,
    ready: mpsc::Sender<Result<(), String>>,
) {
    let mut guard = PanicGuard {
        state: state.clone(),
        clean: false,
    };
    let build = || -> Result<(EngineScheduler, f64), String> {
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
        let mut scheduler =
            EngineScheduler::new(demux, audio, NoVideoDecoder, output, NullVideoOutput);
        scheduler.open();
        Ok((scheduler, duration))
    };
    let (mut worker, duration) = match build() {
        Ok((scheduler, duration)) => (
            Worker {
                scheduler,
                volume: 1.0,
                muted: false,
            },
            duration,
        ),
        Err(message) => {
            update(&state, |status| status.error = Some(message.clone()));
            let _ = ready.send(Err(message));
            guard.clean = true;
            return;
        }
    };
    update(&state, |status| status.duration = duration);
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
        }
    }

    fn publish(&self, state: &Arc<Mutex<EngineStatus>>, now: f64) {
        let position = self.scheduler.clock_position(now);
        let paused = matches!(self.scheduler.state(), State::Paused { .. });
        let ended = matches!(self.scheduler.state(), State::Ended | State::Error);
        let error = self.scheduler.failure().map(str::to_string);
        let (volume, muted) = (self.volume, self.muted);
        update(state, |status| {
            status.position = position;
            status.paused = paused;
            status.ended = ended;
            status.error = error;
            status.volume = volume;
            status.muted = muted;
        });
    }
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
    fn rejects_video_files() {
        let err = match SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null) {
            Ok(_) => panic!("video must be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("video stream"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_missing_files() {
        assert!(SchedulerEngine::load("/no/such/file.wav", SinkKind::Null).is_err());
    }
}
