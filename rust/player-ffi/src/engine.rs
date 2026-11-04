use crate::osd::{composite, OsdState};
use player_ass::RenderedImage;
use player_engine::{video::PresentedFrame, EngineStatus, SchedulerEngine, TrackKind};
use player_lua::{
    queue::{EventSender, Posted},
    CommandRequest, HostError, NativeValue, PlayerBackend, PropertyFormat, ScriptEvent,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_OSD_DURATION_MS: u64 = 1000;

pub struct EngineBackend {
    engine: SchedulerEngine,
    observed: HashMap<u64, (String, PropertyFormat)>,
    delivered: HashMap<u64, NativeValue>,
    enabled_events: Vec<String>,
    fullscreen: bool,
    display: Option<(u32, u32)>,
    mouse_areas: HashMap<String, (f64, f64, f64, f64)>,
    mouse_pos: (f64, f64),
    mouse_seen: bool,
    sub_visible: bool,
    last_log: Option<(String, String)>,
    user_data: HashMap<String, NativeValue>,
    sections: HashMap<String, InputSection>,
    overlays: HashMap<i64, OsdOverlay>,
    osd_message: Option<(String, String)>,
    osd_message_until: Option<Instant>,
    loopback: Vec<Vec<String>>,
    to_script: Vec<(String, Vec<String>)>,
    ran: Vec<Vec<String>>,
    file_events_sent: bool,
    end_event_sent: bool,
    shutdown_sent: bool,
    quit: bool,
    pending_frames: Vec<PresentedFrame>,
    last_frame: Option<PresentedFrame>,
    osd: Option<OsdState>,
    osd_error: Option<String>,
    compose_frames: bool,
}

pub struct InputSection {
    pub contents: String,
    pub enabled: bool,
}

pub struct OsdOverlay {
    pub id: i64,
    pub format: String,
    pub data: String,
    pub res_x: i64,
    pub res_y: i64,
    pub z: i64,
    pub hidden: bool,
}

impl EngineBackend {
    pub fn open(engine: SchedulerEngine) -> Self {
        EngineBackend {
            engine,
            observed: HashMap::new(),
            delivered: HashMap::new(),
            enabled_events: Vec::new(),
            fullscreen: false,
            display: None,
            mouse_areas: HashMap::new(),
            mouse_pos: (0.0, 0.0),
            mouse_seen: false,
            sub_visible: true,
            last_log: None,
            user_data: HashMap::new(),
            sections: HashMap::new(),
            overlays: HashMap::new(),
            osd_message: None,
            osd_message_until: None,
            loopback: Vec::new(),
            to_script: Vec::new(),
            ran: Vec::new(),
            file_events_sent: false,
            end_event_sent: false,
            shutdown_sent: false,
            quit: false,
            pending_frames: Vec::new(),
            last_frame: None,
            osd: None,
            osd_error: None,
            compose_frames: true,
        }
    }

    pub fn set_display_size(&mut self, width: u32, height: u32) {
        self.display = Some((width, height));
    }

    pub fn select_sub_position(&mut self, position: usize) {
        self.engine.select_sub_position(position);
    }

    /// Hands OSD pixels to the consumer instead of blending them into video
    /// frames. The GUI host presents the video and the overlay separately so
    /// libplacebo scales each at the window's pixel size.
    pub fn set_overlay_mode(&mut self) {
        self.compose_frames = false;
    }

    pub fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn set_sub_visible(&mut self, visible: bool) {
        self.sub_visible = visible;
        self.engine.set_sub_visible(visible);
    }

    pub fn sub_visible(&self) -> bool {
        self.sub_visible
    }

    pub fn engine_error(&self) -> Option<String> {
        self.engine.snapshot().error
    }

    pub fn status(&self) -> EngineStatus {
        self.engine.snapshot()
    }

    pub fn mouse_area(&self, section: &str) -> Option<(f64, f64, f64, f64)> {
        self.mouse_areas.get(section).copied()
    }

    pub fn set_mouse_pos(&mut self, x: f64, y: f64) {
        self.mouse_pos = (x, y);
        self.mouse_seen = true;
    }

    pub fn sections(&self) -> Vec<(String, String, bool)> {
        self.sections
            .iter()
            .map(|(name, section)| (name.clone(), section.contents.clone(), section.enabled))
            .collect()
    }

    fn output_size(&self, status: &EngineStatus) -> (i64, i64) {
        let (width, height) = self
            .display
            .or(if status.video_width > 0 && status.video_height > 0 {
                Some((status.video_width, status.video_height))
            } else {
                None
            })
            .unwrap_or((0, 0));
        (width as i64, height as i64)
    }

    fn visible_message(&mut self) -> Option<String> {
        if let Some(until) = self.osd_message_until {
            if Instant::now() >= until {
                self.osd_message = None;
                self.osd_message_until = None;
            }
        }
        self.osd_message
            .as_ref()
            .map(|(text, _)| text.clone())
    }

    fn render_images(
        &mut self,
        status: &EngineStatus,
        width: i64,
        height: i64,
        par: f64,
        now_ms: i64,
        message: Option<&str>,
    ) -> Result<Vec<RenderedImage>, String> {
        let mut images = Vec::new();
        let subtitle_selected = status
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle && track.selected);
        if subtitle_selected {
            images.extend(self.engine.render_subtitles(
                width as i32,
                height as i32,
                par,
                now_ms,
            )?);
        }
        let mut visible: Vec<&OsdOverlay> = self
            .overlays
            .values()
            .filter(|overlay| {
                overlay.format == "ass-events" && !overlay.data.is_empty() && !overlay.hidden
            })
            .collect();
        if visible.is_empty() && message.is_none() {
            return Ok(images);
        }
        visible.sort_by_key(|overlay| (overlay.z, overlay.id));
        if self.osd.is_none() {
            self.osd = Some(OsdState::new()?);
        }
        let osd = self.osd.as_mut().expect("overlay renderer starts");
        images.extend(osd.render(&visible, message, width, height, par)?);
        Ok(images)
    }

    fn composite_frames(&mut self, status: &EngineStatus, fresh: &mut Vec<PresentedFrame>) {
        if !self.compose_frames || fresh.is_empty() {
            return;
        }
        let (width, height) = self.output_size(status);
        if width < 1 || height < 1 {
            return;
        }
        let message = self.visible_message();
        let rendered = self.render_images(
            status,
            width,
            height,
            1.0,
            (status.position * 1000.0).round() as i64,
            message.as_deref(),
        );
        match rendered {
            Ok(images) => {
                self.osd_error = None;
                if images.is_empty() {
                    return;
                }
                for frame in fresh.iter_mut() {
                    if frame.width as i64 != width || frame.height as i64 != height {
                        continue;
                    }
                    if let Err(message) =
                        composite(&mut frame.rgba, frame.width, frame.height, &images)
                    {
                        self.osd_error = Some(message);
                        break;
                    }
                }
            }
            Err(message) => self.osd_error = Some(message),
        }
    }

    pub fn compose(
        &mut self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        par: f64,
    ) -> Result<(), String> {
        let status = self.engine.snapshot();
        let message = self.visible_message();
        let images = self.render_images(
            &status,
            width as i64,
            height as i64,
            par,
            (status.position * 1000.0).round() as i64,
            message.as_deref(),
        )?;
        composite(frame, width, height, &images)
    }

    pub fn render_overlay(
        &mut self,
        width: u32,
        height: u32,
        par: f64,
    ) -> Result<Option<Vec<u8>>, String> {
        let status = self.engine.snapshot();
        let message = self.visible_message();
        let images = self.render_images(
            &status,
            width as i64,
            height as i64,
            par,
            (status.position * 1000.0).round() as i64,
            message.as_deref(),
        )?;
        if images.is_empty() {
            return Ok(None);
        }
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        crate::osd::composite_straight(&mut rgba, width, height, &images)?;
        Ok(Some(rgba))
    }

    pub fn osd_error(&self) -> Option<&String> {
        self.osd_error.as_ref()
    }

    pub fn pump_frames(&mut self) {
        let mut fresh = self.engine.drain_frames();
        let status = self.engine.snapshot();
        self.composite_frames(&status, &mut fresh);
        if let Some(frame) = fresh.last() {
            self.last_frame = Some(frame.clone());
        }
        self.pending_frames.extend(fresh);
        while self.pending_frames.len() > 8 {
            self.pending_frames.remove(0);
        }
    }

    pub fn pump(&mut self, sender: &EventSender) -> Result<Posted, HostError> {
        self.pump_frames();
        let watched: Vec<(u64, String, NativeValue)> = self
            .observed
            .iter()
            .map(|(id, (name, _))| {
                let value = self.value(name).unwrap_or(NativeValue::None);
                (*id, name.clone(), value)
            })
            .collect();
        let mut posted = Posted::Queued;
        for (id, name, value) in watched {
            if self.delivered.get(&id) != Some(&value) {
                posted = sender.post(
                    ScriptEvent::new("property-change")
                        .with_field("id", NativeValue::Int(id as i64))
                        .with_field("name", NativeValue::String(name))
                        .with_field("data", value.clone()),
                )?;
                self.delivered.insert(id, value);
            }
        }
        self.post_lifecycle(sender)?;
        Ok(posted)
    }

    fn value(&self, name: &str) -> Result<NativeValue, String> {
        let status = self.engine.snapshot();
        match name {
            "pause" => Ok(NativeValue::Bool(status.paused)),
            "time-pos" => Ok(NativeValue::Double(status.position)),
            "duration" => Ok(NativeValue::Double(status.duration)),
            "speed" => Ok(NativeValue::Double(status.speed)),
            "volume" => Ok(NativeValue::Double(status.volume as f64 * 100.0)),
            "mute" => Ok(NativeValue::Bool(status.muted)),
            "fullscreen" => Ok(NativeValue::Bool(self.fullscreen)),
            "pid" => Ok(NativeValue::Int(std::process::id() as i64)),
            "working-directory" => Ok(NativeValue::String(
                std::env::current_dir()
                    .map(|dir| dir.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )),
            "aid" => Ok(NativeValue::String("auto".to_owned())),
            "sid" => Ok(sid_value(&status)),
            "sub-visibility" => Ok(NativeValue::Bool(self.sub_visible)),
            "track-list" => Ok(track_list_value(&status)),
            "mouse-pos" => Ok(NativeValue::Map(vec![
                ("x".to_owned(), NativeValue::Double(self.mouse_pos.0)),
                ("y".to_owned(), NativeValue::Double(self.mouse_pos.1)),
                ("hover".to_owned(), NativeValue::Bool(self.mouse_seen)),
            ])),
            "osd-dimensions" => {
                let (width, height) = self
                    .display
                    .or(if status.video_width > 0 && status.video_height > 0 {
                        Some((status.video_width, status.video_height))
                    } else {
                        None
                    })
                    .unwrap_or((0, 0));
                let aspect = if height > 0 {
                    width as f64 / height as f64
                } else {
                    0.0
                };
                Ok(NativeValue::Map(vec![
                    ("w".to_owned(), NativeValue::Int(width as i64)),
                    ("h".to_owned(), NativeValue::Int(height as i64)),
                    ("par".to_owned(), NativeValue::Double(1.0)),
                    ("aspect".to_owned(), NativeValue::Double(aspect)),
                    ("mt".to_owned(), NativeValue::Int(0)),
                    ("mb".to_owned(), NativeValue::Int(0)),
                    ("ml".to_owned(), NativeValue::Int(0)),
                    ("mr".to_owned(), NativeValue::Int(0)),
                ]))
            }
            "options/script-opts" => Ok(NativeValue::Map(Vec::new())),
            _ if name.starts_with("user-data/") => self
                .user_data
                .get(name)
                .cloned()
                .ok_or_else(|| format!("unknown property '{name}'")),
            _ => Err(format!("unknown property '{name}'")),
        }
    }

    fn osd_string(&self, name: &str, value: &NativeValue) -> Result<String, String> {
        match (name, value) {
            ("time-pos" | "duration" | "playback-time", NativeValue::Double(seconds)) => {
                let whole = seconds.abs() as i64;
                let format = if whole >= 3600 { "%h:%M:%S" } else { "%M:%S" };
                super::lua::format_time_fmt(format, *seconds)
            }
            ("percent-pos", NativeValue::Double(percent)) => Ok(format!("{:.0}%", percent)),
            ("volume", NativeValue::Double(volume)) => {
                if volume.fract() == 0.0 {
                    Ok(format!("{}", *volume as i64))
                } else {
                    Ok(format!("{volume:.6}"))
                }
            }
            (_, NativeValue::Bool(on)) => Ok(if *on { "yes".to_owned() } else { "no".to_owned() }),
            (_, NativeValue::Int(number)) => Ok(format!("{number}")),
            (_, NativeValue::Double(number)) => Ok(format!("{number:.6}")),
            (_, NativeValue::String(text)) => Ok(text.clone()),
            _ => Err(format!("property '{name}' has no OSD string form")),
        }
    }
}

fn sid_value(status: &EngineStatus) -> NativeValue {
    let subtitles: Vec<&player_engine::TrackInfo> = status
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect();
    if subtitles.is_empty() {
        return NativeValue::String("auto".to_owned());
    }
    match subtitles.iter().find(|track| track.selected) {
        Some(track) => NativeValue::Int(track.id as i64),
        None => NativeValue::Bool(false),
    }
}

fn track_list_value(status: &EngineStatus) -> NativeValue {
    NativeValue::Array(
        status
            .tracks
            .iter()
            .map(|track| {
                let kind = match track.kind {
                    TrackKind::Audio => "audio",
                    TrackKind::Video => "video",
                    TrackKind::Subtitle => "sub",
                };
                let mut fields = vec![
                    ("id".to_owned(), NativeValue::Int(track.id as i64)),
                    ("type".to_owned(), NativeValue::String(kind.to_owned())),
                    ("src-id".to_owned(), NativeValue::Int(track.src_id as i64)),
                    (
                        "codec".to_owned(),
                        NativeValue::String(track.codec.clone()),
                    ),
                    ("selected".to_owned(), NativeValue::Bool(track.selected)),
                    ("external".to_owned(), NativeValue::Bool(track.external)),
                ];
                if let Some(language) = &track.language {
                    fields.push(("lang".to_owned(), NativeValue::String(language.clone())));
                }
                if let Some(title) = &track.title {
                    fields.push(("title".to_owned(), NativeValue::String(title.clone())));
                }
                NativeValue::Map(fields)
            })
            .collect(),
    )
}

impl PlayerBackend for EngineBackend {
    fn log(&mut self, level: &str, message: &str) {
        self.last_log = Some((level.to_owned(), message.to_owned()));
    }

    fn command(&mut self, command: CommandRequest) -> Result<NativeValue, String> {
        match command {
            CommandRequest::String(text) => {
                let args: Vec<String> =
                    text.split_whitespace().map(str::to_owned).collect();
                self.run_argv(&args)
            }
            CommandRequest::Args(args) => self.run_argv(&args),
            CommandRequest::Native(value) => self.run_native(&value),
        }
    }

    fn get_property(
        &mut self,
        name: &str,
        format: PropertyFormat,
    ) -> Result<NativeValue, String> {
        let value = self.value(name)?;
        match format {
            PropertyFormat::Bool => match value {
                NativeValue::Bool(_) => Ok(value),
                _ => Err(format!("property '{name}' is not boolean")),
            },
            PropertyFormat::Number => match value {
                NativeValue::Double(_) => Ok(value),
                NativeValue::Int(number) => Ok(NativeValue::Double(number as f64)),
                _ => Err(format!("property '{name}' is not numeric")),
            },
            PropertyFormat::String => match value {
                NativeValue::Bool(on) => {
                    Ok(NativeValue::String(if on { "yes" } else { "no" }.to_owned()))
                }
                NativeValue::Int(number) => Ok(NativeValue::String(format!("{number}"))),
                NativeValue::Double(number) => Ok(NativeValue::String(format!("{number:.6}"))),
                NativeValue::String(_) => Ok(value),
                _ => Err(format!("property '{name}' has no string form")),
            },
            PropertyFormat::OsdString => Ok(NativeValue::String(self.osd_string(name, &value)?)),
            PropertyFormat::Native | PropertyFormat::None => Ok(value),
        }
    }

    fn set_property(&mut self, name: &str, value: NativeValue) -> Result<(), String> {
        match (name, value) {
            ("pause", NativeValue::Bool(paused)) => {
                self.engine.set_paused(paused);
                Ok(())
            }
            ("volume", NativeValue::Double(volume)) => {
                self.engine.set_volume(volume as f32 / 100.0);
                Ok(())
            }
            ("volume", NativeValue::Int(volume)) => {
                self.engine.set_volume(volume as f32 / 100.0);
                Ok(())
            }
            ("mute", NativeValue::Bool(muted)) => {
                self.engine.set_muted(muted);
                Ok(())
            }
            ("fullscreen", NativeValue::Bool(fullscreen)) => {
                self.fullscreen = fullscreen;
                Ok(())
            }
            ("speed", NativeValue::Double(speed)) => {
                self.engine.set_speed(speed);
                Ok(())
            }
            ("speed", NativeValue::Int(speed)) => {
                self.engine.set_speed(speed as f64);
                Ok(())
            }
            ("sub-visibility", NativeValue::Bool(visible)) => {
                self.sub_visible = visible;
                self.engine.set_sub_visible(visible);
                Ok(())
            }
            ("sid", NativeValue::Int(id)) => self.select_sub_by_id(id),
            ("sid", NativeValue::Bool(false)) => {
                self.engine.select_sub(None);
                Ok(())
            }
            ("sid", NativeValue::Bool(true)) => {
                self.engine.select_sub(None);
                Ok(())
            }
            ("sid", NativeValue::String(name)) => self.select_sub_by_name(&name),
            ("aid", NativeValue::Int(id)) => self.select_audio_by_id(id),
            (name, value) if name.starts_with("user-data/") => {
                self.user_data.insert(name.to_owned(), value);
                Ok(())
            }
            _ => Err(format!("property '{name}' is not settable")),
        }
    }

    fn del_property(&mut self, name: &str) -> Result<(), String> {
        if name.starts_with("user-data/") {
            self.user_data.remove(name);
            return Ok(());
        }
        Err(format!("property '{name}' cannot be deleted"))
    }

    fn observe_property(
        &mut self,
        id: u64,
        name: &str,
        format: PropertyFormat,
    ) -> Result<(), String> {
        self.observed.insert(id, (name.to_owned(), format));
        Ok(())
    }

    fn unobserve_property(&mut self, id: u64) -> i32 {
        self.observed.remove(&id);
        self.delivered.remove(&id);
        self.observed.len() as i32
    }

    fn request_event(&mut self, name: &str, enable: bool) -> bool {
        const KNOWN: [&str; 8] = [
            "shutdown",
            "client-message",
            "command-reply",
            "file-loaded",
            "start-file",
            "end-file",
            "hook",
            "property-change",
        ];
        if !KNOWN.contains(&name) {
            return false;
        }
        if enable {
            if !self.enabled_events.iter().any(|known| known == name) {
                self.enabled_events.push(name.to_owned());
            }
        } else {
            self.enabled_events.retain(|known| known != name);
        }
        true
    }

    fn find_config_file(&mut self, _name: &str) -> Option<String> {
        None
    }

    fn format_time(&mut self, seconds: f64, format: &str) -> Result<String, String> {
        super::lua::format_time_fmt(format, seconds)
    }

    fn input_set_section_mouse_area(
        &mut self,
        section: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) {
        self.mouse_areas
            .insert(section.to_owned(), (x0, y0, x1, y1));
    }
}

impl EngineBackend {
    pub fn section(&self, name: &str) -> Option<&InputSection> {
        self.sections.get(name)
    }

    pub fn overlay(&self, id: i64) -> Option<&OsdOverlay> {
        self.overlays.get(&id)
    }

    pub fn osd_message(&self) -> Option<&(String, String)> {
        self.osd_message.as_ref()
    }

    pub fn user_value(&self, name: &str) -> Option<NativeValue> {
        self.user_data.get(name).cloned()
    }

    pub fn observed_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.observed.values().map(|(name, _)| name.clone()).collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn enabled_events(&self) -> Vec<String> {
        let mut names = self.enabled_events.clone();
        names.sort();
        names
    }

    pub fn script_outbox(&self) -> &[(String, Vec<String>)] {
        &self.to_script
    }

    pub fn ran(&self) -> &[Vec<String>] {
        &self.ran
    }

    pub fn take_frames(&mut self) -> Vec<PresentedFrame> {
        std::mem::take(&mut self.pending_frames)
    }

    fn enabled(&self, name: &str) -> bool {
        self.enabled_events.iter().any(|known| known == name)
    }

    fn screenshot_raw(&mut self, flags: &str) -> Result<NativeValue, String> {
        let mut frame = self
            .last_frame
            .clone()
            .ok_or_else(|| "no video frame is available for a screenshot".to_owned())?;
        if flags.contains("osd") || flags.contains("subtitles") {
            self.compose(&mut frame.rgba, frame.width, frame.height, 1.0)?;
        }
        Ok(NativeValue::Map(vec![
            ("w".to_owned(), NativeValue::Int(frame.width as i64)),
            ("h".to_owned(), NativeValue::Int(frame.height as i64)),
            (
                "stride".to_owned(),
                NativeValue::Int(frame.width as i64 * 4),
            ),
            ("format".to_owned(), NativeValue::String("rgba".to_owned())),
            ("data".to_owned(), NativeValue::Bytes(frame.rgba)),
        ]))
    }

    fn run_argv(&mut self, args: &[String]) -> Result<NativeValue, String> {
        let Some((name, rest)) = args.split_first() else {
            return Err("empty command".to_owned());
        };
        match name.as_str() {
            "screenshot-raw" => self.screenshot_raw(""),
            "seek" => {
                let seconds: f64 = rest
                    .first()
                    .and_then(|text| text.parse().ok())
                    .ok_or_else(|| "seek needs a seconds argument".to_owned())?;
                let mode = rest.get(1).map(String::as_str).unwrap_or("relative");
                if mode.starts_with("absolute") {
                    self.engine.seek(seconds);
                } else {
                    let position = self.engine.snapshot().position;
                    self.engine.seek(position + seconds);
                }
                Ok(NativeValue::None)
            }
            "cycle" => {
                let property = rest
                    .first()
                    .ok_or_else(|| "cycle needs a property argument".to_owned())?;
                match property.as_str() {
                    "pause" => {
                        let paused = self.engine.snapshot().paused;
                        self.engine.set_paused(!paused);
                        Ok(NativeValue::None)
                    }
                    "mute" => {
                        let muted = self.engine.snapshot().muted;
                        self.engine.set_muted(!muted);
                        Ok(NativeValue::None)
                    }
                    "fullscreen" => {
                        self.fullscreen = !self.fullscreen;
                        Ok(NativeValue::None)
                    }
                    "audio" => Err("the Rust engine has no switchable audio tracks".to_owned()),
                    "sub" => {
                        self.cycle_sub()?;
                        Ok(NativeValue::None)
                    }
                    _ => Err(format!("cannot cycle '{property}'")),
                }
            }
            "add" => {
                let (property, delta) = rest.split_first().and_then(|(first, tail)| {
                    tail.first().and_then(|text| text.parse().ok()).map(|delta: f64| (first, delta))
                }).ok_or_else(|| "add needs a property and a numeric delta".to_owned())?;
                match property.as_str() {
                    "volume" => {
                        let volume = self.engine.snapshot().volume as f64 * 100.0 + delta;
                        self.engine.set_volume(volume as f32 / 100.0);
                        Ok(NativeValue::None)
                    }
                    "speed" => {
                        let speed = self.engine.snapshot().speed + delta;
                        self.engine.set_speed(speed);
                        Ok(NativeValue::None)
                    }
                    _ => Err(format!("cannot add to '{property}'")),
                }
            }
            "set" => {
                let (property, text) = rest
                    .split_first()
                    .and_then(|(first, tail)| tail.first().map(|second| (first, second)))
                    .ok_or_else(|| "set needs a property and a value".to_owned())?;
                self.set_from_string(property, text)?;
                Ok(NativeValue::None)
            }
            "sub-add" => {
                let path = rest
                    .first()
                    .ok_or_else(|| "sub-add needs a file argument".to_owned())?;
                self.engine.add_sub(path)?;
                Ok(NativeValue::None)
            }
            "show-text" => {
                let text = rest
                    .first()
                    .ok_or_else(|| "show-text needs a text argument".to_owned())?;
                let duration = rest.get(1).cloned().unwrap_or_else(|| "-1".to_owned());
                let milliseconds = duration.parse::<i64>().unwrap_or(-1);
                let milliseconds = if milliseconds < 0 {
                    DEFAULT_OSD_DURATION_MS
                } else {
                    milliseconds as u64
                };
                self.osd_message_until =
                    Instant::now().checked_add(Duration::from_millis(milliseconds));
                self.osd_message = Some((text.clone(), duration));
                Ok(NativeValue::None)
            }
            "define-section" => {
                let (name, contents) = rest
                    .split_first()
                    .and_then(|(first, tail)| tail.first().map(|second| (first, second)))
                    .ok_or_else(|| "define-section needs a name and contents".to_owned())?;
                self.sections.insert(
                    name.clone(),
                    InputSection {
                        contents: contents.clone(),
                        enabled: false,
                    },
                );
                Ok(NativeValue::None)
            }
            "enable-section" => {
                let name = rest
                    .first()
                    .ok_or_else(|| "enable-section needs a section name".to_owned())?;
                match self.sections.get_mut(name) {
                    Some(section) => {
                        section.enabled = true;
                        Ok(NativeValue::None)
                    }
                    None => Err(format!("unknown section '{name}'")),
                }
            }
            "disable-section" => {
                let name = rest
                    .first()
                    .ok_or_else(|| "disable-section needs a section name".to_owned())?;
                match self.sections.get_mut(name) {
                    Some(section) => {
                        section.enabled = false;
                        Ok(NativeValue::None)
                    }
                    None => Err(format!("unknown section '{name}'")),
                }
            }
            "script-message" => {
                self.loopback.push(rest.to_vec());
                Ok(NativeValue::None)
            }
            "script-message-to" => {
                let (target, payload) = rest
                    .split_first()
                    .ok_or_else(|| "script-message-to needs a target".to_owned())?;
                self.to_script.push((target.clone(), payload.to_vec()));
                Ok(NativeValue::None)
            }
            "quit" | "quit-watch-later" => {
                self.quit = true;
                Ok(NativeValue::None)
            }
            "run" => {
                let (program, args) = rest
                    .split_first()
                    .ok_or_else(|| "run needs a program".to_owned())?;
                match std::process::Command::new(program).args(args).spawn() {
                    Ok(_) => {
                        self.ran.push(rest.to_vec());
                        Ok(NativeValue::None)
                    }
                    Err(error) => Err(format!("run failed: {error}")),
                }
            }
            _ => Err(format!("unknown command '{name}'")),
        }
    }

    fn run_native(&mut self, value: &NativeValue) -> Result<NativeValue, String> {
        let NativeValue::Map(entries) = value else {
            return Err("native commands need a property map".to_owned());
        };
        let name = entries
            .iter()
            .find(|(key, _)| key == "name")
            .and_then(|(_, value)| match value {
                NativeValue::String(name) => Some(name.clone()),
                _ => None,
            })
            .ok_or_else(|| "native commands need a string name".to_owned())?;
        let field = |key: &str| entries.iter().find(|(name, _)| name == key).map(|(_, value)| value);
        match name.as_str() {
            "screenshot-raw" => {
                let flags = match field("flags") {
                    Some(NativeValue::String(flags)) => flags.as_str(),
                    _ => "",
                };
                self.screenshot_raw(flags)
            }
            "osd-overlay" => {
                let id = match field("id") {
                    Some(NativeValue::Int(id)) => *id,
                    _ => return Err("osd-overlay needs an integer id".to_owned()),
                };
                let format = match field("format") {
                    Some(NativeValue::String(format)) => format.clone(),
                    _ => return Err("osd-overlay needs a string format".to_owned()),
                };
                let data = match field("data") {
                    Some(NativeValue::String(data)) => data.clone(),
                    _ => String::new(),
                };
                if format == "none" || data.is_empty() {
                    self.overlays.remove(&id);
                    return Ok(NativeValue::None);
                }
                let number = |key: &str| match field(key) {
                    Some(NativeValue::Int(number)) => *number,
                    _ => 0,
                };
                let hidden = matches!(field("hidden"), Some(NativeValue::Bool(true)));
                self.overlays.insert(
                    id,
                    OsdOverlay {
                        id,
                        format,
                        data,
                        res_x: number("res_x"),
                        res_y: number("res_y"),
                        z: number("z"),
                        hidden,
                    },
                );
                Ok(NativeValue::None)
            }
            "expand-text" => {
                let text = match field("text") {
                    Some(NativeValue::String(text)) => text.clone(),
                    _ => return Err("expand-text needs a string text".to_owned()),
                };
                Ok(NativeValue::String(self.expand_text(&text)))
            }
            _ => Err(format!("unknown native command '{name}'")),
        }
    }

    fn select_audio_by_id(&mut self, id: i64) -> Result<(), String> {
        let tracks = self.engine.snapshot().tracks;
        let track = tracks
            .iter()
            .find(|track| track.kind == TrackKind::Audio && track.id as i64 == id)
            .ok_or_else(|| format!("no audio track {id}"))?;
        self.engine.select_audio(track.src_id.saturating_sub(1));
        Ok(())
    }

    fn select_sub_by_id(&mut self, id: i64) -> Result<(), String> {
        let tracks = self.engine.snapshot().tracks;
        let track = tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle && track.id as i64 == id)
            .ok_or_else(|| format!("no subtitle track {id}"))?;
        match track.sub_index {
            Some(index) => {
                self.engine.select_sub(Some(index));
                Ok(())
            }
            None => Err(format!("subtitle track {id} uses an unsupported codec")),
        }
    }

    fn select_sub_by_name(&mut self, name: &str) -> Result<(), String> {
        match name {
            "auto" => {
                let tracks = self.engine.snapshot().tracks;
                let first = tracks
                    .iter()
                    .filter(|track| track.kind == TrackKind::Subtitle)
                    .find_map(|track| track.sub_index);
                self.engine.select_sub(first);
                Ok(())
            }
            "no" | "off" | "none" => {
                self.engine.select_sub(None);
                Ok(())
            }
            _ => Err(format!("unknown subtitle selection '{name}'")),
        }
    }

    fn cycle_sub(&mut self) -> Result<(), String> {
        let tracks = self.engine.snapshot().tracks;
        let mut order: Vec<Option<usize>> = tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Subtitle)
            .filter_map(|track| track.sub_index)
            .map(Some)
            .collect();
        if order.is_empty() {
            return Err("the Rust engine has no switchable subtitle tracks".to_owned());
        }
        order.push(None);
        let current = tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Subtitle && track.selected)
            .find_map(|track| track.sub_index);
        let position = order
            .iter()
            .position(|option| *option == current)
            .unwrap_or(order.len() - 1);
        self.engine
            .select_sub(order[(position + 1) % order.len()]);
        Ok(())
    }

    fn expand_text(&mut self, text: &str) -> String {
        let mut expanded = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            expanded.push_str(&rest[..start]);
            rest = &rest[start + 2..];
            match rest.find('}') {
                Some(end) => {
                    let name = &rest[..end];
                    match self.value(name).and_then(|value| {
                        self.osd_string(name, &value).or_else(|_| match value {
                            NativeValue::String(text) => Ok(text),
                            NativeValue::Bool(on) => {
                                Ok(if on { "yes" } else { "no" }.to_owned())
                            }
                            NativeValue::Int(number) => Ok(format!("{number}")),
                            NativeValue::Double(number) => Ok(format!("{number:.6}")),
                            _ => Err(format!("property '{name}' has no string form")),
                        })
                    }) {
                        Ok(replacement) => expanded.push_str(&replacement),
                        Err(_) => {
                            expanded.push_str("${");
                            expanded.push_str(name);
                            expanded.push('}');
                        }
                    }
                    rest = &rest[end + 1..];
                }
                None => {
                    expanded.push_str("${");
                    break;
                }
            }
        }
        expanded.push_str(rest);
        expanded
    }

    fn set_from_string(&mut self, name: &str, text: &str) -> Result<(), String> {
        match name {
            "pause" | "mute" | "fullscreen" | "sub-visibility" => match text {
                "yes" | "true" | "1" => {
                    self.set_property(name, NativeValue::Bool(true))?;
                    Ok(())
                }
                "no" | "false" | "0" => {
                    self.set_property(name, NativeValue::Bool(false))?;
                    Ok(())
                }
                _ => Err(format!("'{text}' is not a boolean")),
            },
            "sid" => match text.parse::<i64>() {
                Ok(id) => self.select_sub_by_id(id),
                Err(_) => self.select_sub_by_name(text),
            },
            "volume" => match text.parse::<f64>() {
                Ok(volume) => {
                    self.engine.set_volume(volume as f32 / 100.0);
                    Ok(())
                }
                Err(_) => Err(format!("'{text}' is not a volume")),
            },
            "speed" => match text.parse::<f64>() {
                Ok(speed) => {
                    self.engine.set_speed(speed);
                    Ok(())
                }
                Err(_) => Err(format!("'{text}' is not a speed")),
            },
            _ if name.starts_with("user-data/") => {
                self.user_data
                    .insert(name.to_owned(), NativeValue::String(text.to_owned()));
                Ok(())
            }
            _ => Err(format!("property '{name}' is not settable")),
        }
    }

    fn post_lifecycle(&mut self, sender: &EventSender) -> Result<Posted, HostError> {
        if !self.file_events_sent {
            if self.enabled("start-file") {
                sender.post(
                    ScriptEvent::new("start-file")
                        .with_field("playlist_entry_id", NativeValue::Int(1)),
                )?;
            }
            if self.enabled("file-loaded") {
                sender.post(ScriptEvent::new("file-loaded"))?;
            }
            self.file_events_sent = true;
        }
        let status = self.engine.snapshot();
        if status.ended && !self.end_event_sent {
            if self.enabled("end-file") {
                let reason = if status.error.is_some() { "error" } else { "eof" };
                sender.post(
                    ScriptEvent::new("end-file")
                        .with_field("reason", NativeValue::String(reason.to_owned()))
                        .with_field("playlist_entry_id", NativeValue::Int(1)),
                )?;
            }
            self.end_event_sent = true;
        }
        for args in std::mem::take(&mut self.loopback) {
            if self.enabled("client-message") {
                let args = args
                    .into_iter()
                    .map(NativeValue::String)
                    .collect::<Vec<_>>();
                sender.post(
                    ScriptEvent::new("client-message").with_field("args", NativeValue::Array(args)),
                )?;
            }
        }
        if self.quit && !self.shutdown_sent {
            if self.enabled("shutdown") {
                sender.post(ScriptEvent::shutdown())?;
            }
            self.shutdown_sent = true;
        }
        Ok(Posted::Queued)
    }
}

