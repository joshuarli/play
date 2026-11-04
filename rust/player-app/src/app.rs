use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use player_audio::sink::SinkKind;
use player_core::command::{
    parse_string, Adjust, Command as CoreCommand, OscVisibility, OscVisibilityChange, Toggle,
    TrackKind as CoreTrackKind,
};
use player_core::input::{classify_input, InputPath};
use player_engine::video::PresentedFrame;
use player_engine::{EngineStatus, SchedulerEngine, StartState, TrackKind};
use player_backend::engine::{EngineBackend, OverlayImage, DEFAULT_OSD_DURATION};
use player_output::{Overlay, OverlayRect};
use player_tui::{Control, Cover, Player as TuiPlayer, Snapshot, Track};

use crate::ipc::{self, IpcServer};
use crate::options::{self, Options, Start};
use crate::osc::{Osc, Playback, Response};
use crate::playlist::{self, Entry, Playlist};
use crate::properties::{self, Probe};
use crate::value::NativeValue;
use crate::window::{HostEvent, HostWindow};

const GATE_FRAMES: u64 = 24;
const GATE_OVERLAY_INK: usize = 100;
const OVERLAY_MIN_INTERVAL: Duration = Duration::from_millis(16);
const SUBTITLE_INTERVAL: Duration = Duration::from_millis(50);

struct Observer {
    name: String,
    last: Option<NativeValue>,
}

