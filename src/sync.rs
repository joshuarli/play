use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// Audio-master A/V sync clock.
///
/// The audio playback position (updated by the audio output via AtomicI64)
/// is the master clock. The display layer's CMTimebase handles video frame
/// pacing; this clock is used for seek position, subtitle timing, and
/// periodic drift correction.
pub struct SyncClock {
    audio_clock: Arc<AtomicI64>,
    paused: bool,
    pause_pts: i64,
}

impl SyncClock {
    pub fn new(audio_clock: Arc<AtomicI64>) -> Self {
        Self {
            audio_clock,
            paused: false,
            pause_pts: 0,
        }
    }

    /// Get the current audio playback position in microseconds.
    pub fn audio_pts(&self) -> i64 {
        if self.paused {
            self.pause_pts
        } else {
            self.audio_clock.load(Ordering::Relaxed)
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        if paused && !self.paused {
            self.pause_pts = self.audio_clock.load(Ordering::Relaxed);
        }
        self.paused = paused;
    }

    #[allow(dead_code)]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Force-set the clock position (used after seek).
    pub fn set_position(&self, pts_us: i64) {
        self.audio_clock.store(pts_us, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_clock(initial: i64) -> (SyncClock, Arc<AtomicI64>) {
        let atom = Arc::new(AtomicI64::new(initial));
        let clock = SyncClock::new(Arc::clone(&atom));
        (clock, atom)
    }

    #[test]
    fn reads_from_atomic() {
        let (clock, atom) = make_clock(0);
        atom.store(42_000_000, Ordering::Relaxed);
        assert_eq!(clock.audio_pts(), 42_000_000);
    }

    #[test]
    fn pause_captures_position() {
        let (mut clock, atom) = make_clock(0);
        atom.store(10_000_000, Ordering::Relaxed);
        clock.set_paused(true);
        // Atomic advances but paused clock doesn't
        atom.store(20_000_000, Ordering::Relaxed);
        assert_eq!(clock.audio_pts(), 10_000_000);
        assert!(clock.is_paused());
    }

    #[test]
    fn unpause_resumes_from_atomic() {
        let (mut clock, atom) = make_clock(0);
        atom.store(10_000_000, Ordering::Relaxed);
        clock.set_paused(true);
        atom.store(20_000_000, Ordering::Relaxed);
        clock.set_paused(false);
        assert_eq!(clock.audio_pts(), 20_000_000);
        assert!(!clock.is_paused());
    }

    #[test]
    fn set_position_updates_atomic() {
        let (clock, atom) = make_clock(0);
        clock.set_position(5_000_000);
        assert_eq!(atom.load(Ordering::Relaxed), 5_000_000);
        assert_eq!(clock.audio_pts(), 5_000_000);
    }

    #[test]
    fn pause_set_position_unpause() {
        let (mut clock, atom) = make_clock(0);
        atom.store(10_000_000, Ordering::Relaxed);
        clock.set_paused(true);
        clock.set_position(30_000_000);
        clock.set_paused(false);
        assert_eq!(clock.audio_pts(), 30_000_000);
    }
}
