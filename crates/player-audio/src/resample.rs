pub struct Resampler {
    in_rate: u32,
    out_rate: u32,
    channels: usize,
    emitted: u64,
    dropped: u64,
    scratch: Vec<f32>,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32, channels: usize) -> Result<Self, String> {
        if in_rate == 0 || out_rate == 0 {
            return Err("resampler needs positive sample rates".to_string());
        }
        if channels == 0 {
            return Err("resampler needs at least one channel".to_string());
        }
        Ok(Resampler {
            in_rate,
            out_rate,
            channels,
            emitted: 0,
            dropped: 0,
            scratch: Vec::new(),
        })
    }

    pub fn is_passthrough(&self) -> bool {
        self.in_rate == self.out_rate
    }

    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    pub fn reset(&mut self) {
        self.emitted = 0;
        self.dropped = 0;
        self.scratch.clear();
    }

    pub fn buffered_input_frames(&self) -> usize {
        self.scratch.len() / self.channels
    }

    fn position(&self) -> f64 {
        self.emitted as f64 * self.in_rate as f64 / self.out_rate as f64 - self.dropped as f64
    }

    pub fn push(&mut self, input: &[f32], output: &mut Vec<f32>) -> Result<(), String> {
        if !input.len().is_multiple_of(self.channels) {
            return Err("resampler input must hold whole frames".to_string());
        }
        if self.is_passthrough() {
            output.extend_from_slice(input);
            return Ok(());
        }
        self.scratch.extend_from_slice(input);
        let available = (self.scratch.len() / self.channels) as f64;
        while self.position() < available - 2.0 {
            let position = self.position();
            let base = position.floor() as usize;
            let frac = (position - base as f64) as f32;
            for channel in 0..self.channels {
                let at = |frame: isize| {
                    if frame < 0 {
                        self.scratch[channel]
                    } else if frame as usize * self.channels + channel >= self.scratch.len() {
                        let last = self.scratch.len() - self.channels + channel;
                        self.scratch[last]
                    } else {
                        self.scratch[frame as usize * self.channels + channel]
                    }
                };
                let base = base as isize;
                output.push(catmull_rom(
                    at(base - 1),
                    at(base),
                    at(base + 1),
                    at(base + 2),
                    frac,
                ));
            }
            self.emitted += 1;
        }
        let drop_frames = (self.position().floor() as usize)
            .saturating_sub(1)
            .min(self.scratch.len() / self.channels);
        self.dropped += drop_frames as u64;
        self.scratch.drain(..drop_frames * self.channels);
        Ok(())
    }

    pub fn finish(&mut self, output: &mut Vec<f32>) -> Result<(), String> {
        if self.is_passthrough() {
            self.scratch.clear();
            self.emitted = 0;
            self.dropped = 0;
            return Ok(());
        }
        if self.scratch.is_empty() {
            return Ok(());
        }
        let input_frames = self.dropped as u128 + (self.scratch.len() / self.channels) as u128;
        let target = (input_frames * self.out_rate as u128).div_ceil(self.in_rate as u128);
        let remaining = target.saturating_sub(self.emitted as u128) as usize;
        let output_start = output.len();
        let mut pad = Vec::new();
        pad.resize(4 * self.channels, 0.0);
        let last = self.scratch.len() - self.channels;
        for frame in pad.chunks_exact_mut(self.channels) {
            frame.copy_from_slice(&self.scratch[last..]);
        }
        self.push(&pad, output)?;
        output.truncate(output_start + remaining * self.channels);
        self.scratch.clear();
        self.emitted = 0;
        self.dropped = 0;
        Ok(())
    }
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * (2.0 * p1 + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f32, frames: usize, channels: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * channels);
        for frame in 0..frames {
            let sample = (2.0 * std::f32::consts::PI * freq * frame as f32 / rate as f32).sin();
            for _ in 0..channels {
                out.push(sample);
            }
        }
        out
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn passthrough_is_bit_exact() {
        let mut resampler = Resampler::new(48000, 48000, 2).unwrap();
        let input = sine(48000, 440.0, 1024, 2);
        let mut output = Vec::new();
        resampler.push(&input, &mut output).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn invalid_rates_are_rejected() {
        assert!(Resampler::new(0, 48000, 2).is_err());
        assert!(Resampler::new(48000, 0, 2).is_err());
        assert!(Resampler::new(48000, 48000, 0).is_err());
    }

    #[test]
    fn partial_frames_are_rejected() {
        let mut resampler = Resampler::new(48000, 44100, 2).unwrap();
        let mut output = Vec::new();
        assert!(resampler.push(&[0.1, 0.2, 0.3], &mut output).is_err());
    }

    #[test]
    fn preserves_tone_level_and_frequency() {
        let mut resampler = Resampler::new(48000, 44100, 2).unwrap();
        let input = sine(48000, 1000.0, 48000, 2);
        let mut output = Vec::new();
        for chunk in input.chunks(4096 * 2) {
            resampler.push(chunk, &mut output).unwrap();
        }
        resampler.finish(&mut output).unwrap();
        let seconds = (output.len() / 2) as f64 / 44100.0;
        assert!((0.99..1.01).contains(&seconds), "duration {seconds}");
        let level = rms(&output);
        assert!(
            (level - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "rms {level}"
        );
        let crossings = output
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| frame[0])
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count();
        assert!(
            (1950..2050).contains(&crossings),
            "zero crossings {crossings}"
        );
    }

    #[test]
    fn split_push_matches_whole_push() {
        let input = sine(48000, 440.0, 8192, 2);
        let mut whole = Resampler::new(48000, 44100, 2).unwrap();
        let mut whole_out = Vec::new();
        whole.push(&input, &mut whole_out).unwrap();
        let mut split = Resampler::new(48000, 44100, 2).unwrap();
        let mut split_out = Vec::new();
        for chunk in input.chunks(997 * 2) {
            split.push(chunk, &mut split_out).unwrap();
        }
        assert_eq!(whole_out, split_out);
    }

    #[test]
    fn silence_stays_silent() {
        let mut resampler = Resampler::new(48000, 44100, 2).unwrap();
        let mut output = Vec::new();
        resampler.push(&vec![0.0; 4096], &mut output).unwrap();
        resampler.finish(&mut output).unwrap();
        assert!(!output.is_empty());
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn capacity_does_not_grow_per_push() {
        let mut resampler = Resampler::new(48000, 44100, 2).unwrap();
        let chunk = sine(48000, 440.0, 512, 2);
        let mut output = Vec::new();
        for _ in 0..4 {
            resampler.push(&chunk, &mut output).unwrap();
        }
        let capacity = resampler.scratch.capacity();
        for _ in 0..32 {
            resampler.push(&chunk, &mut output).unwrap();
        }
        assert_eq!(resampler.scratch.capacity(), capacity);
    }
}
