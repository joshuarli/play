//! Shared types for inter-thread communication and CLI argument parsing.
//!
//! Defines the message enums that flow between threads:
//! - [`Command`] — UI → player (key presses, seek, quit)
//! - [`DemuxCommand`] — player → demuxer (seek, stop, audio track change)
//! - [`DemuxPacket`] — demuxer → player (decoded packets, flush, EOF)
//! - [`UiUpdate`] — player → UI (OSD text, pause state, seek flush)
//!
//! Also contains [`PixelBuffer`] (RAII CVPixelBufferRef), [`VideoFrame`]
//! (pixel buffer + timing), and CLI argument parsing.

use std::path::PathBuf;

/// Commands sent from the UI/input layer to the player thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    PlayPause,
    SeekRelative { seconds: f64, exact: bool },
    SeekAbsolute { target_us: i64 },
    VolumeUp,
    VolumeDown,
    ToggleMute,
    CycleAudioTrack,
    CycleSubtitle,
    AudioDelayIncrease,
    AudioDelayDecrease,
    NextFile,
    PrevFile,
    ToggleFullscreen,
    Quit,
}

/// Why playback of the current file ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Eof,
    NextFile,
    PrevFile,
    Quit,
}

/// Packets flowing from the demuxer to the player.
pub enum DemuxPacket {
    Video(ffmpeg_next::Packet),
    Audio(ffmpeg_next::Packet),
    #[allow(dead_code)]
    Subtitle(ffmpeg_next::Packet),
    /// Seek completed — all subsequent packets are from the new position.
    Flush,
    Eof,
}

/// Commands sent from the player to the demuxer.
#[derive(Debug)]
pub enum DemuxCommand {
    Seek {
        target_pts: i64,
        /// Seek forward (keyframe at or after target) vs backward.
        forward: bool,
    },
    /// Switch to a different audio stream index.
    ChangeAudio(usize),
    Stop,
}

/// RAII wrapper for a retained CVPixelBufferRef.
pub struct PixelBuffer(*mut std::ffi::c_void);

impl PixelBuffer {
    /// Wrap a retained CVPixelBufferRef. Caller must have already called CVPixelBufferRetain.
    pub fn new(ptr: *mut std::ffi::c_void) -> Self {
        Self(ptr)
    }

    /// Take the raw pointer, defusing the Drop. Caller assumes ownership.
    pub fn take(mut self) -> *mut std::ffi::c_void {
        let ptr = self.0;
        self.0 = std::ptr::null_mut();
        ptr
    }
}

unsafe impl Send for PixelBuffer {}

impl Drop for PixelBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { crate::decode_video::release_pixel_buffer(self.0) };
        }
    }
}

/// Video frame ready for display.
pub struct VideoFrame {
    /// Retained CVPixelBufferRef, released on drop via PixelBuffer.
    pub pixel_buffer: Option<PixelBuffer>,
    /// Presentation timestamp in stream timebase microseconds.
    pub pts_us: i64,
    /// Duration of this frame in microseconds.
    pub duration_us: i64,
    /// If true, flush the display layer and reset the timebase before enqueuing.
    /// Bundled with the frame so flush+enqueue are atomic (no VSync gap).
    pub seek_flush: bool,
}

/// Updates sent from the player to the main (UI) thread.
pub enum UiUpdate {
    Osd(String),
    SubtitleText(Option<String>),
    VideoSize {
        width: u32,
        height: u32,
    },
    /// Pause or unpause video display layer.
    Paused(bool),
    /// Flush the display layer and reset timebase after a seek.
    SeekFlush(i64),
    EndOfFile(EndReason),
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

const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "flv", "m4v", "ts", "mp3", "flac", "ogg", "opus", "wav",
    "m4a", "aac", "wma",
];

/// Expand directories into sorted media files; pass through regular files.
pub fn expand_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(p)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|f| {
                    f.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())
                    })
                })
                .collect();
            files.sort();
            out.extend(files);
        } else {
            out.push(p.clone());
        }
    }
    out
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() -> String {
    format!(
        "\
play {VERSION} — macOS media player

Usage: play [OPTIONS] <FILE|DIR>...

Arguments:
  <FILE|DIR>...  One or more media files or directories

Options:
      --volume <N>          Initial volume percentage 0-100 [default: 100]
      --audio-delay <SECS>  Audio delay in seconds, can be negative [default: 0.0]
      --audio-track <N>     Audio track index, 1-based [default: 1]
      --sub-file <PATH>     External SRT subtitle file
      --start <TIME>        Start position (HH:MM:SS, MM:SS, or seconds)
      --no-fullscreen       Start windowed instead of fullscreen
  -v                        Verbose logging (-v info, -vv debug)
  -V, --version             Print version
  -h, --help                Print help"
    )
}

