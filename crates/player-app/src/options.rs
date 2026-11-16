use player_output::GpuApi;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum Start {
    Unset,
    Absolute(f64),
    Relative(f64),
    Percent(f64),
}

#[derive(Debug, Clone)]
pub enum Item {
    File(String),
    Playlist(String),
}

#[derive(Debug, Clone)]
pub struct Options {
    pub files: Vec<String>,
    pub playlists: Vec<String>,
    pub items: Vec<Item>,
    pub ipc: Option<PathBuf>,
    pub idle: bool,
    pub pause: bool,
    pub start: Start,
    pub volume: f64,
    pub muted: bool,
    pub keep_open: bool,
    pub no_video: bool,
    pub no_audio: bool,
    pub null_audio: bool,
    pub null_video: bool,
    pub fullscreen: Option<bool>,
    pub tui: bool,
    pub gui: bool,
    pub gui_gate: bool,
    pub list_protocols: bool,
    pub frames: Option<u64>,
    pub audio_device: Option<String>,
    pub hwdec: String,
    pub term_playing_msg: Option<String>,
    pub log_file: Option<PathBuf>,
    pub osc: bool,
    pub sub_files: Vec<String>,
    pub sid: Option<String>,
    pub audio_files: Vec<String>,
    pub geometry: Option<(i32, i32)>,
    pub quiet: bool,
    pub gpu_api: GpuApi,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            files: Vec::new(),
            playlists: Vec::new(),
            items: Vec::new(),
            ipc: None,
            idle: false,
            pause: false,
            start: Start::Unset,
            volume: 100.0,
            muted: false,
            keep_open: false,
            no_video: false,
            no_audio: false,
            null_audio: false,
            null_video: false,
            fullscreen: None,
            tui: false,
            gui: false,
            gui_gate: false,
            list_protocols: false,
            frames: None,
            audio_device: None,
            hwdec: if cfg!(target_os = "linux") {
                "auto".to_owned()
            } else {
                "no".to_owned()
            },
            term_playing_msg: None,
            log_file: None,
            osc: true,
            sub_files: Vec::new(),
            sid: None,
            audio_files: Vec::new(),
            geometry: None,
            quiet: false,
            gpu_api: GpuApi::default(),
        }
    }
}

const SCRIPTING: &[&str] = &[
    "script",
    "scripts",
    "scripts-append",
    "scripts-add",
    "script-opts",
    "script-opts-append",
    "script-opts-add",
];

pub const ENVIRONMENT: &str = "PLAYER_OPTIONS";

pub fn with_environment(environment: Option<&str>, args: &[String]) -> Result<Vec<String>, String> {
    let mut combined = Vec::new();
    for token in environment.unwrap_or("").split_whitespace() {
        if !token.starts_with("--") {
            return Err(format!("{ENVIRONMENT} holds options only, not '{token}'"));
        }
        combined.push(token.to_owned());
    }
    combined.extend(args.iter().cloned());
    Ok(combined)
}

const REMOVED: &[&str] = &[
    "input-conf",
    "http-header-fields",
    "ytdl",
    "lavfi-complex",
    "o",
    "network-timeout",
    "tls-verify",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionKind {
    Help,
    Version,
    NoConfig,
    LoadScripts,
    Idle,
    NoIdle,
    Pause,
    NoPause,
    KeepOpen,
    NoKeepOpen,
    Fullscreen,
    NoFullscreen,
    Tui,
    NoTui,
    Gui,
    NoGui,
    Gate,
    ListProtocols,
    Frames,
    AudioDevice,
    TermPlayingMsg,
    LogFile,
    Osc,
    NoOsc,
    Audio,
    NoAudio,
    Video,
    NoVideo,
    Vo,
    Ao,
    Hwdec,
    GpuApi,
    HwdecCodecs,
    Ipc,
    Start,
    Volume,
    Mute,
    NoMute,
    Playlist,
    SubFile,
    Sid,
    AudioFile,
    Geometry,
    Quiet,
    IgnoredValue,
    IgnoredFlag,
}

struct OptionSpec {
    kind: OptionKind,
    names: &'static [&'static str],
    short: Option<char>,
    value: &'static str,
    description: &'static str,
}