#[derive(Clone)]
pub struct SharedEngine {
    shared: Arc<Mutex<EngineBackend>>,
}

impl SharedEngine {
    pub fn new(backend: EngineBackend) -> Self {
        SharedEngine {
            shared: Arc::new(Mutex::new(backend)),
        }
    }

    pub fn pump_frames(&self) {
        if let Ok(mut backend) = self.shared.lock() {
            backend.pump_frames();
        }
    }

    pub fn pump(&self, sender: &EventSender) -> Result<Posted, HostError> {
        match self.shared.lock() {
            Ok(mut backend) => backend.pump(sender),
            Err(_) => Err(HostError::Thread("engine backend lock poisoned".to_owned())),
        }
    }

    pub fn inspect<R>(&self, view: impl FnOnce(&EngineBackend) -> R) -> Option<R> {
        self.shared.lock().ok().map(|backend| view(&backend))
    }

    pub fn mutate<R>(&self, update: impl FnOnce(&mut EngineBackend) -> R) -> Option<R> {
        self.shared
            .lock()
            .ok()
            .map(|mut backend| update(&mut backend))
    }
}

impl PlayerBackend for SharedEngine {
    fn log(&mut self, level: &str, message: &str) {
        if let Ok(mut backend) = self.shared.lock() {
            backend.log(level, message);
        }
    }

