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
