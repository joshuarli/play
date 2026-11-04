use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use player_audio::sink::SinkKind;
use player_core::command::{parse_string, Adjust, Command as CoreCommand, Toggle, TrackKind as CoreTrackKind};
use player_engine::video::PresentedFrame;
use player_engine::{EngineStatus, SchedulerEngine};
use player_ffi::engine::{EngineBackend, SharedEngine};
use player_lua::queue::EventSender;
use player_lua::{CommandRequest, NativeValue, PlayerBackend, ScriptEvent, ScriptHost, ScriptModule};
use player_output::{Overlay, OverlayRect};
use player_tui::{Control, Cover, Player as TuiPlayer, Snapshot, Track};

use crate::ipc::{self, IpcServer};
use crate::options::{self, Options, Start};
use crate::playlist::{self, Entry, Playlist};
use crate::properties::{self, Probe};
use crate::window::{HostEvent, HostWindow};

const ASSDRAW: &str = include_str!("../../../player/lua/assdraw.lua");
const OPTIONS: &str = include_str!("../../../player/lua/options.lua");
const OSC: &str = include_str!("../../../player/lua/osc.lua");

const GATE_FRAMES: u64 = 24;
const GATE_OVERLAY_INK: usize = 100;

struct Observer {
    name: String,
    last: Option<NativeValue>,
}

pub struct App {
    pub options: Options,
    pub playlist: Playlist,
    pub probe: Option<Probe>,
    pub shared: Option<SharedEngine>,
    pub host: Option<ScriptHost>,
    pub sender: Option<EventSender>,
    pub ipc: Option<IpcServer>,
    pub paused: bool,
    pub speed: f64,
    pub volume: f64,
    pub muted: bool,
    pub fullscreen: bool,
    pub aid: Option<i64>,
    pub aid_disabled: bool,
    pub eof_reached: bool,
    pub display: Option<(u32, u32)>,
    window: Option<HostWindow>,
    last_frame: Option<PresentedFrame>,
    frame_serial: u64,
    presented_serial: u64,
    osd: Option<Vec<u8>>,
    osd_size: (i32, i32),
    osd_hash: u64,
    last_osd: Instant,
    gui: bool,
    applied_fullscreen: bool,
    presented_size: (i32, i32),
    gate_ready_at: Option<Instant>,
    last_gate_report: Instant,
    last_gate_visibility: Instant,
    bindings: HashMap<String, String>,
    screenshot_index: u32,
    observers: HashMap<u64, Observer>,
    end_sent: bool,
    quit: bool,
    idle_sent: bool,
    pending_failure: Option<String>,
    pending_pause: Option<bool>,
    deferred_events: Vec<NativeValue>,
    ready_events: Vec<NativeValue>,
}

impl App {
    pub fn new(options: Options) -> Self {
        let fullscreen = options.fullscreen;
        App {
            paused: options.pause,
            speed: 1.0,
            volume: options.volume,
            muted: options.muted,
            fullscreen: options.fullscreen,
            aid: None,
            aid_disabled: false,
            options,
            playlist: Playlist::new(),
            probe: None,
            shared: None,
            host: None,
            sender: None,
            ipc: None,
            eof_reached: false,
            display: None,
            window: None,
            last_frame: None,
            frame_serial: 0,
            presented_serial: 0,
            osd: None,
            osd_size: (0, 0),
            osd_hash: 0,
            last_osd: Instant::now()
                .checked_sub(Duration::from_millis(50))
                .unwrap_or_else(Instant::now),
            gui: false,
            applied_fullscreen: fullscreen,
            presented_size: (0, 0),
            gate_ready_at: None,
            last_gate_report: Instant::now(),
            last_gate_visibility: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            bindings: HashMap::new(),
            screenshot_index: 0,
            observers: HashMap::new(),
            end_sent: false,
            quit: false,
            idle_sent: false,
            pending_failure: None,
            pending_pause: None,
            deferred_events: Vec::new(),
            ready_events: Vec::new(),
        }
    }

