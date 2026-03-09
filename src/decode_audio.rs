//! Audio decoder: decodes packets to planar f32 PCM via ffmpeg.
//!
//! Small decoded frames are accumulated into larger buffers (~8192 samples per
//! channel) to reduce the number of CoreAudio schedule calls.  The accumulator
//! uses a swap-and-reuse pattern ([`take_accum`]) to avoid reallocating
//! `Vec<f32>` on every buffer.
//!
//! If the input format isn't already planar float32, a software resampler is
//! created lazily on the first non-f32 frame and reused for the file's lifetime.

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::software::resampling;
use ffmpeg_next::util::frame::audio::Audio;
use ffmpeg_sys_next as ffs;

use crate::time::pts_to_us;

/// Target accumulation size before returning a buffer.
/// Larger = fewer AVAudioPCMBuffers scheduled = smoother playback.
/// 8192 samples ≈ 186ms at 44.1kHz, 170ms at 48kHz.
const ACCUM_TARGET: usize = 8192;

/// Audio decoder: decodes packets to planar f32 PCM.
pub struct AudioDecoder {
    decoder: ffmpeg::decoder::Audio,
    resampler: Option<resampling::Context>,
    stream_time_base: ffmpeg::Rational,
    frame: Audio,
    resampled: Audio,
    pub sample_rate: u32,
    pub channels: u16,
    /// Per-channel accumulation planes.
    accum_planes: Vec<Vec<f32>>,
    /// PTS of the first accumulated sample (microseconds).
    accum_pts_us: i64,
    /// Samples per channel accumulated so far.
    accum_count: usize,
}

// SAFETY: AudioDecoder is only accessed from the player thread. The underlying
// ffmpeg decoder/resampler are not thread-safe but we never share across threads.
unsafe impl Send for AudioDecoder {}

/// A decoded audio buffer with timing info. Samples are per-channel planes.
pub struct AudioBuffer {
    /// Per-channel f32 PCM planes: planes[ch][sample].
    pub planes: Vec<Vec<f32>>,
    /// Number of samples per channel.
    pub samples_per_channel: usize,
    /// Number of channels.
    pub channels: u16,
    /// Sample rate.
    pub sample_rate: u32,
    /// Presentation timestamp in microseconds.
    pub pts_us: i64,
}

impl AudioBuffer {
    /// PTS of the sample just past the end of this buffer.
    pub fn end_us(&self) -> i64 {
        self.pts_us + (self.samples_per_channel as i64 * 1_000_000 / self.sample_rate as i64)
    }
}

impl AudioDecoder {
    pub fn new(stream: &ffmpeg::Stream) -> Result<Self> {
        let mut codec_ctx = CodecContext::from_parameters(stream.parameters())
            .context("Failed to create audio codec context")?;

        // SAFETY: as_mut_ptr() returns the underlying AVCodecContext. We set
        // pkt_timebase before opening the decoder so it can handle priming
        // samples (opus, aac). We also read ch_layout.nb_channels from the
        // modern API since the legacy channel_layout bitmask is often unset.
        let avctx = unsafe { codec_ctx.as_mut_ptr() };
        unsafe {
            (*avctx).pkt_timebase = ffs::AVRational {
                num: stream.time_base().numerator(),
                den: stream.time_base().denominator(),
            };
        }

        let channels = unsafe { (*avctx).ch_layout.nb_channels } as u16;

        let decoder = codec_ctx
            .decoder()
            .audio()
            .context("Failed to open audio decoder")?;

        let sample_rate = decoder.rate();

        Ok(Self {
            decoder,
            resampler: None,
            stream_time_base: stream.time_base(),
            frame: Audio::empty(),
            resampled: Audio::empty(),
            sample_rate,
            channels,
            accum_planes: Vec::new(),
            accum_pts_us: 0,
            accum_count: 0,
        })
    }

    pub fn send_packet(&mut self, packet: &ffmpeg::Packet) -> Result<()> {
        self.decoder.send_packet(packet)?;
        Ok(())
    }

