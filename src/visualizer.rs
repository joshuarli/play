use std::sync::{Arc, Mutex};

use crate::decode_audio::AudioBuffer;

// --- VizRing: shared circular sample buffer ---

const RING_SIZE: usize = 4096;

struct VizRingInner {
    buf: Vec<f32>,
    write_pos: usize,
    written: u64,
}

pub struct VizRing(Mutex<VizRingInner>);

impl VizRing {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(VizRingInner {
            buf: vec![0.0; RING_SIZE],
            write_pos: 0,
            written: 0,
        })))
    }

    /// Mono-mix all channels and append to the ring buffer.
    pub fn push_audio(&self, audio: &AudioBuffer) {
        let mut inner = self.0.lock().unwrap();
        let channels = audio.planes.len();
        if channels == 0 || audio.samples_per_channel == 0 {
            return;
        }
        let inv = 1.0 / channels as f32;
        for i in 0..audio.samples_per_channel {
            let mut sum = 0.0f32;
            for ch in &audio.planes {
                sum += ch[i];
            }
            let sample = sum * inv;
            let wp = inner.write_pos;
            inner.buf[wp] = sample;
            inner.write_pos = (wp + 1) % RING_SIZE;
        }
        inner.written += audio.samples_per_channel as u64;
    }

    /// Copy the most recent `out.len()` samples into `out`.
    /// Returns the number of valid samples copied (may be less than `out.len()`
    /// if the ring hasn't been filled that much yet).
    /// `generation` tracks the reader's position — returns 0 if no new data
    /// has been written since the last call (caller should keep previous bars).
    pub fn read_recent(&self, out: &mut [f32], generation: &mut u64) -> usize {
        let inner = self.0.lock().unwrap();
        if inner.written == *generation {
            return 0; // no new data
        }
        *generation = inner.written;
        let available = (inner.written as usize).min(RING_SIZE).min(out.len());
        if available == 0 {
            return 0;
        }
        // Start reading `available` samples back from write_pos
        let start = (inner.write_pos + RING_SIZE - available) % RING_SIZE;
        for i in 0..available {
            out[i] = inner.buf[(start + i) % RING_SIZE];
        }
        available
    }
}

// --- FFT: radix-2 Cooley-Tukey, in-place ---

fn bit_reverse_permute(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
}

/// In-place radix-2 FFT. `re` and `im` must have power-of-2 length.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());
    bit_reverse_permute(re, im);

    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = -std::f32::consts::TAU / len as f32;
        for i in (0..n).step_by(len) {
            for k in 0..half {
                let w = angle * k as f32;
                let (cos_w, sin_w) = (w.cos(), w.sin());
                let u_re = re[i + k];
                let u_im = im[i + k];
                let v_re = re[i + k + half] * cos_w - im[i + k + half] * sin_w;
                let v_im = re[i + k + half] * sin_w + im[i + k + half] * cos_w;
                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + half] = u_re - v_re;
                im[i + k + half] = u_im - v_im;
            }
        }
        len <<= 1;
    }
}

// --- SpectrumAnalyzer ---

pub const FFT_SIZE: usize = 2048;
const MIN_FREQ: f32 = 50.0;
const MAX_FREQ: f32 = 16000.0;
const DB_FLOOR: f32 = -50.0;

pub struct SpectrumAnalyzer {
    hann: [f32; FFT_SIZE],
    re: Vec<f32>,
    im: Vec<f32>,
    smoothed: Vec<f32>,
    prev_raw: Vec<f32>,
}