    pub fn status(&self) -> EngineStatus {
        self.shared
            .as_ref()
            .and_then(|shared| shared.inspect(|backend| backend.status()))
            .unwrap_or_default()
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<(), String> {
        self.paused = paused;
        self.pending_pause = Some(paused);
        if let Some(shared) = &self.shared {
            shared
                .mutate(|backend| {
                    backend.command(CommandRequest::Args(vec![
                        "set".to_owned(),
                        "pause".to_owned(),
                        if paused { "yes" } else { "no" }.to_owned(),
                    ]))
                })
                .ok_or_else(|| "engine backend lock failed".to_owned())??;
        }
        Ok(())
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.01, 100.0);
        if let Some(shared) = &self.shared {
            let _ = shared.mutate(|backend| {
                backend.set_property("speed", NativeValue::Double(self.speed))
            });
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 200.0) as f64;
        if let Some(shared) = &self.shared {
            let _ = shared.mutate(|backend| {
                backend.command(CommandRequest::Args(vec![
                    "set".to_owned(),
                    "volume".to_owned(),
                    format!("{:.6}", self.volume),
                ]))
            });
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(shared) = &self.shared {
            let _ = shared.mutate(|backend| {
                backend.command(CommandRequest::Args(vec![
                    "set".to_owned(),
                    "mute".to_owned(),
                    if muted { "yes" } else { "no" }.to_owned(),
                ]))
            });
        }
    }

    pub fn apply_fullscreen(&self) {
        if let Some(shared) = &self.shared {
            let _ = shared.mutate(|backend| {
                backend.command(CommandRequest::Args(vec![
                    "set".to_owned(),
                    "fullscreen".to_owned(),
                    if self.fullscreen { "yes" } else { "no" }.to_owned(),
                ]))
            });
        }
    }

    pub fn select_audio(&mut self, id: i64) -> Result<(), String> {
        let Some(probe) = &self.probe else {
            return Ok(());
        };
        let audios: Vec<_> = probe
            .streams
            .iter()
            .filter(|stream| stream.kind == player_media::StreamKind::Audio)
            .collect();
        let Some(stream) = audios.get((id - 1).max(0) as usize) else {
            return Err(format!("no audio track {id}"));
        };
        if let Some(shared) = &self.shared {
            shared
                .mutate(|backend| backend.set_property("aid", NativeValue::Int(id)))
                .ok_or_else(|| "engine backend lock failed".to_owned())??;
        }
        self.aid = Some(id);
        self.aid_disabled = false;
        if !self.options.quiet {
            println!("Using Symphonia for {}", stream.codec_name);
        }
        Ok(())
    }

    pub fn disable_audio(&mut self) {
        self.aid = None;
        self.aid_disabled = true;
    }

    pub fn auto_audio(&mut self) {
        self.aid_disabled = false;
        if self.aid.is_none() {
            self.aid = Some(1);
        }
        if let Some(shared) = &self.shared
            && let Some(id) = self.aid
        {
            let _ = shared.mutate(|backend| backend.set_property("aid", NativeValue::Int(id)));
        }
    }

    pub fn select_sub(&mut self, id: Option<i64>) -> Result<(), String> {
        let Some(shared) = &self.shared else {
            return Ok(());
        };
        shared
            .mutate(|backend| {
                backend.set_property(
                    "sid",
                    id.map(NativeValue::Int).unwrap_or(NativeValue::Bool(false)),
                )
            })
            .ok_or_else(|| "engine backend lock failed".to_owned())?
    }

    pub fn select_sub_id(&mut self, id: i64) -> Result<(), String> {
        self.select_sub(Some(id))
    }

    pub fn select_sub_name(&mut self, name: &str) -> Result<(), String> {
        let Some(shared) = &self.shared else {
            return Ok(());
        };
        shared
            .mutate(|backend| {
                backend.set_property("sid", NativeValue::String(name.to_owned()))
            })
            .ok_or_else(|| "engine backend lock failed".to_owned())?
    }

    pub fn set_display(&mut self, width: u32, height: u32) {
        self.display = Some((width, height));
        if let Some(shared) = &self.shared {
            shared.mutate(|backend| backend.set_display_size(width, height));
        }
    }

    pub fn set_osc_visible(&mut self) {
        if let Some(shared) = &self.shared {
            let _ = shared.mutate(|backend| {
                backend.command(CommandRequest::Args(vec![
                    "script-message".to_owned(),
                    "osc-visibility".to_owned(),
                    "cycle".to_owned(),
                ]))
            });
        }
    }

    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    fn unload(&mut self) {
        self.host = None;
        self.sender = None;
        self.shared = None;
        self.probe = None;
        self.end_sent = false;
        self.eof_reached = false;
        self.pending_pause = None;
    }

    fn spawn_osc(&mut self) {
        let Some(shared) = self.shared.clone() else {
            return;
        };
        if !self.options.osc {
            return;
        }
        match ScriptHost::spawn_with_modules(
            "osc",
            "@osc.lua",
            OSC,
            vec![
                ScriptModule::new("mp.assdraw", ASSDRAW),
                ScriptModule::new("mp.options", OPTIONS),
            ],
            shared,
        ) {
            Ok(host) => {
                self.sender = Some(host.event_sender().sender());
                self.host = Some(host);
            }
            Err(_) => {
                self.sender = None;
                self.host = None;
            }
        }
    }

    pub fn load_current(&mut self) -> Result<(), String> {
        self.unload();
        self.defer_event(ipc::event(
            "start-file",
            vec![("playlist_entry_id".to_owned(), NativeValue::Int(1))],
        ));
        let Some(entry) = self.playlist.current_entry().cloned() else {
            return Ok(());
        };
        let probe = properties::probe(&entry.path)
            .map_err(|error| format!("failed to open {}: {error}", entry.path))?;
        let has_audio = probe
            .streams
            .iter()
            .any(|stream| stream.kind == player_media::StreamKind::Audio);
        let has_video = probe
            .streams
            .iter()
            .any(|stream| stream.kind == player_media::StreamKind::Video);
        let sink = if self.options.null_audio || self.options.no_audio {
            SinkKind::Null
        } else {
            SinkKind::Default
        };
        let engine = if self.options.tui || self.options.no_video {
            SchedulerEngine::load_audio_only(&entry.path, sink)
        } else if self.options.null_video {
            SchedulerEngine::load_null_video(&entry.path, sink)
        } else {
            SchedulerEngine::load(&entry.path, sink)
        };
        let engine = match engine {
            Ok(engine) => engine,
            Err(error) => {
                if has_video {
                    self.defer_event(ipc::event("video-reconfig", Vec::new()));
                    self.defer_event(ipc::event("video-reconfig", Vec::new()));
                }
                return Err(format!("failed to open {}: {error}", entry.path));
            }
        };
        if has_audio {
            self.defer_event(ipc::event("audio-reconfig", Vec::new()));
            self.defer_event(ipc::event("audio-reconfig", Vec::new()));
        }
        let mut backend = EngineBackend::open(engine);
        if self.options.null_video || self.options.no_video || self.gui {
            backend.set_overlay_mode();
        }
        backend.set_display_size(
            self.display.map(|(width, _)| width).unwrap_or(1280),
            self.display.map(|(_, height)| height).unwrap_or(720),
        );
        self.shared = Some(SharedEngine::new(backend));
        self.probe = Some(probe);
        self.paused = self.options.pause;
        self.volume = self.options.volume;
        self.muted = self.options.muted;
        self.eof_reached = false;
        self.end_sent = false;
        self.idle_sent = false;
        self.pending_failure = None;
        self.aid = self
            .probe
            .as_ref()
            .and_then(|probe| {
                probe
                    .streams
                    .iter()
                    .filter(|stream| stream.kind == player_media::StreamKind::Audio)
                    .position(|_| true)
            })
            .map(|_| 1);
        if !self.options.tui {
            self.spawn_osc();
        }
        if self.paused {
            self.set_paused(true)?;
        }
        self.set_volume(self.volume as f32);
        self.set_muted(self.muted);
        self.set_speed(self.speed);
        self.apply_fullscreen();
        if let Some(shared) = &self.shared {
            for file in &self.options.sub_files {
                let result = shared
                    .mutate(|backend| {
                        backend.command(CommandRequest::Args(vec![
                            "sub-add".to_owned(),
                            file.clone(),
                        ]))
                    })
                    .ok_or_else(|| "engine backend lock failed".to_owned())?;
                if let Err(message) = result {
                    eprintln!("player: cannot load subtitle {file}: {message}");
                }
            }
        }
        match self.options.start {
            Start::Absolute(seconds) => self.seek_absolute(seconds),
            Start::Percent(percent) => {
                let duration = self
                    .probe
                    .as_ref()
                    .map(|probe| probe.duration)
                    .unwrap_or(0.0);
                self.seek_absolute(duration * percent / 100.0);
            }
            Start::Relative(seconds) => self.seek_absolute(seconds.max(0.0)),
            Start::Unset => {}
        }
        self.log_streams();
        self.defer_event(ipc::event("file-loaded", Vec::new()));
        let attached_video = self.probe.as_ref().is_some_and(|probe| {
            probe
                .streams
                .iter()
                .any(|stream| stream.attached && stream.kind == player_media::StreamKind::Video)
        });
        if attached_video {
            self.defer_event(ipc::event("video-reconfig", Vec::new()));
            if has_audio {
                self.defer_event(ipc::event("audio-reconfig", Vec::new()));
            }
            self.defer_event(ipc::event("video-reconfig", Vec::new()));
        } else {
            if has_audio {
                self.defer_event(ipc::event("audio-reconfig", Vec::new()));
            }
            if has_video {
                self.defer_event(ipc::event("video-reconfig", Vec::new()));
                self.defer_event(ipc::event("video-reconfig", Vec::new()));
            }
        }
        Ok(())
    }

    fn log_streams(&self) {
        if self.options.quiet || self.options.tui {
            return;
        }
        let Some(probe) = &self.probe else {
            return;
        };
        if probe.streams.iter().any(|stream| stream.kind == player_media::StreamKind::Video) {
            println!("VO: [null]");
        }
        if let Some(audio) = probe
            .streams
            .iter()
            .find(|stream| stream.kind == player_media::StreamKind::Audio)
        {
            println!("AO: [null]");
            println!("Using Symphonia for {}", audio.codec_name);
        }
    }

    pub fn seek_absolute(&mut self, seconds: f64) {
        if let Some(shared) = &self.shared {
            let _ = shared.mutate(|backend| {
                backend.command(CommandRequest::Args(vec![
                    "seek".to_owned(),
                    format!("{:.6}", seconds.max(0.0)),
                    "absolute".to_owned(),
                ]))
            });
        }
        self.send_event(ipc::event("seek", Vec::new()));
        self.send_event(ipc::event("playback-restart", Vec::new()));
        self.send_event(ipc::event("playback-restart", Vec::new()));
    }

    pub fn seek_relative(&mut self, seconds: f64) {
        if let Some(shared) = &self.shared {
            let _ = shared.mutate(|backend| {
                backend.command(CommandRequest::Args(vec![
                    "seek".to_owned(),
                    format!("{seconds:.6}"),
                    "relative".to_owned(),
                ]))
            });
        }
        self.send_event(ipc::event("seek", Vec::new()));
        self.send_event(ipc::event("playback-restart", Vec::new()));
        self.send_event(ipc::event("playback-restart", Vec::new()));
    }

    fn defer_eof_reconfigs(&mut self) {
        let (audio, video) = self
            .probe
            .as_ref()
            .map(|probe| {
                (
                    probe
                        .streams
                        .iter()
                        .any(|stream| stream.kind == player_media::StreamKind::Audio),
                    probe
                        .streams
                        .iter()
                        .any(|stream| stream.kind == player_media::StreamKind::Video),
                )
            })
            .unwrap_or((false, false));
        if audio {
            self.defer_event(ipc::event("audio-reconfig", Vec::new()));
        }
        if video {
            self.defer_event(ipc::event("video-reconfig", Vec::new()));
        }
    }

    fn loadfile(&mut self, path: &str, mode: &str) -> Result<(), String> {
        let entry = playlist::local_entry(path)?;
        match mode {
            "append" | "append-play" => {
                self.playlist.append(vec![entry]);
                if mode == "append-play" && self.shared.is_none() {
                    self.playlist.set_current(Some(self.playlist.count() - 1));
                    self.load_current()?;
                }
            }
            _ if mode.starts_with("insert-at:") => {
                let index = mode["insert-at:".len()..]
                    .parse::<usize>()
                    .map_err(|_| "bad insert index".to_owned())?;
                if self.playlist.count() == 0 {
                    self.playlist.push(entry);
                } else {
                    self.playlist.insert(index, vec![entry]);
                }
            }
            "insert-next" => {
                let index = self.playlist.current().map(|index| index + 1).unwrap_or(0);
                self.playlist.insert(index, vec![entry]);
            }
            _ => {
                self.playlist.clear();
                self.playlist.push(entry);
                self.playlist.set_current(Some(0));
                self.load_current()?;
            }
        }
        Ok(())
    }

    fn loadlist(&mut self, path: &str, mode: &str) -> Result<(), String> {
        let entries = playlist::read_playlist(path)?;
        match mode {
            "append" => self.playlist.append(entries),
            _ => {
                self.playlist.clear();
                self.playlist.append(entries);
                self.playlist.set_current(Some(0));
                self.load_current()?;
            }
        }
        Ok(())
    }

    pub fn run(&mut self) -> Result<(), String> {
        if let Some(path) = &self.options.input_conf {
            self.bindings = read_bindings(path)?;
        }
        if let Some(path) = &self.options.log_file {
            std::fs::File::create(path)
                .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        }
        let bundled = std::env::var("MPVBUNDLE").as_deref() == Ok("true");
        if self.options.files.is_empty() && self.options.playlists.is_empty() {
            if self.options.idle || self.options.tui || bundled {
                self.options.idle = true;
            } else {
                print!("{}", options::help());
                return Ok(());
            }
        }
        for item in self.options.items.clone() {
            match item {
                options::Item::File(file) => {
                    let extension = Path::new(&file)
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if matches!(extension.as_str(), "m3u" | "m3u8" | "pls") {
                        self.playlist.append(playlist::read_playlist(&file)?);
                    } else {
                        self.playlist.push(playlist::local_entry(&file)?);
                    }
                }
                options::Item::Playlist(list) => {
                    self.playlist.append(playlist::read_playlist(&list)?);
                }
            }
        }
        if self.options.gui_gate {
            self.options.gui = true;
            self.options.keep_open = true;
        }
        let wants_gui = self.options.gui
            || bundled
            || (!self.options.tui && !self.options.null_video && !self.options.no_video);
        if wants_gui && (self.playlist.count() > 0 || bundled) {
            self.open_window()?;
            self.gui = true;
        }
        if self.playlist.count() > 0 {
            self.playlist.set_current(Some(0));
            self.load_current()?;
        }
        if let Some(path) = &self.options.ipc {
            self.ipc = Some(IpcServer::bind(path)?);
        }
        if self.options.tui {
            self.options.quiet = true;
            let result = player_tui::run(self);
            self.shutdown();
            return result;
        }
        let mut last_status = Instant::now();
        loop {
            if self.quit {
                break;
            }
            let messages = self.ipc.as_mut().map(IpcServer::poll).unwrap_or_default();
            if !messages.is_empty() {
                for message in messages {
                    self.handle_ipc(message);
                }
                self.deliver_observers();
            }
            if self.gui {
                self.present_window()?;
            } else {
                self.tick()?;
            }
            if self.shared.is_none() && !self.options.idle && !self.options.tui {
                break;
            }
            if last_status.elapsed() >= Duration::from_millis(50) {
                last_status = Instant::now();
                self.deliver_observers();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.shutdown();
        Ok(())
    }

    fn open_files(&mut self, paths: Vec<String>, append: bool) -> Result<(), String> {
        let mut entries = Vec::new();
        for path in paths {
            let extension = Path::new(&path)
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(extension.as_str(), "m3u" | "m3u8" | "pls") {
                entries.extend(playlist::read_playlist(&path)?);
            } else {
                entries.push(playlist::local_entry(&path)?);
            }
        }
        if entries.is_empty() {
            return Ok(());
        }
        if append && self.playlist.count() > 0 {
            for entry in entries {
                self.playlist.push(entry);
            }
            if self.shared.is_none() {
                let index = self.playlist.count() - 1;
                self.playlist.set_current(Some(index));
                self.load_current()?;
            }
            return Ok(());
        }
        self.playlist = Playlist::new();
        for entry in entries {
            self.playlist.push(entry);
        }
        self.playlist.set_current(Some(0));
        if let Err(error) = self.load_current() {
            eprintln!("player: {error}");
            self.unload();
        }
        Ok(())
    }

    fn open_window(&mut self) -> Result<(), String> {
        let (width, height) = self.options.geometry.unwrap_or((1280, 720));
        let title = self
            .playlist
            .current_entry()
            .and_then(|entry| Path::new(&entry.path).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("player");
        let mut window = HostWindow::open(title, width.max(64), height.max(64))?;
        if self.options.fullscreen {
            window.set_fullscreen(true);
        }
        self.display = Some((window.width().max(1) as u32, window.height().max(1) as u32));
        self.window = Some(window);
        Ok(())
    }

    fn present_window(&mut self) -> Result<(), String> {
        let events = {
            let Some(window) = self.window.as_mut() else {
                return self.tick();
            };
            if window.poll()? {
                self.quit = true;
                return Ok(());
            }
            window.drain_input()?
        };
        for event in events {
            match event {
                HostEvent::Key { name, pressed } => {
                    if !pressed {
                        continue;
                    }
                    if name == "ESCAPE" {
                        if self.fullscreen {
                            self.keypress("f")?;
                        } else {
                            self.quit = true;
                        }
                    } else {
                        self.keypress(&name)?;
                    }
                }
                HostEvent::PointerMotion { x, y } => self.mouse_move(x, y)?,
                HostEvent::PointerButton { name, pressed } => {
                    if pressed {
                        self.keypress(&name)?;
                    }
                }
                HostEvent::PointerLeave => {
                    self.script_message_to("osc", &["osc-hide".to_owned()])?;
                }
                HostEvent::OpenFiles { paths, append } => {
                    self.open_files(paths, append)?;
                }
            }
        }
        self.tick()?;
        let wanted = self.fullscreen;
        if wanted != self.applied_fullscreen
            && let Some(window) = self.window.as_mut()
        {
            window.set_fullscreen(wanted);
            self.applied_fullscreen = wanted;
        }
        let new_frames = self.frame_serial != self.presented_serial;
        let (width, height) = self
            .window
            .as_ref()
            .map(|window| (window.width().max(1), window.height().max(1)))
            .unwrap_or((1, 1));
        let size = (width, height);
        let now = Instant::now();
        let mut osd_changed = false;
        if size != self.osd_size || now.duration_since(self.last_osd) >= Duration::from_millis(50) {
            self.last_osd = now;
            let rendered = self
                .shared
                .as_ref()
                .and_then(|shared| {
                    shared.mutate(|backend| {
                        backend.set_display_size(width as u32, height as u32);
                        backend
                            .render_overlay(width as u32, height as u32, 1.0)
                            .ok()
                            .flatten()
                    })
                })
                .flatten();
            let hash = rendered.as_deref().map(hash_bytes).unwrap_or(0);
            osd_changed = hash != self.osd_hash || size != self.osd_size;
            self.osd = rendered;
            self.osd_size = size;
            self.osd_hash = hash;
        }
        if let Some(frame) = self.last_frame.as_ref()
            && (new_frames || osd_changed || size != self.presented_size)
        {
            let overlay = self
                .osd
                .as_ref()
                .filter(|data| data.len() == width as usize * height as usize * 4)
                .map(|data| Overlay {
                    data,
                    stride: width as usize * 4,
                    width,
                    height,
                    dst: OverlayRect {
                        x0: 0.0,
                        y0: 0.0,
                        x1: width as f32,
                        y1: height as f32,
                    },
                });
            if let Some(window) = self.window.as_mut() {
                window.present(frame, overlay.as_ref())?;
                self.presented_serial = self.frame_serial;
                self.presented_size = size;
            }
        }
        if self.options.gui_gate {
            self.gui_gate(width, height)?;
        }
        Ok(())
    }

    fn gui_gate(&mut self, width: i32, height: i32) -> Result<(), String> {
        if self.last_gate_visibility.elapsed() >= Duration::from_millis(200) {
            self.last_gate_visibility = Instant::now();
            let visibility = self
                .shared
                .as_ref()
                .and_then(|shared| shared.inspect(|backend| backend.user_value("user-data/osc/visibility")))
                .flatten();
            if visibility != Some(NativeValue::String("always".to_owned())) {
                self.script_message_to("osc", &["osc-hide".to_owned()])?;
                self.script_message_to("osc", &["osc-visibility".to_owned(), "always".to_owned()])?;
            }
        }
        let ink = self
            .osd
            .as_deref()
            .map(|data| data.chunks_exact(4).filter(|pixel| pixel[3] > 0).count())
            .unwrap_or(0);
        let status = self.status();
        if self.last_gate_report.elapsed() >= Duration::from_secs(1) {
            self.last_gate_report = Instant::now();
            let frames = self.window.as_ref().map(HostWindow::presented).unwrap_or(0);
            eprintln!(
                "player-gui gate: frames={frames} drained={} pos={:.2} ended={} error={:?} \
                 dims={}x{} tracks={} osd={}",
                status.presented,
                status.position,
                status.ended,
                status.error,
                status.video_width,
                status.video_height,
                status.tracks.len(),
                self.osd.as_ref().map(Vec::len).unwrap_or(0),
            );
        }
        let frames = self.window.as_ref().map(HostWindow::presented).unwrap_or(0);
        let played_to_end = status.ended || status.position >= 2.5;
        if frames >= GATE_FRAMES && ink >= GATE_OVERLAY_INK && played_to_end {
            if self.gate_ready_at.is_none() {
                self.gate_ready_at = Some(Instant::now());
            } else if self
                .gate_ready_at
                .is_some_and(|ready| ready.elapsed() >= Duration::from_millis(200))
            {
                println!("player-gui gate: PASS frames={frames} overlay_ink={ink}");
                self.quit = true;
            }
        }
        if self.presented_size != (width, height) && self.gate_ready_at.is_some() {
            self.gate_ready_at = None;
        }
        Ok(())
    }

    fn shutdown(&mut self) {
        if !self.options.quiet {
            println!("Exiting... (Quit)");
        }
        self.send_event(ipc::event("shutdown", Vec::new()));
        self.unload();
    }

    pub fn tick(&mut self) -> Result<(), String> {
        self.flush_deferred();
        if let Some(message) = self.pending_failure.take() {
            eprintln!("player: {message}");
            self.defer_event(ipc::event(
                "end-file",
                vec![
                    ("reason".to_owned(), NativeValue::String("error".to_owned())),
                    ("playlist_entry_id".to_owned(), NativeValue::Int(1)),
                ],
            ));
            self.end_sent = true;
        }
        let Some(shared) = self.shared.clone() else {
            if self.options.idle && !self.idle_sent {
                self.defer_event(ipc::event("idle", Vec::new()));
                self.idle_sent = true;
            }
            return Ok(());
        };
        if let Some(sender) = &self.sender {
            shared
                .pump(sender)
                .map_err(|error| format!("engine pump failed: {error}"))?;
        } else {
            shared.pump_frames();
        }
        if let Some(host) = self.host.as_mut()
            && let Some(result) = host.poll()
        {
            result.map_err(|error| format!("osc stopped: {error}"))?;
        }
        if let Some(frames) = shared.mutate(|backend| backend.take_frames())
            && let Some(frame) = frames.into_iter().last()
        {
            self.last_frame = Some(frame);
            self.frame_serial += 1;
        }
        let status = shared
            .inspect(|backend| backend.status())
            .unwrap_or_default();
        self.paused = match self.pending_pause {
            Some(target) if status.paused == target => {
                self.pending_pause = None;
                target
            }
            Some(_) => self.paused,
            None => status.paused,
        };
        self.volume = status.volume as f64 * 100.0;
        self.muted = status.muted;
        self.speed = status.speed;
        self.fullscreen = shared
            .inspect(|backend| backend.fullscreen())
            .unwrap_or(self.fullscreen);
        if status.ended && !self.end_sent {
            self.end_sent = true;
            let reason = "eof";
            self.eof_reached = true;
            if status.error.is_some() {
                self.defer_event(ipc::event("playback-restart", Vec::new()));
            }
            self.defer_eof_reconfigs();
            self.defer_event(ipc::event(
                "end-file",
                vec![
                    ("reason".to_owned(), NativeValue::String(reason.to_owned())),
                    ("playlist_entry_id".to_owned(), NativeValue::Int(1)),
                ],
            ));
            if let Some(error) = status.error.as_deref() {
                eprintln!("player: {error}");
            }
            if self.options.keep_open {
                self.set_paused(true)?;
            } else if let Some(next) = self.playlist.next_index() {
                self.playlist.set_current(Some(next));
                self.load_current()?;
            } else {
                self.unload();
            }
        }
        Ok(())
    }

    fn deliver_observers(&mut self) {
        let names: Vec<(u64, String)> = self
            .observers
            .iter()
            .map(|(id, observer)| (*id, observer.name.clone()))
            .collect();
        for (id, name) in names {
            let Ok(value) = properties::get(self, &name) else {
                continue;
            };
            let changed = self
                .observers
                .get(&id)
                .and_then(|observer| observer.last.as_ref())
                != Some(&value);
            if changed {
                if let Some(observer) = self.observers.get_mut(&id) {
                    observer.last = Some(value.clone());
                }
                self.send_event(ipc::event(
                    "property-change",
                    vec![
                        ("id".to_owned(), NativeValue::Int(id as i64)),
                        ("name".to_owned(), NativeValue::String(name)),
                        ("data".to_owned(), value),
                    ],
                ));
            }
        }
    }

    fn send_event(&mut self, event: NativeValue) {
        if let Some(ipc) = self.ipc.as_mut() {
            ipc.send(&event);
        }
    }

    fn defer_event(&mut self, event: NativeValue) {
        self.deferred_events.push(event);
    }

    fn flush_deferred(&mut self) {
        let ready = std::mem::take(&mut self.ready_events);
        for event in ready {
            self.send_event(event);
        }
        self.ready_events = std::mem::take(&mut self.deferred_events);
    }

    fn handle_ipc(&mut self, message: NativeValue) {
        let NativeValue::Map(fields) = message else {
            return;
        };
        let request_id = fields.iter().find_map(|(name, value)| {
            if name == "request_id" {
                match value {
                    NativeValue::Int(id) => Some(*id),
                    _ => None,
                }
            } else {
                None
            }
        });
        if fields.iter().any(|(name, _)| name == "invalid") {
            return;
        }
        let Some((_, command)) = fields.iter().find(|(name, _)| name == "command") else {
            return;
        };
        let args = match command {
            NativeValue::Array(values) => values
                .iter()
                .filter_map(|value| match value {
                    NativeValue::String(text) => Some(text.clone()),
                    NativeValue::Int(number) => Some(number.to_string()),
                    NativeValue::Double(number) => Some(number.to_string()),
                    NativeValue::Bool(value) => Some(value.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            NativeValue::String(text) => {
                text.split_whitespace().map(str::to_owned).collect()
            }
            _ => return,
        };
        let result = self.command(&args);
        if let Some(request_id) = request_id {
            self.send_event(ipc::reply(request_id, result));
        }
    }

    fn command(&mut self, args: &[String]) -> Result<NativeValue, String> {
        match player_core::command::parse_args(args) {
            Ok(Some(command)) => {
                self.execute_core(command)?;
                return Ok(NativeValue::None);
            }
            Err(error) => return Err(error.to_string()),
            Ok(None) => {}
        }
        let Some((name, rest)) = args.split_first() else {
            return Err("empty command".to_owned());
        };
        match name.as_str() {
            "get_property" => {
                let property = rest.first().ok_or_else(|| "get_property needs a name".to_owned())?;
                properties::get(self, property)
            }
            "set_property" => {
                let property = rest.first().ok_or_else(|| "set_property needs a name".to_owned())?;
                let value = rest.get(1).cloned().unwrap_or_default();
                let value = parse_value(&value);
                properties::set(self, property, &value)?;
                Ok(NativeValue::None)
            }
            "set" => {
                let property = rest.first().ok_or_else(|| "set needs a property".to_owned())?;
                let value = rest.get(1).ok_or_else(|| "set needs a value".to_owned())?;
                properties::set(self, property, &parse_value(value))?;
                Ok(NativeValue::None)
            }
            "loadfile" => {
                let path = rest.first().ok_or_else(|| "loadfile needs a path".to_owned())?;
                let mode = rest.get(1).map(String::as_str).unwrap_or("replace");
                if let Err(message) = self.loadfile(path, mode) {
                    self.unload();
                    self.pending_failure = Some(message);
                }
                Ok(NativeValue::None)
            }
            "loadlist" => {
                let path = rest.first().ok_or_else(|| "loadlist needs a path".to_owned())?;
                let mode = rest.get(1).map(String::as_str).unwrap_or("replace");
                self.loadlist(path, mode)?;
                Ok(NativeValue::None)
            }
            "stop" => {
                self.unload();
                self.idle_sent = false;
                Ok(NativeValue::None)
            }
            "quit" | "quit-watch-later" => {
                self.quit = true;
                Ok(NativeValue::None)
            }
            "seek" => {
                let target = rest.first().ok_or_else(|| "seek needs a target".to_owned())?;
                let seconds = parse_time(target)?;
                let mode = rest.get(1).map(String::as_str).unwrap_or("relative");
                if mode.starts_with("absolute") {
                    self.seek_absolute(seconds);
                } else {
                    self.seek_relative(seconds);
                }
                Ok(NativeValue::None)
            }
            "cycle" => {
                let property = rest.first().ok_or_else(|| "cycle needs a property".to_owned())?;
                match property.as_str() {
                    "pause" => self.set_paused(!self.paused)?,
                    "mute" => self.set_muted(!self.muted),
                    "fullscreen" => {
                        self.fullscreen = !self.fullscreen;
                        self.apply_fullscreen();
                    }
                    "sub" => {
                        let status = self.status();
                        let subtitles: Vec<_> = status
                            .tracks
                            .iter()
                            .filter(|track| track.kind == player_engine::TrackKind::Subtitle)
                            .collect();
                        let current = subtitles.iter().position(|track| track.selected);
                        let next = match current {
                            Some(index) if index + 1 < subtitles.len() => Some(index + 1),
                            Some(_) => None,
                            None => Some(0),
                        };
                        let id = next
                            .and_then(|index| subtitles.get(index))
                            .map(|track| track.id as i64);
                        self.select_sub(id)?;
                    }
                    _ => return Err(format!("cannot cycle '{property}'")),
                }
                Ok(NativeValue::None)
            }
            "add" => {
                let property = rest.first().ok_or_else(|| "add needs a property".to_owned())?;
                let delta: f64 = rest
                    .get(1)
                    .ok_or_else(|| "add needs a delta".to_owned())?
                    .parse()
                    .map_err(|_| "invalid delta".to_owned())?;
                if property == "volume" {
                    self.set_volume((self.volume + delta) as f32);
                }
                if property == "speed" {
                    self.set_speed(self.speed + delta);
                }
                Ok(NativeValue::None)
            }
            "frame-step" => {
                self.set_paused(true)?;
                self.seek_relative(1.0 / 24.0);
                Ok(NativeValue::None)
            }
            "frame-back-step" => {
                self.set_paused(true)?;
                self.seek_relative(-1.0 / 24.0);
                Ok(NativeValue::None)
            }
            "observe_property" => {
                let id: i64 = rest
                    .first()
                    .ok_or_else(|| "observe_property needs an id".to_owned())?
                    .parse()
                    .map_err(|_| "invalid observer id".to_owned())?;
                let property = rest
                    .get(1)
                    .ok_or_else(|| "observe_property needs a name".to_owned())?;
                self.observers.insert(
                    id as u64,
                    Observer {
                        name: property.clone(),
                        last: None,
                    },
                );
                Ok(NativeValue::None)
            }
            "unobserve_property" => {
                let id: i64 = rest
                    .first()
                    .ok_or_else(|| "unobserve_property needs an id".to_owned())?
                    .parse()
                    .map_err(|_| "invalid observer id".to_owned())?;
                self.observers.remove(&(id as u64));
                Ok(NativeValue::None)
            }
            "playlist-next" => {
                if let Some(next) = self.playlist.next_index() {
                    self.playlist.set_current(Some(next));
                    self.load_current()?;
                }
                Ok(NativeValue::None)
            }
            "playlist-prev" => {
                if let Some(previous) = self.playlist.previous_index() {
                    self.playlist.set_current(Some(previous));
                    self.load_current()?;
                }
                Ok(NativeValue::None)
            }
            "playlist-remove" => {
                let index: usize = rest
                    .first()
                    .ok_or_else(|| "playlist-remove needs an index".to_owned())?
                    .parse()
                    .map_err(|_| "invalid index".to_owned())?;
                self.playlist.remove(index);
                Ok(NativeValue::None)
            }
            "playlist-move" => {
                let from: usize = rest
                    .first()
                    .ok_or_else(|| "playlist-move needs a source".to_owned())?
                    .parse()
                    .map_err(|_| "invalid index".to_owned())?;
                let to: usize = rest
                    .get(1)
                    .ok_or_else(|| "playlist-move needs a target".to_owned())?
                    .parse()
                    .map_err(|_| "invalid index".to_owned())?;
                self.playlist.move_entry(from, to);
                Ok(NativeValue::None)
            }
            "playlist-clear" => {
                self.playlist.clear();
                Ok(NativeValue::None)
            }
            "playlist-play-index" => {
                let index: usize = rest
                    .first()
                    .ok_or_else(|| "playlist-play-index needs an index".to_owned())?
                    .parse()
                    .map_err(|_| "invalid index".to_owned())?;
                if index < self.playlist.count() {
                    self.playlist.set_current(Some(index));
                    self.load_current()?;
                }
                Ok(NativeValue::None)
            }
            "keypress" => {
                if let Some(key) = rest.first() {
                    self.keypress(key)?;
                }
                Ok(NativeValue::None)
            }
            "script-message" => {
                self.script_message(rest)?;
                Ok(NativeValue::None)
            }
            "script-message-to" => {
                let Some((target, payload)) = rest.split_first() else {
                    return Err("script-message-to needs a target".to_owned());
                };
                self.script_message_to(target, payload)?;
                Ok(NativeValue::None)
            }
            "script-binding" => {
                let binding = rest
                    .first()
                    .ok_or_else(|| "script-binding needs a name".to_owned())?;
                let (_, name) = binding
                    .split_once('/')
                    .ok_or_else(|| format!("invalid script binding '{binding}'"))?;
                self.post_script_message(&[
                    "key-binding".to_owned(),
                    name.to_owned(),
                    "d".to_owned(),
                ])?;
                Ok(NativeValue::None)
            }
            "mouse" => {
                let x: f64 = rest
                    .first()
                    .ok_or_else(|| "mouse needs an x coordinate".to_owned())?
                    .parse()
                    .map_err(|_| "invalid mouse x".to_owned())?;
                let y: f64 = rest
                    .get(1)
                    .ok_or_else(|| "mouse needs a y coordinate".to_owned())?
                    .parse()
                    .map_err(|_| "invalid mouse y".to_owned())?;
                self.mouse_move(x, y)?;
                Ok(NativeValue::None)
            }
            "sub-add" => {
                let path = rest.first().ok_or_else(|| "sub-add needs a path".to_owned())?;
                if let Some(shared) = &self.shared {
                    shared
                        .mutate(|backend| {
                            backend.command(CommandRequest::Args(vec![
                                "sub-add".to_owned(),
                                path.clone(),
                            ]))
                        })
                        .ok_or_else(|| "engine backend lock failed".to_owned())??;
                }
                Ok(NativeValue::None)
            }
            "show-text" => {
                if let Some(shared) = &self.shared {
                    let mut argv = vec!["show-text".to_owned()];
                    argv.extend(rest.iter().cloned());
                    shared
                        .mutate(|backend| backend.command(CommandRequest::Args(argv)))
                        .ok_or_else(|| "engine backend lock failed".to_owned())??;
                }
                Ok(NativeValue::None)
            }
            "screenshot" => self.write_screenshot(None),
            "screenshot-to-file" => {
                let path = rest
                    .first()
                    .ok_or_else(|| "screenshot-to-file needs a path".to_owned())?;
                self.write_screenshot(Some(path))
            }
            "screenshot-raw" => self.screenshot_raw(),
            _ => Err(format!("unknown command '{name}'")),
        }
    }

    fn write_screenshot(&mut self, path: Option<&str>) -> Result<NativeValue, String> {
        let frame = self
            .last_frame
            .as_ref()
            .ok_or_else(|| "no video frame is available for a screenshot".to_owned())?;
        let png = crate::png::encode_png(&frame.rgba, frame.width, frame.height)?;
        let target = match path {
            Some(path) => Path::new(path).to_path_buf(),
            None => {
                self.screenshot_index += 1;
                Path::new(&format!("shot{:04}.png", self.screenshot_index)).to_path_buf()
            }
        };
        std::fs::write(&target, png)
            .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
        Ok(NativeValue::None)
    }

    fn screenshot_raw(&self) -> Result<NativeValue, String> {
        let frame = self
            .last_frame
            .as_ref()
            .ok_or_else(|| "no video frame is available for a screenshot".to_owned())?;
        Ok(NativeValue::Map(vec![
            ("w".to_owned(), NativeValue::Int(frame.width as i64)),
            ("h".to_owned(), NativeValue::Int(frame.height as i64)),
            (
                "stride".to_owned(),
                NativeValue::Int(frame.width as i64 * 4),
            ),
            ("format".to_owned(), NativeValue::String("rgba".to_owned())),
            ("data".to_owned(), NativeValue::Bytes(frame.rgba.clone())),
        ]))
    }

    fn keypress(&mut self, key: &str) -> Result<(), String> {
        if let Some(text) = self.bindings.get(key).cloned() {
            if let Err(message) = self.run_binding(&text) {
                eprintln!("player: {message}");
            }
            return Ok(());
        }
        if let Some((_, name)) = self.section_binding(key)? {
            let mouse_button = key.to_ascii_lowercase().starts_with("mbtn");
            if mouse_button {
                self.post_script_message(&[
                    "key-binding".to_owned(),
                    name.clone(),
                    "dm".to_owned(),
                ])?;
                self.post_script_message(&[
                    "key-binding".to_owned(),
                    name,
                    "um".to_owned(),
                ])?;
            } else {
                self.post_script_message(&[
                    "key-binding".to_owned(),
                    name,
                    "d".to_owned(),
                ])?;
            }
            return Ok(());
        }
        match key {
            "SPACE" => self.set_paused(!self.paused)?,
            "f" => {
                self.fullscreen = !self.fullscreen;
                self.apply_fullscreen();
            }
            "m" => self.set_muted(!self.muted),
            "[" => self.set_speed(self.speed - 0.1),
            "]" => self.set_speed(self.speed + 0.1),
            "." => {
                self.command(&["frame-step".to_owned()])?;
            }
            "q" => self.quit = true,
            "o" => self.set_osc_visible(),
            "j" => {
                self.command(&["cycle".to_owned(), "sub".to_owned()])?;
            }
            "RIGHT" => self.seek_relative(5.0),
            "LEFT" => self.seek_relative(-5.0),
            "UP" => self.set_volume((self.volume + 2.0) as f32),
            "DOWN" => self.set_volume((self.volume - 2.0) as f32),
            "9" => self.set_volume((self.volume + 2.0) as f32),
            "0" => self.set_volume((self.volume - 2.0) as f32),
            _ => {}
        }
        Ok(())
    }

    fn run_binding(&mut self, text: &str) -> Result<(), String> {
        let mut commands = Vec::new();
        for part in split_commands(text)? {
            let command = parse_string(&part)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("unknown command '{part}'"))?;
            commands.push(command);
        }
        for command in commands {
            self.execute_core(command)?;
        }
        Ok(())
    }

    fn execute_core(&mut self, command: CoreCommand) -> Result<(), String> {
        match command {
            CoreCommand::Pause(toggle) => {
                self.set_paused(toggle_value(toggle, self.paused))?;
            }
            CoreCommand::Fullscreen(toggle) => {
                self.fullscreen = toggle_value(toggle, self.fullscreen);
                self.apply_fullscreen();
            }
            CoreCommand::Mute(toggle) => {
                self.set_muted(toggle_value(toggle, self.muted));
            }
            CoreCommand::Seek { amount, mode, .. } => match mode {
                player_core::command::SeekMode::Absolute => self.seek_absolute(amount),
                player_core::command::SeekMode::AbsolutePercent => {
                    let duration = self.probe.as_ref().map_or(0.0, |probe| probe.duration);
                    self.seek_absolute(duration * amount / 100.0);
                }
                player_core::command::SeekMode::RelativePercent => {
                    let duration = self.probe.as_ref().map_or(0.0, |probe| probe.duration);
                    self.seek_relative(duration * amount / 100.0);
                }
                player_core::command::SeekMode::Relative => self.seek_relative(amount),
            },
            CoreCommand::FrameStep(direction) => {
                self.set_paused(true)?;
                self.seek_relative(direction as f64 / 24.0);
            }
            CoreCommand::Volume(adjust) => {
                let volume = match adjust {
                    Adjust::Set(value) => value,
                    Adjust::Add(delta) => self.volume + delta,
                };
                self.set_volume(volume as f32);
            }
            CoreCommand::Speed(adjust) => {
                let speed = match adjust {
                    Adjust::Set(value) => value,
                    Adjust::Add(delta) => self.speed + delta,
                };
                self.set_speed(speed);
            }
            CoreCommand::Stop => self.unload(),
            CoreCommand::Quit => self.quit = true,
            CoreCommand::LoadFile { path, mode } => {
                let mode = match mode {
                    player_core::command::LoadMode::Replace => "replace".to_owned(),
                    player_core::command::LoadMode::Append => "append".to_owned(),
                    player_core::command::LoadMode::InsertNext => "insert-next".to_owned(),
                    player_core::command::LoadMode::InsertAt(index) => {
                        format!("insert-at:{index}")
                    }
                };
                if let Err(message) = self.loadfile(&path, &mode) {
                    self.unload();
                    self.pending_failure = Some(message);
                }
            }
            CoreCommand::PlaylistNext => {
                if let Some(next) = self.playlist.next_index() {
                    self.playlist.set_current(Some(next));
                    self.load_current()?;
                }
            }
            CoreCommand::PlaylistPrevious => {
                if let Some(previous) = self.playlist.previous_index() {
                    self.playlist.set_current(Some(previous));
                    self.load_current()?;
                }
            }
            CoreCommand::Screenshot => {
                self.write_screenshot(None)?;
            }
            CoreCommand::SubtitleAdd(path) => {
                if let Some(shared) = &self.shared {
                    shared
                        .mutate(|backend| {
                            backend.command(CommandRequest::Args(vec![
                                "sub-add".to_owned(),
                                path.clone(),
                            ]))
                        })
                        .ok_or_else(|| "engine backend lock failed".to_owned())??;
                }
            }
            CoreCommand::SelectTrack { kind, id } => match kind {
                CoreTrackKind::Audio => match id {
                    Some(id) => self.select_audio(id)?,
                    None => self.disable_audio(),
                },
                CoreTrackKind::Subtitle => match id {
                    Some(id) => self.select_sub_id(id)?,
                    None => self.select_sub(None)?,
                },
                CoreTrackKind::Video => {
                    return Err("video track selection is not supported".to_owned());
                }
            },
            CoreCommand::SelectTrackAuto(kind) => match kind {
                CoreTrackKind::Audio => self.auto_audio(),
                CoreTrackKind::Subtitle => self.select_sub(None)?,
                CoreTrackKind::Video => {}
            },
            CoreCommand::CycleTrack(kind) => match kind {
                CoreTrackKind::Audio => self.cycle_audio()?,
                CoreTrackKind::Subtitle => {
                    self.command(&["cycle".to_owned(), "sub".to_owned()])?;
                }
                CoreTrackKind::Video => {}
            },
        }
        Ok(())
    }

    fn cycle_audio(&mut self) -> Result<(), String> {
        let status = self.status();
        let audios: Vec<_> = status
            .tracks
            .iter()
            .filter(|track| track.kind == player_engine::TrackKind::Audio)
            .collect();
        if audios.is_empty() {
            return Err("no audio tracks".to_owned());
        }
        let current = self
            .aid
            .and_then(|id| audios.iter().position(|track| track.id as i64 == id));
        let next = match current {
            Some(index) if index + 1 < audios.len() => Some(index + 1),
            Some(_) if self.aid_disabled => None,
            Some(_) => None,
            None => Some(0),
        };
        match next.and_then(|index| audios.get(index)) {
            Some(track) => self.select_audio(track.id as i64),
            None => {
                self.disable_audio();
                Ok(())
            }
        }
    }

    fn script_message(&mut self, args: &[String]) -> Result<(), String> {
        if let Some(shared) = &self.shared {
            let mut argv = vec!["script-message".to_owned()];
            argv.extend(args.iter().cloned());
            shared
                .mutate(|backend| backend.command(CommandRequest::Args(argv)))
                .ok_or_else(|| "engine backend lock failed".to_owned())??;
        }
        Ok(())
    }

    fn script_message_to(&mut self, _target: &str, args: &[String]) -> Result<(), String> {
        self.post_script_message(args)
    }

    fn post_script_message(&mut self, args: &[String]) -> Result<(), String> {
        let Some(host) = self.host.as_ref() else {
            return Ok(());
        };
        let args = NativeValue::Array(
            args.iter()
                .map(|value| NativeValue::String(value.clone()))
                .collect(),
        );
        host.post_event(ScriptEvent::new("client-message").with_field("args", args))
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn tui_pump(&mut self) {
        let messages = self.ipc.as_mut().map(IpcServer::poll).unwrap_or_default();
        if !messages.is_empty() {
            for message in messages {
                self.handle_ipc(message);
            }
            self.deliver_observers();
        }
        if let Err(error) = self.tick() {
            eprintln!("player: {error}");
            self.quit = true;
        }
    }

    fn playlist_next(&mut self) -> Result<(), String> {
        if let Some(next) = self.playlist.next_index() {
            self.playlist.set_current(Some(next));
            self.load_current()?;
        }
        Ok(())
    }

    fn playlist_previous(&mut self) -> Result<(), String> {
        if let Some(previous) = self.playlist.previous_index() {
            self.playlist.set_current(Some(previous));
            self.load_current()?;
        }
        Ok(())
    }

    fn mouse_move(&mut self, x: f64, y: f64) -> Result<(), String> {
        if let Some(shared) = &self.shared {
            shared.mutate(|backend| backend.set_mouse_pos(x, y));
        }
        if let Some((_, name)) = self.section_binding("mouse_move")? {
            self.post_script_message(&[
                "key-binding".to_owned(),
                name,
                "pm".to_owned(),
            ])?;
        }
        Ok(())
    }

    fn section_binding(&self, key: &str) -> Result<Option<(String, String)>, String> {
        let Some(shared) = &self.shared else {
            return Ok(None);
        };
        let sections = shared
            .inspect(|backend| backend.sections())
            .unwrap_or_default();
        for (section, contents, enabled) in sections {
            if !enabled {
                continue;
            }
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((bound, command)) = line.split_once(char::is_whitespace) else {
                    continue;
                };
                if !bound.eq_ignore_ascii_case(key) {
                    continue;
                }
                let Some(binding) = command.trim().strip_prefix("script-binding ") else {
                    continue;
                };
                let (_, name) = binding
                    .split_once('/')
                    .ok_or_else(|| format!("invalid script binding '{binding}'"))?;
                return Ok(Some((section, name.to_owned())));
            }
        }
        Ok(None)
    }

    pub fn observe_property(&mut self, id: u64, name: &str) {
        self.observers.insert(
            id,
            Observer {
                name: name.to_owned(),
                last: None,
            },
        );
    }
}

impl TuiPlayer for App {
    fn snapshot(&mut self) -> Snapshot {
        self.tui_pump();
        let status = self.status();
        let path = self
            .playlist
            .current_entry()
            .map(|entry| entry.path.clone())
            .unwrap_or_default();
        let probe = self.probe.as_ref();
        let title = metadata_value(probe, "title").unwrap_or_else(|| {
            Path::new(&path)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "NO FILE".to_owned())
        });
        let artist = ["artist", "album_artist", "albumartist"]
            .iter()
            .find_map(|key| metadata_value(probe, key))
            .unwrap_or_default();
        let album = metadata_value(probe, "album").unwrap_or_default();
        let year = ["date", "year"]
            .iter()
            .find_map(|key| metadata_value(probe, key))
            .map(|date| date.chars().take(4).collect())
            .unwrap_or_default();
        let format = probe
            .map(|probe| probe.format.clone())
            .unwrap_or_default();
        let duration = probe.map(|probe| probe.duration).unwrap_or(0.0);
        Snapshot {
            path: path.clone(),
            track: Track {
                title,
                artist,
                album,
                year,
                format,
            },
            pos: status.position,
            duration,
            playing: !self.paused && !path.is_empty(),
            volume: status.volume as f64,
            muted: status.muted,
        }
    }

    fn control(&mut self, control: Control) -> Result<(), String> {
        match control {
            Control::Pause => self.set_paused(!self.paused),
            Control::Seek(delta) => {
                self.seek_relative(delta as f64);
                Ok(())
            }
            Control::Volume(delta) => {
                self.set_volume((self.volume + delta as f64).clamp(0.0, 100.0) as f32);
                Ok(())
            }
            Control::Mute => {
                self.set_muted(!self.muted);
                Ok(())
            }
            Control::Next => self.playlist_next(),
            Control::Previous => self.playlist_previous(),
            Control::Restart => {
                self.seek_absolute(0.0);
                Ok(())
            }
            Control::Quit => {
                self.quit = true;
                Ok(())
            }
        }
    }

    fn cover_art(&mut self, path: &str) -> Option<Cover> {
        crate::art::cover_art(path)
    }

    fn shutdown(&mut self) -> bool {
        self.quit
    }
}

fn metadata_value(probe: Option<&Probe>, name: &str) -> Option<String> {
    probe.and_then(|probe| {
        probe
            .metadata
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
            .filter(|value| !value.is_empty())
    })
}

fn parse_value(value: &str) -> NativeValue {
    match value {
        "yes" | "true" => NativeValue::Bool(true),
        "no" | "false" => NativeValue::Bool(false),
        _ => match value.parse::<i64>() {
            Ok(number) => NativeValue::Int(number),
            Err(_) => match value.parse::<f64>() {
                Ok(number) => NativeValue::Double(number),
                Err(_) => NativeValue::String(value.to_owned()),
            },
        },
    }
}

fn parse_time(value: &str) -> Result<f64, String> {
    if let Some(percent) = value.strip_suffix('%') {
        return percent
            .parse()
            .map_err(|_| "invalid percent".to_owned());
    }
    if let Some(rest) = value.strip_prefix('+') {
        return rest.parse().map_err(|_| "invalid time".to_owned());
    }
    if let Some(rest) = value.strip_prefix('-') {
        return Ok(-rest.parse::<f64>().map_err(|_| "invalid time".to_owned())?);
    }
    value.parse().map_err(|_| "invalid time".to_owned())
}

fn hash_bytes(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn toggle_value(toggle: Toggle, current: bool) -> bool {
    match toggle {
        Toggle::Toggle => !current,
        Toggle::Set(value) => value,
    }
}

fn read_bindings(path: &Path) -> Result<HashMap<String, String>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut bindings = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, command)) = line.split_once(char::is_whitespace) {
            let command = command.trim();
            if !command.is_empty() {
                bindings.insert(key.to_owned(), command.to_owned());
            }
        }
    }
    Ok(bindings)
}

fn split_commands(text: &str) -> Result<Vec<String>, String> {
    let mut commands = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => {
                let command = text[start..index].trim();
                if !command.is_empty() {
                    commands.push(command.to_owned());
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted {
        return Err("unterminated quote".to_owned());
    }
    let command = text[start..].trim();
    if !command.is_empty() {
        commands.push(command.to_owned());
    }
    Ok(commands)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let options = match options::parse(args) {
        Ok(options) => options,
        Err(message) if message == "__version__" => {
            println!("player mpv-compatible 0.41.0");
            return Ok(());
        }
        Err(message) if message == "__help__" => {
            print!("{}", options::help());
            return Ok(());
        }
        Err(message) => return Err(message),
    };
    let mut app = App::new(options);
    app.run()
}

pub fn entry(path: &str) -> Result<Entry, String> {
    playlist::local_entry(path)
}
