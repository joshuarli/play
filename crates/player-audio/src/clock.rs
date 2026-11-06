pub struct AudioClock {
    rate: u32,
    offset: f64,
    anchor_written: u64,
    paused: bool,
    pause_position: f64,
}

impl AudioClock {
    pub fn new(rate: u32) -> Self {
        AudioClock {
            rate,
            offset: 0.0,
            anchor_written: 0,
            paused: false,
            pause_position: 0.0,
        }
    }

    pub fn position(&self, written: u64, queued: u64) -> f64 {
        if self.paused {
            return self.pause_position;
        }
        let played = written.saturating_sub(queued).saturating_sub(self.anchor_written);
        (self.offset + played as f64 / self.rate as f64).max(0.0)
    }

    pub fn set_paused(&mut self, paused: bool, written: u64, queued: u64) {
        if paused == self.paused {
            return;
        }
        if paused {
            self.pause_position = self.position(written, queued);
            self.paused = true;
        } else {
            self.offset = self.pause_position;
            self.anchor_written = written.saturating_sub(queued);
            self.paused = false;
        }
    }

    pub fn seek(&mut self, seconds: f64, written: u64) {
        let target = seconds.max(0.0);
        if self.paused {
            self.pause_position = target;
        } else {
            self.offset = target;
            self.anchor_written = written;
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_counts_played_frames() {
        let clock = AudioClock::new(48000);
        assert_eq!(clock.position(48000, 0), 1.0);
        assert_eq!(clock.position(48000, 4800), 0.9);
    }

    #[test]
    fn pause_freezes_and_resume_continues() {
        let mut clock = AudioClock::new(48000);
        clock.set_paused(true, 96000, 0);
        assert_eq!(clock.position(96000, 0), 2.0);
        assert_eq!(clock.position(200000, 0), 2.0);
        clock.set_paused(false, 96000, 0);
        assert_eq!(clock.position(144000, 0), 3.0);
    }

    #[test]
    fn seek_resets_anchor() {
        let mut clock = AudioClock::new(48000);
        clock.seek(10.0, 240000);
        assert_eq!(clock.position(240000, 0), 10.0);
        assert_eq!(clock.position(288000, 0), 11.0);
    }

    #[test]
    fn seek_while_paused_moves_freeze_point() {
        let mut clock = AudioClock::new(48000);
        clock.set_paused(true, 48000, 0);
        clock.seek(5.0, 48000);
        assert_eq!(clock.position(999999, 0), 5.0);
        clock.set_paused(false, 48000, 0);
        assert_eq!(clock.position(96000, 0), 6.0);
    }
}
