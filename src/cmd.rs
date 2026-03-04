use std::path::PathBuf;

/// Commands sent from the UI/input layer to the player thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    PlayPause,
    SeekRelative { seconds: f64, exact: bool },
    VolumeUp,
    VolumeDown,
    CycleAudioTrack,
    CycleSubtitle,
    AudioDelayIncrease,
    AudioDelayDecrease,
    NextFile,
    PrevFile,
    ToggleFullscreen,
    Quit,
}

/// Packets flowing from the demuxer to the player.
pub enum DemuxPacket {
    Video(ffmpeg_next::Packet),
    Audio(ffmpeg_next::Packet),
    Subtitle(ffmpeg_next::Packet),
    /// Seek completed — all subsequent packets are from the new position.
    Flush,
    Eof,
}

/// Commands sent from the player to the demuxer.
#[derive(Debug)]
#[allow(dead_code)]
pub enum DemuxCommand {
    Seek {
        target_pts: i64,
        /// Seek forward (keyframe at or after target) vs backward.
        forward: bool,
    },
    Flush,
    Stop,
}

/// Video frame ready for display. Releases CVPixelBuffer on drop.
#[allow(dead_code)]
pub struct VideoFrame {
    /// Raw pointer to CVPixelBufferRef (retained, released on drop).
    pub pixel_buffer: *mut std::ffi::c_void,
    /// Presentation timestamp in stream timebase microseconds.
    pub pts_us: i64,
    /// Duration of this frame in microseconds.
    pub duration_us: i64,
    /// Video width.
    pub width: u32,
    /// Video height.
    pub height: u32,
}

unsafe impl Send for VideoFrame {}

impl Drop for VideoFrame {
    fn drop(&mut self) {
        if !self.pixel_buffer.is_null() {
            unsafe { crate::decode_video::release_pixel_buffer(self.pixel_buffer) };
            self.pixel_buffer = std::ptr::null_mut();
        }
    }
}

impl VideoFrame {
    /// Take ownership of the pixel buffer pointer, preventing release on drop.
    pub fn take_pixel_buffer(&mut self) -> *mut std::ffi::c_void {
        std::mem::replace(&mut self.pixel_buffer, std::ptr::null_mut())
    }
}

/// Updates sent from the player to the main (UI) thread.
#[allow(dead_code)]
pub enum UiUpdate {
    Osd(String),
    SubtitleText(Option<String>),
    PlaybackPosition { current_us: i64, duration_us: i64 },
    VideoSize { width: u32, height: u32 },
    /// Pause or unpause video display layer.
    Paused(bool),
    /// Flush the display layer and reset timebase after a seek.
    SeekFlush(i64),
    EndOfFile,
}

/// Parsed CLI arguments.
#[derive(Debug, Clone)]
pub struct Args {
    /// One or more media file paths.
    pub files: Vec<PathBuf>,
    /// Initial volume percentage (0-100).
    pub volume: u32,
    /// Audio delay in seconds (can be negative).
    pub audio_delay: f64,
    /// Audio track index (1-based).
    pub audio_track: usize,
    /// External SRT subtitle file.
    pub sub_file: Option<PathBuf>,
    /// Start position (HH:MM:SS, MM:SS, or seconds).
    pub start: Option<String>,
    /// Start in fullscreen.
    pub fullscreen: bool,
    /// Verbose logging (-v for stream info, -vv for debug).
    pub verbose: u8,
}

const USAGE: &str = "\
Usage: play [OPTIONS] <FILE>...

Arguments:
  <FILE>...  One or more media file paths

Options:
      --volume <N>          Initial volume percentage 0-100 [default: 100]
      --audio-delay <SECS>  Audio delay in seconds, can be negative [default: 0.0]
      --audio-track <N>     Audio track index, 1-based [default: 1]
      --sub-file <PATH>     External SRT subtitle file
      --start <TIME>        Start position (HH:MM:SS, MM:SS, or seconds)
      --no-fullscreen       Start windowed instead of fullscreen
  -v                        Verbose logging (-v info, -vv debug)
  -h, --help                Print help";

pub fn parse_args() -> anyhow::Result<Args> {
    parse_from(std::env::args().skip(1).collect())
}

