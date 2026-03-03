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

/// Video frame ready for display.
#[allow(dead_code)]
pub struct VideoFrame {
    /// Raw pointer to CVPixelBufferRef. Caller is responsible for retain/release.
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
#[derive(Debug, Clone, clap::Parser)]
#[command(name = "play", about = "Minimal macOS media player")]
pub struct Args {
    /// One or more mp4 file paths.
    #[arg(required = true)]
    pub files: Vec<PathBuf>,

    /// Initial volume percentage (0-100).
    #[arg(long, default_value = "100")]
    pub volume: u32,

    /// Audio delay in seconds (can be negative).
    #[arg(long = "audio-delay", default_value = "0.0")]
    pub audio_delay: f64,

    /// Audio track index (1-based).
    #[arg(long = "audio-track", default_value = "1")]
    pub audio_track: usize,

    /// External SRT subtitle file.
    #[arg(long = "sub-file")]
    pub sub_file: Option<PathBuf>,

    /// Start position (HH:MM:SS, MM:SS, or seconds).
    #[arg(long)]
    pub start: Option<String>,

    /// Start in fullscreen.
    #[arg(long)]
    pub fullscreen: bool,

    /// Verbose logging (-v for stream info, -vv for debug).
    #[arg(short = 'v', action = clap::ArgAction::Count)]
    pub verbose: u8,
}
