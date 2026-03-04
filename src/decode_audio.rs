use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::software::resampling;
use ffmpeg_next::util::frame::audio::Audio;

use crate::time::pts_to_us;

/// Audio decoder: decodes packets to interleaved f32 PCM.
pub struct AudioDecoder {
    decoder: ffmpeg::decoder::Audio,
    resampler: Option<resampling::Context>,
    stream_time_base: ffmpeg::Rational,
    frame: Audio,
    resampled: Audio,
    sample_buf: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

unsafe impl Send for AudioDecoder {}

/// A decoded audio buffer with timing info.
pub struct AudioBuffer {
    /// Interleaved f32 PCM samples.
    pub samples: Vec<f32>,
    /// Number of channels.
    pub channels: u16,
    /// Sample rate.
    pub sample_rate: u32,
    /// Presentation timestamp in microseconds.
    pub pts_us: i64,
}

impl AudioDecoder {
    pub fn new(stream: &ffmpeg::Stream) -> Result<Self> {
        let codec_ctx = CodecContext::from_parameters(stream.parameters())
            .context("Failed to create audio codec context")?;
        let decoder = codec_ctx
            .decoder()
            .audio()
            .context("Failed to open audio decoder")?;

        let sample_rate = decoder.rate();
        let channels = decoder.channel_layout().channels() as u16;

        Ok(Self {
            decoder,
            resampler: None,
            stream_time_base: stream.time_base(),
            frame: Audio::empty(),
            resampled: Audio::empty(),
            sample_buf: Vec::new(),
            sample_rate,
            channels,
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

    pub fn receive_buffer(&mut self) -> Option<AudioBuffer> {
        match self.decoder.receive_frame(&mut self.frame) {
            Ok(()) => {
                let pts = self.frame.pts().unwrap_or(0);
                let pts_us = pts_to_us(pts, self.stream_time_base);

                // Ensure resampler is set up (to f32 planar → f32 interleaved)
                let resampler = self.resampler.get_or_insert_with(|| {
                    resampling::Context::get(
                        self.frame.format(),
                        self.frame.channel_layout(),
                        self.frame.rate(),
                        ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                        self.frame.channel_layout(),
                        self.frame.rate(),
                    )
                    .expect("Failed to create resampler")
                });

                let mut delay = resampler.run(&self.frame, &mut self.resampled).ok();

                // Drain any remaining samples
                while let Some(Some(_)) = delay.as_ref().map(|d| d.as_ref()) {
                    delay = resampler.flush(&mut self.resampled).ok();
                }

                // Extract f32 samples from resampled frame
                let nb_samples = self.resampled.samples() as usize;
                let channels = self.channels as usize;
                let total = nb_samples * channels;

                if total == 0 {
                    return None;
                }

                let data_ptr = self.resampled.data(0);
                self.sample_buf.clear();
                self.sample_buf.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(data_ptr.as_ptr() as *const f32, total)
                });

                Some(AudioBuffer {
                    samples: std::mem::take(&mut self.sample_buf),
                    channels: self.channels,
                    sample_rate: self.sample_rate,
                    pts_us,
                })
            }
            Err(_) => None,
        }
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
        self.resampler = None; // recreate on next decode
    }
}
