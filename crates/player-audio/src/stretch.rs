pub struct TimeStretch {
    channels: usize,
    hop: usize,
    search: usize,
    speed: f64,
    input: Vec<f32>,
    input_start: u64,
    input_frames: u64,
    output_frames: u64,
    overlap: Vec<f32>,
}

impl TimeStretch {
    pub fn new(rate: u32, channels: usize) -> Self {
        let hop = (rate as usize / 100).max(1);
        Self { channels, hop, search: (hop / 2).max(1), speed: 1.0, input: Vec::new(), input_start: 0, input_frames: 0, output_frames: 0, overlap: Vec::new() }
    }

    pub fn reset(&mut self, speed: f64) -> Result<(), String> {
        if !speed.is_finite() || !(0.01..=100.0).contains(&speed) {
            return Err("audio speed must be between 0.01 and 100".to_string());
        }
        self.speed = speed;
        self.input.clear();
        self.input_start = 0;
        self.input_frames = 0;
        self.output_frames = 0;
        self.overlap.clear();
        Ok(())
    }

    pub fn discard(&mut self) {
        self.input.clear();
        self.input_start = 0;
        self.input_frames = 0;
        self.output_frames = 0;
        self.overlap.clear();
    }

    pub fn push(&mut self, samples: &[f32], output: &mut Vec<f32>) -> Result<(), String> {
        if self.channels == 0 || !samples.len().is_multiple_of(self.channels) {
            return Err("time stretch input must hold whole frames".to_string());
        }
        if self.speed == 1.0 {
            output.extend_from_slice(samples);
            return Ok(());
        }
        self.input.extend_from_slice(samples);
        self.input_frames += (samples.len() / self.channels) as u64;
        self.produce(output, false);
        Ok(())
    }

    pub fn pump(&mut self, output: &mut Vec<f32>) {
        if self.speed != 1.0 {
            self.produce(output, false);
        }
    }

    pub fn finish(&mut self, output: &mut Vec<f32>) {
        if self.speed != 1.0 {
            self.produce(output, true);
        }
    }

    pub fn finished(&self) -> bool {
        self.speed == 1.0 || self.output_frames >= (self.input_frames as f64 / self.speed).round() as u64
    }

    fn sample(&self, frame: u64, channel: usize) -> f32 {
        if frame < self.input_start || frame >= self.input_frames {
            return 0.0;
        }
        self.input[(frame - self.input_start) as usize * self.channels + channel]
    }

    fn produce(&mut self, output: &mut Vec<f32>, finishing: bool) {
        let target = (self.input_frames as f64 / self.speed).round() as u64;
        let limit = self.output_frames.saturating_add((self.hop * 4) as u64);
        while self.output_frames < target && self.output_frames < limit {
            let expected = (self.output_frames as f64 * self.speed).round() as u64;
            if !finishing && (target - self.output_frames < self.hop as u64
                || expected.saturating_add((self.search + 2 * self.hop) as u64) > self.input_frames) {
                break;
            }
            let mut best = expected;
            if !self.overlap.is_empty() {
                let low = expected.saturating_sub(self.search as u64).max(self.input_start);
                let high = expected.saturating_add(self.search as u64).min(self.input_frames.saturating_sub(1));
                let mut best_score = f64::NEG_INFINITY;
                for candidate in low..=high {
                    let mut dot = 0.0f64;
                    let mut aa = 1e-20f64;
                    let mut bb = 1e-20f64;
                    for i in 0..self.hop {
                        for ch in 0..self.channels {
                            let a = self.overlap[i * self.channels + ch] as f64;
                            let b = self.sample(candidate + i as u64, ch) as f64;
                            dot += a * b;
                            aa += a * a;
                            bb += b * b;
                        }
                    }
                    let score = dot / (aa * bb).sqrt();
                    if score > best_score {
                        best_score = score;
                        best = candidate;
                    }
                }
            }
            let count = (target - self.output_frames).min(self.hop as u64) as usize;
            for i in 0..count {
                let weight = i as f32 / self.hop as f32;
                for ch in 0..self.channels {
                    let sample = self.sample(best + i as u64, ch);
                    output.push(if self.overlap.is_empty() { sample } else {
                        self.overlap[i * self.channels + ch] * (1.0 - weight) + sample * weight
                    });
                }
            }
            let mut overlap = Vec::with_capacity(self.hop * self.channels);
            for i in 0..self.hop {
                for ch in 0..self.channels {
                    overlap.push(self.sample(best + (self.hop + i) as u64, ch));
                }
            }
            self.overlap = overlap;
            self.output_frames += count as u64;
            let next = (self.output_frames as f64 * self.speed).round() as u64;
            let discard_to = next.saturating_sub(self.search as u64).min(self.input_frames);
            if discard_to > self.input_start {
                let frames = (discard_to - self.input_start) as usize;
                self.input.drain(..frames * self.channels);
                self.input_start = discard_to;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_slowdown_is_released_in_bounded_device_blocks() {
        let mut stretch = TimeStretch::new(8000, 1);
        stretch.reset(0.01).unwrap();
        let mut output = Vec::new();
        stretch.push(&vec![0.0; 320], &mut output).unwrap();
        assert!(output.len() <= 320);
        loop {
            let before = output.len();
            stretch.pump(&mut output);
            assert!(output.len() - before <= 320);
            if output.len() == before { break; }
        }
        while !stretch.finished() {
            let before = output.len();
            stretch.finish(&mut output);
            assert!(output.len() - before <= 320);
        }
        assert_eq!(output.len(), 32_000);
    }

    #[test]
    fn duration_pitch_streaming_and_reset() {
        let rate = 8000;
        let input: Vec<f32> = (0..rate).map(|i| (2.0 * std::f32::consts::PI * 200.0 * i as f32 / rate as f32).sin()).collect();
        for speed in [0.5, 1.0, 2.0] {
            let mut stretch = TimeStretch::new(rate, 1);
            stretch.reset(speed).unwrap();
            let mut output = Vec::new();
            for chunk in input.chunks(137) {
                stretch.push(chunk, &mut output).unwrap();
                loop {
                    let before = output.len();
                    stretch.pump(&mut output);
                    if output.len() == before { break; }
                }
            }
            while !stretch.finished() { stretch.finish(&mut output); }
            assert_eq!(output.len(), (rate as f64 / speed).round() as usize);
            let body = &output[output.len()/4..output.len()*3/4];
            let crossings = body.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
            let pitch = crossings as f64 * rate as f64 / body.len() as f64;
            assert!((pitch - 200.0).abs() < 5.0, "{speed}: {pitch} Hz");
            stretch.reset(speed).unwrap();
            let mut again = Vec::new();
            for chunk in input.chunks(137) {
                stretch.push(chunk, &mut again).unwrap();
                loop {
                    let before = again.len();
                    stretch.pump(&mut again);
                    if again.len() == before { break; }
                }
            }
            while !stretch.finished() { stretch.finish(&mut again); }
            assert_eq!(output, again);
        }
    }
}
