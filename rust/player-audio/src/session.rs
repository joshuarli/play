use player_media::{StreamInfo, StreamKind};
use symphonia::core::audio::{Channels, GenericAudioBufferRef, Position};
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_OPUS,
    CODEC_ID_PCM_F32BE, CODEC_ID_PCM_F32LE, CODEC_ID_PCM_F64BE, CODEC_ID_PCM_F64LE,
    CODEC_ID_PCM_S16BE, CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24BE, CODEC_ID_PCM_S24LE,
    CODEC_ID_PCM_S32BE, CODEC_ID_PCM_S32LE, CODEC_ID_PCM_S8, CODEC_ID_PCM_U8,
    CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{
    AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions,
};
use symphonia::core::errors::Error as CodecError;
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration, Timestamp};

pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: Channels,
    pub frames: usize,
    pub pcm: Vec<f32>,
}

pub struct DecoderSession {
    decoder: Box<dyn AudioDecoder>,
    stream_index: usize,
    time_base: (i32, i32),
    cursor: Option<u64>,
}

fn codec_id_for_name(name: &str) -> Option<AudioCodecId> {
    match name {
        "aac" => Some(CODEC_ID_AAC),
        "alac" => Some(CODEC_ID_ALAC),
        "flac" => Some(CODEC_ID_FLAC),
        "mp3" => Some(CODEC_ID_MP3),
        "opus" => Some(CODEC_ID_OPUS),
        "vorbis" => Some(CODEC_ID_VORBIS),
        "pcm_s8" => Some(CODEC_ID_PCM_S8),
        "pcm_u8" => Some(CODEC_ID_PCM_U8),
        "pcm_s16le" => Some(CODEC_ID_PCM_S16LE),
        "pcm_s16be" => Some(CODEC_ID_PCM_S16BE),
        "pcm_s24le" => Some(CODEC_ID_PCM_S24LE),
        "pcm_s24be" => Some(CODEC_ID_PCM_S24BE),
        "pcm_s32le" => Some(CODEC_ID_PCM_S32LE),
        "pcm_s32be" => Some(CODEC_ID_PCM_S32BE),
        "pcm_f32le" => Some(CODEC_ID_PCM_F32LE),
        "pcm_f32be" => Some(CODEC_ID_PCM_F32BE),
        "pcm_f64le" => Some(CODEC_ID_PCM_F64LE),
        "pcm_f64be" => Some(CODEC_ID_PCM_F64BE),
        _ => None,
    }
}

impl DecoderSession {
    pub fn open(stream: &StreamInfo) -> Result<Self, String> {
        if stream.kind != StreamKind::Audio {
            return Err(format!("stream {} is not audio", stream.index));
        }
        let codec = codec_id_for_name(&stream.codec_name).ok_or_else(|| {
            format!(
                "audio codec '{}' is not in the Symphonia closure",
                stream.codec_name
            )
        })?;
        let mut params = AudioCodecParameters::new();
        params.for_codec(codec);
        if stream.sample_rate > 0 {
            params.with_sample_rate(stream.sample_rate as u32);
        }
        if stream.channel_mask != 0 {
            if let Some(channels) =
                Position::from_bits(stream.channel_mask).map(Channels::Positioned)
            {
                params.with_channels(channels);
            }
        } else if stream.channels > 0 && stream.channels <= u16::MAX as i32 {
            params.with_channels(Channels::Discrete(stream.channels as u16));
        }
        if !stream.extradata.is_empty() {
            params.with_extra_data(stream.extradata.clone().into_boxed_slice());
        }
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|err| err.to_string())?;
        Ok(DecoderSession {
            decoder,
            stream_index: stream.index,
            time_base: stream.time_base,
            cursor: None,
        })
    }

    pub fn stream_index(&self) -> usize {
        self.stream_index
    }

    pub fn time_base(&self) -> (i32, i32) {
        self.time_base
    }

    pub fn reset(&mut self) {
        self.decoder.reset();
        self.cursor = None;
    }

    pub fn decode(&mut self, packet: &player_media::Packet) -> Result<DecodedAudio, String> {
        let (num, den) = self.time_base;
        let den = if den == 0 { 1 } else { den };
        let num = if num == 0 { 1 } else { num };
        let to_ticks = |seconds: f64| (seconds * den as f64 / num as f64).round().max(0.0) as u64;
        let timestamp = packet.pts_seconds.map(to_ticks).or(self.cursor).unwrap_or(0);
        let duration = packet.duration_seconds.map(to_ticks).unwrap_or(0);
        let packet = Packet::new(
            1,
            Timestamp::from(timestamp.min(i64::MAX as u64) as i64),
            Duration::from(duration),
            packet.data.clone(),
        );
        let decoded = match self.decoder.decode(&packet) {
            Ok(decoded) => Ok(interleaved(&decoded)),
            Err(CodecError::ResetRequired) => {
                self.decoder.reset();
                self.decoder
                    .decode(&packet)
                    .map_err(|err| err.to_string())
                    .map(|decoded| interleaved(&decoded))
            }
            Err(err) => Err(err.to_string()),
        }?;
        if decoded.sample_rate > 0 {
            let advance =
                decoded.frames as u64 * den as u64 / decoded.sample_rate as u64;
            self.cursor = Some(timestamp.saturating_add(advance));
        } else {
            self.cursor = Some(timestamp);
        }
        Ok(decoded)
    }
}

