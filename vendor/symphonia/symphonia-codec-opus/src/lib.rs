// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![warn(rust_2018_idioms)]
#![forbid(unsafe_code)]
// The following lints are allowed in all Symphonia crates. Please see clippy.toml for their
// justification.
#![allow(clippy::comparison_chain)]
#![allow(clippy::excessive_precision)]
#![allow(clippy::identity_op)]
#![allow(clippy::manual_range_contains)]
// Disable to better express the specification.
#![allow(clippy::collapsible_else_if)]

use symphonia_core::audio::sample::SampleFormat;
use symphonia_core::audio::{
    AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef,
};
use symphonia_core::codecs::CodecInfo;
use symphonia_core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia_core::codecs::audio::{AudioCodecParameters, AudioDecoderOptions};
use symphonia_core::codecs::audio::{AudioDecoder, FinalizeResult};
use symphonia_core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia_core::errors::{Error, Result, decode_error, unsupported_error};
use symphonia_core::io::BufReader;
use symphonia_core::packet::PacketRef;
use symphonia_core::support_audio_codec;

use symphonia_common::xiph::audio::opus::OpusHead;

use log::debug;
use ruopus::OpusDecoder as RuOpusDecoder;
use ruopus::packet::PacketError;

const OPUS_HEAD_MAX_VERSION: u8 = 0x0f;
const OPUS_SAMPLE_RATE: u32 = 48_000;
const OPUS_MAX_PACKET_DURATION_MS: u32 = 120;

fn map_packet_error(err: PacketError) -> Error {
    Error::DecodeError(match err {
        PacketError::Empty => "opus: packet is empty",
        PacketError::FrameTooLarge => "opus: frame too large",
        PacketError::Code1UnevenPayload => "opus: code 1 packet payload length is not even",
        PacketError::InvalidFrameLength => "opus: invalid frame length",
        PacketError::InvalidFrameCount => "opus: invalid frame count",
        PacketError::InvalidPadding => "opus: code 3 packet padding overrun",
        PacketError::CbrPayloadNotDivisible => {
            "opus: cbr code 3 payload is not an exact multiple of the frame count"
        }
        _ => "opus: invalid packet",
    })
}

fn parse_opus_head(extra_data: &[u8]) -> Result<OpusHead> {
    let mut reader = BufReader::new(extra_data);
    OpusHead::read(&mut reader, OPUS_HEAD_MAX_VERSION)
}

/// Opus decoder.
pub struct OpusDecoder {
    /// Codec parameters.
    params: AudioCodecParameters,
    /// The underlying Opus decoder.
    decoder: RuOpusDecoder,
    /// Output buffer.
    buf: AudioBuffer<f32>,
    /// The output sample rate.
    sample_rate: u32,
    /// The number of output channels.
    channels: usize,
    /// The number of leading samples (at the output sample rate) left to discard.
    pre_skip: usize,
    /// The OpusHead output gain, as a linear scale factor.
    gain: f32,
}