pub struct App {
    pub options: Options,
    pub playlist: Playlist,
    pub probe: Option<Probe>,
    pub backend: Option<EngineBackend>,
    pub(crate) osc: Option<Osc>,
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
    pub window_scale: f64,
    pub(crate) window: Option<HostWindow>,
    pub(crate) windowed_display: Option<(u32, u32)>,
    pub(crate) logical_display: Option<(u32, u32)>,
    last_frame: Option<PresentedFrame>,
    frame_serial: u64,
    presented_serial: u64,
    frames_shown: u64,
    osd: Option<OverlayImage>,
    osd_size: (i32, i32),
    last_osd: Instant,
    rendered_osc: Option<u64>,
    rendered_message: bool,
    osd_error: Option<String>,
    gui: bool,
    applied_fullscreen: bool,
    presented_size: (i32, i32),
    gate_ready_at: Option<Instant>,
    last_gate_report: Instant,
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
        let fullscreen = options.fullscreen.unwrap_or(false);
        let osc = (options.osc && !options.tui)
            .then(|| Osc::new(OscVisibility::Auto, Instant::now()));
        App {
            paused: options.pause,
            speed: 1.0,
            volume: options.volume,
            muted: options.muted,
            fullscreen,
            aid: None,
            aid_disabled: false,
            options,
            playlist: Playlist::new(),
            probe: None,
            backend: None,
            osc,
            ipc: None,
            eof_reached: false,
            display: None,
            window_scale: 1.0,
            window: None,
            windowed_display: None,
            logical_display: None,
            last_frame: None,
            frame_serial: 0,
            presented_serial: 0,
            frames_shown: 0,
            osd: None,
            osd_size: (0, 0),
            last_osd: Instant::now()
                .checked_sub(SUBTITLE_INTERVAL)
                .unwrap_or_else(Instant::now),
            rendered_osc: None,
            rendered_message: false,
            osd_error: None,
            gui: false,
            applied_fullscreen: fullscreen,
            presented_size: (0, 0),
            gate_ready_at: None,
            last_gate_report: Instant::now(),
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
        self.backend
            .as_ref()
            .map(EngineBackend::status)
            .unwrap_or_default()
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<(), String> {
        self.paused = paused;
        self.pending_pause = Some(paused);
        if let Some(backend) = &self.backend {
            backend.set_paused(paused);
        }
        Ok(())
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.01, 100.0);
        if let Some(backend) = &self.backend {
            backend.set_speed(self.speed);
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 200.0) as f64;
        if let Some(backend) = &self.backend {
            backend.set_volume_percent(self.volume);
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(backend) = &self.backend {
            backend.set_muted(muted);
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
        if let Some(backend) = &self.backend {
            backend.select_audio_id(id)?;
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
        if let Some(backend) = &self.backend
            && let Some(id) = self.aid
            && let Err(message) = backend.select_audio_id(id)
        {
            eprintln!("player: {message}");
        }
    }

    pub fn select_sub(&mut self, id: Option<i64>) -> Result<(), String> {
        let Some(backend) = &self.backend else {
            return Ok(());
        };
        match id {
            Some(id) => backend.select_sub_id(id),
            None => {
                backend.deselect_sub();
                Ok(())
            }
        }
    }

    pub fn select_first_sub(&mut self) {
        if let Some(backend) = &self.backend {
            backend.select_first_sub();
        }
    }

    pub fn set_sub_visible(&mut self, visible: bool) {
        if let Some(backend) = &mut self.backend {
            backend.set_sub_visible(visible);
        }
    }

    pub fn set_display(&mut self, width: u32, height: u32) {
        self.display = Some((width, height));
    }

    pub fn show_text(&mut self, text: &str, duration: Duration) {
        if let Some(backend) = &mut self.backend {
            backend.show_text(text, duration);
        }
    }

    pub fn set_osc_visibility(&mut self, change: OscVisibilityChange) -> Result<(), String> {
        let osc = self
            .osc
            .as_mut()
            .ok_or_else(|| "the OSC is disabled".to_owned())?;
        let mode = match change {
            OscVisibilityChange::Cycle => osc.visibility().next(),
            OscVisibilityChange::Set(mode) => mode,
        };
        osc.set_visibility(mode, Instant::now());
        self.show_text(&format!("OSC visibility: {}", mode.name()), DEFAULT_OSD_DURATION);
        Ok(())
    }

    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn dropped_frames(&self) -> u64 {
        let status = self.status();
        let unshown = if self.gui {
            status.presented.saturating_sub(self.frames_shown)
        } else {
            0
        };
        status.dropped_late + unshown
    }

    fn unload(&mut self) {
        self.frames_shown = 0;
        self.backend = None;
        self.probe = None;
        self.end_sent = false;
        self.eof_reached = false;
        self.pending_pause = None;
    }

    pub fn load_current(&mut self) -> Result<(), String> {
        if !self.gui
            && !self.options.tui
            && !self.options.null_video
            && !self.options.no_video
            && self.window.is_none()
        {
            self.open_window()?;
            self.gui = true;
        }
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
        } else if let Some(device) = &self.options.audio_device {
            let device = device.strip_prefix("alsa/").unwrap_or(device);
            SinkKind::Device(device.to_owned())
        } else {
            SinkKind::Default
        };
        let start = StartState {
            volume: (self.options.volume / 100.0) as f32,
            muted: self.options.muted,
            paused: self.options.pause,
        };
        let engine = if self.options.tui || self.options.no_video {
            SchedulerEngine::load_audio_only(&entry.path, sink, start)
        } else if self.options.null_video {
            SchedulerEngine::load_null_video(&entry.path, sink, start)
        } else {
            SchedulerEngine::load(&entry.path, sink, start)
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
        self.backend = Some(EngineBackend::open(engine));
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
        if let Some(backend) = &self.backend {
            match self.options.sid.as_deref() {
                Some("no") => {}
                Some(value) if value != "auto" => {
                    if let Ok(id) = value.parse::<i64>()
                        && id > 0
                    {
                        backend.select_sub_position(id as usize - 1);
                    }
                }
                _ => backend.select_sub_position(0),
            }
        }
        if let Some(osc) = &mut self.osc {
            osc.start_file(Instant::now());
        }
        if !self.options.quiet {
            println!("Playing: {}", entry.path);
        }
        if self.paused {
            self.set_paused(true)?;
        }
        self.set_volume(self.volume as f32);
        self.set_muted(self.muted);
        self.set_speed(self.speed);
        if let Some(backend) = &self.backend {
            for file in &self.options.sub_files {
                if let Err(message) = backend.add_sub(file) {
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
        if let Some(template) = &self.options.term_playing_msg {
            println!("{}", expand_message(self, template));
        }
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
        if probe
            .streams
            .iter()
            .any(|stream| stream.kind == player_media::StreamKind::Video)
        {
            let output = if self.options.null_video || self.options.no_video {
                "null"
            } else {
                "gpu-next"
            };
            println!("VO: [{output}]");
        }
        if self.options.no_audio {
            return;
        }
        if let Some(audio) = probe
            .streams
            .iter()
            .find(|stream| stream.kind == player_media::StreamKind::Audio)
        {
            let output = if self.options.null_audio {
                "null"
            } else if cfg!(target_os = "macos") {
                "coreaudio"
            } else {
                "alsa"
            };
            println!("AO: [{output}]");
            if let Some(device) = self
                .options
                .audio_device
                .as_deref()
                .and_then(|device| device.strip_prefix("alsa/"))
            {
                println!("[ao/alsa] opening device '{device}'");
            }
            println!("Using Symphonia for {}", audio.codec_name);
        }
    }

    pub fn seek_absolute(&mut self, seconds: f64) {
        if let Some(backend) = &self.backend {
            backend.seek_to(seconds.max(0.0));
        }
        self.send_event(ipc::event("seek", Vec::new()));
        self.send_event(ipc::event("playback-restart", Vec::new()));
        self.send_event(ipc::event("playback-restart", Vec::new()));
    }

    pub fn seek_relative(&mut self, seconds: f64) {
        if let Some(backend) = &self.backend {
            backend.seek_to(backend.status().position + seconds);
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
        let entries = playlist::entries_for_path(path)?;
        match mode {
            "append" | "append-play" => {
                let first = self.playlist.count();
                self.playlist.append(entries);
                if mode == "append-play" && self.backend.is_none() {
                    self.playlist.set_current(Some(first));
                    self.load_current()?;
                }
            }
            _ if mode.starts_with("insert-at:") => {
                let index = mode["insert-at:".len()..]
                    .parse::<usize>()
                    .map_err(|_| "bad insert index".to_owned())?;
                if self.playlist.count() == 0 {
                    self.playlist.append(entries);
                } else {
                    self.playlist.insert(index, entries);
                }
            }
            "insert-next" => {
                let index = self.playlist.current().map(|index| index + 1).unwrap_or(0);
                self.playlist.insert(index, entries);
            }
            _ => {
                self.playlist.clear();
                self.playlist.append(entries);
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
        self.options.sub_files = local_inputs(std::mem::take(&mut self.options.sub_files));
        self.options.audio_files = local_inputs(std::mem::take(&mut self.options.audio_files));
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
                    self.playlist.append(playlist::entries_for_path(&file)?);
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
            if self.backend.is_none() && !self.options.idle && !self.options.tui {
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
            entries.extend(playlist::entries_for_path(&path)?);
        }
        if entries.is_empty() {
            return Ok(());
        }
        if append && self.playlist.count() > 0 {
            for entry in entries {
                self.playlist.push(entry);
            }
            if self.backend.is_none() {
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
        let fullscreen = self
            .options
            .fullscreen
            .unwrap_or(self.options.geometry.is_none());
        if fullscreen {
            window.set_fullscreen(true);
            self.fullscreen = true;
            self.applied_fullscreen = true;
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
                HostEvent::PointerButton { pressed } => self.pointer_button(pressed)?,
                HostEvent::Wheel { up } => self.wheel(up)?,
                HostEvent::PointerLeave => {
                    if let Some(osc) = &mut self.osc {
                        osc.pointer_leave(Instant::now());
                    }
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
            if wanted {
                self.windowed_display = self.display;
                self.logical_display = None;
            } else {
                self.logical_display = self.windowed_display;
            }
            window.set_fullscreen(wanted);
            self.applied_fullscreen = wanted;
        }
        let new_frames = self.frame_serial != self.presented_serial;
        let actual = self
            .window
            .as_ref()
            .map(|window| (window.width().max(1), window.height().max(1)))
            .unwrap_or((1, 1));
        if self.fullscreen {
            self.logical_display = None;
        }
        if let Some(logical) = self.logical_display
            && actual == (logical.0 as i32, logical.1 as i32)
        {
            self.logical_display = None;
        }
        let (width, height) = self
            .logical_display
            .map(|(width, height)| (width as i32, height as i32))
            .unwrap_or(actual);
        let size = (width, height);
        if self.display != Some((width as u32, height as u32)) {
            self.set_display(width as u32, height as u32);
        }
        let osd_changed = self.render_window_overlay(width as u32, height as u32);
        if let Some(frame) = self.last_frame.as_ref()
            && (new_frames || osd_changed || size != self.presented_size)
        {
            let overlay = self.osd.as_ref().map(|image| Overlay {
                data: &image.rgba,
                stride: image.width as usize * 4,
                width: image.width as i32,
                height: image.height as i32,
                dst: OverlayRect {
                    x0: image.x as f32,
                    y0: image.y as f32,
                    x1: (image.x + image.width) as f32,
                    y1: (image.y + image.height) as f32,
                },
            });
            if let Some(window) = self.window.as_mut() {
                window.present(frame, overlay.as_ref())?;
                if new_frames {
                    self.frames_shown += 1;
                }
                self.presented_serial = self.frame_serial;
                self.presented_size = size;
            }
        }
        if self.options.gui_gate {
            self.gui_gate(width, height)?;
        }
        Ok(())
    }

    fn render_window_overlay(&mut self, width: u32, height: u32) -> bool {
        let Some(backend) = self.backend.as_mut() else {
            return self.osd.take().is_some();
        };
        let size = (width as i32, height as i32);
        let now = Instant::now();
        let since = now.duration_since(self.last_osd);
        if let Some(osc) = &mut self.osc {
            osc.overlay();
        }
        let osc_revision = self.osc.as_ref().map(Osc::revision);
        let message = backend.has_message();
        let due = size != self.osd_size
            || (since >= OVERLAY_MIN_INTERVAL
                && (osc_revision != self.rendered_osc || message != self.rendered_message))
            || (since >= SUBTITLE_INTERVAL && (message || backend.subtitles_selected()));
        if !due {
            return false;
        }
        let osc = self.osc.as_mut().and_then(Osc::overlay);
        let rendered = match backend.render_overlay(width, height, osc) {
            Ok(rendered) => {
                self.osd_error = None;
                rendered
            }
            Err(message) => {
                if self.osd_error.as_deref() != Some(message.as_str()) {
                    eprintln!("player: OSD render failed: {message}");
                    self.osd_error = Some(message);
                }
                None
            }
        };
        self.last_osd = now;
        self.rendered_osc = osc_revision;
        self.rendered_message = message;
        let changed = rendered != self.osd || size != self.osd_size;
        self.osd = rendered;
        self.osd_size = size;
        changed
    }

    fn gui_gate(&mut self, width: i32, height: i32) -> Result<(), String> {
        if let Some(osc) = &mut self.osc
            && osc.visibility() != OscVisibility::Always
        {
            osc.set_visibility(OscVisibility::Always, Instant::now());
        }
        let ink = self
            .osd
            .as_ref()
            .map(|image| image.rgba.chunks_exact(4).filter(|pixel| pixel[3] > 0).count())
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
                self.osd.as_ref().map(|image| image.rgba.len()).unwrap_or(0),
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
        let Some(backend) = self.backend.as_ref() else {
            if self.options.idle && !self.idle_sent {
                self.defer_event(ipc::event("idle", Vec::new()));
                self.idle_sent = true;
            }
            return Ok(());
        };
        if let Some(frame) = backend.latest_frame() {
            self.last_frame = Some(frame);
            self.frame_serial += 1;
        }
        let status = backend.status();
        if self
            .options
            .frames
            .is_some_and(|limit| status.presented >= limit)
        {
            self.quit = true;
        }
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
        self.update_osc(&status);
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

    fn update_osc(&mut self, status: &EngineStatus) {
        let Some(osc) = self.osc.as_mut() else {
            return;
        };
        let count = |kind: TrackKind| status.tracks.iter().filter(|track| track.kind == kind).count();
        let playback = Playback {
            paused: self.paused,
            fullscreen: self.fullscreen,
            muted: self.muted,
            volume: self.volume,
            position: status.position,
            duration: self.probe.as_ref().map_or(status.duration, |probe| probe.duration),
            audio_tracks: count(TrackKind::Audio),
            sub_tracks: count(TrackKind::Subtitle),
            aid: if self.aid_disabled { None } else { self.aid },
            sid: status
                .tracks
                .iter()
                .find(|track| track.kind == TrackKind::Subtitle && track.selected)
                .map(|track| track.id as i64),
        };
        let (width, height) = self.display.unwrap_or((1280, 720));
        osc.update(playback, width, height, Instant::now());
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
                    "fullscreen" => self.fullscreen = !self.fullscreen,
                    "sub" => self.cycle_sub()?,
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
                let key = rest.first().ok_or_else(|| "keypress needs a key".to_owned())?;
                self.keypress(key)?;
                Ok(NativeValue::None)
            }
            "keydown" | "keyup" => {
                let key = rest
                    .first()
                    .ok_or_else(|| format!("{name} needs a key"))?;
                if !key.eq_ignore_ascii_case("MBTN_LEFT") {
                    return Err(format!("{name} supports MBTN_LEFT only"));
                }
                self.pointer_button(name == "keydown")?;
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
            "mouse-leave" => {
                if let Some(osc) = &mut self.osc {
                    osc.pointer_leave(Instant::now());
                }
                Ok(NativeValue::None)
            }
            "sub-add" => {
                let path = rest.first().ok_or_else(|| "sub-add needs a path".to_owned())?;
                if let Some(backend) = &self.backend {
                    backend.add_sub(path)?;
                }
                Ok(NativeValue::None)
            }
            "show-text" => {
                let text = rest
                    .first()
                    .ok_or_else(|| "show-text needs a text argument".to_owned())?;
                let duration = match rest.get(1).map(|value| value.parse::<i64>()) {
                    None => DEFAULT_OSD_DURATION,
                    Some(Ok(milliseconds)) if milliseconds < 0 => DEFAULT_OSD_DURATION,
                    Some(Ok(milliseconds)) => Duration::from_millis(milliseconds as u64),
                    Some(Err(_)) => return Err("show-text duration is not an integer".to_owned()),
                };
                self.show_text(text, duration);
                Ok(NativeValue::None)
            }
            "screenshot" => self.write_screenshot(None),
            "screenshot-to-file" => {
                let path = rest
                    .first()
                    .ok_or_else(|| "screenshot-to-file needs a path".to_owned())?;
                let content = match rest.get(1).map(String::as_str) {
                    None | Some("video") => ScreenshotContent::Video,
                    Some("window") => ScreenshotContent::Window,
                    Some(other) => {
                        return Err(format!("screenshot-to-file flags are video or window, not '{other}'"));
                    }
                };
                self.write_screenshot_with(Some(path), content)
            }
            _ => Err(format!("unknown command '{name}'")),
        }
    }

    fn write_screenshot(&mut self, path: Option<&str>) -> Result<NativeValue, String> {
        self.write_screenshot_with(path, ScreenshotContent::Video)
    }

    fn write_screenshot_with(
        &mut self,
        path: Option<&str>,
        content: ScreenshotContent,
    ) -> Result<NativeValue, String> {
        let frame = self
            .last_frame
            .as_ref()
            .ok_or_else(|| "no video frame is available for a screenshot".to_owned())?;
        let (width, height) = (frame.width, frame.height);
        let mut rgba = frame.rgba.clone();
        if content == ScreenshotContent::Window {
            let backend = self
                .backend
                .as_mut()
                .ok_or_else(|| "no file is loaded".to_owned())?;
            let osc = self.osc.as_mut().and_then(Osc::overlay);
            backend.compose(&mut rgba, width, height, osc)?;
        }
        let png = crate::png::encode_png(&rgba, width, height)?;
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

    fn keypress(&mut self, key: &str) -> Result<(), String> {
        if key.eq_ignore_ascii_case("MBTN_LEFT") {
            self.pointer_button(true)?;
            return self.pointer_button(false);
        }
        if key.eq_ignore_ascii_case("WHEEL_UP") {
            return self.wheel(true);
        }
        if key.eq_ignore_ascii_case("WHEEL_DOWN") {
            return self.wheel(false);
        }
        if self.run_bound_key(key) {
            return Ok(());
        }
        match key {
            "SPACE" => self.set_paused(!self.paused)?,
            "f" => self.fullscreen = !self.fullscreen,
            "m" => self.set_muted(!self.muted),
            "[" => self.set_speed(self.speed - 0.1),
            "]" => self.set_speed(self.speed + 0.1),
            "." => {
                self.command(&["frame-step".to_owned()])?;
            }
            "q" => self.quit = true,
            "o" => {
                if self.osc.is_some() {
                    self.set_osc_visibility(OscVisibilityChange::Cycle)?;
                }
            }
            "j" => self.cycle_sub()?,
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

    fn run_bound_key(&mut self, key: &str) -> bool {
        let Some(text) = self.bindings.get(key).cloned() else {
            return false;
        };
        if let Err(message) = self.run_binding(&text) {
            eprintln!("player: {message}");
        }
        true
    }

    fn pointer_button(&mut self, pressed: bool) -> Result<(), String> {
        let response = self
            .osc
            .as_mut()
            .map_or(Response::Pass, |osc| osc.button(pressed));
        match response {
            Response::Command(command) => self.execute_core(command),
            Response::Pass if pressed => {
                self.run_bound_key("MBTN_LEFT");
                Ok(())
            }
            Response::Pass | Response::Consumed => Ok(()),
        }
    }

    fn wheel(&mut self, up: bool) -> Result<(), String> {
        let response = self.osc.as_mut().map_or(Response::Pass, |osc| osc.wheel(up));
        match response {
            Response::Command(command) => self.execute_core(command),
            Response::Pass => {
                self.run_bound_key(if up { "WHEEL_UP" } else { "WHEEL_DOWN" });
                Ok(())
            }
            Response::Consumed => Ok(()),
        }
    }

    fn mouse_move(&mut self, x: f64, y: f64) -> Result<(), String> {
        let response = self
            .osc
            .as_mut()
            .map_or(Response::Pass, |osc| osc.pointer_move(x, y, Instant::now()));
        match response {
            Response::Command(command) => self.execute_core(command),
            Response::Pass | Response::Consumed => Ok(()),
        }
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
                if let Some(backend) = &self.backend {
                    backend.add_sub(&path)?;
                }
            }
            CoreCommand::SelectTrack { kind, id } => match kind {
                CoreTrackKind::Audio => match id {
                    Some(id) => self.select_audio(id)?,
                    None => self.disable_audio(),
                },
                CoreTrackKind::Subtitle => match id {
                    Some(id) => self.select_sub(Some(id))?,
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
                CoreTrackKind::Subtitle => self.cycle_sub()?,
                CoreTrackKind::Video => {}
            },
            CoreCommand::OscVisibility(change) => self.set_osc_visibility(change)?,
        }
        Ok(())
    }

    fn cycle_sub(&mut self) -> Result<(), String> {
        let status = self.status();
        let subtitles: Vec<_> = status
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Subtitle)
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
        self.select_sub(id)
    }

    fn cycle_audio(&mut self) -> Result<(), String> {
        let status = self.status();
        let audios: Vec<_> = status
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Audio)
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

fn expand_message(app: &App, template: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match properties::get(app, name) {
            Ok(NativeValue::String(value)) => out.push_str(&value),
            Ok(NativeValue::Int(value)) => out.push_str(&value.to_string()),
            Ok(NativeValue::Double(value)) => out.push_str(&value.to_string()),
            Ok(NativeValue::Bool(value)) => out.push_str(if value { "yes" } else { "no" }),
            _ => {}
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn local_inputs(paths: Vec<String>) -> Vec<String> {
    let mut kept = Vec::new();
    for path in paths {
        match classify_input(&path, false) {
            Ok(InputPath::Local(path)) => kept.push(path.to_string_lossy().into_owned()),
            Err(error) => eprintln!("{error}"),
        }
    }
    kept
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
    if options.list_protocols {
        println!("file://");
        return Ok(());
    }
    let mut app = App::new(options);
    app.run()
}

pub fn entry(path: &str) -> Result<Entry, String> {
    playlist::local_entry(path)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScreenshotContent {
    Video,
    Window,
}
