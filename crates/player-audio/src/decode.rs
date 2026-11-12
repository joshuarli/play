use crate::mix::{mix_into, Layout};
use crate::resample::Resampler;
use crate::session::DecoderSession;
use player_media::{Packet as MediaPacket, StreamInfo};
use symphonia::core::audio::Channels;

pub struct ReadyChunk {
    pub pts: f64,
    pub samples: Vec<f32>,
}

pub struct PacketDecoder {
    decoder: DecoderSession,
    source_channels: Channels,
    target_layout: Layout,
    resampler: Resampler,
    target_rate: u32,
    mixed: Vec<f32>,
    converted: Vec<f32>,
    output_cursor: Option<f64>,
    expected_source_pts: Option<f64>,
    timestamp_tick: f64,
}

fn channels_of(info: &StreamInfo) -> Channels {
    if info.channel_mask != 0
        && let Some(channels) = symphonia::core::audio::Position::from_bits(info.channel_mask)
            .map(Channels::Positioned)
    {
        return channels;
    }
    Channels::Discrete(info.channels.max(0) as u16)
}

pub fn target_layout(info: &StreamInfo) -> Result<Layout, String> {
    match channels_of(info).count() {
        0 => Err(format!("audio stream '{}' has no channels", info.codec_name)),
        1 => Ok(Layout::Mono),
        _ => Ok(Layout::Stereo),
    }
}

impl PacketDecoder {
    pub fn open(stream: &StreamInfo, target_rate: u32) -> Result<Self, String> {
        let target_layout = target_layout(stream)?;
        Self::open_with_layout(stream, target_rate, target_layout)
    }

    pub fn open_with_layout(
        stream: &StreamInfo,
        target_rate: u32,
        target_layout: Layout,
    ) -> Result<Self, String> {
        if target_rate == 0 {
            return Err("decoder needs a positive output rate".to_string());
        }
        if stream.sample_rate <= 0 {
            return Err("audio stream has no sample rate".to_string());
        }
        let source_channels = channels_of(stream);
        if source_channels.count() == 0 {
            return Err(format!("audio stream '{}' has no channels", stream.codec_name));
        }
        let resampler = Resampler::new(
            stream.sample_rate as u32,
            target_rate,
            target_layout.channels(),
        )?;
        let decoder = DecoderSession::open(stream)?;
        Ok(PacketDecoder {
            decoder,
            source_channels,
            target_layout,
            resampler,
            target_rate,
            mixed: Vec::new(),
            converted: Vec::new(),
            output_cursor: None,
            expected_source_pts: None,
            timestamp_tick: if stream.time_base.1 != 0 {
                (stream.time_base.0 as f64 / stream.time_base.1 as f64).abs()
            } else {
                0.0
            },
        })
    }

    pub fn source_channels(&self) -> usize {
        self.source_channels.count()
    }

    pub fn target_layout(&self) -> Layout {
        self.target_layout
    }

    pub fn target_rate(&self) -> u32 {
        self.target_rate
    }

