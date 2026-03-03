use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// Audio-master A/V sync clock.
///
/// The audio playback position (updated by the audio output via AtomicI64)
/// is the master clock. Video frames are displayed, dropped, or delayed
/// relative to this clock.
pub struct SyncClock {
    audio_clock: Arc<AtomicI64>,
    paused: bool,
    pause_pts: i64,
}

/// Decision for what to do with a video frame.
#[derive(Debug)]
pub enum SyncAction {
    /// Display the frame now.
    Display,
    /// Drop the frame (too late).
    Drop,
    /// Wait this many microseconds before displaying.
    Wait(u64),
}

/// Threshold for dropping late frames (50ms).
const DROP_THRESHOLD_US: i64 = 50_000;

/// Threshold for waiting on early frames (50ms).
const WAIT_THRESHOLD_US: i64 = 50_000;

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

    /// Decide what to do with a video frame given its PTS.
    pub fn decide(&self, video_pts_us: i64) -> SyncAction {
        let audio_pts = self.audio_pts();
        let diff = video_pts_us - audio_pts;

        if diff < -DROP_THRESHOLD_US {
            // Frame is too late, drop it
            log::trace!("Sync: drop frame (diff={diff}us)");
            SyncAction::Drop
        } else if diff > WAIT_THRESHOLD_US {
            // Frame is too early, wait
            let wait = diff as u64;
            log::trace!("Sync: wait {wait}us for frame");
            SyncAction::Wait(wait)
        } else {
            // Frame is on time (within ±50ms), display it
            SyncAction::Display
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
