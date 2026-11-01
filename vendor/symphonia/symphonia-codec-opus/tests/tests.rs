use symphonia_codec_opus::OpusDecoder;
use symphonia_core::audio::{Channels, layouts};
use symphonia_core::codecs::audio::{
    AudioCodecParameters, AudioDecoder, AudioDecoderOptions, well_known::CODEC_ID_AAC,
    well_known::CODEC_ID_OPUS,
};
use symphonia_core::errors::Error;
use symphonia_core::packet::Packet;
use symphonia_core::units::{Duration, Timestamp};

fn opus_head(channels: u8, pre_skip: u16, mapping_family: u8) -> Box<[u8]> {
    let mut data = vec![0u8; 19];

    data[..8].copy_from_slice(b"OpusHead");
    data[8] = 1;
    data[9] = channels;
    data[10..12].copy_from_slice(&pre_skip.to_le_bytes());
    data[12..16].copy_from_slice(&48_000u32.to_le_bytes());
    data[18] = mapping_family;

    data.into_boxed_slice()
}

fn params(channels: Channels, extra_data: Option<Box<[u8]>>) -> AudioCodecParameters {
    let mut params = AudioCodecParameters::new();

    params.for_codec(CODEC_ID_OPUS).with_sample_rate(48_000).with_channels(channels);

    if let Some(extra_data) = extra_data {
        params.with_extra_data(extra_data);
    }

    params
}

fn encode_packet() -> Vec<u8> {
    let mut encoder = ruopus::OpusEncoder::new(2);
    let pcm: Vec<f32> =
        (0..960 * 2).map(|i| ((i as f32) * 0.05).sin() * 0.25).collect();

    encoder.encode_auto(&pcm, 1_275).expect("encode packet")
}

fn packet(data: Vec<u8>) -> Packet {
    Packet::new(0, Timestamp::new(0), Duration::new(960), data)
}

#[test]
fn decodes_stereo_packet() {
    let mut decoder = OpusDecoder::try_new(
        &params(layouts::CHANNEL_LAYOUT_STEREO, Some(opus_head(2, 0, 0))),
        &AudioDecoderOptions::default(),
    )
    .unwrap();

    let decoded = decoder.decode(&packet(encode_packet())).unwrap();

    assert_eq!(decoded.spec().rate(), 48_000);
    assert_eq!(decoded.spec().channels().count(), 2);
    assert_eq!(decoded.frames(), 960);
}

#[test]
fn trims_pre_skip_across_packets() {
    let mut decoder = OpusDecoder::try_new(
        &params(layouts::CHANNEL_LAYOUT_STEREO, Some(opus_head(2, 1_200, 0))),
        &AudioDecoderOptions::default(),
    )
    .unwrap();

    let data = encode_packet();

    assert_eq!(decoder.decode(&packet(data.clone())).unwrap().frames(), 0);
    assert_eq!(decoder.decode(&packet(data)).unwrap().frames(), 720);
}

#[test]
fn trims_packet_start_and_end() {
    let mut decoder = OpusDecoder::try_new(
        &params(layouts::CHANNEL_LAYOUT_STEREO, Some(opus_head(2, 0, 0))),
        &AudioDecoderOptions::default(),
    )
    .unwrap();

    let mut packet = packet(encode_packet());
    packet.trim_start = Duration::new(10);
    packet.trim_end = Duration::new(20);

    assert_eq!(decoder.decode(&packet).unwrap().frames(), 930);
}

#[test]
fn reset_discards_pending_pre_skip() {
    let mut decoder = OpusDecoder::try_new(
        &params(layouts::CHANNEL_LAYOUT_STEREO, Some(opus_head(2, 312, 0))),
        &AudioDecoderOptions::default(),
    )
    .unwrap();

    let data = encode_packet();

    assert_eq!(decoder.decode(&packet(data.clone())).unwrap().frames(), 648);

    decoder.reset();

    assert_eq!(decoder.decode(&packet(data)).unwrap().frames(), 960);
}

#[test]
fn decodes_without_opus_head() {
    let mut decoder = OpusDecoder::try_new(
        &params(layouts::CHANNEL_LAYOUT_STEREO, None),
        &AudioDecoderOptions::default(),
    )
    .unwrap();

    assert_eq!(decoder.decode(&packet(encode_packet())).unwrap().frames(), 960);
}

#[test]
fn rejects_invalid_codec() {
    let mut params = AudioCodecParameters::new();
    params.for_codec(CODEC_ID_AAC);

    let err = match OpusDecoder::try_new(&params, &AudioDecoderOptions::default()) {
        Ok(_) => panic!("expected an unsupported codec error"),
        Err(err) => err,
    };

    assert!(matches!(err, Error::Unsupported(_)));
}

#[test]
fn rejects_unsupported_sample_rate() {
    let mut params = params(layouts::CHANNEL_LAYOUT_STEREO, Some(opus_head(2, 0, 0)));
    params.with_sample_rate(44_100);

    let err = match OpusDecoder::try_new(&params, &AudioDecoderOptions::default()) {
        Ok(_) => panic!("expected an unsupported sample rate error"),
        Err(err) => err,
    };

    assert!(matches!(err, Error::Unsupported(_)));
}

#[test]
fn rejects_multichannel() {
    let params =
        params(Channels::Discrete(3), Some(opus_head(3, 0, 1)));

    let err = match OpusDecoder::try_new(&params, &AudioDecoderOptions::default()) {
        Ok(_) => panic!("expected an unsupported channel count error"),
        Err(err) => err,
    };

    assert!(matches!(err, Error::Unsupported(_)));
}

#[test]
fn rejects_malformed_opus_head() {
    let params =
        params(layouts::CHANNEL_LAYOUT_STEREO, Some(Box::from(&b"OpusHead"[..])));

    assert!(OpusDecoder::try_new(&params, &AudioDecoderOptions::default()).is_err());
}