impl SpectrumAnalyzer {
    pub fn new(max_bars: usize) -> Self {
        let mut hann = [0.0f32; FFT_SIZE];
        for i in 0..FFT_SIZE {
            hann[i] =
                0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos());
        }
        Self {
            hann,
            re: vec![0.0; FFT_SIZE],
            im: vec![0.0; FFT_SIZE],
            smoothed: vec![0.0; max_bars],
            prev_raw: vec![0.0; max_bars],
        }
    }

    /// Compute bar heights from raw samples. Returns normalized [0,1] values.
    /// `samples` should have at least `FFT_SIZE` elements; extras are ignored.
    /// `bar_count` must be <= the `max_bars` passed to `new()`.
    pub fn compute(&mut self, samples: &[f32], bar_count: usize, sample_rate: u32) -> &[f32] {
        // Window + copy into FFT buffers
        let n = samples.len().min(FFT_SIZE);
        for i in 0..n {
            self.re[i] = samples[i] * self.hann[i];
        }
        for i in n..FFT_SIZE {
            self.re[i] = 0.0;
        }
        self.im.iter_mut().for_each(|v| *v = 0.0);

        fft(&mut self.re, &mut self.im);

        // Magnitude spectrum (first half only)
        let half = FFT_SIZE / 2;
        // Reuse `re` buffer for magnitudes
        for i in 0..half {
            self.re[i] = (self.re[i] * self.re[i] + self.im[i] * self.im[i]).sqrt();
        }

        // Log-frequency binning
        let freq_per_bin = sample_rate as f32 / FFT_SIZE as f32;
        let log_min = MIN_FREQ.ln();
        let log_max = MAX_FREQ.ln();

        // Ensure smoothed buffers are large enough
        if self.smoothed.len() < bar_count {
            self.smoothed.resize(bar_count, 0.0);
        }
        if self.prev_raw.len() < bar_count {
            self.prev_raw.resize(bar_count, 0.0);
        }

        for bar in 0..bar_count {
            let t0 = bar as f32 / bar_count as f32;
            let t1 = (bar + 1) as f32 / bar_count as f32;
            let f0 = (log_min + t0 * (log_max - log_min)).exp();
            let f1 = (log_min + t1 * (log_max - log_min)).exp();

            let bin_lo = ((f0 / freq_per_bin) as usize).min(half.saturating_sub(1));
            let bin_hi = ((f1 / freq_per_bin) as usize).max(bin_lo + 1).min(half);

            // Use peak (max) rather than average — gives more responsive display
            let mut peak = 0.0f32;
            for b in bin_lo..bin_hi {
                peak = peak.max(self.re[b]);
            }

            // dB scale — normalize magnitude by FFT_SIZE/2 first so a full-scale
            // sine reads ~0 dB instead of ~66 dB
            let norm_mag = peak / (FFT_SIZE as f32 * 0.5);
            let db = if norm_mag > 1e-10 {
                20.0 * norm_mag.log10()
            } else {
                DB_FLOOR
            };
            let raw = ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0);

            // Temporal smoothing: average with previous raw to reduce jitter,
            // then apply asymmetric exponential smoothing (fast rise, slow fall).
            let blended = (raw + self.prev_raw[bar]) * 0.5;
            self.prev_raw[bar] = raw;

            let prev = self.smoothed[bar];
            let smoothed = if blended > prev {
                // Fast rise: jump most of the way
                prev + 0.6 * (blended - prev)
            } else {
                // Slow fall: gravity-like decay
                prev + 0.12 * (blended - prev)
            };
            self.smoothed[bar] = smoothed;
        }

        &self.smoothed[..bar_count]
    }
}

// --- Rendering ---

const BAR_CHARS: [char; 9] = [
    ' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
    '\u{2588}',
];

/// Compute how many bars fit for the given terminal width.
/// Each bar is 2 chars wide with 1 char gap. Capped at 64.
pub fn bar_count_for_width(cols: u16) -> usize {
    ((cols as usize + 1) / 3).min(64).max(1)
}