    fn command(&mut self, command: CommandRequest) -> Result<NativeValue, String> {
        match self.shared.lock() {
            Ok(mut backend) => backend.command(command),
            Err(_) => Err("engine backend lock poisoned".to_owned()),
        }
    }

    fn get_property(
        &mut self,
        name: &str,
        format: PropertyFormat,
    ) -> Result<NativeValue, String> {
        match self.shared.lock() {
            Ok(mut backend) => backend.get_property(name, format),
            Err(_) => Err("engine backend lock poisoned".to_owned()),
        }
    }

    fn set_property(&mut self, name: &str, value: NativeValue) -> Result<(), String> {
        match self.shared.lock() {
            Ok(mut backend) => backend.set_property(name, value),
            Err(_) => Err("engine backend lock poisoned".to_owned()),
        }
    }

    fn del_property(&mut self, name: &str) -> Result<(), String> {
        match self.shared.lock() {
            Ok(mut backend) => backend.del_property(name),
            Err(_) => Err("engine backend lock poisoned".to_owned()),
        }
    }

    fn observe_property(
        &mut self,
        id: u64,
        name: &str,
        format: PropertyFormat,
    ) -> Result<(), String> {
        match self.shared.lock() {
            Ok(mut backend) => backend.observe_property(id, name, format),
            Err(_) => Err("engine backend lock poisoned".to_owned()),
        }
    }