    pub fn feed(&mut self, packet: &MediaPacket) -> Result<Vec<ReadyChunk>, String> {
        let packet_pts = packet.pts_seconds.or(self.expected_source_pts).unwrap_or(0.0);
        let decoded = self.decoder.decode(packet)?;
        if decoded.channels.count() != self.source_channels.count() {
            return Err(format!(
                "audio layout changed mid-stream from {:?} to {:?}",
                self.source_channels, decoded.channels
            ));
        }
        if decoded.frames == 0 {
            return Ok(Vec::new());
        }
        if decoded.sample_rate == 0 || !packet_pts.is_finite() {
            return Err("audio packet has an invalid rate or timestamp".to_string());
        }
        let mut ready = Vec::with_capacity(2);
        let rate_changed = decoded.sample_rate != self.resampler.in_rate();
        let ticks_per_frame = if self.timestamp_tick > 0.0 {
            1.0 / (decoded.sample_rate as f64 * self.timestamp_tick)
        } else {
            0.0
        };
        let sample_aligned = ticks_per_frame >= 1.0
            && (ticks_per_frame - ticks_per_frame.round()).abs() < 1e-8;
        let tolerance = self.timestamp_tick * if sample_aligned { 0.5 } else { 1.0 } + 1e-9;
        let discontinuous = self.expected_source_pts
            .is_some_and(|expected| (packet_pts - expected).abs() > tolerance);
        if rate_changed || discontinuous {
            if let Some(tail) = self.finish()? {
                ready.push(tail);
            }
            if rate_changed {
                self.resampler = Resampler::new(
                    decoded.sample_rate,
                    self.target_rate,
                    self.target_layout.channels(),
                )?;
            }
        }
        let source_start = self.expected_source_pts.unwrap_or(packet_pts);
        self.output_cursor.get_or_insert(packet_pts);
        self.expected_source_pts = Some(source_start + decoded.frames as f64 / decoded.sample_rate as f64);
        self.mixed.clear();
        mix_into(
            &decoded.pcm,
            &decoded.channels,
            self.target_layout,
            &mut self.mixed,
        );
        self.resampler.push(&self.mixed, &mut self.converted)?;
        if !self.converted.is_empty() {
            ready.push(self.take_chunk());
        }
        Ok(ready)
    }

    pub fn finish(&mut self) -> Result<Option<ReadyChunk>, String> {
        self.resampler.finish(&mut self.converted)?;
        self.resampler.reset();
        let tail = if self.converted.is_empty() {
            None
        } else {
            Some(self.take_chunk())
        };
        self.output_cursor = None;
        self.expected_source_pts = None;
        Ok(tail)
    }

    pub fn reset(&mut self) {
        self.decoder.reset();
        self.resampler.reset();
        self.mixed.clear();
        self.converted.clear();
        self.output_cursor = None;
        self.expected_source_pts = None;
    }

    fn take_chunk(&mut self) -> ReadyChunk {
        let pts = self.output_cursor.expect("decoded samples have a timestamp");
        self.output_cursor = Some(pts + self.converted.len() as f64 / (self.target_rate as f64 * self.target_layout.channels() as f64));
        ReadyChunk {
            pts,
            samples: std::mem::take(&mut self.converted),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_media::{MediaSession, StreamKind};

    fn pcm_info(time_base: (i32, i32)) -> StreamInfo {
        StreamInfo {
            index: 0, kind: StreamKind::Audio, codec_name: "pcm_s16le".to_string(), profile: -99,
            width: 0, height: 0, sample_rate: 44_100, channels: 1, time_base,
            duration_seconds: None, extradata: Vec::new(), channel_mask: 0,
            attached: false, default: false, forced: false, language: None, title: None,
        }
    }

    fn pcm_packet(pts: f64, value: i16, frames: usize) -> MediaPacket {
        MediaPacket {
            stream_index: 0, pts_seconds: Some(pts), dts_seconds: None,
            duration_seconds: Some(frames as f64 / 44_100.0), keyframe: false,
            data: (0..frames).flat_map(|_| value.to_le_bytes()).collect(),
        }
    }

    #[test]
    fn resampled_contiguous_packets_have_adjacent_output_timestamps() {
        let mut decoder = PacketDecoder::open(&pcm_info((1, 44_100)), 48_000).unwrap();
        let mut first_chunks = decoder.feed(&pcm_packet(1.0, 12_000, 441)).unwrap();
        let mut second_chunks = decoder.feed(&pcm_packet(1.01, 12_000, 441)).unwrap();
        assert_eq!(first_chunks.len(), 1);
        assert_eq!(second_chunks.len(), 1);
        let first = first_chunks.remove(0);
        let second = second_chunks.remove(0);
        let end = first.pts + first.samples.len() as f64 / 48_000.0;
        assert!((second.pts - end).abs() < 1e-10, "first ends {end}, second starts {}", second.pts);
    }

    #[test]
    fn resampled_discontinuities_drain_old_samples_without_blending() {
        for second_pts in [1.25, 1.005, 1.01 + 1.0 / 44_100.0] {
            let mut decoder = PacketDecoder::open(&pcm_info((1, 44_100)), 48_000).unwrap();
            let first = decoder.feed(&pcm_packet(1.0, 12_000, 441)).unwrap();
            assert_eq!(first.len(), 1);
            let spans = decoder.feed(&pcm_packet(second_pts, -12_000, 441)).unwrap();
            assert_eq!(spans.len(), 2);
            let old_tail = &spans[0];
            let new_span = &spans[1];
            let first_end = first[0].pts + first[0].samples.len() as f64 / 48_000.0;
            assert!((old_tail.pts - first_end).abs() < 1e-10);
            assert_eq!(first[0].samples.len() + old_tail.samples.len(), 480);
            assert!(old_tail.samples.iter().all(|sample| *sample > 0.36));
            assert!((new_span.pts - second_pts).abs() < 1e-10);
            assert!(new_span.samples.iter().all(|sample| *sample < -0.36));
            let tail = decoder.finish().unwrap().unwrap();
            assert!((tail.pts - (new_span.pts + new_span.samples.len() as f64 / 48_000.0)).abs() < 1e-10);
            assert_eq!(new_span.samples.len() + tail.samples.len(), 480);
            assert!(tail.samples.iter().all(|sample| *sample < -0.36));
            assert!(decoder.finish().unwrap().is_none());
        }
    }

    #[test]
    fn reset_discards_the_pending_resampler_tail_and_timestamp() {
        let mut decoder = PacketDecoder::open(&pcm_info((1, 44_100)), 48_000).unwrap();
        decoder.feed(&pcm_packet(1.0, 12_000, 441)).unwrap();
        decoder.reset();
        let spans = decoder.feed(&pcm_packet(3.0, -12_000, 441)).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].pts, 3.0);
        assert!(spans[0].samples.iter().all(|sample| *sample < -0.36));
    }

