use crate::clock::AudioClock;
use crate::pipeline::Pipeline;
use crate::sink::{AudioSink, NullSink};
use player_media::{MediaSession, StreamKind};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

pub enum SinkKind {
    Null,
    Default,
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

enum Command {
    SetPaused(bool),
    Seek(f64),
    SetVolume(f32),
    SetMuted(bool),
    Stop,
}

struct WorkerState {
    status: EngineStatus,
}

struct Probe {
    stream_index: usize,
    duration: f64,
    sample_rate: u32,
    channels: usize,
    channel_mask: u64,
}

pub struct AudioEngine {
    commands: mpsc::Sender<Command>,
    state: Arc<Mutex<WorkerState>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl AudioEngine {
    pub fn load(path: &str, kind: SinkKind) -> Result<Self, String> {
        let probe = probe(path)?;
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let state = Arc::new(Mutex::new(WorkerState {
            status: EngineStatus {
                duration: probe.duration,
                volume: 1.0,
                ..EngineStatus::default()
            },
        }));
        let worker_state = state.clone();
        let owned_path = path.to_owned();
        let worker = thread::Builder::new()
            .name("player-audio".to_string())
            .spawn(move || {
                run_worker(owned_path, probe, kind, command_rx, worker_state, ready_tx)
            })
            .map_err(|err| format!("audio worker failed to start: {err}"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(AudioEngine {
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
                Err("audio worker died before reporting readiness".to_string())
            }
        }
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn set_paused(&self, paused: bool) {
        self.send(Command::SetPaused(paused));
    }

    pub fn seek(&self, seconds: f64) {
        self.send(Command::Seek(seconds.max(0.0)));
    }

    pub fn set_volume(&self, volume: f32) {
        self.send(Command::SetVolume(volume.clamp(0.0, 2.0)));
    }

    pub fn set_muted(&self, muted: bool) {
        self.send(Command::SetMuted(muted));
    }

    pub fn snapshot(&self) -> EngineStatus {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .status
            .clone()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn probe(path: &str) -> Result<Probe, String> {
    let session = MediaSession::open(path).map_err(|err| err.to_string())?;
    let mut audio = None;
    for stream in session.streams() {
        match stream.kind {
            StreamKind::Video if !stream.attached => {
                return Err(format!("{path} has a video stream"));
            }
            StreamKind::Audio if audio.is_none() => {
                audio = Some(stream);
            }
            _ => {}
        }
    }
    let stream = audio.ok_or_else(|| format!("{path} has no audio stream"))?;
    if stream.sample_rate <= 0 || stream.channels <= 0 {
        return Err(format!("{path} has no usable audio format"));
    }
    Ok(Probe {
        stream_index: stream.index,
        duration: session.duration_seconds().unwrap_or(0.0).max(0.0),
        sample_rate: stream.sample_rate as u32,
        channels: stream.channels as usize,
        channel_mask: stream.channel_mask,
    })
}

fn update(state: &Arc<Mutex<WorkerState>>, apply: impl FnOnce(&mut EngineStatus)) {
    if let Ok(mut guard) = state.lock() {
        apply(&mut guard.status);
    }
}

struct PanicGuard {
    state: Arc<Mutex<WorkerState>>,
    clean: bool,
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        if !self.clean {
            update(&self.state, |status| {
                status.error = Some("audio worker died".to_string());
                status.ended = true;
            });
        }
    }
}

fn run_worker(
    path: String,
    probe: Probe,
    kind: SinkKind,
    commands: mpsc::Receiver<Command>,
    state: Arc<Mutex<WorkerState>>,
    ready: mpsc::Sender<Result<(), String>>,
) {
    let mut guard = PanicGuard {
        state: state.clone(),
        clean: false,
    };
    let build = || -> Result<(Box<dyn AudioSink>, Pipeline), String> {
        let sink: Box<dyn AudioSink> = match kind {
            SinkKind::Null => Box::new(NullSink::new(probe.channels, probe.sample_rate)),
            SinkKind::Default => {
                crate::open_default_sink(probe.channels, probe.sample_rate, probe.channel_mask)?
            }
        };
        let pipeline = Pipeline::open(&path, probe.stream_index, sink.rate())?;
        Ok((sink, pipeline))
    };
    let (mut sink, mut pipeline) = match build() {
        Ok(built) => built,
        Err(message) => {
            update(&state, |status| status.error = Some(message.clone()));
            let _ = ready.send(Err(message));
            guard.clean = true;
            return;
        }
    };
    if let Some(duration) = pipeline.duration_seconds() {
        update(&state, |status| status.duration = duration.max(0.0));
    }
    if ready.send(Ok(())).is_err() {
        guard.clean = true;
        return;
    }
    let rate = sink.rate();
    let mut pump = Pump {
        sink,
        pipeline,
        clock: AudioClock::new(rate),
        written: 0,
        volume: 1.0,
        muted: false,
        paused: false,
        ended: false,
        stopped: false,
        state: state.clone(),
    };
    let mut chunk = Vec::new();
    while !pump.stopped {
        while let Ok(command) = commands.try_recv() {
            pump.apply(command);
        }
        if pump.stopped {
            break;
        }
        if pump.parked() {
            pump.publish();
            match commands.recv() {
                Ok(command) => pump.apply(command),
                Err(_) => pump.stopped = true,
            }
            pump.publish();
            continue;
        }
        chunk.clear();
        if pump.pump_chunk(&mut chunk) {
            continue;
        }
    }
    let _ = pump.sink.flush();
    guard.clean = true;
}

struct Pump {
    sink: Box<dyn AudioSink>,
    pipeline: Pipeline,
    clock: AudioClock,
    written: u64,
    volume: f32,
    muted: bool,
    paused: bool,
    ended: bool,
    stopped: bool,
    state: Arc<Mutex<WorkerState>>,
}

impl Pump {
    fn parked(&self) -> bool {
        self.paused || self.ended
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::SetPaused(next) => {
                if next == self.paused {
                    return;
                }
                if next {
                    let queued = self.sink.queued_frames();
                    let _ = self.sink.pause();
                    self.clock.set_paused(true, self.written, queued);
                } else {
                    let _ = self.sink.resume();
                    self.clock.set_paused(false, self.written, self.sink.queued_frames());
                    self.ended = false;
                }
                self.paused = next;
            }
            Command::Seek(seconds) => match self.pipeline.seek_seconds(seconds) {
                Ok(()) => {
                    let _ = self.sink.flush();
                    self.clock.seek(seconds, self.written);
                    self.ended = false;
                }
                Err(message) => {
                    update(&self.state, |status| status.error = Some(message));
                }
            },
            Command::SetVolume(next) => self.volume = next,
            Command::SetMuted(next) => self.muted = next,
            Command::Stop => self.stopped = true,
        }
    }

    fn publish(&mut self) {
        let position = self.clock.position(self.written, self.sink.queued_frames());
        let (paused, ended, volume, muted) = (self.paused, self.ended, self.volume, self.muted);
        update(&self.state, |status| {
            status.position = position;
            status.paused = paused;
            status.ended = ended;
            status.volume = volume;
            status.muted = muted;
        });
    }

    fn pump_chunk(&mut self, chunk: &mut Vec<f32>) -> bool {
        match self.pipeline.next_chunk(chunk) {
            Ok(done) => {
                let gain = if self.muted { 0.0 } else { self.volume };
                if gain != 1.0 {
                    for sample in chunk.iter_mut() {
                        *sample *= gain;
                    }
                }
                if let Err(message) = self.sink.write(chunk) {
                    update(&self.state, |status| {
                        status.error = Some(message);
                        status.ended = true;
                    });
                    self.ended = true;
                    return true;
                }
                self.written += (chunk.len() / self.sink.channels()) as u64;
                if done {
                    let _ = self.sink.finish();
                    self.ended = true;
                }
                self.publish();
                false
            }
            Err(message) => {
                update(&self.state, |status| {
                    status.error = Some(message);
                    status.ended = true;
                });
                self.ended = true;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

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

    fn load_null(name: &str) -> AudioEngine {
        AudioEngine::load(&fixture(name), SinkKind::Null).expect("engine loads")
    }

    fn poll_until(engine: &AudioEngine, timeout: Duration, ready: impl Fn(&EngineStatus) -> bool) -> EngineStatus {
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
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn plays_to_end_with_null_sink() {
        let engine = load_null("audio-wav.wav");
        let status = poll_until(&engine, Duration::from_secs(10), |status| status.ended);
        assert!(status.error.is_none(), "unexpected error: {:?}", status.error);
        assert!((2.5..3.6).contains(&status.position), "position {}", status.position);
        assert!(status.duration > 0.0);
    }

    #[test]
    fn pause_freezes_position() {
        let engine = load_null("audio-flac.flac");
        engine.set_paused(true);
        std::thread::sleep(Duration::from_millis(100));
        let frozen = engine.snapshot();
        assert!(frozen.paused);
        let first = frozen.position;
        std::thread::sleep(Duration::from_millis(100));
        let second = engine.snapshot().position;
        assert_eq!(first, second);
        engine.set_paused(false);
        let status = poll_until(&engine, Duration::from_secs(10), |status| status.ended);
        assert!(status.position > first);
    }

    #[test]
    fn seek_moves_position() {
        let engine = load_null("audio-mp3.mp3");
        engine.set_paused(true);
        engine.seek(2.0);
        let status = poll_until(&engine, Duration::from_secs(10), |status| {
            (status.position - 2.0).abs() < 0.05
        });
        assert!(status.paused);
    }

    #[test]
    fn seek_after_end_restarts() {
        let engine = load_null("audio-mono.wav");
        poll_until(&engine, Duration::from_secs(10), |status| status.ended);
        engine.seek(0.0);
        let status = poll_until(&engine, Duration::from_secs(10), |status| {
            !status.ended && status.position > 0.5
        });
        assert!(status.error.is_none());
    }

    #[test]
    fn rejects_video_files() {
        let err = match AudioEngine::load(&fixture("h264.mp4"), SinkKind::Null) {
            Ok(_) => panic!("video must be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("video stream"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_missing_files() {
        assert!(AudioEngine::load("/no/such/file.wav", SinkKind::Null).is_err());
    }

    #[test]
    fn volume_and_mute_apply() {
        let engine = load_null("audio-wav.wav");
        engine.set_volume(0.5);
        engine.set_muted(true);
        let status = poll_until(&engine, Duration::from_secs(10), |status| {
            status.muted && status.volume == 0.5
        });
        assert!(status.muted);
        engine.set_muted(false);
        let status = poll_until(&engine, Duration::from_secs(10), |status| !status.muted);
        assert!(!status.muted);
    }
}

use std::ffi::{c_char, c_int, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

#[repr(C)]
pub struct EngineSnapshotFfi {
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub paused: c_int,
    pub muted: c_int,
    pub ended: c_int,
    pub has_error: c_int,
    pub error: [c_char; 256],
}

#[repr(C)]
pub struct AudioProbeFfi {
    pub engine_ready: c_int,
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub title: [c_char; 128],
    pub artist: [c_char; 128],
    pub album: [c_char; 128],
    pub date: [c_char; 32],
}

fn copy_string(out: &mut [c_char], value: &str) {
    let bytes = value.as_bytes();
    let length = bytes.len().min(out.len().saturating_sub(1));
    for (slot, byte) in out[..length].iter_mut().zip(bytes[..length].iter()) {
        *slot = *byte as c_char;
    }
    out[length] = 0;
}

fn probe_metadata(session: &MediaSession) -> (String, String, String, String) {
    let entries = session.metadata();
    let find = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| {
                entries
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value.clone())
            })
            .unwrap_or_default()
    };
    (
        find(&["Title"]),
        find(&["Artist", "album_artist", "AlbumArtist"]),
        find(&["Album"]),
        find(&["Date", "Year"]),
    )
}

/// Returns 1 when the file is audio-only and the Rust engine can play it,
/// 0 when the C engine should keep it, and a negative value on probe error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_probe(
    path: *const c_char,
    out: *mut AudioProbeFfi,
    err: *mut c_char,
    err_cap: usize,
) -> c_int {
    if path.is_null() || out.is_null() {
        return -1;
    }
    // SAFETY: path is a valid NUL-terminated string for this call.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<AudioProbeFfi, String> {
        let session = MediaSession::open(&path).map_err(|err| err.to_string())?;
        let mut audio = None;
        for stream in session.streams() {
            match stream.kind {
                StreamKind::Video if !stream.attached => {
                    return Ok(AudioProbeFfi::empty());
                }
                StreamKind::Audio if audio.is_none() => {
                    audio = Some(stream);
                }
                _ => {}
            }
        }
        let stream = match audio {
            Some(stream) => stream,
            None => return Ok(AudioProbeFfi::empty()),
        };
        let engine_ready = crate::session::DecoderSession::open(&stream).is_ok();
        let (title, artist, album, date) = probe_metadata(&session);
        let mut probe = AudioProbeFfi::empty();
        if engine_ready {
            probe.engine_ready = 1;
        }
        probe.duration = session.duration_seconds().unwrap_or(0.0).max(0.0);
        probe.sample_rate = stream.sample_rate.max(0) as u32;
        probe.channels = stream.channels.clamp(0, u16::MAX as i32) as u16;
        copy_string(&mut probe.title, &title);
        copy_string(&mut probe.artist, &artist);
        copy_string(&mut probe.album, &album);
        copy_string(&mut probe.date, &date);
        Ok(probe)
    }));
    match result {
        Ok(Ok(probe)) => {
            // SAFETY: out points to a valid probe struct for this call.
            unsafe { ptr::write(out, probe) };
            let ready = unsafe { (*out).engine_ready };
            if ready == 1 { 1 } else { 0 }
        }
        Ok(Err(message)) => {
            // SAFETY: err has err_cap writable bytes per the function contract.
            unsafe { write_ffi_error(err, err_cap, &message) };
            -2
        }
        Err(_) => {
            // SAFETY: err has err_cap writable bytes per the function contract.
            unsafe { write_ffi_error(err, err_cap, "panic in player_audio_probe") };
            -3
        }
    }
}

impl AudioProbeFfi {
    fn empty() -> Self {
        AudioProbeFfi {
            engine_ready: 0,
            duration: 0.0,
            sample_rate: 0,
            channels: 0,
            title: [0; 128],
            artist: [0; 128],
            album: [0; 128],
            date: [0; 32],
        }
    }
}

unsafe fn write_ffi_error(err: *mut c_char, cap: usize, message: &str) {
    if err.is_null() || cap == 0 {
        return;
    }
    let bytes = message.as_bytes();
    let length = bytes.len().min(cap - 1);
    // SAFETY: err has cap writable bytes.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), err.cast::<u8>(), length);
        *err.add(length) = 0;
    }
}

/// # Safety
///
/// `path` must be NUL-terminated; `err` must have `err_cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_load(
    path: *const c_char,
    null_sink: c_int,
    err: *mut c_char,
    err_cap: usize,
) -> *mut AudioEngine {
    if path.is_null() {
        unsafe { write_ffi_error(err, err_cap, "null path") };
        return ptr::null_mut();
    }
    // SAFETY: path is NUL-terminated for this call.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
    let kind = if null_sink != 0 { SinkKind::Null } else { SinkKind::Default };
    let result = catch_unwind(AssertUnwindSafe(|| AudioEngine::load(&path, kind)));
    match result {
        Ok(Ok(engine)) => Box::into_raw(Box::new(engine)),
        Ok(Err(message)) => {
            unsafe { write_ffi_error(err, err_cap, &message) };
            ptr::null_mut()
        }
        Err(_) => {
            unsafe { write_ffi_error(err, err_cap, "panic in player_audio_engine_load") };
            ptr::null_mut()
        }
    }
}

unsafe fn with_engine<R>(handle: *mut AudioEngine, default: R, apply: impl FnOnce(&AudioEngine) -> R) -> R {
    if handle.is_null() {
        return default;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: handle came from player_audio_engine_load and is still live.
        apply(unsafe { &*handle })
    }));
    result.unwrap_or(default)
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and still be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_set_paused(handle: *mut AudioEngine, paused: c_int) {
    unsafe { with_engine(handle, (), |engine| engine.set_paused(paused != 0)) };
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and still be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_seek(handle: *mut AudioEngine, seconds: f64) {
    unsafe { with_engine(handle, (), |engine| engine.seek(seconds)) };
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and still be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_set_volume(handle: *mut AudioEngine, volume: f32) {
    unsafe { with_engine(handle, (), |engine| engine.set_volume(volume)) };
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and still be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_set_muted(handle: *mut AudioEngine, muted: c_int) {
    unsafe { with_engine(handle, (), |engine| engine.set_muted(muted != 0)) };
}

/// # Safety
///
/// `handle` and `out` must be valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_snapshot(
    handle: *mut AudioEngine,
    out: *mut EngineSnapshotFfi,
) -> c_int {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: handle and out are valid for this call.
        let engine = unsafe { &*handle };
        let status = engine.snapshot();
        let mut snapshot = EngineSnapshotFfi {
            position: status.position,
            duration: status.duration,
            volume: status.volume,
            paused: status.paused as c_int,
            muted: status.muted as c_int,
            ended: status.ended as c_int,
            has_error: status.error.is_some() as c_int,
            error: [0; 256],
        };
        if let Some(message) = status.error {
            copy_string(&mut snapshot.error, &message);
        }
        // SAFETY: out is writable for one snapshot.
        unsafe { ptr::write(out, snapshot) };
    }));
    if result.is_err() { -1 } else { 0 }
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_destroy(handle: *mut AudioEngine) {
    if handle.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: handle is live and never used after this call.
        drop(unsafe { Box::from_raw(handle) });
    }));
}
