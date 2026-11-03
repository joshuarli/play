use crate::mix::{layout_of, mix_into, Layout};
use crate::resample::Resampler;
use crate::session::DecoderSession;
use player_media::{Packet as MediaPacket, StreamInfo};
use symphonia::core::audio::Channels;

const CHUNK_FRAMES: usize = 2048;

pub struct ReadyChunk {
    pub pts: f64,
    pub samples: Vec<f32>,
}

pub struct PacketDecoder {
    decoder: DecoderSession,
    source_layout: Layout,
    target_layout: Layout,
    resampler: Resampler,
    target_rate: u32,
    mixed: Vec<f32>,
    converted: Vec<f32>,
    chunk_pts: Option<f64>,
}

fn channels_of(info: &StreamInfo) -> Channels {
    if info.channel_mask != 0 {
        if let Some(channels) = symphonia::core::audio::Position::from_bits(info.channel_mask)
            .map(Channels::Positioned)
        {
            return channels;
        }
    }
    Channels::Discrete(info.channels.max(0) as u16)
}

impl PacketDecoder {
    pub fn open(stream: &StreamInfo, target_rate: u32) -> Result<Self, String> {
        if target_rate == 0 {
            return Err("decoder needs a positive output rate".to_string());
        }
        if stream.sample_rate <= 0 {
            return Err("audio stream has no sample rate".to_string());
        }
        let source_layout = layout_of(&channels_of(stream)).ok_or_else(|| {
            format!(
                "audio layout of '{}' has no pipeline mapping",
                stream.codec_name
            )
        })?;
        let target_layout = match source_layout {
            Layout::Mono => Layout::Mono,
            Layout::Stereo => Layout::Stereo,
            Layout::FiveOne => Layout::Stereo,
        };
        let resampler = Resampler::new(
            stream.sample_rate as u32,
            target_rate,
            target_layout.channels(),
        )?;
        let decoder = DecoderSession::open(stream)?;
        Ok(PacketDecoder {
            decoder,
            source_layout,
            target_layout,
            resampler,
            target_rate,
            mixed: Vec::new(),
            converted: Vec::new(),
            chunk_pts: None,
        })
    }

    pub fn source_layout(&self) -> Layout {
        self.source_layout
    }

    pub fn target_layout(&self) -> Layout {
        self.target_layout
    }

    pub fn target_rate(&self) -> u32 {
        self.target_rate
    }

    pub fn feed(&mut self, packet: &MediaPacket, packet_pts: f64) -> Result<Option<ReadyChunk>, String> {
        let decoded = self.decoder.decode(packet)?;
        let layout = layout_of(&decoded.channels)
            .ok_or_else(|| "decoded audio layout has no pipeline mapping".to_string())?;
        if layout != self.source_layout {
            return Err("audio layout changed mid-stream".to_string());
        }
        if decoded.sample_rate != self.resampler.in_rate() {
            self.resampler = Resampler::new(
                decoded.sample_rate,
                self.target_rate,
                self.target_layout.channels(),
            )?;
        }
        if self.converted.is_empty() {
            self.chunk_pts = Some(packet_pts);
        }
        self.mixed.clear();
        mix_into(
            &decoded.pcm,
            &decoded.channels,
            self.target_layout,
            &mut self.mixed,
        );
        self.resampler.push(&self.mixed, &mut self.converted)?;
        if self.converted.len() >= CHUNK_FRAMES * self.target_layout.channels() {
            Ok(Some(self.take_chunk()))
        } else {
            Ok(None)
        }
    }

    pub fn finish(&mut self) -> Result<Option<ReadyChunk>, String> {
        let mut tail = Vec::new();
        self.resampler.finish(&mut tail)?;
        self.converted.extend_from_slice(&tail);
        self.resampler.reset();
        if self.converted.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.take_chunk()))
        }
    }

    pub fn reset(&mut self) {
        self.decoder.reset();
        self.resampler.reset();
        self.mixed.clear();
        self.converted.clear();
        self.chunk_pts = None;
    }

    fn take_chunk(&mut self) -> ReadyChunk {
        let pts = self.chunk_pts.take().unwrap_or(0.0);
        ReadyChunk {
            pts,
            samples: std::mem::take(&mut self.converted),
        }
    }
}
