// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// Previous Author: Kostya Shishkov <kostya.shiskov@gmail.com>
//
// This source file includes code originally written for the NihAV
// project. With the author's permission, it has been relicensed for,
// and ported to the Symphonia project.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use symphonia_core::audio::{
    AsGenericAudioBufferRef, AudioBuffer, AudioSpec, Channels, GenericAudioBufferRef, Position,
};
use symphonia_core::codecs::CodecInfo;
use symphonia_core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia_core::codecs::audio::{AudioCodecParameters, AudioDecoderOptions};
use symphonia_core::codecs::audio::{AudioDecoder, FinalizeResult};
use symphonia_core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia_core::errors::{Result, unsupported_error};
use symphonia_core::io::{BitReaderLtr, FiniteBitStream, ReadBitsLtr};
use symphonia_core::packet::PacketRef;
use symphonia_core::{codec_profile, support_audio_codec};

use symphonia_common::mpeg::audio::{AacElementType, AacProgramConfig, AudioObjectType, AudioSpecificConfig};

mod codebooks;
mod common;
mod cpe;
mod dsp;
mod ics;
mod window;

use common::*;

/// Advanced Audio Coding (AAC) decoder.
///
/// Implements a decoder for Advanced Audio Decoding Low-Complexity (AAC-LC) as defined in
/// ISO/IEC 13818-7 and ISO/IEC 14496-3.
/// Map the bitstream element order of a standard channel configuration (ISO/IEC 14496-3,
/// Table 1.19) to planes of a buffer laid out in speaker-position order.
fn element_planes(channels: &Channels) -> Option<Vec<usize>> {
    let mask = match channels {
        Channels::Positioned(mask) => mask,
        Channels::Discrete(count @ 1..=2) => return Some((0..*count as usize).collect()),
        _ => return None,
    };
    let order: &[Position] = match channels.count() {
        1 => &[Position::FRONT_CENTER],
        2 => &[Position::FRONT_LEFT, Position::FRONT_RIGHT],
        3 => &[Position::FRONT_CENTER, Position::FRONT_LEFT, Position::FRONT_RIGHT],
        4 => &[
            Position::FRONT_CENTER,
            Position::FRONT_LEFT,
            Position::FRONT_RIGHT,
            Position::REAR_CENTER,
        ],
        5 => &[
            Position::FRONT_CENTER,
            Position::FRONT_LEFT,
            Position::FRONT_RIGHT,
            Position::REAR_LEFT,
            Position::REAR_RIGHT,
        ],
        6 => &[
            Position::FRONT_CENTER,
            Position::FRONT_LEFT,
            Position::FRONT_RIGHT,
            Position::REAR_LEFT,
            Position::REAR_RIGHT,
            Position::LFE1,
        ],
        8 if mask.contains(Position::SIDE_LEFT) => &[
            Position::FRONT_CENTER,
            Position::FRONT_LEFT,
            Position::FRONT_RIGHT,
            Position::SIDE_LEFT,
            Position::SIDE_RIGHT,
            Position::REAR_LEFT,
            Position::REAR_RIGHT,
            Position::LFE1,
        ],
        8 => &[
            Position::FRONT_CENTER,
            Position::FRONT_LEFT_CENTER,
            Position::FRONT_RIGHT_CENTER,
            Position::FRONT_LEFT,
            Position::FRONT_RIGHT,
            Position::REAR_LEFT,
            Position::REAR_RIGHT,
            Position::LFE1,
        ],
        _ => return None,
    };
    let expected = order.iter().fold(Position::empty(), |all, position| all | *position);
    if expected != *mask && channels.count() > 1 {
        return None;
    }
    if channels.count() == 1 {
        return Some(vec![0]);
    }
    Some(order.iter().map(|position| (mask.bits() & (position.bits() - 1)).count_ones() as usize).collect())
}

pub struct AacDecoder {
    // info: NACodecInfoRef,
    asc: AudioSpecificConfig,
    allow_inband_pce: bool,
    pairs: Vec<cpe::ChannelPair>,
    /// Output buffer plane for each channel in bitstream element order.
    planes: Vec<usize>,
    dsp: dsp::Dsp,
    sbinfo: GASubbandInfo,
    params: AudioCodecParameters,
    buf: AudioBuffer<f32>,
}