    fn unobserve_property(&mut self, id: u64) -> i32 {
        self.shared
            .lock()
            .map(|mut backend| backend.unobserve_property(id))
            .unwrap_or(0)
    }

    fn request_event(&mut self, name: &str, enable: bool) -> bool {
        self.shared
            .lock()
            .map(|mut backend| backend.request_event(name, enable))
            .unwrap_or(false)
    }

    fn find_config_file(&mut self, name: &str) -> Option<String> {
        let _ = name;
        None
    }

    fn format_time(&mut self, seconds: f64, format: &str) -> Result<String, String> {
        match self.shared.lock() {
            Ok(mut backend) => backend.format_time(seconds, format),
            Err(_) => Err("engine backend lock poisoned".to_owned()),
        }
    }

    fn input_set_section_mouse_area(
        &mut self,
        section: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) {
        if let Ok(mut backend) = self.shared.lock() {
            backend.input_set_section_mouse_area(section, x0, y0, x1, y1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_audio::sink::SinkKind;

    fn fixture(name: &str) -> String {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root")
            .join(".work/fixtures")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    fn backend() -> EngineBackend {
        let engine =
            SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null).expect("engine loads");
        EngineBackend::open(engine)
    }

    fn video_backend() -> EngineBackend {
        let engine =
            SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null).expect("engine loads");
        EngineBackend::open(engine)
    }

    #[test]
    fn properties_match_idle_shapes() {
        let mut backend = backend();
        assert_eq!(
            backend.get_property("pause", PropertyFormat::Bool),
            Ok(NativeValue::Bool(false))
        );
        assert_eq!(
            backend.get_property("pause", PropertyFormat::String),
            Ok(NativeValue::String("no".to_owned()))
        );
        match backend.get_property("duration", PropertyFormat::Number) {
            Ok(NativeValue::Double(duration)) => assert!(duration > 0.0),
            other => panic!("unexpected duration: {other:?}"),
        }
        assert_eq!(
            backend.get_property("volume", PropertyFormat::Number),
            Ok(NativeValue::Double(100.0))
        );
        assert_eq!(
            backend.get_property("aid", PropertyFormat::String),
            Ok(NativeValue::String("auto".to_owned()))
        );
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status.tracks.len() >= 1
        });
        assert_eq!(
            sub_ids(
                &backend
                    .value("track-list")
                    .expect("track-list reads after open")
            ),
            vec![("audio".to_owned(), 1, true, false)]
        );
        assert_eq!(
            backend.get_property("mouse-pos", PropertyFormat::Native),
            Ok(NativeValue::Map(vec![
                ("x".to_owned(), NativeValue::Double(0.0)),
                ("y".to_owned(), NativeValue::Double(0.0)),
                ("hover".to_owned(), NativeValue::Bool(false)),
            ]))
        );
        assert_eq!(
            backend.get_property("osd-dimensions", PropertyFormat::Native),
            Ok(NativeValue::Map(vec![
                ("w".to_owned(), NativeValue::Int(0)),
                ("h".to_owned(), NativeValue::Int(0)),
                ("par".to_owned(), NativeValue::Double(1.0)),
                ("aspect".to_owned(), NativeValue::Double(0.0)),
                ("mt".to_owned(), NativeValue::Int(0)),
                ("mb".to_owned(), NativeValue::Int(0)),
                ("ml".to_owned(), NativeValue::Int(0)),
                ("mr".to_owned(), NativeValue::Int(0)),
            ]))
        );
        backend.set_display_size(1280, 720);
        assert_eq!(
            backend.get_property("osd-dimensions", PropertyFormat::Native),
            Ok(NativeValue::Map(vec![
                ("w".to_owned(), NativeValue::Int(1280)),
                ("h".to_owned(), NativeValue::Int(720)),
                ("par".to_owned(), NativeValue::Double(1.0)),
                ("aspect".to_owned(), NativeValue::Double(1280.0 / 720.0)),
                ("mt".to_owned(), NativeValue::Int(0)),
                ("mb".to_owned(), NativeValue::Int(0)),
                ("ml".to_owned(), NativeValue::Int(0)),
                ("mr".to_owned(), NativeValue::Int(0)),
            ]))
        );
        assert!(backend
            .get_property("no-such-prop", PropertyFormat::Native)
            .is_err());
        assert!(backend
            .get_property("pause", PropertyFormat::Number)
            .is_err());
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn video_dims_feed_osd_size() {
        let mut backend = video_backend();
        assert_eq!(
            backend.get_property("osd-dimensions", PropertyFormat::Native),
            Ok(NativeValue::Map(vec![
                ("w".to_owned(), NativeValue::Int(320)),
                ("h".to_owned(), NativeValue::Int(240)),
                ("par".to_owned(), NativeValue::Double(1.0)),
                ("aspect".to_owned(), NativeValue::Double(320.0 / 240.0)),
                ("mt".to_owned(), NativeValue::Int(0)),
                ("mb".to_owned(), NativeValue::Int(0)),
                ("ml".to_owned(), NativeValue::Int(0)),
                ("mr".to_owned(), NativeValue::Int(0)),
            ]))
        );
        backend.set_display_size(1920, 1080);
        match backend.get_property("osd-dimensions", PropertyFormat::Native) {
            Ok(NativeValue::Map(entries)) => {
                let cell = |key: &str| {
                    entries
                        .iter()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.clone())
                };
                assert_eq!(cell("w"), Some(NativeValue::Int(1920)));
                assert_eq!(cell("h"), Some(NativeValue::Int(1080)));
            }
            other => panic!("unexpected dimensions: {other:?}"),
        }
    }

