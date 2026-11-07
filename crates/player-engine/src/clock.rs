pub const NOPTS: i64 = i64::MIN;

pub const MAX_LATE_SECONDS: f64 = 0.1;

pub fn rescale_to_seconds(timestamp: i64, numerator: i32, denominator: i32) -> Option<f64> {
    if timestamp == NOPTS || denominator == 0 {
        return None;
    }
    Some(timestamp as f64 * numerator as f64 / denominator as f64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    AudioDriven,
    SystemDriven,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MasterClock {
    mode: ClockMode,
    speed: f64,
    device_time: f64,
    anchor_ref: f64,
    anchor_content: f64,
    queued_latency: f64,
    paused_position: Option<f64>,
}

impl MasterClock {
    pub fn new(mode: ClockMode) -> Self {
        MasterClock {
            mode,
            speed: 1.0,
            device_time: 0.0,
            anchor_ref: 0.0,
            anchor_content: 0.0,
            queued_latency: 0.0,
            paused_position: None,
        }
    }

    pub fn set_speed(&mut self, speed: f64, now: f64) {
        let current = self.position(now);
        self.speed = speed.max(0.0);
        self.anchor_content = current;
        self.anchor_ref = self.reference(now);
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    pub fn observe_device(&mut self, device_time: f64, queued_seconds: f64, _now: f64) {
        self.device_time = device_time;
        self.queued_latency = queued_seconds.max(0.0);
    }

    pub fn set_paused(&mut self, paused: bool, now: f64) {
        if paused {
            self.paused_position = Some(self.position(now));
        } else if let Some(frozen) = self.paused_position.take() {
            self.anchor_content = frozen;
            self.anchor_ref = self.reference(now);
        }
    }

    pub fn seek_rebase(&mut self, position: f64, now: f64) {
        self.anchor_content = position;
        self.anchor_ref = self.reference(now);
        if self.paused_position.is_some() {
            self.paused_position = Some(position);
        }
    }

    pub fn use_system_time(&mut self, now: f64) {
        let position = self.position(now);
        self.mode = ClockMode::SystemDriven;
        self.anchor_content = position;
        self.anchor_ref = now;
    }

    pub fn use_audio_time(&mut self, now: f64) {
        let position = self.position(now);
        self.mode = ClockMode::AudioDriven;
        self.anchor_content = position;
        self.anchor_ref = self.device_time;
    }

    pub fn is_audio_driven(&self) -> bool {
        self.mode == ClockMode::AudioDriven
    }

    pub fn position(&self, now: f64) -> f64 {
        if let Some(frozen) = self.paused_position {
            return frozen;
        }
        self.anchor_content + (self.reference(now) - self.anchor_ref).max(0.0) * self.speed
    }

    pub fn output_latency(&self) -> f64 {
        self.queued_latency
    }

    fn reference(&self, now: f64) -> f64 {
        match self.mode {
            ClockMode::AudioDriven => self.device_time,
            ClockMode::SystemDriven => now,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameVerdict {
    Present,
    DropLate,
    Wait,
}

pub fn judge_frame(presentation_time: f64, now: f64) -> FrameVerdict {
    if presentation_time > now {
        FrameVerdict::Wait
    } else if now - presentation_time > MAX_LATE_SECONDS {
        FrameVerdict::DropLate
    } else {
        FrameVerdict::Present
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn timebase_conversion_matches_av_rescale_q() {
        assert!(close(
            rescale_to_seconds(90_000, 1, 90_000).unwrap(),
            1.0
        ));
        assert!(close(rescale_to_seconds(1536, 1, 48000).unwrap(), 0.032));
        assert!(close(
            rescale_to_seconds(-90_000, 1, 90_000).unwrap(),
            -1.0
        ));
        assert_eq!(rescale_to_seconds(NOPTS, 1, 90_000), None);
        assert_eq!(rescale_to_seconds(100, 1, 0), None);
    }

    #[test]
    fn audio_clock_follows_device_and_freezes_on_pause() {
        let mut clock = MasterClock::new(ClockMode::AudioDriven);
        clock.observe_device(1.0, 0.05, 100.0);
        assert!(close(clock.position(100.5), 1.0));
        assert!(close(clock.output_latency(), 0.05));
        clock.set_paused(true, 100.5);
        clock.observe_device(2.0, 0.05, 101.0);
        assert!(close(clock.position(101.0), 1.0));
        clock.set_paused(false, 101.0);
        clock.observe_device(2.5, 0.05, 102.5);
        assert!(close(clock.position(103.0), 1.5));
    }

    #[test]
    fn system_clock_advances_with_monotonic_time_and_speed() {
        let mut clock = MasterClock::new(ClockMode::SystemDriven);
        assert!(close(clock.position(10.0), 10.0));
        clock.set_speed(2.0, 10.0);
        assert!(close(clock.position(11.0), 12.0));
        clock.set_paused(true, 11.0);
        assert!(close(clock.position(50.0), 12.0));
        clock.set_paused(false, 60.0);
        assert!(close(clock.position(61.0), 14.0));
    }

    #[test]
    fn seek_rebases_content_without_rewinding_device_time() {
        let mut clock = MasterClock::new(ClockMode::AudioDriven);
        clock.observe_device(30.0, 0.0, 200.0);
        assert!(close(clock.position(200.0), 30.0));
        clock.seek_rebase(5.0, 200.0);
        assert!(close(clock.position(200.0), 5.0));
        clock.observe_device(30.5, 0.0, 201.0);
        assert!(close(clock.position(201.0), 5.5));

        let mut sys = MasterClock::new(ClockMode::SystemDriven);
        sys.seek_rebase(5.0, 200.0);
        assert!(close(sys.position(201.0), 6.0));
        sys.set_paused(true, 201.0);
        sys.seek_rebase(9.0, 202.0);
        assert!(close(sys.position(300.0), 9.0));
    }

    #[test]
    fn system_time_tail_keeps_the_position_moving() {
        let mut clock = MasterClock::new(ClockMode::AudioDriven);
        clock.observe_device(3.0, 0.0, 100.0);
        clock.use_system_time(100.0);
        assert!(close(clock.position(100.5), 3.5));
        assert!(!clock.is_audio_driven());
    }

    #[test]
    fn audio_time_resumes_from_the_system_position() {
        let mut clock = MasterClock::new(ClockMode::AudioDriven);
        clock.observe_device(3.0, 0.0, 100.0);
        clock.use_system_time(100.0);
        clock.observe_device(3.0, 0.0, 101.0);
        clock.use_audio_time(101.0);
        assert!(clock.is_audio_driven());
        assert!(close(clock.position(101.0), 4.0));
        clock.observe_device(3.25, 0.0, 102.0);
        assert!(close(clock.position(102.0), 4.25));
    }

    #[test]
    fn device_discontinuity_snaps_without_error() {
        let mut clock = MasterClock::new(ClockMode::AudioDriven);
        clock.observe_device(10.0, 0.0, 100.0);
        clock.observe_device(45.0, 0.0, 100.1);
        assert!(close(clock.position(100.1), 45.0));
    }

    #[test]
    fn frame_verdicts_never_present_stale_frames() {
        assert_eq!(judge_frame(1.05, 1.0), FrameVerdict::Wait);
        assert_eq!(judge_frame(1.0, 1.0), FrameVerdict::Present);
        assert_eq!(judge_frame(0.95, 1.0), FrameVerdict::Present);
        assert_eq!(judge_frame(0.5, 1.0), FrameVerdict::DropLate);
        assert_eq!(judge_frame(-100.0, 1.0), FrameVerdict::DropLate);
    }
}