impl OpusDecoder {
    pub fn try_new(params: &AudioCodecParameters, _opts: &AudioDecoderOptions) -> Result<Self> {
        // This decoder only supports Opus.
        if params.codec != CODEC_ID_OPUS {
            return unsupported_error("opus: invalid codec");
        }

        let sample_rate = params.sample_rate.unwrap_or(OPUS_SAMPLE_RATE);

        if !matches!(sample_rate, 8_000 | 12_000 | 16_000 | 24_000 | 48_000) {
            return unsupported_error("opus: unsupported sample rate");
        }

        let head = match params.extra_data.as_deref() {
            Some(extra_data) if extra_data.starts_with(b"OpusHead") => {
                Some(parse_opus_head(extra_data)?)
            }
            _ => None,
        };

        let channels = match head.as_ref().map(|head| head.channels.clone()).or_else(|| params.channels.clone()) {
            Some(channels) => channels,
            _ => return unsupported_error("opus: channels or channel layout is required"),
        };

        let num_channels = channels.count();

        if !(1..=2).contains(&num_channels) {
            return unsupported_error("opus: unsupported number of channels");
        }

        let pre_skip = head
            .as_ref()
            .map(|head| {
                (u32::from(head.pre_skip) as usize * sample_rate as usize) / OPUS_SAMPLE_RATE as usize
            })
            .unwrap_or(0);

        let gain = head.as_ref().map(|head| head.gain).unwrap_or(0);

        let gain = if gain == 0 {
            1.0
        }
        else {
            10.0f32.powf(f32::from(gain) / (20.0 * 256.0))
        };

        // Update the codec parameters with the information from the OpusHead, and the format of
        // the decoded audio.
        let mut params = params.clone();
        params
            .with_channels(channels.clone())
            .with_sample_rate(sample_rate)
            .with_sample_format(SampleFormat::F32)
            .with_bits_per_sample(32);

        let max_frames = OPUS_MAX_PACKET_DURATION_MS as usize * sample_rate as usize / 1_000;
        let buf = AudioBuffer::new(AudioSpec::new(sample_rate, channels), max_frames);

        debug!("opus: sample rate {}, channels {}, pre-skip {}", sample_rate, num_channels, pre_skip);

        Ok(OpusDecoder {
            params,
            decoder: RuOpusDecoder::with_rate(sample_rate, num_channels),
            buf,
            sample_rate,
            channels: num_channels,
            pre_skip,
            gain,
        })
    }

    fn decode_inner(&mut self, packet: &PacketRef<'_>) -> Result<()> {
        let mut pcm = self.decoder.decode_packet(packet.data).map_err(map_packet_error)?;

        if self.gain != 1.0 {
            for sample in &mut pcm {
                *sample *= self.gain;
            }
        }

        if pcm.len() % self.channels != 0 {
            return decode_error("opus: decoded sample count is not a multiple of channels");
        }

        let frames = pcm.len() / self.channels;

        if frames > self.buf.capacity() {
            return decode_error("opus: decoded packet too large");
        }

        let mut trim_start = packet.trim_start.get() as usize;

        // Pre-skip is expressed in 48 kHz samples. Discard it from the start of the stream, even
        // when it spans more than one packet.
        if self.pre_skip > 0 {
            let skip = self.pre_skip.min(frames.saturating_sub(trim_start));
            trim_start += skip;
            self.pre_skip -= skip;
        }

        self.buf.clear();
        self.buf.render_uninit(Some(frames));
        self.buf.copy_from_slice_interleaved(&pcm);
        self.buf.trim(trim_start, packet.trim_end.get() as usize);

        Ok(())
    }
}

impl AudioDecoder for OpusDecoder {
    fn reset(&mut self) {
        self.decoder = RuOpusDecoder::with_rate(self.sample_rate, self.channels);
        self.buf.clear();
        self.pre_skip = 0;
    }

    fn codec_info(&self) -> &CodecInfo {
        &Self::supported_codecs().first().expect("at least one codec registered").info
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.params
    }

    fn decode_ref(&mut self, packet: &PacketRef<'_>) -> Result<GenericAudioBufferRef<'_>> {
        if let Err(e) = self.decode_inner(packet) {
            self.buf.clear();
            Err(e)
        }
        else {
            Ok(self.buf.as_generic_audio_buffer_ref())
        }
    }

    fn finalize(&mut self) -> FinalizeResult {
        Default::default()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.buf.as_generic_audio_buffer_ref()
    }
}

impl RegisterableAudioDecoder for OpusDecoder {
    fn try_registry_new(
        params: &AudioCodecParameters,
        opts: &AudioDecoderOptions,
    ) -> Result<Box<dyn AudioDecoder>>
    where
        Self: Sized,
    {
        Ok(Box::new(OpusDecoder::try_new(params, opts)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &[support_audio_codec!(CODEC_ID_OPUS, "opus", "Opus")]
    }
}