    #[test]
    fn set_roundtrips_through_engine() {
        let mut backend = backend();
        backend
            .set_property("pause", NativeValue::Bool(true))
            .expect("pause sets");
        wait_for_property(&mut backend, "pause", |value| {
            value == &NativeValue::Bool(true)
        });
        backend
            .set_property("volume", NativeValue::Double(50.0))
            .expect("volume sets");
        wait_for_property(&mut backend, "volume", |value| match value {
            NativeValue::Double(volume) => (volume - 50.0).abs() < 0.01,
            _ => false,
        });
        assert!(backend
            .set_property("duration", NativeValue::Double(1.0))
            .is_err());
        assert!(backend
            .set_property("pause", NativeValue::Double(1.0))
            .is_err());
    }

    #[test]
    fn observe_pump_delivers_changes_once() {
        use player_lua::queue::{event_queue, Received};
        let mut backend = backend();
        backend
            .observe_property(7, "pause", PropertyFormat::Bool)
            .expect("observe registers");
        backend
            .observe_property(9, "volume", PropertyFormat::Number)
            .expect("observe registers");
        let (sender, receiver) = event_queue();
        backend.pump(&sender).expect("pump posts initials");
        let mut seen = Vec::new();
        while let Received::Event(event) =
            receiver.recv_timeout(std::time::Duration::from_millis(100))
        {
            seen.push(event);
            if seen.len() == 2 {
                break;
            }
        }
        assert_eq!(seen.len(), 2, "initial values post once each");
        for event in &seen {
            assert_eq!(event.name, "property-change");
        }
        backend.pump(&sender).expect("steady state is silent");
        assert!(matches!(
            receiver.recv_timeout(std::time::Duration::from_millis(100)),
            Received::Closed | Received::Timeout
        ));
        backend
            .set_property("pause", NativeValue::Bool(true))
            .expect("pause sets");
        std::thread::sleep(std::time::Duration::from_millis(100));
        backend.pump(&sender).expect("change posts");
        match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Received::Event(event) => {
                assert_eq!(event.name, "property-change");
                let id = event
                    .fields
                    .iter()
                    .find(|(name, _)| name == "id")
                    .map(|(_, value)| value.clone());
                assert_eq!(id, Some(NativeValue::Int(7)));
            }
            _ => panic!("expected the pause change event"),
        }
        assert_eq!(backend.unobserve_property(7), 1);
        assert_eq!(backend.unobserve_property(9), 0);
    }

    fn argv(command: &[&str]) -> Vec<String> {
        command.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn wait_for_property(
        backend: &mut EngineBackend,
        name: &str,
        ready: impl Fn(&NativeValue) -> bool,
    ) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Ok(value) = backend.get_property(name, PropertyFormat::Native) {
                if ready(&value) {
                    return;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "property '{name}' never reached the expected value"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn poll_while(
        backend: &mut EngineBackend,
        timeout: std::time::Duration,
        done: impl Fn(player_engine::EngineStatus) -> bool,
    ) -> player_engine::EngineStatus {        use player_lua::queue::event_queue;
        let (sender, _) = event_queue();
        let deadline = std::time::Instant::now() + timeout;
        loop {
            backend.pump(&sender).expect("pump drains frames");
            let status = backend.engine.snapshot();
            if done(status.clone()) {
                return status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting: {status:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn commands_drive_playback_state() {
        let mut backend = backend();
        backend
            .command(CommandRequest::String("cycle pause".to_owned()))
            .expect("cycle pause runs");
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status.paused
        });
        assert!(backend.engine.snapshot().paused);
        backend
            .command(CommandRequest::Args(argv(&["seek", "1.0", "absolute"])))
            .expect("seek runs");
        let status = poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            (status.position - 1.0).abs() < 0.05
        });
        assert!((status.position - 1.0).abs() < 0.05);
        backend
            .command(CommandRequest::Args(argv(&["cycle", "pause"])))
            .expect("cycle pause toggles back");
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            !status.paused
        });
        assert!(!backend.engine.snapshot().paused);
        backend
            .command(CommandRequest::Args(argv(&["add", "volume", "10"])))
            .expect("add volume runs");
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            (status.volume as f64 * 100.0 - 110.0).abs() < 0.5
        });
        match backend.get_property("volume", PropertyFormat::Number) {
            Ok(NativeValue::Double(volume)) => assert!((volume - 110.0).abs() < 0.5),
            other => panic!("unexpected volume: {other:?}"),
        }
        backend
            .command(CommandRequest::Args(argv(&["set", "fullscreen", "yes"])))
            .expect("set fullscreen runs");
        assert_eq!(
            backend.get_property("fullscreen", PropertyFormat::Bool),
            Ok(NativeValue::Bool(true))
        );
        backend
            .command(CommandRequest::Args(argv(&["show-text", "fixture", "500"])))
            .expect("show-text runs");
        assert_eq!(
            backend.osd_message(),
            Some(&("fixture".to_owned(), "500".to_owned()))
        );
        assert!(backend
            .command(CommandRequest::Args(argv(&["cycle", "audio"])))
            .is_err());
        assert!(backend
            .command(CommandRequest::Args(argv(&["cycle", "sub"])))
            .is_err());
        assert!(backend
            .command(CommandRequest::String("frobnicate".to_owned()))
            .is_err());
        assert!(backend.command(CommandRequest::Args(vec![])).is_err());
        assert!(backend
            .command(CommandRequest::Args(argv(&["run", "no-such-program-fixture"])))
            .is_err());
    }

    fn sub_ids(list: &NativeValue) -> Vec<(String, i64, bool, bool)> {
        let NativeValue::Array(entries) = list else {
            panic!("track-list is an array");
        };
        entries
            .iter()
            .map(|entry| {
                let NativeValue::Map(fields) = entry else {
                    panic!("track-list entries are maps");
                };
                let get = |key: &str| {
                    fields
                        .iter()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.clone())
                };
                let kind = match get("type") {
                    Some(NativeValue::String(kind)) => kind,
                    other => panic!("track type is a string, got {other:?}"),
                };
                let id = match get("id") {
                    Some(NativeValue::Int(id)) => id,
                    other => panic!("track id is an int, got {other:?}"),
                };
                let selected = get("selected") == Some(NativeValue::Bool(true));
                let external = get("external") == Some(NativeValue::Bool(true));
                (kind, id, selected, external)
            })
            .collect()
    }

    #[test]
    fn track_list_reports_per_type_ids() {
        let mut backend = video_backend();
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status.tracks.len() >= 2
        });
        let list = backend.value("track-list").expect("track-list reads");
        assert_eq!(
            sub_ids(&list),
            vec![
                ("audio".to_owned(), 1, true, false),
                ("video".to_owned(), 1, true, false),
            ]
        );
    }

    #[test]
    fn subtitle_surface_selects_and_toggles() {
        let mut backend = video_backend();
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status.tracks.len() >= 2
        });
        assert!(backend
            .command(CommandRequest::Args(argv(&[
                "sub-add",
                &fixture("external.srt")
            ])))
            .is_ok());
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status.tracks.len() >= 3
        });
        let list = backend.value("track-list").expect("track-list reads");
        assert_eq!(
            sub_ids(&list),
            vec![
                ("audio".to_owned(), 1, true, false),
                ("video".to_owned(), 1, true, false),
                ("sub".to_owned(), 1, true, true),
            ]
        );
        assert_eq!(
            backend.value("sid").expect("sid reads selected"),
            NativeValue::Int(1)
        );
        backend
            .set_property("sid", NativeValue::String("no".to_owned()))
            .expect("sid no deselects");
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status
                .tracks
                .iter()
                .all(|track| track.kind != player_engine::TrackKind::Subtitle || !track.selected)
        });
        assert_eq!(
            backend.value("sid").expect("sid reads deselected"),
            NativeValue::Bool(false)
        );
        backend
            .command(CommandRequest::Args(argv(&["cycle", "sub"])))
            .expect("cycle sub reselects");
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status.tracks.iter().any(|track| {
                track.kind == player_engine::TrackKind::Subtitle && track.selected
            })
        });
        assert_eq!(
            backend.value("sid").expect("sid reads reselected"),
            NativeValue::Int(1)
        );
        backend
            .set_property("sub-visibility", NativeValue::Bool(false))
            .expect("sub-visibility sets");
        assert_eq!(
            backend.value("sub-visibility").expect("sub-visibility reads"),
            NativeValue::Bool(false)
        );
        assert!(backend
            .command(CommandRequest::Args(argv(&["sub-add", "no-such-sub.srt"])))
            .is_err());
    }

    #[test]
    fn sections_and_script_messages_record() {
        let mut backend = backend();
        backend
            .command(CommandRequest::Args(argv(&[
                "define-section",
                "osc",
                "showhide foo",
            ])))
            .expect("define runs");
        assert!(!backend.section("osc").expect("section stored").enabled);
        assert!(backend
            .command(CommandRequest::Args(argv(&["enable-section", "missing"])))
            .is_err());
        backend
            .command(CommandRequest::Args(argv(&["enable-section", "osc"])))
            .expect("enable runs");
        assert!(backend.section("osc").expect("section stored").enabled);
        backend
            .command(CommandRequest::String("disable-section osc".to_owned()))
            .expect("disable runs");
        assert!(!backend.section("osc").expect("section stored").enabled);
        backend
            .command(CommandRequest::Args(argv(&[
                "script-message-to",
                "osc",
                "visibility",
                "auto",
            ])))
            .expect("script-message-to runs");
        assert_eq!(
            backend.script_outbox(),
            &[(
                "osc".to_owned(),
                vec!["visibility".to_owned(), "auto".to_owned()]
            )]
        );
    }

    #[test]
    fn overlay_mode_delivers_video_frames() {
        use player_lua::queue::event_queue;
        let mut backend = video_backend();
        backend.set_overlay_mode();
        let (sender, _) = event_queue();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut frames = 0;
        loop {
            backend.pump(&sender).expect("pump drains frames");
            frames += backend.take_frames().len();
            let status = backend.engine.snapshot();
            if status.ended || status.error.is_some() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "timed out: {status:?}");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(frames > 0, "overlay mode delivered no video frames");
    }

    #[test]
    fn script_message_loops_back_as_client_message() {
        use player_lua::queue::{event_queue, Received};
        let mut backend = backend();
        assert!(backend.request_event("client-message", true));
        backend
            .command(CommandRequest::Args(argv(&[
                "script-message",
                "fixture-message",
                "payload",
            ])))
            .expect("script-message runs");
        let (sender, receiver) = event_queue();
        backend.pump(&sender).expect("pump posts loopback");
        match receiver.recv_timeout(std::time::Duration::from_millis(500)) {
            Received::Event(event) => {
                assert_eq!(event.name, "client-message");
                let args = event
                    .fields
                    .iter()
                    .find(|(name, _)| name == "args")
                    .map(|(_, value)| value.clone());
                assert_eq!(
                    args,
                    Some(NativeValue::Array(vec![
                        NativeValue::String("fixture-message".to_owned()),
                        NativeValue::String("payload".to_owned()),
                    ]))
                );
            }
            _ => panic!("expected the looped-back message"),
        }
    }

    #[test]
    fn native_overlay_records_and_removes() {
        let mut backend = backend();
        let overlay = NativeValue::Map(vec![
            ("name".to_owned(), NativeValue::String("osd-overlay".to_owned())),
            ("id".to_owned(), NativeValue::Int(1)),
            ("format".to_owned(), NativeValue::String("ass-events".to_owned())),
            ("data".to_owned(), NativeValue::String("{\\an7}fixture".to_owned())),
            ("res_x".to_owned(), NativeValue::Int(1280)),
            ("res_y".to_owned(), NativeValue::Int(720)),
        ]);
        assert_eq!(
            backend.command(CommandRequest::Native(overlay)),
            Ok(NativeValue::None)
        );
        let stored = backend.overlay(1).expect("overlay stored");
        assert_eq!(stored.format, "ass-events");
        assert_eq!(stored.data, "{\\an7}fixture");
        assert_eq!((stored.res_x, stored.res_y), (1280, 720));
        let remove = NativeValue::Map(vec![
            ("name".to_owned(), NativeValue::String("osd-overlay".to_owned())),
            ("id".to_owned(), NativeValue::Int(1)),
            ("format".to_owned(), NativeValue::String("none".to_owned())),
            ("data".to_owned(), NativeValue::String(String::new())),
        ]);
        assert_eq!(
            backend.command(CommandRequest::Native(remove)),
            Ok(NativeValue::None)
        );
        assert!(backend.overlay(1).is_none());
        let expand = NativeValue::Map(vec![
            ("name".to_owned(), NativeValue::String("expand-text".to_owned())),
            (
                "text".to_owned(),
                NativeValue::String("volume ${volume} ${missing} trailing".to_owned()),
            ),
        ]);
        match backend.command(CommandRequest::Native(expand)) {
            Ok(NativeValue::String(expanded)) => {
                assert!(expanded.starts_with("volume 100"), "got: {expanded}");
                assert!(expanded.contains("${missing}"), "got: {expanded}");
                assert!(expanded.ends_with("trailing"), "got: {expanded}");
            }
            other => panic!("unexpected expansion: {other:?}"),
        }
        assert!(backend
            .command(CommandRequest::Native(NativeValue::Map(vec![(
                "name".to_owned(),
                NativeValue::String("no-such-command".to_owned())
            )])))
            .is_err());
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn osd_overlay_composites_into_a_frame() {
        let mut backend = video_backend();
        backend.set_display_size(320, 240);
        let overlay = NativeValue::Map(vec![
            ("name".to_owned(), NativeValue::String("osd-overlay".to_owned())),
            ("id".to_owned(), NativeValue::Int(1)),
            ("format".to_owned(), NativeValue::String("ass-events".to_owned())),
            (
                "data".to_owned(),
                NativeValue::String("{\\an7\\pos(40,30)\\fs40\\bord0\\shad0}Hi".to_owned()),
            ),
            ("res_x".to_owned(), NativeValue::Int(320)),
            ("res_y".to_owned(), NativeValue::Int(240)),
        ]);
        backend
            .command(CommandRequest::Native(overlay))
            .expect("overlay stores");
        let mut frame = vec![0u8; 320 * 240 * 4];
        for value in frame.iter_mut().skip(3).step_by(4) {
            *value = 255;
        }
        backend
            .compose(&mut frame, 320, 240, 1.0)
            .expect("overlay composes");
        assert!(frame.iter().any(|value| *value > 0), "overlay drew no ink");
        assert_eq!(&frame[..3], &[0, 0, 0], "corner stays untouched");
        assert!(backend.osd_error().is_none(), "osd reported an error");
    }

    #[test]
    fn osd_message_uses_the_default_timeout() {
        let mut backend = backend();
        backend
            .command(CommandRequest::String("show-text fixture".to_owned()))
            .expect("show-text runs");
        assert!(backend.visible_message().is_some());
        backend.osd_message_until = Some(Instant::now() - Duration::from_millis(1));
        assert!(backend.visible_message().is_none());
        backend
            .command(CommandRequest::String("show-text fixture 5000".to_owned()))
            .expect("show-text runs");
        assert!(backend.visible_message().is_some());
        assert!(backend.osd_message_until.is_some());
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn external_subtitle_composites_into_a_frame() {
        let mut backend = video_backend();
        backend.set_display_size(320, 240);
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status.tracks.len() >= 2
        });
        backend
            .command(CommandRequest::Args(argv(&[
                "sub-add",
                &fixture("external.srt"),
            ])))
            .expect("subtitle adds");
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            status
                .tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.selected)
        });
        backend.engine.set_paused(true);
        backend.engine.seek(0.6);
        poll_while(&mut backend, std::time::Duration::from_secs(5), |status| {
            (status.position - 0.6).abs() < 0.05
        });
        let mut frame = vec![0u8; 320 * 240 * 4];
        for value in frame.iter_mut().skip(3).step_by(4) {
            *value = 255;
        }
        backend
            .compose(&mut frame, 320, 240, 1.0)
            .expect("subtitles compose");
        let inked = frame
            .chunks_exact(4)
            .filter(|pixel| pixel[..3].iter().any(|value| *value > 0))
            .count();
        assert!(inked > 100, "subtitle drew {inked} pixels");
        assert!(backend.osd_error().is_none(), "osd reported an error");
    }

    #[test]
    fn user_data_roundtrips() {
        let mut backend = backend();
        assert!(backend
            .get_property("user-data/osc/visibility", PropertyFormat::Native)
            .is_err());
        backend
            .set_property(
                "user-data/osc/visibility",
                NativeValue::String("auto".to_owned()),
            )
            .expect("user-data sets");
        assert_eq!(
            backend.get_property("user-data/osc/visibility", PropertyFormat::Native),
            Ok(NativeValue::String("auto".to_owned()))
        );
        backend
            .del_property("user-data/osc/visibility")
            .expect("user-data deletes");
        assert!(backend
            .get_property("user-data/osc/visibility", PropertyFormat::Native)
            .is_err());
    }

    #[test]
    fn file_lifecycle_posts_once() {
        use player_lua::queue::{event_queue, Received};
        let mut backend = backend();
        assert!(backend.request_event("start-file", true));
        assert!(backend.request_event("file-loaded", true));
        let (sender, receiver) = event_queue();
        backend.pump(&sender).expect("first pump posts");
        let mut names = Vec::new();
        while let Received::Event(event) =
            receiver.recv_timeout(std::time::Duration::from_millis(200))
        {
            names.push(event.name);
        }
        assert_eq!(names, vec!["start-file", "file-loaded"]);
        let start = backend.file_events_sent;
        assert!(start);
        backend.pump(&sender).expect("second pump is silent");
        assert!(matches!(
            receiver.recv_timeout(std::time::Duration::from_millis(200)),
            Received::Closed | Received::Timeout
        ));
    }

    #[test]
    fn end_file_and_quit_post_once() {
        use player_lua::queue::{event_queue, Received};
        let engine = SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null)
            .expect("engine loads");
        let mut backend = EngineBackend::open(engine);
        assert!(backend.request_event("end-file", true));
        assert!(backend.request_event("shutdown", true));
        backend.engine.seek(2.0);
        let (sender, receiver) = event_queue();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut end_seen = false;
        while !end_seen {
            backend.pump(&sender).expect("pump posts end-file");
            while let Received::Event(event) =
                receiver.recv_timeout(std::time::Duration::from_millis(50))
            {
                if event.name == "end-file" {
                    let reason = event
                        .fields
                        .iter()
                        .find(|(name, _)| name == "reason")
                        .map(|(_, value)| value.clone());
                    assert_eq!(reason, Some(NativeValue::String("eof".to_owned())));
                    end_seen = true;
                }
            }
            assert!(std::time::Instant::now() < deadline, "timed out waiting for end-file");
        }
        backend
            .command(CommandRequest::Args(argv(&["quit"])))
            .expect("quit runs");
        backend.pump(&sender).expect("pump posts shutdown");
        let mut shutdown_seen = false;
        while let Received::Event(event) =
            receiver.recv_timeout(std::time::Duration::from_millis(200))
        {
            if event.name == "shutdown" {
                shutdown_seen = true;
            }
        }
        assert!(shutdown_seen, "shutdown posted after quit");
    }

    const ASSDRAW: &str = include_str!("../../player-lua/lua/assdraw.lua");
    const OPTIONS: &str = include_str!("../../player-lua/lua/options.lua");
    const OSC: &str = include_str!("../../player-lua/lua/osc.lua");

    fn drive_while(
        shared: &SharedEngine,
        sender: &player_lua::queue::EventSender,
        host: &mut player_lua::ScriptHost,
        timeout: std::time::Duration,
        done: impl Fn() -> bool,
    ) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            shared.pump(sender).expect("pump posts lifecycle");
            if let Some(result) = host.poll() {
                result.expect("osc stays alive");
            }
            if done() {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out driving osc"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn osc_ready(shared: &SharedEngine) -> bool {
        shared
            .inspect(|backend| {
                let sections_ready = ["showhide", "input"].iter().all(|name| {
                    backend
                        .section(name)
                        .map(|section| section.enabled)
                        .unwrap_or(false)
                });
                let overlay_ready = backend.overlay(1).map(|overlay| {
                    overlay.format == "ass-events"
                        && overlay.data.contains("100%")
                        && overlay.data.contains("00:00")
                        && overlay.data.contains("00:0")
                        && overlay.data.contains("mpv-osd-symbols")
                });
                let observed = backend.observed_names();
                let observed_ready = [
                    "duration",
                    "fullscreen",
                    "mute",
                    "osd-dimensions",
                    "pause",
                    "time-pos",
                    "track-list",
                    "volume",
                ]
                .iter()
                .all(|name| observed.iter().any(|known| known == name));
                let enabled = backend.enabled_events();
                let events_ready = ["file-loaded", "shutdown", "start-file"]
                    .iter()
                    .all(|name| enabled.iter().any(|known| known == name));
                sections_ready
                    && overlay_ready.unwrap_or(false)
                    && observed_ready
                    && events_ready
            })
            .unwrap_or(false)
    }

    #[test]
    fn osc_loads_and_drives_engine_headless() {
        use player_lua::{ScriptHost, ScriptModule};
        let shared = SharedEngine::new(backend());
        shared
            .mutate(|backend| {
                backend
                    .set_property("pause", NativeValue::Bool(true))
                    .expect("pause freezes the clock");
            })
            .expect("backend mutates");
        let mut host = ScriptHost::spawn_with_modules(
            "osc",
            "@osc.lua",
            OSC,
            vec![
                ScriptModule::new("mp.assdraw", ASSDRAW),
                ScriptModule::new("mp.options", OPTIONS),
            ],
            shared.clone(),
        )
        .expect("osc spawns");
        let sender = host.event_sender().sender();

        drive_while(
            &shared,
            &sender,
            &mut host,
            std::time::Duration::from_secs(15),
            || {
                shared
                    .inspect(|backend| {
                        backend
                            .enabled_events()
                            .iter()
                            .any(|name| name == "client-message")
                    })
                    .unwrap_or(false)
            },
        );
        shared
            .mutate(|backend| {
                backend
                    .command(CommandRequest::Args(argv(&["script-message", "osc-visibility", "always"])))
                    .expect("visibility message queues");
            })
            .expect("backend mutates");

        drive_while(
            &shared,
            &sender,
            &mut host,
            std::time::Duration::from_secs(15),
            || osc_ready(&shared),
        );

        drive_while(
            &shared,
            &sender,
            &mut host,
            std::time::Duration::from_secs(15),
            || {
                shared
                    .inspect(|backend| {
                        backend.user_value("user-data/osc/visibility")
                            == Some(NativeValue::String("always".to_owned()))
                    })
                    .unwrap_or(false)
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(700));
        shared
            .mutate(|backend| {
                backend
                    .command(CommandRequest::Args(argv(&["script-message", "osc-show"])))
                    .expect("show message queues");
            })
            .expect("backend mutates");

        drive_while(
            &shared,
            &sender,
            &mut host,
            std::time::Duration::from_secs(15),
            || {
                shared
                    .inspect(|backend| {
                        backend.overlay(1).map(|overlay| {
                            !overlay.data.is_empty()
                                && overlay.data.contains("00:00")
                                && overlay.data.contains("100%")
                        })
                    })
                    .unwrap_or(None)
                    .unwrap_or(false)
            },
        );
        shared
            .mutate(|backend| {
                backend
                    .command(CommandRequest::Args(argv(&["script-message", "osc-visibility", "never"])))
                    .expect("hide message queues");
            })
            .expect("backend inspects");

        drive_while(
            &shared,
            &sender,
            &mut host,
            std::time::Duration::from_secs(15),
            || {
                shared
                    .inspect(|backend| {
                        backend.overlay(1).is_none()
                            && backend.user_value("user-data/osc/visibility")
                                == Some(NativeValue::String("never".to_owned()))
                            && backend.osd_message()
                                == Some(&(
                                    "OSC visibility: never".to_owned(),
                                    "-1".to_owned(),
                                ))
                    })
                    .unwrap_or(false)
            },
        );
        host.post_event(ScriptEvent::new("start-file"))
            .expect("start-file posts");
        shared
            .mutate(|backend| {
                backend
                    .command(CommandRequest::Args(argv(&[
                        "script-message",
                        "osc-visibility",
                        "always",
                    ])))
                    .expect("show message queues");
            })
            .expect("backend inspects");

        drive_while(
            &shared,
            &sender,
            &mut host,
            std::time::Duration::from_secs(15),
            || {
                shared
                    .inspect(|backend| {
                        backend.overlay(1).map(|overlay| !overlay.data.is_empty())
                    })
                    .unwrap_or(None)
                    .unwrap_or(false)
            },
        );
        shared
            .mutate(|backend| {
                backend
                    .command(CommandRequest::Args(argv(&["quit"])))
                    .expect("quit runs");
            })
            .expect("backend inspects");
        shared.pump(&sender).expect("pump posts shutdown");
        host.wait_timeout(std::time::Duration::from_secs(10))
            .expect("osc exits on shutdown");
        assert!(
            shared
                .inspect(|backend| backend.overlay(1).is_none())
                .unwrap_or(false),
            "shutdown hides the overlay"
        );
    }

    #[test]
    fn request_event_filters_unknown_names() {
        let mut backend = backend();
        assert!(backend.request_event("property-change", true));
        assert!(backend.request_event("shutdown", true));
        assert!(!backend.request_event("no-such-event", true));
        assert!(backend.request_event("property-change", false));
    }

    #[test]
    fn helpers_behave() {
        let mut backend = backend();
        assert!(backend.find_config_file("anything").is_none());
        assert_eq!(
            backend.format_time(61.0, "%M:%S"),
            Ok("01:01".to_owned())
        );
        assert_eq!(
            backend.get_property("time-pos", PropertyFormat::OsdString),
            Ok(NativeValue::String("00:00".to_owned()))
        );
        match backend.get_property("duration", PropertyFormat::OsdString) {
            Ok(NativeValue::String(text)) => {
                assert!(text.starts_with("00:0"), "unexpected duration text: {text}")
            }
            other => panic!("unexpected duration: {other:?}"),
        }
        assert_eq!(
            backend.get_property("volume", PropertyFormat::OsdString),
            Ok(NativeValue::String("100".to_owned()))
        );
        backend.input_set_section_mouse_area("osc", 0.0, 0.0, 100.0, 20.0);
        assert_eq!(
            backend.mouse_area("osc"),
            Some((0.0, 0.0, 100.0, 20.0))
        );
        assert_eq!(backend.mouse_area("missing"), None);
        backend.log("warn", "probe message");
    }
}