fn parse_from(argv: Vec<String>) -> anyhow::Result<Args> {
    let mut files = Vec::new();
    let mut volume: u32 = 100;
    let mut audio_delay: f64 = 0.0;
    let mut audio_track: usize = 1;
    let mut sub_file: Option<PathBuf> = None;
    let mut start: Option<String> = None;
    let mut fullscreen = true;
    let mut verbose: u8 = 0;

    let mut iter = argv.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--volume" => {
                let val = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--volume requires a value"))?;
                volume = val.parse().map_err(|_| {
                    anyhow::anyhow!("invalid value '{val}' for --volume: expected integer")
                })?;
            }
            "--audio-delay" => {
                let val = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--audio-delay requires a value"))?;
                audio_delay = val.parse().map_err(|_| {
                    anyhow::anyhow!("invalid value '{val}' for --audio-delay: expected number")
                })?;
            }
            "--audio-track" => {
                let val = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--audio-track requires a value"))?;
                audio_track = val.parse().map_err(|_| {
                    anyhow::anyhow!("invalid value '{val}' for --audio-track: expected integer")
                })?;
            }
            "--sub-file" => {
                let val = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--sub-file requires a value"))?;
                sub_file = Some(PathBuf::from(val));
            }
            "--start" => {
                let val = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--start requires a value"))?;
                start = Some(val);
            }
            "--fullscreen" => fullscreen = true,
            "--no-fullscreen" => fullscreen = false,
            s if s.starts_with("-v") && s.chars().skip(1).all(|c| c == 'v') => {
                verbose = (s.len() - 1).min(255) as u8;
            }
            s if s.starts_with('-') => {
                anyhow::bail!("unknown option '{s}'\n\n{USAGE}");
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }

    if files.is_empty() {
        anyhow::bail!("required arguments not provided: <FILE>...\n\n{USAGE}");
    }

    Ok(Args {
        files,
        volume,
        audio_delay,
        audio_track,
        sub_file,
        start,
        fullscreen,
        verbose,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_file() {
        let a = parse_from(args(&["video.mp4"])).unwrap();
        assert_eq!(a.files, vec![PathBuf::from("video.mp4")]);
        assert_eq!(a.volume, 100);
        assert_eq!(a.audio_delay, 0.0);
        assert_eq!(a.audio_track, 1);
        assert_eq!(a.verbose, 0);
        assert!(a.fullscreen);
    }

    #[test]
    fn all_flags() {
        let a = parse_from(args(&[
            "--volume",
            "50",
            "--audio-delay",
            "-0.5",
            "--audio-track",
            "2",
            "--sub-file",
            "subs.srt",
            "--start",
            "1:30",
            "--no-fullscreen",
            "-vv",
            "file.mp4",
        ]))
        .unwrap();
        assert_eq!(a.volume, 50);
        assert_eq!(a.audio_delay, -0.5);
        assert_eq!(a.audio_track, 2);
        assert_eq!(a.sub_file, Some(PathBuf::from("subs.srt")));
        assert_eq!(a.start, Some("1:30".to_string()));
        assert!(!a.fullscreen);
        assert_eq!(a.verbose, 2);
        assert_eq!(a.files, vec![PathBuf::from("file.mp4")]);
    }

    #[test]
    fn multiple_files() {
        let a = parse_from(args(&["a.mp4", "b.mp4", "c.mkv"])).unwrap();
        assert_eq!(a.files.len(), 3);
    }

    #[test]
    fn verbose_counting() {
        assert_eq!(parse_from(args(&["f.mp4"])).unwrap().verbose, 0);
        assert_eq!(parse_from(args(&["-v", "f.mp4"])).unwrap().verbose, 1);
        assert_eq!(parse_from(args(&["-vv", "f.mp4"])).unwrap().verbose, 2);
        assert_eq!(parse_from(args(&["-vvv", "f.mp4"])).unwrap().verbose, 3);
    }

    #[test]
    fn missing_files_is_error() {
        let e = parse_from(args(&[])).unwrap_err();
        assert!(e.to_string().contains("required arguments"));
    }

    #[test]
    fn unknown_flag_is_error() {
        let e = parse_from(args(&["--nope", "f.mp4"])).unwrap_err();
        assert!(e.to_string().contains("unknown option"));
    }
}