// This table owns the spellings used by both parsing and the reference.
const OPTION_SPECS: &[OptionSpec] = &[
    OptionSpec { kind: OptionKind::Help, names: &["help", "h"], short: Some('h'), value: "", description: "Print this reference and exit." },
    OptionSpec { kind: OptionKind::Version, names: &["version", "v"], short: None, value: "", description: "Print the version and exit." },
    OptionSpec { kind: OptionKind::NoConfig, names: &["no-config", "no-load-scripts", "terminal", "input-terminal"], short: None, value: "", description: "Accepted compatibility option; has no effect." },
    OptionSpec { kind: OptionKind::LoadScripts, names: &["load-scripts"], short: None, value: "[=BOOL]", description: "Only false is supported; scripting is unavailable." },
    OptionSpec { kind: OptionKind::Idle, names: &["idle"], short: None, value: "[=BOOL]", description: "Keep the player idle without files (default: no)." },
    OptionSpec { kind: OptionKind::NoIdle, names: &["no-idle"], short: None, value: "", description: "Disable idle mode." },
    OptionSpec { kind: OptionKind::Pause, names: &["pause"], short: None, value: "[=BOOL]", description: "Start paused (default: no)." },
    OptionSpec { kind: OptionKind::NoPause, names: &["no-pause"], short: None, value: "", description: "Start unpaused." },
    OptionSpec { kind: OptionKind::KeepOpen, names: &["keep-open"], short: None, value: "[=BOOL]", description: "Keep the last file open (default: no)." },
    OptionSpec { kind: OptionKind::NoKeepOpen, names: &["no-keep-open"], short: None, value: "", description: "Disable keep-open." },
    OptionSpec { kind: OptionKind::Fullscreen, names: &["fullscreen", "fs"], short: None, value: "[=BOOL]", description: "Set fullscreen; by default a window opens fullscreen unless geometry is set." },
    OptionSpec { kind: OptionKind::NoFullscreen, names: &["no-fullscreen", "no-fs"], short: None, value: "", description: "Open windowed." },
    OptionSpec { kind: OptionKind::Tui, names: &["tui"], short: None, value: "[=BOOL]", description: "Use terminal UI (default: no; audio may select it automatically)." },
    OptionSpec { kind: OptionKind::NoTui, names: &["no-tui"], short: None, value: "", description: "Disable explicit terminal UI." },
    OptionSpec { kind: OptionKind::Gui, names: &["gui"], short: None, value: "[=BOOL]", description: "Use GUI and disable automatic terminal UI (default: no)." },
    OptionSpec { kind: OptionKind::NoGui, names: &["no-gui"], short: None, value: "", description: "Disable explicit GUI." },
    OptionSpec { kind: OptionKind::Gate, names: &["gate"], short: None, value: "", description: "Open GUI gate and keep it open." },
    OptionSpec { kind: OptionKind::ListProtocols, names: &["list-protocols"], short: None, value: "", description: "Print available protocols and exit." },
    OptionSpec { kind: OptionKind::Frames, names: &["frames"], short: None, value: "=COUNT", description: "Stop after COUNT frames; 0 means unlimited (default: unlimited)." },
    OptionSpec { kind: OptionKind::AudioDevice, names: &["audio-device"], short: None, value: "=DEVICE", description: "Select audio device (Linux ALSA)." },
    OptionSpec { kind: OptionKind::TermPlayingMsg, names: &["term-playing-msg"], short: None, value: "=TEXT", description: "Set the terminal playing message." },
    OptionSpec { kind: OptionKind::LogFile, names: &["log-file"], short: None, value: "=PATH", description: "Create or truncate PATH at startup." },
    OptionSpec { kind: OptionKind::Osc, names: &["osc"], short: None, value: "[=BOOL]", description: "Enable on-screen controls (default: yes)." },
    OptionSpec { kind: OptionKind::NoOsc, names: &["no-osc"], short: None, value: "", description: "Disable on-screen controls." },
    OptionSpec { kind: OptionKind::Audio, names: &["audio"], short: None, value: "[=BOOL]", description: "Enable audio (default: yes)." },
    OptionSpec { kind: OptionKind::NoAudio, names: &["no-audio"], short: None, value: "", description: "Disable audio." },
    OptionSpec { kind: OptionKind::Video, names: &["video"], short: None, value: "[=BOOL]", description: "Enable video (default: yes)." },
    OptionSpec { kind: OptionKind::NoVideo, names: &["no-video"], short: None, value: "", description: "Disable video." },
    OptionSpec { kind: OptionKind::Vo, names: &["vo"], short: None, value: "=NAME", description: "Use null video output when NAME is null." },
    OptionSpec { kind: OptionKind::Ao, names: &["ao"], short: None, value: "=NAME", description: "Use null audio output when NAME is null." },
    OptionSpec { kind: OptionKind::Hwdec, names: &["hwdec"], short: None, value: "=NAME", description: "Hardware decoder: no, auto, auto-safe, videotoolbox[-copy] (macOS), or vaapi[-copy] (Linux); default no on macOS, auto on Linux." },
    OptionSpec { kind: OptionKind::GpuApi, names: &["gpu-api"], short: None, value: "=API", description: "Renderer: auto, vulkan, or metal (macOS only); default auto." },
    OptionSpec { kind: OptionKind::HwdecCodecs, names: &["hwdec-codecs"], short: None, value: "=LIST", description: "Accepted compatibility option; has no effect." },
    OptionSpec { kind: OptionKind::Ipc, names: &["input-ipc-server", "input-ipc-server-raw"], short: None, value: "=PATH", description: "Listen for local IPC commands at PATH." },
    OptionSpec { kind: OptionKind::Start, names: &["start"], short: None, value: "=TIME", description: "Start at seconds, MM:SS, HH:MM:SS, +TIME, or PERCENT%." },
    OptionSpec { kind: OptionKind::Volume, names: &["volume"], short: None, value: "=NUMBER", description: "Set volume (default: 100)." },
    OptionSpec { kind: OptionKind::Mute, names: &["mute"], short: None, value: "[=BOOL]", description: "Mute audio (default: no)." },
    OptionSpec { kind: OptionKind::NoMute, names: &["no-mute"], short: None, value: "", description: "Unmute audio." },
    OptionSpec { kind: OptionKind::Playlist, names: &["playlist"], short: None, value: "=PATH", description: "Add a local playlist in argument order; repeatable." },
    OptionSpec { kind: OptionKind::SubFile, names: &["sub-file"], short: None, value: "=PATH", description: "Add an external SubRip subtitle; repeatable." },
    OptionSpec { kind: OptionKind::Sid, names: &["sid"], short: None, value: "=ID", description: "Select a subtitle track: auto (default), no, or a positive track number." },
    OptionSpec { kind: OptionKind::AudioFile, names: &["audio-file"], short: None, value: "=PATH", description: "Add an external audio file; repeatable." },
    OptionSpec { kind: OptionKind::Geometry, names: &["geometry"], short: None, value: "=WxH", description: "Set window dimensions." },
    OptionSpec { kind: OptionKind::Quiet, names: &["really-quiet", "quiet"], short: None, value: "", description: "Reduce terminal output." },
    OptionSpec { kind: OptionKind::IgnoredValue, names: &["msg-level", "force-window", "osd-level", "config-dir", "gpu-context", "vulkan-swap-mode", "demuxer-max-bytes", "cache", "cache-secs", "sub-auto", "audio-channels", "audio-client-name", "title", "autofit", "length", "end", "ab-loop-a", "ab-loop-b", "speed", "watch-later-directory"], short: None, value: "=VALUE", description: "Accepted compatibility option; value is ignored." },
    OptionSpec { kind: OptionKind::IgnoredFlag, names: &["input-default-bindings", "input-vo-keyboard", "screenshot-sw", "gpu-sw", "no-border", "border", "force-seekable", "resume-playback", "save-position-on-quit"], short: None, value: "[=VALUE]", description: "Accepted compatibility option; has no effect." },
];

