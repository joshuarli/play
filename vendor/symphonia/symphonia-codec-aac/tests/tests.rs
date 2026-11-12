use symphonia_codec_aac::{AacDecoder, AdtsReader};
use symphonia_core::codecs::audio::{
    AudioCodecParameters, AudioDecoder, AudioDecoderOptions, well_known::CODEC_ID_AAC,
};
use symphonia_core::errors;
use symphonia_core::formats::probe::ProbeableFormat;
use symphonia_core::io::MediaSourceStream;

fn test_decode(data: Vec<u8>) -> symphonia_core::errors::Result<()> {
    let data = std::io::Cursor::new(data);

    let mss = MediaSourceStream::new(Box::new(data), Default::default());

    let mut reader = AdtsReader::try_probe_new(mss, Default::default())?;

    let mut decoder = AacDecoder::try_new(
        AudioCodecParameters::new().for_codec(CODEC_ID_AAC),
        &AudioDecoderOptions::default(),
    )?;

    loop {
        match reader.next_packet()? {
            Some(packet) => {
                let _ = decoder.decode(&packet);
            }
            None => break,
        };
    }

    Ok(())
}

#[test]
fn invalid_channels_aac() {
    let file = vec![
        0xff, 0xf1, 0xaf, 0xce, 0x02, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xfb,
        0xaf,
    ];

    let err = test_decode(file).unwrap_err();

    assert!(matches!(err, errors::Error::Unsupported(_)));
}

#[test]
fn in_band_sbr_is_rejected() {
    use symphonia_core::audio::layouts::CHANNEL_LAYOUT_STEREO;
    use symphonia_core::packet::PacketRef;
    use symphonia_core::units::{Duration, Timestamp};

    let mut params = AudioCodecParameters::new();
    params.for_codec(CODEC_ID_AAC).with_sample_rate(44100).with_channels(CHANNEL_LAYOUT_STEREO);
    let mut decoder = AacDecoder::try_new(&params, &AudioDecoderOptions::default()).unwrap();
    for extension in [0xd, 0xe] {
        let data = [0xc3, (extension << 5) as u8, 0x1c];
        let packet = PacketRef::new(0, Timestamp::ZERO, Duration::ZERO, &data);
        assert!(matches!(decoder.decode_ref(&packet), Err(errors::Error::Unsupported("aac: SBR requires the HE-AAC decoder"))));
    }
}