    #[test]
    fn missing_packet_timestamp_continues_the_current_resampled_span() {
        let mut decoder = PacketDecoder::open(&pcm_info((1, 44_100)), 48_000).unwrap();
        let first = decoder.feed(&pcm_packet(1.0, 12_000, 441)).unwrap();
        let mut packet = pcm_packet(1.01, 12_000, 441);
        packet.pts_seconds = None;
        let second = decoder.feed(&packet).unwrap();
        assert_eq!(second.len(), 1);
        let end = first[0].pts + first[0].samples.len() as f64 / 48_000.0;
        assert!((second[0].pts - end).abs() < 1e-10);
    }

    #[test]
    fn coarse_packet_timebase_does_not_split_contiguous_audio() {
        let mut decoder = PacketDecoder::open(&pcm_info((1, 1000)), 48_000).unwrap();
        let first = decoder.feed(&pcm_packet(0.0, 12_000, 1024)).unwrap();
        let second = decoder.feed(&pcm_packet(0.023, -12_000, 1024)).unwrap();
        assert_eq!(second.len(), 1);
        let end = first[0].pts + first[0].samples.len() as f64 / 48_000.0;
        assert!((second[0].pts - end).abs() < 1e-10);
    }

    #[test]
    fn packet_discontinuity_survives_before_chunks_are_combined() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.work/fixtures/audio-wav.wav");
        let mut session = MediaSession::open(path.to_str().unwrap()).unwrap();
        let info = session.streams().into_iter().find(|s| s.kind == StreamKind::Audio).unwrap();
        let mut decoder = PacketDecoder::open(&info, info.sample_rate as u32).unwrap();
        let mut chunks = Vec::new();
        while chunks.len() < 2 {
            let mut packet = session.read_packet().unwrap();
            if packet.stream_index == info.index {
                let pts = if chunks.is_empty() { 1.0 } else { 2.0 };
                packet.pts_seconds = Some(pts);
                chunks.extend(decoder.feed(&packet).unwrap());
            }
        }
        assert_eq!(chunks[0].pts, 1.0);
        assert_eq!(chunks[1].pts, 2.0);
    }
}