fn interleaved(decoded: &GenericAudioBufferRef<'_>) -> DecodedAudio {
    let spec = decoded.spec().clone();
    let mut pcm = Vec::with_capacity(decoded.frames() * spec.channels().count());
    decoded.copy_to_vec_interleaved::<f32>(&mut pcm);
    DecodedAudio {
        sample_rate: spec.rate(),
        channels: spec.channels().clone(),
        frames: decoded.frames(),
        pcm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_media::MediaSession;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");
        let dir = root.join(".work/fixtures");
        assert!(
            dir.join("audio-wav.wav").is_file(),
            "run tests/fixtures/gen.sh first"
        );
        dir
    }

    fn fixture(name: &str) -> String {
        fixtures().join(name).to_string_lossy().into_owned()
    }

    fn audio_stream(session: &MediaSession) -> StreamInfo {
        session
            .streams()
            .into_iter()
            .find(|stream| stream.kind == StreamKind::Audio)
            .expect("audio stream")
    }

    fn drain(session: &mut MediaSession, decoder: &mut DecoderSession) -> (usize, DecodedAudio) {
        let mut total = 0;
        let mut last = None;
        loop {
            match session.read_packet() {
                Ok(packet) => {
                    if packet.stream_index != decoder.stream_index() {
                        continue;
                    }
                    let decoded = decoder.decode(&packet).expect("packet decodes");
                    total += decoded.frames;
                    last = Some(decoded);
                }
                Err(_) => break,
            }
        }
        (total, last.expect("at least one audio packet"))
    }

    #[test]
    fn rejects_non_audio_stream() {
        let session = MediaSession::open(&fixture("h264.mp4")).expect("fixture opens");
        let video = session
            .streams()
            .into_iter()
            .find(|stream| stream.kind == StreamKind::Video)
            .expect("video stream");
        let err = match DecoderSession::open(&video) {
            Ok(_) => panic!("video must be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("not audio"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_unsupported_codec() {
        let stream = StreamInfo {
            index: 0,
            kind: StreamKind::Audio,
            codec_name: "ac3".to_string(),
            width: 0,
            height: 0,
            sample_rate: 48000,
            channels: 6,
            time_base: (1, 48000),
            extradata: Vec::new(),
            channel_mask: 0,
            attached: false,
            language: None,
            title: None,
        };
        let err = match DecoderSession::open(&stream) {
            Ok(_) => panic!("ac3 must be rejected"),
            Err(err) => err,
        };
        assert!(
            err.contains("not in the Symphonia closure"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn synthesizes_missing_timestamps() {
        let mut session = MediaSession::open(&fixture("audio-wav.wav")).expect("fixture opens");
        let info = audio_stream(&session);
        let mut decoder = DecoderSession::open(&info).expect("wav session opens");
        let mut real = None;
        for _ in 0..16 {
            let packet = session.read_packet().expect("packet");
            if packet.stream_index != info.index {
                continue;
            }
            if packet.pts_seconds.is_none() {
                continue;
            }
            real = Some(packet);
            break;
        }
        let mut packet = real.expect("timestamped packet");
        packet.pts_seconds = None;
        let first = decoder.decode(&packet).expect("first packet starts at zero");
        assert!(first.frames > 0);
        packet.pts_seconds = None;
        let second = decoder.decode(&packet).expect("cursor advances synthesis");
        assert!(second.frames > 0);
    }

    #[test]
    fn decodes_each_audio_codec() {
        let cases = [
            ("audio-alac.m4a", "alac", 48000u32, 2usize),
            ("audio-flac.flac", "flac", 48000, 2),
            ("audio-mp3.mp3", "mp3", 48000, 2),
            ("audio-ogg.ogg", "opus", 48000, 2),
            ("audio-wav.wav", "pcm_s16le", 48000, 2),
            ("audio-mono.wav", "pcm_s16le", 48000, 1),
            ("surround51.wav", "pcm_s16le", 48000, 6),
            ("h264.mp4", "aac", 48000, 2),
        ];
        for (file, codec, rate, channels) in cases {
            let mut session = MediaSession::open(&fixture(file)).expect("fixture opens");
            let info = audio_stream(&session);
            assert_eq!(info.codec_name, codec, "{file}");
            let expected_tb = info.time_base;
            assert!(expected_tb.1 > 0, "{file}: time base must be positive");
            let mut decoder = DecoderSession::open(&info).expect("decoder opens");
            let (total, last) = drain(&mut session, &mut decoder);
            assert_eq!(last.sample_rate, rate, "{file}");
            assert_eq!(last.channels.count(), channels, "{file}");
            assert_eq!(info.sample_rate, rate as i32, "{file}");
            assert_eq!(info.channels, channels as i32, "{file}");
            assert!(total > 0, "{file}: no frames decoded");
            assert!(
                last.pcm.iter().all(|sample| sample.is_finite()),
                "{file}: non-finite PCM"
            );
            let duration = total as f64 / rate as f64;
            assert!(
                (2.5..3.6).contains(&duration),
                "{file}: decoded {duration:.2}s, expected ~3s"
            );
        }
    }

    #[test]
    fn aac_carries_extradata() {
        let session = MediaSession::open(&fixture("h264.mp4")).expect("fixture opens");
        let info = audio_stream(&session);
        assert_eq!(info.codec_name, "aac");
        assert!(
            !info.extradata.is_empty(),
            "AAC without extradata cannot be configured"
        );
    }

    #[test]
    fn seek_resets_decode_position() {
        let mut session = MediaSession::open(&fixture("audio-wav.wav")).expect("fixture opens");
        let info = audio_stream(&session);
        let mut decoder = DecoderSession::open(&info).expect("decoder opens");
        let mut first_pts = None;
        for _ in 0..4 {
            let packet = session.read_packet().expect("packet");
            if packet.stream_index != info.index {
                continue;
            }
            if first_pts.is_none() {
                first_pts = packet.pts_seconds;
            }
        }
        session.seek_seconds(2.0).expect("seek works");
        decoder.reset();
        let mut after_pts = None;
        for _ in 0..32 {
            let packet = session.read_packet().expect("packet after seek");
            if packet.stream_index != info.index {
                continue;
            }
            let decoded = decoder.decode(&packet).expect("decodes after seek");
            assert!(decoded.frames > 0);
            after_pts = packet.pts_seconds;
            break;
        }
        let before = first_pts.expect("packets before seek");
        let after = after_pts.expect("packets after seek");
        assert!(
            after - before > 1.0,
            "seek did not advance: {before:.2} -> {after:.2}"
        );
    }

    #[test]
    fn sessions_are_independent() {
        let session = MediaSession::open(&fixture("audio-wav.wav")).expect("fixture opens");
        let info = audio_stream(&session);
        let mut first = DecoderSession::open(&info).expect("first opens");
        let mut second = DecoderSession::open(&info).expect("second opens");
        first.reset();
        let mut session2 = MediaSession::open(&fixture("audio-wav.wav")).expect("reopen");
        let packet = session2.read_packet().expect("packet");
        assert!(packet.stream_index == info.index);
        let decoded = second.decode(&packet).expect("independent session decodes");
        assert!(decoded.frames > 0);
    }
}