    pub fn send_eof(&mut self) -> Result<()> {
        self.decoder.send_eof()?;
        Ok(())
    }

    /// Receive the next decoded audio buffer. Accumulates small decoded
    /// frames into larger buffers (~8192 samples per channel) to reduce
    /// the number of AVAudioPCMBuffers scheduled on the audio engine.
    pub fn receive_buffer(&mut self) -> Option<AudioBuffer> {
        loop {
            match self.decoder.receive_frame(&mut self.frame) {
                Ok(()) => {
                    let pts = self.frame.pts().unwrap_or(0);
                    let pts_us = pts_to_us(pts, self.stream_time_base);

                    // SAFETY: as_ptr() returns the valid AVFrame after successful
                    // receive_frame(). ch_layout.nb_channels is always set.
                    let channels = unsafe { (*self.frame.as_ptr()).ch_layout.nb_channels } as usize;
                    let nb_samples = self.frame.samples();

                    if self.channels == 0 || self.channels != channels as u16 {
                        self.channels = channels as u16;
                    }

                    if nb_samples == 0 || channels == 0 {
                        continue;
                    }

                    let target_fmt =
                        ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Planar);

                    let source: &Audio = if self.frame.format() == target_fmt {
                        &self.frame
                    } else {
                        let layout = ffmpeg::ChannelLayout::default(channels as i32);
                        if self.resampler.is_none() {
                            match resampling::Context::get(
                                self.frame.format(),
                                layout,
                                self.frame.rate(),
                                target_fmt,
                                layout,
                                self.frame.rate(),
                            ) {
                                Ok(r) => {
                                    self.resampler = Some(r);
                                }
                                Err(e) => {
                                    log::error!("Failed to create audio resampler: {e}");
                                    continue;
                                }
                            }
                        }
                        let resampler = self.resampler.as_mut().unwrap();

                        let mut delay = match resampler.run(&self.frame, &mut self.resampled) {
                            Ok(d) => d,
                            Err(e) => {
                                log::warn!("Audio resample error: {e}");
                                continue;
                            }
                        };
                        while delay.is_some() {
                            match resampler.flush(&mut self.resampled) {
                                Ok(d) => delay = d,
                                Err(e) => {
                                    log::warn!("Audio resample flush error: {e}");
                                    break;
                                }
                            }
                        }
                        &self.resampled
                    };

                    let nb_samples = source.samples();
                    if nb_samples == 0 {
                        continue;
                    }

                    // Initialize accumulator planes if needed
                    if self.accum_planes.len() != channels {
                        self.accum_planes = vec![Vec::new(); channels];
                    }

                    // Record PTS of the first frame in this accumulation
                    if self.accum_count == 0 {
                        self.accum_pts_us = pts_us;
                    }

                    // Append decoded samples to per-channel accumulation planes
                    for ch in 0..channels {
                        self.accum_planes[ch].extend_from_slice(source.plane::<f32>(ch));
                    }
                    self.accum_count += nb_samples;

                    // Return accumulated buffer if we've reached the target
                    if self.accum_count >= ACCUM_TARGET {
                        return Some(self.take_accum());
                    }
                    // Otherwise keep decoding more frames
                }
                // EAGAIN (need more input) and EOF (drain complete) are normal.
                // Other errors indicate actual decode failures.
                Err(ref e) if matches!(e, ffmpeg::Error::Eof) => {
                    if self.accum_count > 0 {
                        return Some(self.take_accum());
                    }
                    return None;
                }
                Err(ref e) if matches!(e, ffmpeg::Error::Other { .. }) => {
                    // EAGAIN — return accumulated samples if any
                    if self.accum_count > 0 {
                        return Some(self.take_accum());
                    }
                    return None;
                }
                Err(e) => {
                    log::warn!("Audio receive_frame error: {e}");
                    if self.accum_count > 0 {
                        return Some(self.take_accum());
                    }
                    return None;
                }
            }
        }
    }

    /// Drain any remaining accumulated samples (call after send_eof + receive_buffer loop).
    pub fn drain_accum(&mut self) -> Option<AudioBuffer> {
        if self.accum_count > 0 {
            Some(self.take_accum())
        } else {
            None
        }
    }

    fn take_accum(&mut self) -> AudioBuffer {
        let count = self.accum_count;
        let planes: Vec<Vec<f32>> = self
            .accum_planes
            .iter_mut()
            .map(|plane| {
                let mut fresh = Vec::with_capacity(ACCUM_TARGET);
                std::mem::swap(plane, &mut fresh);
                fresh // the filled plane; `plane` now has ACCUM_TARGET capacity
            })
            .collect();
        self.accum_count = 0;

        AudioBuffer {
            planes,
            samples_per_channel: count,
            channels: self.channels,
            sample_rate: self.sample_rate,
            pts_us: self.accum_pts_us,
        }
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
        self.resampler = None;
        // Discard any partially accumulated buffer
        for plane in &mut self.accum_planes {
            plane.clear();
        }
        self.accum_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- AudioBuffer::end_us ---

    #[test]
    fn end_us_one_second_at_48k() {
        let buf = AudioBuffer {
            planes: vec![vec![0.0; 48000]],
            samples_per_channel: 48000,
            channels: 1,
            sample_rate: 48000,
            pts_us: 0,
        };
        assert_eq!(buf.end_us(), 1_000_000);
    }

    #[test]
    fn end_us_with_pts_offset() {
        let buf = AudioBuffer {
            planes: vec![vec![0.0; 24000]],
            samples_per_channel: 24000,
            channels: 1,
            sample_rate: 48000,
            pts_us: 500_000,
        };
        // 500ms offset + 500ms of samples = 1s
        assert_eq!(buf.end_us(), 1_000_000);
    }

    #[test]
    fn end_us_at_44100() {
        let buf = AudioBuffer {
            planes: vec![vec![0.0; 44100]],
            samples_per_channel: 44100,
            channels: 1,
            sample_rate: 44100,
            pts_us: 0,
        };
        assert_eq!(buf.end_us(), 1_000_000);
    }

    #[test]
    fn end_us_stereo_same_as_mono() {
        let mono = AudioBuffer {
            planes: vec![vec![0.0; 48000]],
            samples_per_channel: 48000,
            channels: 1,
            sample_rate: 48000,
            pts_us: 0,
        };
        let stereo = AudioBuffer {
            planes: vec![vec![0.0; 48000], vec![0.0; 48000]],
            samples_per_channel: 48000,
            channels: 2,
            sample_rate: 48000,
            pts_us: 0,
        };
        // Duration is per-channel, so channel count doesn't affect end_us
        assert_eq!(mono.end_us(), stereo.end_us());
    }

    #[test]
    fn end_us_single_sample() {
        let buf = AudioBuffer {
            planes: vec![vec![0.0; 1]],
            samples_per_channel: 1,
            channels: 1,
            sample_rate: 48000,
            pts_us: 0,
        };
        // 1 sample at 48kHz = 1_000_000 / 48000 = 20us (integer division)
        assert_eq!(buf.end_us(), 20);
    }

    #[test]
    fn end_us_zero_samples() {
        let buf = AudioBuffer {
            planes: vec![],
            samples_per_channel: 0,
            channels: 1,
            sample_rate: 48000,
            pts_us: 1_000_000,
        };
        // No samples → end equals start
        assert_eq!(buf.end_us(), 1_000_000);
    }

    #[test]
    fn end_us_large_buffer() {
        let buf = AudioBuffer {
            planes: vec![vec![0.0; 480000]],
            samples_per_channel: 480000,
            channels: 1,
            sample_rate: 48000,
            pts_us: 0,
        };
        // 10 seconds of audio
        assert_eq!(buf.end_us(), 10_000_000);
    }

    // --- ACCUM_TARGET constant ---

    #[test]
    fn accum_target_is_reasonable() {
        // 8192 samples at 48kHz ≈ 170ms — within acceptable scheduling latency
        let duration_ms = ACCUM_TARGET as f64 / 48000.0 * 1000.0;
        assert!(duration_ms > 100.0 && duration_ms < 250.0);
    }
}