impl AacDecoder {
    pub fn try_new(params: &AudioCodecParameters, _opts: &AudioDecoderOptions) -> Result<Self> {
        // This decoder only supports AAC.
        if params.codec != CODEC_ID_AAC {
            return unsupported_error("aac: invalid codec");
        }

        // If extra data present, parse the audio specific config
        let allow_inband_pce = params.extra_data.is_none();
        let asc = if let Some(extra_data_buf) = &params.extra_data {
            validate!(extra_data_buf.len() >= 2);
            AudioSpecificConfig::read(extra_data_buf)?
        }
        else {
            // Otherwise, assume there is no ASC and use the codec parameters for ADTS.
            let mut asc = AudioSpecificConfig::default();

            asc.object_type = AudioObjectType::Lc;
            asc.samples = 1024;

            asc.sample_rate = match params.sample_rate {
                Some(rate) => rate,
                None => return unsupported_error("aac: sample rate is required"),
            };

            asc.channels = params.channels.clone();

            asc
        };

        if asc.object_type != AudioObjectType::Lc || asc.sbr_present || asc.samples != 1024 {
            return unsupported_error("aac: aac too complex");
        }
        let mut params = params.clone();
        params.with_sample_rate(asc.sample_rate);
        let sbinfo = GASubbandInfo::find(asc.sample_rate);
        // An in-band PCE may be needed before the actual buffer can be allocated.
        let initial_channels = asc.channels.clone().unwrap_or(Channels::Discrete(1));
        let buf = AudioBuffer::new(AudioSpec::new(asc.sample_rate, initial_channels), asc.samples);
        let mut decoder = AacDecoder {
            asc, allow_inband_pce, pairs: Vec::new(), planes: Vec::new(), dsp: dsp::Dsp::new(), sbinfo, params, buf,
        };
        if let Some(pce) = decoder.asc.program_config.clone() {
            decoder.install_pce(pce)?;
        }
        else if let Some(channels) = decoder.asc.channels.clone() {
            decoder.planes = match element_planes(&channels) {
                Some(planes) => planes,
                None if allow_inband_pce => Vec::new(),
                None => return unsupported_error("aac: channel layout has no standard element order"),
            };
            decoder.params.with_channels(channels);
        }
        Ok(decoder)
    }

    fn install_pce(&mut self, pce: AacProgramConfig) -> Result<()> {
        validate!(pce.object_type == self.asc.object_type && pce.sample_rate == self.asc.sample_rate);
        if let Some(existing) = &self.asc.program_config {
            validate!(*existing == pce);
            if !self.planes.is_empty() { return Ok(()); }
        }
        else if self.asc.channels.is_some() && !self.allow_inband_pce {
            return unsupported_error("aac: in-band PCE changes fixed channel configuration");
        }
        let mask = match &pce.channels {
            Channels::Positioned(mask) => *mask,
            _ => return unsupported_error("aac: invalid PCE layout"),
        };
        let mut planes = Vec::new();
        let mut pairs = Vec::new();
        for element in &pce.elements {
            let channel = planes.len();
            for position in &element.positions {
                planes.push((mask.bits() & (position.bits() - 1)).count_ones() as usize);
            }
            pairs.push(cpe::ChannelPair::new(element.kind == AacElementType::Pair, channel, self.sbinfo));
        }
        self.buf = AudioBuffer::new(AudioSpec::new(self.asc.sample_rate, pce.channels.clone()), self.asc.samples);
        self.buf.render_uninit(None);
        self.params.with_channels(pce.channels.clone());
        self.asc.channels = Some(pce.channels.clone());
        self.asc.program_config = Some(pce);
        self.planes = planes;
        self.pairs = pairs;
        Ok(())
    }

    fn set_pair(&mut self, pair_no: usize, channel: usize, pair: bool) -> Result<()> {
        if self.pairs.len() <= pair_no {
            self.pairs.push(cpe::ChannelPair::new(pair, channel, self.sbinfo));
        }
        else {
            validate!(self.pairs[pair_no].channel == channel);
            validate!(self.pairs[pair_no].is_pair == pair);
        }

        let max_channels = self.asc.channels.as_ref().map_or(0, |channels| channels.count());
        validate!(if pair { channel + 1 } else { channel } < max_channels);

        Ok(())
    }

