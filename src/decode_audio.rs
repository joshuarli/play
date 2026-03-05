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

        // Set packet timebase so decoders can handle priming samples (opus, aac)
        let avctx = unsafe { codec_ctx.as_mut_ptr() };
        unsafe {
            (*avctx).pkt_timebase = ffs::AVRational {
                num: stream.time_base().numerator(),
                den: stream.time_base().denominator(),
            };
        }

        // Read channel count from the modern ch_layout API — the old
        // channel_layout bitmask is unset for opus/aac, giving 0 channels.
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
                        let resampler = self.resampler.get_or_insert_with(|| {
                            resampling::Context::get(
                                self.frame.format(),
                                layout,
                                self.frame.rate(),
                                target_fmt,
                                layout,
                                self.frame.rate(),
                            )
                            .expect("Failed to create resampler")
                        });

                        let mut delay = resampler.run(&self.frame, &mut self.resampled).ok();
                        while let Some(Some(_)) = delay.as_ref().map(|d| d.as_ref()) {
                            delay = resampler.flush(&mut self.resampled).ok();
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
                Err(_) => {
                    // No more frames — return whatever's accumulated
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
                fresh
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