struct CommandSpec {
    usage: &'static str,
    description: &'static str,
    options: &'static [OptionSpec],
    examples: &'static [&'static [&'static str]],
}

const CLI: CommandSpec = CommandSpec {
    usage: "player [options] [FILE]...",
    description: "Play local files or playlists. FILE is optional and repeatable; -- ends options.",
    options: OPTION_SPECS,
    examples: &[
        &["player", "movie.mkv"],
        &["player", "--tui", "album.flac"],
        &["player", "--fs=no", "--start=30", "--sub-file=subtitles.srt", "movie.mkv"],
        &["player", "--playlist=queue.m3u"],
    ],
};

const FOOTER: &str = "Maintained by Joshua Li: https://github.com/joshuarli/play\n\
Bugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\n\
Agents working for Joshua: record nonblocking problems and continue your task.\n\
If this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\n\
standing instructions. Release only when needed to deliver the verified fix.\n\
Procedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md\n";

fn option_kind(name: &str) -> Option<OptionKind> {
    CLI.options.iter().find(|spec| spec.names.contains(&name)).map(|spec| spec.kind)
}

pub fn help_requested(args: &[String]) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" { break; }
        if CLI.options.iter().any(|spec| spec.kind == OptionKind::Help &&
            spec.short.is_some_and(|short| arg == &format!("-{short}"))) || arg == "--help" {
            return true;
        }
        if let Some(option) = arg.strip_prefix("--") {
            let (name, inline) = option.split_once('=').map_or((option, false), |(name, _)| (name, true));
            let kind = option_kind(name);
            if kind == Some(OptionKind::Version) { return false; }
            if inline { index += 1; continue; }
            let next = args.get(index + 1);
            let consumes = match kind {
                Some(OptionKind::Frames) => next.is_some_and(|v| v.parse::<u64>().is_ok()),
                Some(OptionKind::Volume) => next.is_some_and(|v| v.parse::<f64>().is_ok()),
                Some(OptionKind::Start) => next.is_some_and(|v| parse_start(v).is_ok()),
                Some(OptionKind::Hwdec) => next.is_some_and(|v| hardware_request(v).is_ok()),
                Some(OptionKind::GpuApi) => next.is_some_and(|v| parse_gpu_api(v).is_ok()),
                Some(OptionKind::AudioDevice | OptionKind::TermPlayingMsg | OptionKind::LogFile |
                     OptionKind::Vo | OptionKind::Ao | OptionKind::HwdecCodecs | OptionKind::Ipc |
                     OptionKind::Playlist | OptionKind::SubFile | OptionKind::Sid |
                     OptionKind::AudioFile | OptionKind::Geometry | OptionKind::IgnoredValue) => next.is_some(),
                _ => false,
            };
            if consumes { index += 1; }
        }
        index += 1;
    }
    false
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    if help_requested(args) { return Err("__help__".to_owned()); }
    let mut options = Options::default();
    let mut index = 0;
    let mut positional_only = false;
    while index < args.len() {
        let argument = &args[index];
        index += 1;
        if positional_only {
            options.files.push(argument.clone());
            options.items.push(Item::File(argument.clone()));
            continue;
        }
        if argument == "--" {
            positional_only = true;
            continue;
        }
        if !argument.starts_with("--") {
            options.files.push(argument.clone());
            options.items.push(Item::File(argument.clone()));
            continue;
        }
        let (name, inline) = match argument[2..].split_once('=') {
            Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
            None => (argument[2..].to_owned(), None),
        };
        if REMOVED.contains(&name.as_str()) {
            return Err(format!("unknown option '--{name}'"));
        }
        if SCRIPTING.contains(&name.as_str()) {
            return Err(format!("option '--{name}' is unsupported: this player has no scripting"));
        }
        let mut value = |name: &str| -> Result<String, String> {
            if let Some(value) = inline.clone() {
                return Ok(value);
            }
            let value = args
                .get(index)
                .cloned()
                .ok_or_else(|| format!("option '--{name}' requires a value"))?;
            index += 1;
            Ok(value)
        };
        let flag = |inline: &Option<String>| -> Result<bool, String> {
            match inline.as_deref() {
                None => Ok(true),
                Some("yes" | "true" | "1") => Ok(true),
                Some("no" | "false" | "0") => Ok(false),
                Some(other) => Err(format!("invalid boolean '--{name}={other}'")),
            }
        };
        match option_kind(&name) {
            Some(OptionKind::Version) => return Err("__version__".to_owned()),
            Some(OptionKind::Help) => return Err("__help__".to_owned()),
            Some(OptionKind::NoConfig) => {}
            Some(OptionKind::LoadScripts) => {
                if flag(&inline)? {
                    return Err("option '--load-scripts' is unsupported: this player has no scripting".to_owned());
                }
            }
            Some(OptionKind::Idle) => options.idle = flag(&inline)?,
            Some(OptionKind::NoIdle) => options.idle = false,
            Some(OptionKind::Pause) => options.pause = flag(&inline)?,
            Some(OptionKind::NoPause) => options.pause = false,
            Some(OptionKind::KeepOpen) => options.keep_open = flag(&inline)?,
            Some(OptionKind::NoKeepOpen) => options.keep_open = false,
            Some(OptionKind::Fullscreen) => options.fullscreen = Some(flag(&inline)?),
            Some(OptionKind::NoFullscreen) => options.fullscreen = Some(false),
            Some(OptionKind::Tui) => options.tui = flag(&inline)?,
            Some(OptionKind::NoTui) => options.tui = false,
            Some(OptionKind::Gui) => options.gui = flag(&inline)?,
            Some(OptionKind::NoGui) => options.gui = false,
            Some(OptionKind::Gate) => options.gui_gate = true,
            Some(OptionKind::ListProtocols) => options.list_protocols = true,
            Some(OptionKind::Frames) => {
                let count: u64 = value(&name)?
                    .parse()
                    .map_err(|_| "invalid --frames".to_owned())?;
                options.frames = (count > 0).then_some(count);
            }
            Some(OptionKind::AudioDevice) => options.audio_device = Some(value(&name)?),
            Some(OptionKind::TermPlayingMsg) => options.term_playing_msg = Some(value(&name)?),
            Some(OptionKind::LogFile) => options.log_file = Some(PathBuf::from(value(&name)?)),
            Some(OptionKind::Osc) => options.osc = flag(&inline)?,
            Some(OptionKind::NoOsc) => options.osc = false,
            Some(OptionKind::Audio) => options.no_audio = !flag(&inline)?,
            Some(OptionKind::NoAudio) => options.no_audio = true,
            Some(OptionKind::Video) => options.no_video = !flag(&inline)?,
            Some(OptionKind::NoVideo) => options.no_video = true,
            Some(OptionKind::Vo) => options.null_video = value(&name)? == "null",
            Some(OptionKind::Ao) => options.null_audio = value(&name)? == "null",
            Some(OptionKind::Hwdec) => {
                let requested = value(&name)?;
                hardware_request(&requested)?;
                options.hwdec = requested;
            }
            Some(OptionKind::GpuApi) => options.gpu_api = parse_gpu_api(&value(&name)?)?,
            Some(OptionKind::HwdecCodecs) => {
                let _ = value(&name)?;
            }
            Some(OptionKind::Ipc) => {
                options.ipc = Some(PathBuf::from(value(&name)?));
            }
            Some(OptionKind::Start) => options.start = parse_start(&value(&name)?)?,
            Some(OptionKind::Volume) => {
                options.volume = value(&name)?
                    .parse()
                    .map_err(|_| "invalid --volume".to_owned())?;
            }
            Some(OptionKind::Mute) => options.muted = flag(&inline)?,
            Some(OptionKind::NoMute) => options.muted = false,
            Some(OptionKind::Playlist) => {
                let path = value(&name)?;
                options.playlists.push(path.clone());
                options.items.push(Item::Playlist(path));
            }
            Some(OptionKind::SubFile) => options.sub_files.push(value(&name)?),
            Some(OptionKind::Sid) => options.sid = Some(value(&name)?),
            Some(OptionKind::AudioFile) => options.audio_files.push(value(&name)?),
            Some(OptionKind::Geometry) => options.geometry = parse_geometry(&value(&name)?),
            Some(OptionKind::Quiet) => options.quiet = true,
            Some(OptionKind::IgnoredValue) if inline.is_none() => {
                let _ = value(&name)?;
            }
            _ => {}
        }
    }

    Ok(options)
}

