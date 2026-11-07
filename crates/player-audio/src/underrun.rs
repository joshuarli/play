#[derive(Debug, Default)]
pub struct Underruns {
    count: u64,
    playing: bool,
    expected_drain: bool,
}

impl Underruns {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn data(&mut self) {
        self.playing = true;
        self.expected_drain = false;
    }

    pub fn flush(&mut self) {
        self.playing = false;
        self.expected_drain = true;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn finish(&mut self) {
        self.playing = false;
        self.expected_drain = true;
    }

    pub fn starved(&mut self) -> bool {
        if self.expected_drain {
            self.expected_drain = false;
            self.playing = false;
            return false;
        }
        if self.playing {
            self.playing = false;
            self.count += 1;
            return true;
        }
        false
    }

    pub fn count(&self) -> u64 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_starved_queue_before_the_first_data_is_not_an_underrun() {
        let mut underruns = Underruns::new();
        assert!(!underruns.starved());
        assert_eq!(underruns.count(), 0);
    }

    #[test]
    fn starvation_during_playback_counts_once_until_data_returns() {
        let mut underruns = Underruns::new();
        underruns.data();
        assert!(underruns.starved());
        assert_eq!(underruns.count(), 1);
        assert!(!underruns.starved(), "the queue is still empty");
        assert_eq!(underruns.count(), 1);
        underruns.data();
        assert!(underruns.starved());
        assert_eq!(underruns.count(), 2);
    }

    #[test]
    fn a_flush_or_seek_drain_is_not_an_underrun() {
        let mut underruns = Underruns::new();
        underruns.data();
        underruns.flush();
        assert!(!underruns.starved());
        assert_eq!(underruns.count(), 0);
        underruns.data();
        assert!(underruns.starved());
        assert_eq!(underruns.count(), 1);
    }

    #[test]
    fn the_end_of_stream_drain_is_not_an_underrun() {
        let mut underruns = Underruns::new();
        underruns.data();
        underruns.finish();
        assert!(!underruns.starved());
        assert_eq!(underruns.count(), 0);
    }

    #[test]
    fn a_pause_is_not_an_underrun() {
        let mut underruns = Underruns::new();
        underruns.data();
        underruns.pause();
        assert!(!underruns.starved());
        assert_eq!(underruns.count(), 0);
    }
}
