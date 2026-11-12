use crate::decode::PacketDecoder;
use crate::mix::Layout;
use player_media::{MediaSession, StreamKind};

pub struct Pipeline {
    session: MediaSession,
    stream_index: usize,
    core: PacketDecoder,
    finished: bool,
}

impl Pipeline {
    pub fn open(path: &str, stream_index: usize, target_rate: u32) -> Result<Self, String> {
        let session = MediaSession::open(path).map_err(|err| err.to_string())?;
        let info = session
            .streams()
            .into_iter()
            .find(|stream| stream.index == stream_index && stream.kind == StreamKind::Audio)
            .ok_or_else(|| format!("no audio stream {stream_index}"))?;
        let core = PacketDecoder::open(&info, target_rate)?;
        Ok(Pipeline {
            session,
            stream_index,
            core,
            finished: false,
        })
    }

    pub fn source_channels(&self) -> usize {
        self.core.source_channels()
    }

    pub fn target_layout(&self) -> Layout {
        self.core.target_layout()
    }

    pub fn target_rate(&self) -> u32 {
        self.core.target_rate()
    }

    pub fn seek_seconds(&mut self, seconds: f64) -> Result<(), String> {
        self.session
            .seek_seconds(seconds)
            .map_err(|err| err.to_string())?;
        self.core.reset();
        self.finished = false;
        Ok(())
    }

    pub fn duration_seconds(&self) -> Option<f64> {
        self.session.duration_seconds()
    }

    pub fn next_chunk(&mut self, output: &mut Vec<f32>) -> Result<bool, String> {
        if self.finished {
            if let Some(chunk) = self.core.finish()? {
                output.extend_from_slice(&chunk.samples);
            }
            return Ok(true);
        }
        loop {
            let packet = match self.session.read_packet() {
                Ok(packet) => packet,
                Err(_) => {
                    self.finished = true;
                    if let Some(chunk) = self.core.finish()? {
                        output.extend_from_slice(&chunk.samples);
                    }
                    return Ok(true);
                }
            };
            if packet.stream_index != self.stream_index {
                continue;
            }
            let chunks = self.core.feed(&packet)?;
            if !chunks.is_empty() {
                for chunk in chunks {
                    output.extend_from_slice(&chunk.samples);
                }
                return Ok(false);
            }
        }
    }