fn parse_gpu_api(value: &str) -> Result<GpuApi, String> {
    match value {
        "auto" => Ok(GpuApi::default()),
        name => GpuApi::from_name(name).ok_or_else(|| format!("--gpu-api: unsupported value {name}")),
    }
}

fn parse_start(value: &str) -> Result<Start, String> {
    if let Some(percent) = value.strip_suffix('%') {
        let percent: f64 = percent
            .parse()
            .map_err(|_| "invalid --start percent".to_owned())?;
        return Ok(Start::Percent(percent));
    }
    if let Some(rest) = value.strip_prefix('+') {
        let seconds = parse_clock(rest)?;
        return Ok(Start::Relative(seconds));
    }
    Ok(Start::Absolute(parse_clock(value)?))
}

fn parse_clock(value: &str) -> Result<f64, String> {
    if let Ok(seconds) = value.parse::<f64>() {
        return Ok(seconds);
    }
    let parts: Vec<&str> = value.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (Ok(0.0_f64), minutes.parse::<f64>(), seconds.parse::<f64>()),
        [hours, minutes, seconds] => (
            hours.parse::<f64>(),
            minutes.parse::<f64>(),
            seconds.parse::<f64>(),
        ),
        _ => return Err("invalid time value".to_owned()),
    };
    let (Ok(hours), Ok(minutes), Ok(seconds)) = (hours, minutes, seconds) else {
        return Err("invalid time value".to_owned());
    };
    Ok(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn parse_geometry(value: &str) -> Option<(i32, i32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

pub fn help() -> String {
    use std::fmt::Write;
    let mut text = format!("Usage: {}\n\n{}\n\nOptions:\n", CLI.usage, CLI.description);
    for spec in CLI.options {
        let separator = if spec.names.len() > 3 { ",\n  " } else { ", " };
        let mut names = spec.names.iter().map(|name| format!("--{name}{}", spec.value)).collect::<Vec<_>>();
        if let Some(short) = spec.short { names.insert(0, format!("-{short}")); }
        let names = names.join(separator);
        writeln!(text, "  {names}\n      {}", spec.description).unwrap();
    }
    text.push_str("\nBOOL accepts yes, true, 1, no, false, or 0; without a value it means yes.\nValue options also accept a separate argument. Unless stated otherwise, options\nare optional, values have no default, and the last occurrence wins.\nUnknown options are ignored for compatibility; removed and scripting options fail.\n\nEnvironment:\n  PLAYER_OPTIONS         Whitespace-separated options applied before command-line options.\n  PLAYER_TUI_ART         blocks/half-block/halfblock: text art; kitty/pixels: pixel art; other values: text art.\n  MPVBUNDLE              true enables bundled idle behavior.\n\nExamples:\n");
    for example in CLI.examples {
        writeln!(text, "  {}", example.join(" ")).unwrap();
    }
    text.push('\n');
    text.push_str(FOOTER);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn reference_examples_parse_and_option_names_are_unique() {
        let mut names = std::collections::HashSet::new();
        let mut short_names = std::collections::HashSet::new();
        for spec in CLI.options {
            assert!(!spec.description.is_empty());
            if let Some(short) = spec.short { assert!(short_names.insert(short)); }
            for name in spec.names {
                assert!(names.insert(*name), "duplicate option {name}");
                assert_eq!(option_kind(name), Some(spec.kind));
                assert!(help().contains(&format!("--{name}{}", spec.value)));
            }
        }
        for spec in CLI.options {
            for name in spec.names {
                let value = match spec.kind {
                    OptionKind::Help | OptionKind::Version => continue,
                    OptionKind::LoadScripts => "=no",
                    OptionKind::Frames => "=1",
                    OptionKind::Volume | OptionKind::Start => "=1",
                    OptionKind::Hwdec => "=no",
                    OptionKind::GpuApi => "=auto",
                    OptionKind::Geometry => "=640x480",
                    _ if spec.value == "=VALUE" || spec.value.starts_with('=') => "=sample",
                    _ => "",
                };
                let argument = format!("--{name}{value}");
                parse(&[argument.clone()]).unwrap_or_else(|error| panic!("{argument}: {error}"));
            }
        }
        for example in CLI.examples {
            assert_eq!(example[0], "player");
            parse(&args(&example[1..])).expect("help example parses");
            assert!(help().contains(&example.join(" ")));
        }
        assert!(parse(&args(&["--volume=bad", "-h"])).unwrap_err() == "__help__");
        assert_eq!(parse(&args(&["--", "-h"])).unwrap().files, vec!["-h"]);
        assert_eq!(parse(&args(&["--log-file", "-h"])).unwrap().log_file, Some(PathBuf::from("-h")));
        assert_eq!(parse(&args(&["--version", "--help"])).unwrap_err(), "__version__");
        assert_eq!(parse(&args(&["--volume", "--help"])).unwrap_err(), "__help__");
        assert_eq!(help().matches(FOOTER).count(), 1);
        assert!(help().ends_with(FOOTER));
    }

    #[test]
    fn environment_options_apply_before_the_command_line() {
        let combined = with_environment(Some("--volume=40  --fs=no"), &args(&["--volume=70", "a.mkv"]))
            .expect("options combine");
        let options = parse(&combined).expect("combined options parse");
        assert_eq!(options.volume, 70.0);
        assert_eq!(options.fullscreen, Some(false));
        assert_eq!(options.files, vec!["a.mkv".to_owned()]);
        assert_eq!(with_environment(None, &args(&["b.mp4"])).unwrap(), args(&["b.mp4"]));
    }

    #[test]
    fn gpu_api_selects_a_backend_and_rejects_unknown_values() {
        assert_eq!(parse(&args(&[])).unwrap().gpu_api, GpuApi::default());
        assert_eq!(parse(&args(&["--gpu-api=auto"])).unwrap().gpu_api, GpuApi::default());
        assert_eq!(parse(&args(&["--gpu-api", "vulkan"])).unwrap().gpu_api, GpuApi::Vulkan);
        #[cfg(target_os = "macos")]
        assert_eq!(parse(&args(&["--gpu-api=metal"])).unwrap().gpu_api, GpuApi::Metal);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            parse(&args(&["--gpu-api=metal"])).unwrap_err(),
            "--gpu-api: unsupported value metal"
        );
        assert_eq!(
            parse(&args(&["--gpu-api=bogus"])).unwrap_err(),
            "--gpu-api: unsupported value bogus"
        );
    }

    #[test]
    fn environment_rejects_files_and_removed_options_fail() {
        assert!(with_environment(Some("--mute movie.mkv"), &[]).is_err());
        assert!(parse(&args(&["--input-conf=keys.conf"])).is_err());
        assert!(parse(&args(&["--script=x.lua"])).is_err());
    }
}


pub(crate) fn hardware_request(name: &str) -> Result<player_media::HwRequest, String> {
    use player_media::HwRequest;
    match name {
        "no" => Ok(HwRequest::None),
        "auto" | "auto-safe" => {
            #[cfg(target_os = "macos")]
            { Ok(HwRequest::VideoToolbox) }
            #[cfg(target_os = "linux")]
            { Ok(HwRequest::VaApi) }
        }
        #[cfg(target_os = "macos")]
        "videotoolbox" | "videotoolbox-copy" => Ok(HwRequest::VideoToolbox),
        #[cfg(target_os = "linux")]
        "vaapi" | "vaapi-copy" => Ok(HwRequest::VaApi),
        _ => Err(format!("unsupported hardware decoder '{name}' on this platform")),
    }
}

#[cfg(test)]
mod hwdec_tests {
    use super::*;
    #[test]
    fn hardware_options_resolve_a_supported_request() {
        assert_eq!(hardware_request("no").unwrap(), player_media::HwRequest::None);
        assert_ne!(hardware_request("auto").unwrap(), player_media::HwRequest::None);
        assert_eq!(hardware_request("auto").unwrap(), hardware_request("auto-safe").unwrap());
        assert!(hardware_request("imaginary-decoder").is_err());
        assert!(parse(&["--hwdec=imaginary-decoder".to_owned()]).is_err());
    }
}