pub fn parse_args() -> anyhow::Result<Args> {
    parse_from(std::env::args().skip(1).collect())
}

/// Take the next value for a `--flag`, supporting both `--flag value` (via
/// the iterator) and the pre-split value from `--flag=value`.
fn take_value<'a>(
    flag: &str,
    inline: Option<&'a str>,
    iter: &mut impl Iterator<Item = String>,
    buf: &'a mut String,
) -> anyhow::Result<&'a str> {
    if let Some(v) = inline {
        return Ok(v);
    }
    *buf = iter
        .next()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))?;
    Ok(buf.as_str())
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
    let mut positional_only = false;

    let mut iter = argv.into_iter();
    while let Some(arg) = iter.next() {
        if positional_only {
            files.push(PathBuf::from(arg));
            continue;
        }

        // Split --flag=value into (flag, Some(value))
        let (flag, inline_val) = if arg.starts_with("--") && arg.contains('=') {
            let (f, v) = arg.split_once('=').unwrap();
            (f.to_string(), Some(v.to_string()))
        } else {
            (arg.clone(), None)
        };
        let inline_ref = inline_val.as_deref();
        // Buffer for take_value when reading from iterator
        let mut val_buf = String::new();

        match flag.as_str() {
            "--" => {
                positional_only = true;
            }
            "-h" | "--help" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("play {VERSION}");
                std::process::exit(0);
            }
            "--volume" => {
                let val = take_value("--volume", inline_ref, &mut iter, &mut val_buf)?;
                volume = val
                    .parse::<u32>()
                    .map_err(|_| {
                        anyhow::anyhow!("invalid value '{val}' for --volume: expected integer")
                    })?
                    .min(100);
            }
            "--audio-delay" => {
                let val = take_value("--audio-delay", inline_ref, &mut iter, &mut val_buf)?;
                audio_delay = val.parse().map_err(|_| {
                    anyhow::anyhow!("invalid value '{val}' for --audio-delay: expected number")
                })?;
            }
            "--audio-track" => {
                let val = take_value("--audio-track", inline_ref, &mut iter, &mut val_buf)?;
                audio_track = val.parse().map_err(|_| {
                    anyhow::anyhow!("invalid value '{val}' for --audio-track: expected integer")
                })?;
            }
            "--sub-file" => {
                let val = take_value("--sub-file", inline_ref, &mut iter, &mut val_buf)?;
                sub_file = Some(PathBuf::from(val));
            }
            "--start" => {
                let val = take_value("--start", inline_ref, &mut iter, &mut val_buf)?;
                start = Some(val.to_string());
            }
            "--fullscreen" => fullscreen = true,
            "--no-fullscreen" => fullscreen = false,
            s if s.starts_with("-v") && s.chars().skip(1).all(|c| c == 'v') => {
                verbose = (s.len() - 1).min(255) as u8;
            }
            s if s.starts_with('-') => {
                anyhow::bail!("unknown option '{s}'\n\n{}", usage());
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }

    if files.is_empty() {
        anyhow::bail!("required arguments not provided: <FILE>...\n\n{}", usage());
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

    #[test]
    fn equals_syntax() {
        let a = parse_from(args(&["--volume=50", "--audio-delay=-0.5", "f.mp4"])).unwrap();
        assert_eq!(a.volume, 50);
        assert_eq!(a.audio_delay, -0.5);
    }

    #[test]
    fn double_dash_terminates_options() {
        let a = parse_from(args(&["--", "--not-a-flag.mp4"])).unwrap();
        assert_eq!(a.files, vec![PathBuf::from("--not-a-flag.mp4")]);
    }

    #[test]
    fn volume_clamped_to_100() {
        let a = parse_from(args(&["--volume", "200", "f.mp4"])).unwrap();
        assert_eq!(a.volume, 100);
    }
}