    fn decode_ga<B: ReadBitsLtr + FiniteBitStream>(&mut self, bs: &mut B) -> Result<()> {
        let mut cur_pair = 0;
        let mut cur_ch = 0;
        let mut seen = vec![false; self.pairs.len()];
        while bs.bits_left() > 3 {
            let id = bs.read_bits_leq32(3)?;

            match id {
                0 | 1 | 3 => {
                    let tag = bs.read_bits_leq32(4)? as u8;
                    let kind = match id {
                        0 => AacElementType::Single,
                        1 => AacElementType::Pair,
                        3 => AacElementType::Lfe,
                        _ => unreachable!(),
                    };
                    let pair = kind == AacElementType::Pair;
                    let (pair_no, channel) = if let Some(pce) = &self.asc.program_config {
                        let index = match pce.elements.iter().position(|e| e.kind == kind && e.tag == tag) {
                            Some(index) => index,
                            None => return symphonia_core::errors::decode_error("aac: element absent from PCE"),
                        };
                        validate!(!seen[index]);
                        seen[index] = true;
                        (index, self.pairs[index].channel)
                    } else {
                        validate!(!self.planes.is_empty());
                        (cur_pair, cur_ch)
                    };
                    self.set_pair(pair_no, channel, pair)?;
                    if pair { self.pairs[pair_no].decode_ga_cpe(bs, self.asc.object_type)?; }
                    else { self.pairs[pair_no].decode_ga_sce(bs, self.asc.object_type)?; }
                    cur_pair += 1;
                    cur_ch += if pair { 2 } else { 1 };
                }
                2 => {
                    // ID_CCE
                    return unsupported_error("aac: coupling channel element");
                }
                4 => {
                    // ID_DSE
                    let _id = bs.read_bits_leq32(4)?;
                    let align = bs.read_bool()?;
                    let mut count = bs.read_bits_leq32(8)?;
                    if count == 255 {
                        count += bs.read_bits_leq32(8)?;
                    }
                    if align {
                        bs.realign(); // ????
                    }
                    bs.ignore_bits(count * 8)?; // no SBR payload or such
                }
                5 => {
                    // ID_PCE
                    validate!(cur_pair == 0);
                    let pce = AacProgramConfig::read(bs)?;
                    self.install_pce(pce)?;
                    seen = vec![false; self.pairs.len()];
                }
                6 => {
                    // ID_FIL
                    let mut count = bs.read_bits_leq32(4)? as usize;
                    if count == 15 {
                        count += bs.read_bits_leq32(8)? as usize;
                        count -= 1;
                    }

                    // Check if the ID_FIL element contains SBR data. Note that ID_FIL elements with
                    // SBR data may not contain other extension payloads.
                    if count > 0 {
                        let ext_type = bs.read_bits_leq32(4)?;

                        match ext_type {
                            // EXT_SBR_DATA (0xd)
                            // EXT_SBR_DATA_CRC (0xe)
                            0xd | 0xe => self.asc.sbr_present = true,
                            // EXT_FILL (0x0)
                            // EXT_FILL_DATA (0x1)
                            // EXT_DATA_ELEMENT (0x2)
                            // EXT_DYNAMIC_RANGE (0xb)
                            // EXT_SAC_DATA (0xc)
                            _ => (),
                        }

                        // Ignore extension payload(s).
                        bs.ignore_bits(4)?;
                        for _ in 0..count - 1 {
                            bs.ignore_bits(8)?;
                        }
                    }
                }
                7 => {
                    // ID_TERM
                    break;
                }
                _ => unreachable!(),
            };
        }
        if self.asc.program_config.is_some() {
            validate!(seen.iter().all(|seen| *seen));
        } else {
            validate!(cur_ch == self.planes.len() && cur_ch > 0);
        }
        let rate_idx = GASubbandInfo::find_idx(self.asc.sample_rate);
        for pair in 0..cur_pair {
            self.pairs[pair].synth_audio(&mut self.dsp, &mut self.buf, rate_idx, &self.planes);
        }
        Ok(())
    }

