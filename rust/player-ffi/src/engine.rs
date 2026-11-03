use player_engine::SchedulerEngine;
use player_lua::{
    queue::{EventSender, Posted},
    CommandRequest, HostError, NativeValue, PlayerBackend, PropertyFormat, ScriptEvent,
};
use std::collections::HashMap;

pub struct EngineBackend {
    engine: SchedulerEngine,
    observed: HashMap<u64, (String, PropertyFormat)>,
    delivered: HashMap<u64, NativeValue>,
    enabled_events: Vec<String>,
    fullscreen: bool,
    display: Option<(u32, u32)>,
    mouse_areas: HashMap<String, (f64, f64, f64, f64)>,
    last_log: Option<(String, String)>,
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
            last_log: None,
        }
    }

    pub fn set_display_size(&mut self, width: u32, height: u32) {
        self.display = Some((width, height));
    }

    pub fn mouse_area(&self, section: &str) -> Option<(f64, f64, f64, f64)> {
        self.mouse_areas.get(section).copied()
    }

    pub fn pump(&mut self, sender: &EventSender) -> Result<Posted, HostError> {
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
        Ok(posted)
    }

    fn value(&self, name: &str) -> Result<NativeValue, String> {
        let status = self.engine.snapshot();
        match name {
            "pause" => Ok(NativeValue::Bool(status.paused)),
            "time-pos" => Ok(NativeValue::Double(status.position)),
            "duration" => Ok(NativeValue::Double(status.duration)),
            "volume" => Ok(NativeValue::Double(status.volume as f64 * 100.0)),
            "mute" => Ok(NativeValue::Bool(status.muted)),
            "fullscreen" => Ok(NativeValue::Bool(self.fullscreen)),
            "pid" => Ok(NativeValue::Int(std::process::id() as i64)),
            "working-directory" => Ok(NativeValue::String(
                std::env::current_dir()
                    .map(|dir| dir.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )),
            "aid" | "sid" => Ok(NativeValue::String("auto".to_owned())),
            "track-list" => Ok(NativeValue::Array(Vec::new())),
            "mouse-pos" => Ok(NativeValue::Map(vec![
                ("x".to_owned(), NativeValue::Double(0.0)),
                ("y".to_owned(), NativeValue::Double(0.0)),
                ("hover".to_owned(), NativeValue::Bool(false)),
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

impl PlayerBackend for EngineBackend {
    fn log(&mut self, level: &str, message: &str) {
        self.last_log = Some((level.to_owned(), message.to_owned()));
    }

    fn command(&mut self, _command: CommandRequest) -> Result<NativeValue, String> {
        Err("command surface lands in S7b-2".to_owned())
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
            _ => Err(format!("property '{name}' is not settable")),
        }
    }

    fn del_property(&mut self, name: &str) -> Result<(), String> {
        Err(format!("property '{name}' cannot be deleted"))
    }

    fn observe_property(
        &mut self,
        id: u64,
        name: &str,
        format: PropertyFormat,
    ) -> Result<(), String> {
        self.value(name)?;
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
        assert_eq!(
            backend.get_property("track-list", PropertyFormat::Native),
            Ok(NativeValue::Array(Vec::new()))
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
        assert!(backend
            .get_property("no-such-prop", PropertyFormat::Native)
            .is_err());
        assert!(backend
            .get_property("pause", PropertyFormat::Number)
            .is_err());
    }

    #[test]
    fn set_roundtrips_through_engine() {
        let mut backend = backend();
        backend
            .set_property("pause", NativeValue::Bool(true))
            .expect("pause sets");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(
            backend.get_property("pause", PropertyFormat::Bool),
            Ok(NativeValue::Bool(true))
        );
        backend
            .set_property("volume", NativeValue::Double(50.0))
            .expect("volume sets");
        std::thread::sleep(std::time::Duration::from_millis(100));
        match backend.get_property("volume", PropertyFormat::Number) {
            Ok(NativeValue::Double(volume)) => assert!((volume - 50.0).abs() < 0.01),
            other => panic!("unexpected volume: {other:?}"),
        }
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
        assert_eq!(
            backend.get_property("duration", PropertyFormat::OsdString),
            Ok(NativeValue::String("00:03".to_owned()))
        );
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