    pub fn drain_all(&mut self) -> Result<Vec<f32>, String> {
        let mut all = Vec::new();
        loop {
            let mut chunk = Vec::new();
            let done = self.next_chunk(&mut chunk)?;
            all.extend_from_slice(&chunk);
            if done {
                break;
            }
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn audio_index(path: &str) -> usize {
        use player_media::{MediaSession, StreamKind};
        MediaSession::open(path)
            .expect("fixture opens")
            .streams()
            .into_iter()
            .find(|stream| stream.kind == StreamKind::Audio)
            .expect("audio stream")
            .index
    }

    fn open(path: &str, rate: u32) -> Pipeline {
        let index = audio_index(path);
        Pipeline::open(path, index, rate).expect("pipeline opens")
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn drain_matches_fixture_duration() {
        let cases = [
            ("audio-alac.m4a", 2, Layout::Stereo),
            ("audio-flac.flac", 2, Layout::Stereo),
            ("audio-mp3.mp3", 2, Layout::Stereo),
            ("audio-ogg.ogg", 2, Layout::Stereo),
            ("audio-wav.wav", 2, Layout::Stereo),
            ("audio-mono.wav", 1, Layout::Mono),
            ("surround51.wav", 6, Layout::Stereo),
            ("h264.mp4", 2, Layout::Stereo),
        ];
        for (file, source, target) in cases {
            let mut pipeline = open(&fixture(file), 48000);
            assert_eq!(pipeline.source_channels(), source, "{file}");
            assert_eq!(pipeline.target_layout(), target, "{file}");
            let pcm = pipeline.drain_all().expect("drain works");
            let seconds = pcm.len() as f64 / target.channels() as f64 / 48000.0;
            assert!(
                (2.5..3.6).contains(&seconds),
                "{file}: drained {seconds:.2}s"
            );
            assert!(
                pcm.iter().all(|sample| sample.is_finite()),
                "{file}: non-finite PCM"
            );
            assert!(rms(&pcm) > 0.01, "{file}: drained silence");
        }
    }

    #[test]
    fn surround_downmix_keeps_both_channels_equal() {
        let mut pipeline = open(&fixture("surround51.wav"), 48000);
        assert_eq!(pipeline.source_channels(), 6);
        assert_eq!(pipeline.target_layout(), Layout::Stereo);
        let pcm = pipeline.drain_all().expect("drain works");
        let (left, right): (Vec<f32>, Vec<f32>) =
            pcm.as_chunks::<2>().0.iter().map(|frame| (frame[0], frame[1])).unzip();
        assert!(
            rms(&left) > 0.03,
            "left channel is silent: layout order wrong?"
        );
        assert!((rms(&left) - rms(&right)).abs() < 0.005);
        let difference = left
            .iter()
            .zip(right.iter())
            .map(|(l, r)| (l - r).abs())
            .sum::<f32>()
            / left.len() as f32;
        assert!(
            difference < 0.01,
            "centre tone must downmix equally: {difference}"
        );
    }

    #[test]
    fn mono_stays_mono() {
        let mut pipeline = open(&fixture("audio-mono.wav"), 48000);
        assert_eq!(pipeline.target_layout(), Layout::Mono);
        let pcm = pipeline.drain_all().expect("drain works");
        let seconds = pcm.len() as f64 / 48000.0;
        assert!((2.5..3.6).contains(&seconds), "got {seconds:.2}s");
        assert!(rms(&pcm) > 0.01);
    }

    #[test]
    fn resample_output_matches_native_duration() {
        let mut native = open(&fixture("audio-wav.wav"), 48000);
        let native_pcm = native.drain_all().expect("drain works");
        let mut converted = open(&fixture("audio-wav.wav"), 44100);
        let converted_pcm = converted.drain_all().expect("drain works");
        let native_seconds = native_pcm.len() as f64 / 2.0 / 48000.0;
        let converted_seconds = converted_pcm.len() as f64 / 2.0 / 44100.0;
        assert!(
            (native_seconds - converted_seconds).abs() < 0.02,
            "{native_seconds} vs {converted_seconds}"
        );
        assert!((rms(&converted_pcm) - rms(&native_pcm)).abs() < 0.02);
    }

    #[test]
    fn drain_is_deterministic() {
        let mut first = open(&fixture("audio-flac.flac"), 48000);
        let mut second = open(&fixture("audio-flac.flac"), 48000);
        assert_eq!(first.drain_all().expect("drain"), second.drain_all().expect("drain"));
    }

    #[test]
    fn seek_continues_from_new_position() {
        let mut pipeline = open(&fixture("audio-wav.wav"), 48000);
        let mut before = Vec::new();
        pipeline.next_chunk(&mut before).expect("first chunk");
        assert!(!before.is_empty());
        pipeline.seek_seconds(2.0).expect("seek works");
        let mut after = Vec::new();
        let mut done = false;
        while !done {
            let mut chunk = Vec::new();
            done = pipeline.next_chunk(&mut chunk).expect("chunk after seek");
            after.extend_from_slice(&chunk);
        }
        let seconds = after.len() as f64 / 2.0 / 48000.0;
        assert!(
            (0.8..1.2).contains(&seconds),
            "expected ~1s after seek to 2.0, got {seconds:.2}s"
        );
    }

    #[test]
    fn rejects_unknown_stream_and_layout() {
        assert!(Pipeline::open(&fixture("audio-wav.wav"), 7, 48000).is_err());
        assert!(Pipeline::open(&fixture("audio-wav.wav"), 0, 0).is_err());
    }
}