/// Render the visualizer bars into a pre-allocated buffer with ANSI escape codes.
/// `bars` contains normalized [0,1] heights. `rows` is the number of text rows.
/// Cursor-positions each row and clears it before drawing.
pub fn render_bars(
    out: &mut String,
    bars: &[f32],
    rows: usize,
    start_row: u16,
    start_col: u16,
    total_width: u16,
) {
    out.clear();
    let total_levels = rows * 8; // 8 sub-levels per row
    let bar_count = bars.len();

    for row in 0..rows {
        // Row 0 = top, row (rows-1) = bottom
        let row_from_bottom = rows - 1 - row;
        let level_base = row_from_bottom * 8;

        // Color gradient: bottom=green, mid=yellow, top=red
        let frac = row_from_bottom as f32 / rows.max(1) as f32;
        let color = if frac < 0.4 {
            "32" // green
        } else if frac < 0.7 {
            "33" // yellow
        } else {
            "31" // red
        };

        // Position cursor at start of this row and clear the viz region
        // Move to column 1, erase entire line, then move to start_col
        out.push_str(&format!(
            "\x1b[{};1H\x1b[2K\x1b[{};{}H\x1b[{}m",
            start_row + row as u16,
            start_row + row as u16,
            start_col,
            color
        ));

        for (i, &h) in bars.iter().enumerate() {
            let filled_levels = (h * total_levels as f32) as usize;
            let char_idx = if filled_levels > level_base + 8 {
                8 // fully filled
            } else if filled_levels > level_base {
                filled_levels - level_base
            } else {
                0 // empty
            };
            let ch = BAR_CHARS[char_idx];
            // 2-char wide bar
            out.push(ch);
            out.push(ch);
            // 1-char gap (except after last bar)
            if i + 1 < bar_count {
                out.push(' ');
            }
        }
    }

    // Reset color
    out.push_str("\x1b[0m");
    let _ = total_width; // reserved for future padding
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viz_ring_push_and_read() {
        let ring = VizRing::new();
        let buf = AudioBuffer {
            planes: vec![vec![1.0; 100], vec![-1.0; 100]],
            samples_per_channel: 100,
            channels: 2,
            sample_rate: 48000,
            pts_us: 0,
        };
        ring.push_audio(&buf);

        let mut generation = 0u64;
        let mut out = [0.0f32; 50];
        let n = ring.read_recent(&mut out, &mut generation);
        assert_eq!(n, 50);
        assert_eq!(generation, 100);
        // Mono mix of 1.0 and -1.0 = 0.0
        for s in &out {
            assert!(s.abs() < 1e-6);
        }
    }

    #[test]
    fn viz_ring_no_new_data_returns_zero() {
        let ring = VizRing::new();
        let buf = AudioBuffer {
            planes: vec![vec![0.5; 100]],
            samples_per_channel: 100,
            channels: 1,
            sample_rate: 48000,
            pts_us: 0,
        };
        ring.push_audio(&buf);

        let mut generation = 0u64;
        let mut out = [0.0f32; 50];
        let n = ring.read_recent(&mut out, &mut generation);
        assert_eq!(n, 50);

        // Second read with no new data — returns 0
        let n2 = ring.read_recent(&mut out, &mut generation);
        assert_eq!(n2, 0);

        // Push more data — should read again
        ring.push_audio(&buf);
        let n3 = ring.read_recent(&mut out, &mut generation);
        assert_eq!(n3, 50);
    }

    #[test]
    fn viz_ring_wraps_around() {
        let ring = VizRing::new();
        let buf = AudioBuffer {
            planes: vec![vec![0.5; RING_SIZE + 100]],
            samples_per_channel: RING_SIZE + 100,
            channels: 1,
            sample_rate: 48000,
            pts_us: 0,
        };
        ring.push_audio(&buf);

        let mut generation = 0u64;
        let mut out = [0.0f32; 100];
        let n = ring.read_recent(&mut out, &mut generation);
        assert_eq!(n, 100);
        for s in &out {
            assert!((s - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn viz_ring_empty_read() {
        let ring = VizRing::new();
        let mut generation = 0u64;
        let mut out = [0.0f32; 10];
        assert_eq!(ring.read_recent(&mut out, &mut generation), 0);
    }

    #[test]
    fn fft_dc_signal() {
        let mut re = vec![1.0f32; FFT_SIZE];
        let mut im = vec![0.0f32; FFT_SIZE];
        fft(&mut re, &mut im);
        // DC bin should have magnitude == FFT_SIZE, others ~0
        assert!((re[0] - FFT_SIZE as f32).abs() < 1e-3);
        for i in 1..FFT_SIZE {
            assert!(
                (re[i] * re[i] + im[i] * im[i]).sqrt() < 1e-3,
                "bin {i} should be ~0"
            );
        }
    }

    #[test]
    fn fft_single_frequency() {
        // Generate a pure sine at bin 64 (= 64 * sample_rate / FFT_SIZE Hz)
        let mut re = vec![0.0f32; FFT_SIZE];
        let mut im = vec![0.0f32; FFT_SIZE];
        let freq_bin = 64;
        for i in 0..FFT_SIZE {
            re[i] = (std::f32::consts::TAU * freq_bin as f32 * i as f32 / FFT_SIZE as f32).sin();
        }
        fft(&mut re, &mut im);

        // Find peak bin
        let mut max_mag = 0.0f32;
        let mut max_bin = 0;
        for i in 0..FFT_SIZE / 2 {
            let mag = (re[i] * re[i] + im[i] * im[i]).sqrt();
            if mag > max_mag {
                max_mag = mag;
                max_bin = i;
            }
        }
        assert_eq!(max_bin, freq_bin);
        assert!(max_mag > FFT_SIZE as f32 * 0.4); // should be ~FFT_SIZE/2
    }

    #[test]
    fn spectrum_analyzer_silence() {
        let mut analyzer = SpectrumAnalyzer::new(32);
        let silence = vec![0.0f32; FFT_SIZE];
        let bars = analyzer.compute(&silence, 32, 48000);
        // All bars should be 0 (or very close after smoothing from 0)
        for &b in bars {
            assert!(b < 0.01, "silent input should produce near-zero bars, got {b}");
        }
    }

    #[test]
    fn bar_count_calculation() {
        assert_eq!(bar_count_for_width(80), 27);
        assert_eq!(bar_count_for_width(200), 64); // capped
        assert_eq!(bar_count_for_width(3), 1);
    }

    #[test]
    fn render_bars_produces_output() {
        let bars = vec![0.5; 8];
        let mut output = String::new();
        render_bars(&mut output, &bars, 4, 5, 10, 80);
        assert!(!output.is_empty());
        // Should contain ANSI escape sequences
        assert!(output.contains("\x1b["));
        // Should contain block characters
        assert!(output.contains('\u{2584}') || output.contains('\u{2588}'));
    }
}
