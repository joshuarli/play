pub enum SinkKind {
    Null,
    Default,
    Device(String),
}

pub trait AudioSink {
    fn write(&mut self, frames: &[f32]) -> Result<(), String>;
    fn channels(&self) -> usize;
    fn rate(&self) -> u32;
    fn queued_frames(&self) -> u64;
    fn pause(&mut self) -> Result<(), String>;
    fn resume(&mut self) -> Result<(), String>;
    fn flush(&mut self) -> Result<(), String>;
    fn finish(&mut self) -> Result<(), String> {
        Ok(())
    }
}

pub struct NullSink {
    channels: usize,
    rate: u32,
    frames: u64,
    peak: f32,
    energy: f64,
    paused: bool,
    played_base: u64,
    anchor: Option<std::time::Instant>,
}

impl NullSink {
    pub fn new(channels: usize, rate: u32) -> Self {
        NullSink {
            channels,
            rate,
            frames: 0,
            peak: 0.0,
            energy: 0.0,
            paused: false,
            played_base: 0,
            anchor: None,
        }
    }

    pub fn frames_written(&self) -> u64 {
        self.frames
    }

    pub fn peak(&self) -> f32 {
        self.peak
    }

    pub fn rms(&self) -> f32 {
        if self.frames == 0 {
            return 0.0;
        }
        (self.energy / (self.frames * self.channels as u64) as f64).sqrt() as f32
    }
}

impl NullSink {
    fn played(&self) -> u64 {
        let Some(start) = self.anchor else {
            return self.played_base;
        };
        let elapsed = (start.elapsed().as_secs_f64() * self.rate as f64) as u64;
        (self.played_base + elapsed).min(self.frames)
    }
}

impl AudioSink for NullSink {
    fn write(&mut self, frames: &[f32]) -> Result<(), String> {
        if self.channels == 0 {
            return Err("null sink has no channels".to_string());
        }
        if frames.len() % self.channels != 0 {
            return Err("sink write must hold whole frames".to_string());
        }
        if self.paused {
            return Err("sink is paused".to_string());
        }
        if self.played() >= self.frames {
            self.played_base = self.frames;
            self.anchor = Some(std::time::Instant::now());
        }
        self.frames += (frames.len() / self.channels) as u64;
        for sample in frames {
            let value = sample.abs();
            if value > self.peak {
                self.peak = value;
            }
            self.energy += (value * value) as f64;
        }
        Ok(())
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn queued_frames(&self) -> u64 {
        self.frames - self.played()
    }

    fn pause(&mut self) -> Result<(), String> {
        self.played_base = self.played();
        self.anchor = None;
        self.paused = true;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), String> {
        self.paused = false;
        self.anchor = Some(std::time::Instant::now());
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        self.played_base = self.frames;
        self.anchor = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_sink_accounts_frames_and_level() {
        let mut sink = NullSink::new(2, 48000);
        sink.write(&[0.5, -0.5, 0.0, 0.0]).unwrap();
        assert_eq!(sink.frames_written(), 2);
        assert_eq!(sink.peak(), 0.5);
        assert!((sink.rms() - 0.3535).abs() < 0.001);
        assert_eq!(sink.queued_frames(), 2, "written frames queue until played");
        sink.flush().unwrap();
        assert_eq!(sink.queued_frames(), 0);
    }

    #[test]
    fn null_sink_plays_queued_frames_in_real_time_and_holds_while_paused() {
        let mut sink = NullSink::new(1, 1000);
        sink.write(&[0.0; 200]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let queued = sink.queued_frames();
        assert!((80..=110).contains(&queued), "about 100 of 200 frames left, got {queued}");
        sink.pause().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(sink.queued_frames(), queued, "a paused sink plays nothing");
        sink.resume().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(sink.queued_frames(), 0);
    }

    #[test]
    fn null_sink_rejects_partial_frames() {
        let mut sink = NullSink::new(2, 48000);
        assert!(sink.write(&[0.1, 0.2, 0.3]).is_err());
    }

    #[test]
    fn paused_sink_refuses_writes() {
        let mut sink = NullSink::new(1, 44100);
        sink.pause().unwrap();
        assert!(sink.write(&[0.1]).is_err());
        sink.resume().unwrap();
        sink.write(&[0.1]).unwrap();
        assert_eq!(sink.frames_written(), 1);
    }
}