    // fn flush(&mut self) {
    //     for pair in self.pairs.iter_mut() {
    //         pair.ics[0].delay = [0.0; 1024];
    //         pair.ics[1].delay = [0.0; 1024];
    //     }
    // }

    fn decode_inner(&mut self, packet: &PacketRef<'_>) -> Result<()> {
        // Clear the audio output buffer.
        self.buf.clear();
        self.buf.render_uninit(None);

        let mut bs = BitReaderLtr::new(packet.data);

        // Choose decode step based on the object type.
        match self.asc.object_type {
            AudioObjectType::Lc => self.decode_ga(&mut bs)?,
            _ => return unsupported_error("aac: object type"),
        }

        Ok(())
    }
}

impl AudioDecoder for AacDecoder {
    fn reset(&mut self) {
        for pair in self.pairs.iter_mut() {
            pair.reset();
        }
    }

    fn codec_info(&self) -> &CodecInfo {
        // Only one codec is supported.
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

impl RegisterableAudioDecoder for AacDecoder {
    fn try_registry_new(
        params: &AudioCodecParameters,
        opts: &AudioDecoderOptions,
    ) -> Result<Box<dyn AudioDecoder>>
    where
        Self: Sized,
    {
        Ok(Box::new(AacDecoder::try_new(params, opts)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        use symphonia_core::codecs::audio::well_known::profiles::CODEC_PROFILE_AAC_LC;

        &[support_audio_codec!(
            CODEC_ID_AAC,
            "aac",
            "Advanced Audio Coding",
            &[codec_profile!(CODEC_PROFILE_AAC_LC, "aac-lc", "Low Complexity"),]
        )]
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use symphonia_core::audio::Audio;
    use symphonia_core::units::{Duration, Timestamp};

    // FFmpeg 9.0.2 AAC-LC encodes one distinct tone per speaker at 48 kHz.
    // MP4 payload fixtures contain big-endian u32 lengths followed by raw AAC packets.
    const QUAD_ASC: [u8; 24] = [
        0x11, 0x80, 0x04, 0xc4, 0x04, 0x00, 0x21, 0x10, 0x0c, 0x4c, 0x61, 0x76,
        0x63, 0x36, 0x33, 0x2e, 0x31, 0x2e, 0x31, 0x30, 0x32, 0x56, 0xe5, 0x00,
    ];

    fn packets(mut data: &[u8], adts: bool) -> Vec<&[u8]> {
        let mut packets = Vec::new();
        while !data.is_empty() {
            let (header, len) = if adts {
                (7, ((usize::from(data[3]) & 3) << 11) | (usize::from(data[4]) << 3)
                    | (usize::from(data[5]) >> 5))
            } else {
                (4, 4 + u32::from_be_bytes(data[..4].try_into().unwrap()) as usize)
            };
            packets.push(&data[header..len]);
            data = &data[len..];
        }
        packets
    }

    fn decoder(extra: Option<&[u8]>) -> AacDecoder {
        let mut params = AudioCodecParameters::new();
        params.for_codec(CODEC_ID_AAC).with_sample_rate(48000);
        if let Some(extra) = extra { params.extra_data = Some(extra.into()); }
        AacDecoder::try_new(&params, &AudioDecoderOptions::default()).unwrap()
    }

    fn check_tones(decoder: &mut AacDecoder, packets: &[&[u8]], expected: &[f32]) {
        let mut energy = vec![vec![0.0f64; expected.len()]; expected.len()];
        for (n, data) in packets.iter().enumerate() {
            let packet = PacketRef::new(0, Timestamp::new(0), Duration::new(1024), data);
            decoder.decode_inner(&packet).unwrap();
            if n < 2 { continue; }
            for (ch, row) in energy.iter_mut().enumerate() {
                let samples = decoder.buf.plane(ch).unwrap();
                for (tone, freq) in expected.iter().enumerate() {
                    let (mut re, mut im) = (0.0, 0.0);
                    for (i, sample) in samples.iter().enumerate() {
                        let phase = 2.0 * std::f64::consts::PI * f64::from(*freq) * i as f64 / 48000.0;
                        re += f64::from(*sample) * phase.cos();
                        im += f64::from(*sample) * phase.sin();
                    }
                    row[tone] += re * re + im * im;
                }
            }
        }
        for (ch, row) in energy.iter().enumerate() {
            let strongest = row.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
            assert_eq!(strongest, ch, "channel {ch}, energies {row:?}");
        }
    }

    #[test]
    fn config_12_routes_each_speaker() {
        let data = include_bytes!("../../tests/fixtures/layout-7.1.packets");
        let packets = packets(data, false);
        let mut decoder = decoder(Some(&[0x11, 0xe0]));
        check_tones(&mut decoder, &packets, &[310.0, 410.0, 510.0, 70.0, 610.0, 710.0, 810.0, 910.0]);
    }

    #[test]
    fn in_band_pce_allocates_and_routes_quad() {
        let data = include_bytes!("../../tests/fixtures/layout-quad.aac");
        let packets = packets(data, true);
        let mut decoder = decoder(None);
        check_tones(&mut decoder, &packets, &[310.0, 410.0, 610.0, 710.0]);
        assert_eq!(decoder.params.channels, Some(Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT | Position::REAR_LEFT | Position::REAR_RIGHT)));
    }
    #[test]
    fn asc_pce_allocates_and_routes_quad() {
        let data = include_bytes!("../../tests/fixtures/layout-quad.packets");
        let packets = packets(data, false);
        let asc = QUAD_ASC;
        let mut decoder = decoder(Some(&asc));
        check_tones(&mut decoder, &packets, &[310.0, 410.0, 610.0, 710.0]);
        for end in 0..9 { assert!(AudioSpecificConfig::read(&asc[..end]).is_err()); }
        let mut duplicate = asc;
        duplicate[7] &= 0x0f;
        assert!(AudioSpecificConfig::read(&duplicate).is_err());
        let mut wrong_rate = asc;
        wrong_rate[2] ^= 0x04;
        assert!(AudioSpecificConfig::read(&wrong_rate).is_err());
    }

    #[test]
    fn pce_routes_by_element_tag_instead_of_declaration_order() {
        let data = include_bytes!("../../tests/fixtures/layout-quad.packets");
        let packets = packets(data, false);
        let asc = QUAD_ASC;
        let mut decoder = decoder(Some(&asc));
        let mut pce = decoder.asc.program_config.take().unwrap();
        pce.elements.reverse();
        decoder.asc.channels = None;
        decoder.planes.clear();
        decoder.install_pce(pce).unwrap();
        check_tones(&mut decoder, &packets, &[310.0, 410.0, 610.0, 710.0]);
    }

    #[test]
    fn pce_rejects_unknown_and_missing_elements() {
        let data = include_bytes!("../../tests/fixtures/layout-quad.packets");
        let len = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
        let asc = QUAD_ASC;
        let mut decoder = decoder(Some(&asc));
        decoder.asc.program_config.as_mut().unwrap().elements[0].tag = 15;
        let packet = PacketRef::new(0, Timestamp::new(0), Duration::new(1024), &data[4..4 + len]);
        assert!(decoder.decode_inner(&packet).is_err());
        let mut no_config = self::decoder(None);
        assert!(no_config.decode_inner(&packet).is_err());
    }

    #[test]
    fn pce_routes_wide_front_and_rear_pairs() {
        let data = include_bytes!("../../tests/fixtures/layout-wide-pce.aac");
        let packets = packets(data, true);
        let mut decoder = decoder(None);
        check_tones(&mut decoder, &packets, &[310.0, 410.0, 510.0, 70.0, 610.0, 710.0, 810.0, 910.0]);
    }

    #[test]
    fn in_band_pce_overrides_demuxer_layout() {
        let data = include_bytes!("../../tests/fixtures/layout-quad.aac");
        let packets = packets(data, true);
        let mut params = AudioCodecParameters::new();
        params.for_codec(CODEC_ID_AAC).with_sample_rate(48000).with_channels(Channels::Discrete(4));
        let mut decoder = AacDecoder::try_new(&params, &AudioDecoderOptions::default()).unwrap();
        check_tones(&mut decoder, &packets, &[310.0, 410.0, 610.0, 710.0]);
    }

}
