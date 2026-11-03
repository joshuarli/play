#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackState {
    paused: bool,
    fullscreen: bool,
    mute: bool,
    volume: f64,
    speed: f64,
    time_pos: f64,
    duration: Option<f64>,
    eof_reached: bool,
    aid: Option<i64>,
    sid: Option<i64>,
    vid: Option<i64>,
    sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateChange {
    pub sequence: u64,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaybackState {
    pub fn new() -> Self {
        Self {
            paused: false,
            fullscreen: false,
            mute: false,
            volume: 100.0,
            speed: 1.0,
            time_pos: 0.0,
            duration: None,
            eof_reached: false,
            aid: None,
            sid: None,
            vid: None,
            sequence: 0,
        }
    }

    pub fn set_bool(&mut self, name: &str, value: bool) -> Option<StateChange> {
        let target = match name {
            "pause" => &mut self.paused,
            "fullscreen" => &mut self.fullscreen,
            "mute" => &mut self.mute,
            "eof-reached" => &mut self.eof_reached,
            _ => return None,
        };
        if *target == value {
            return None;
        }
        *target = value;
        self.sequence += 1;
        Some(StateChange {
            sequence: self.sequence,
        })
    }

    pub fn set_number(&mut self, name: &str, value: f64) -> Option<StateChange> {
        if !value.is_finite() {
            return None;
        }
        let target = match name {
            "volume" => &mut self.volume,
            "speed" => &mut self.speed,
            "time-pos" => &mut self.time_pos,
            "duration" => {
                if value < 0.0 {
                    self.duration?;
                    self.duration = None;
                    self.sequence += 1;
                    return Some(StateChange {
                        sequence: self.sequence,
                    });
                }
                if self.duration == Some(value) {
                    return None;
                }
                self.duration = Some(value);
                self.sequence += 1;
                return Some(StateChange {
                    sequence: self.sequence,
                });
            }
            _ => return None,
        };
        if *target == value {
            return None;
        }
        *target = value;
        self.sequence += 1;
        Some(StateChange {
            sequence: self.sequence,
        })
    }

    pub fn set_time_pos(&mut self, value: f64) -> bool {
        if !value.is_finite() || self.time_pos == value {
            return false;
        }
        self.time_pos = value;
        self.sequence += 1;
        true
    }

    pub fn set_duration(&mut self, value: f64) -> bool {
        self.set_number("duration", value).is_some()
    }

    pub fn set_eof_reached(&mut self, value: bool) -> bool {
        self.set_bool("eof-reached", value).is_some()
    }

    pub fn set_track(&mut self, name: &str, value: i64) -> Option<StateChange> {
        let target = match name {
            "aid" => &mut self.aid,
            "sid" => &mut self.sid,
            "vid" => &mut self.vid,
            _ => return None,
        };
        let next = if value == -2 { None } else { Some(value) };
        if *target == next {
            return None;
        }
        *target = next;
        self.sequence += 1;
        Some(StateChange {
            sequence: self.sequence,
        })
    }

    pub fn get_track(&self, name: &str) -> Option<i64> {
        match name {
            "aid" => Some(self.aid.unwrap_or(-2)),
            "sid" => Some(self.sid.unwrap_or(-2)),
            "vid" => Some(self.vid.unwrap_or(-2)),
            _ => None,
        }
    }

    pub fn supports_track(name: &str) -> bool {
        matches!(name, "aid" | "sid" | "vid")
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        match name {
            "pause" => Some(self.paused),
            "fullscreen" => Some(self.fullscreen),
            "mute" => Some(self.mute),
            "eof-reached" => Some(self.eof_reached),
            _ => None,
        }
    }

    pub fn supports_bool(name: &str) -> bool {
        matches!(name, "pause" | "fullscreen" | "mute" | "eof-reached")
    }

    pub fn supports_number(name: &str) -> bool {
        matches!(name, "volume" | "speed" | "time-pos" | "duration")
    }

    pub fn get_number(&self, name: &str) -> Option<f64> {
        match name {
            "volume" => Some(self.volume),
            "speed" => Some(self.speed),
            "time-pos" => Some(self.time_pos),
            "duration" => self.duration,
            _ => None,
        }
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_tracks_changes_and_sequences() {
        let mut state = PlaybackState::new();
        assert_eq!(state.sequence(), 0);
        assert_eq!(state.get_bool("pause"), Some(false));
        assert!(state.set_bool("pause", true).is_some());
        assert_eq!(state.sequence(), 1);
        assert!(state.set_bool("pause", true).is_none());
        assert_eq!(state.sequence(), 1);
        assert!(state.set_bool("unknown", true).is_none());
        assert_eq!(state.get_bool("unknown"), None);
    }

    #[test]
    fn state_tracks_numbers_and_duration() {
        let mut state = PlaybackState::new();
        assert_eq!(state.get_number("volume"), Some(100.0));
        assert!(state.set_number("volume", 55.0).is_some());
        assert_eq!(state.get_number("volume"), Some(55.0));
        assert!(state.set_number("volume", 55.0).is_none());
        assert_eq!(state.get_number("speed"), Some(1.0));
        assert!(state.set_number("speed", 1.5).is_some());
        assert_eq!(state.get_number("speed"), Some(1.5));
        assert!(state.set_number("duration", 30.0).is_some());
        assert_eq!(state.get_number("duration"), Some(30.0));
        assert!(state.set_number("duration", 30.0).is_none());
        assert!(state.set_number("duration", -1.0).is_some());
        assert_eq!(state.get_number("duration"), None);
        assert!(state.set_number("time-pos", 4.5).is_some());
        assert_eq!(state.get_number("time-pos"), Some(4.5));
        assert!(state.set_number("volume", f64::NAN).is_none());
    }

    #[test]
    fn state_tracks_selection() {
        let mut state = PlaybackState::new();
        assert_eq!(state.get_track("aid"), Some(-2));
        assert!(state.set_track("aid", 1).is_some());
        assert_eq!(state.get_track("aid"), Some(1));
        assert!(state.set_track("aid", 1).is_none());
        assert!(state.set_track("aid", -2).is_some());
        assert_eq!(state.get_track("aid"), Some(-2));
        assert!(state.set_track("vid", -1).is_some());
        assert_eq!(state.get_track("vid"), Some(-1));
        assert!(state.set_track("bogus", 1).is_none());
        assert_eq!(state.get_track("bogus"), None);
        assert!(PlaybackState::supports_track("sid"));
        assert!(!PlaybackState::supports_track("pause"));
    }

    #[test]
    fn state_dedicated_setters_detect_changes() {
        let mut state = PlaybackState::new();
        assert!(state.set_time_pos(1.0));
        assert!(!state.set_time_pos(1.0));
        assert!(state.set_duration(10.0));
        assert!(!state.set_duration(10.0));
        assert!(state.set_eof_reached(true));
        assert!(!state.set_eof_reached(true));
    }
}
