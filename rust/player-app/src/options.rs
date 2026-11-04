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
    pub input_conf: Option<PathBuf>,
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
    pub fullscreen: bool,
    pub tui: bool,
    pub gui: bool,
    pub gui_gate: bool,
    pub log_file: Option<PathBuf>,
    pub osc: bool,
    pub scripts: Vec<String>,
    pub sub_files: Vec<String>,
    pub audio_files: Vec<String>,
    pub geometry: Option<(i32, i32)>,
    pub quiet: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            files: Vec::new(),
            playlists: Vec::new(),
            items: Vec::new(),
            ipc: None,
            input_conf: None,
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
            fullscreen: false,
            tui: false,
            gui: false,
            gui_gate: false,
            log_file: None,
            osc: true,
            scripts: Vec::new(),
            sub_files: Vec::new(),
            audio_files: Vec::new(),
            geometry: None,
            quiet: false,
        }
    }
}

const REMOVED: &[&str] = &[
    "http-header-fields",
    "ytdl",
    "lavfi-complex",
    "o",
    "network-timeout",
    "tls-verify",
];

pub fn parse(args: &[String]) -> Result<Options, String> {
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
        match name.as_str() {
            "version" | "v" => return Err("__version__".to_owned()),
            "help" | "h" => return Err("__help__".to_owned()),
            "no-config" | "load-scripts" | "no-load-scripts" | "terminal" | "input-terminal" => {}
            "idle" => options.idle = flag(&inline)?,
            "no-idle" => options.idle = false,
            "pause" => options.pause = flag(&inline)?,
            "no-pause" => options.pause = false,
            "keep-open" => options.keep_open = flag(&inline)?,
            "no-keep-open" => options.keep_open = false,
            "fullscreen" | "fs" => options.fullscreen = flag(&inline)?,
            "no-fullscreen" | "no-fs" => options.fullscreen = false,
            "tui" => options.tui = flag(&inline)?,
            "no-tui" => options.tui = false,
            "gui" => options.gui = flag(&inline)?,
            "no-gui" => options.gui = false,
            "gate" => options.gui_gate = true,
            "log-file" => options.log_file = Some(PathBuf::from(value(&name)?)),
            "osc" => options.osc = flag(&inline)?,
            "no-osc" => options.osc = false,
            "audio" => options.no_audio = !flag(&inline)?,
            "no-audio" => options.no_audio = true,
            "video" => options.no_video = !flag(&inline)?,
            "no-video" => options.no_video = true,
            "vo" => options.null_video = value(&name)? == "null",
            "ao" => options.null_audio = value(&name)? == "null",
            "hwdec" | "hwdec-codecs" => {
                let _ = value(&name)?;
            }
            "input-ipc-server" | "input-ipc-server-raw" => {
                options.ipc = Some(PathBuf::from(value(&name)?));
            }
            "input-conf" => options.input_conf = Some(PathBuf::from(value(&name)?)),
            "start" => options.start = parse_start(&value(&name)?)?,
            "volume" => {
                options.volume = value(&name)?
                    .parse()
                    .map_err(|_| "invalid --volume".to_owned())?;
            }
            "mute" => options.muted = flag(&inline)?,
            "no-mute" => options.muted = false,
            "playlist" => {
                let path = value(&name)?;
                options.playlists.push(path.clone());
                options.items.push(Item::Playlist(path));
            }
            "sub-file" => options.sub_files.push(value(&name)?),
            "audio-file" => options.audio_files.push(value(&name)?),
            "script" => options.scripts.push(value(&name)?),
            "geometry" => options.geometry = parse_geometry(&value(&name)?),
            "really-quiet" | "quiet" => options.quiet = true,
            "msg-level" | "input-default-bindings"
            | "input-vo-keyboard" | "force-window" | "osd-level" | "screenshot-sw"
            | "config-dir" | "gpu-context" | "gpu-sw" | "vulkan-swap-mode"
            | "demuxer-max-bytes" | "cache" | "cache-secs" | "sub-auto"
            | "audio-channels" | "audio-device" | "audio-client-name" | "title"
            | "no-border" | "border" | "autofit" | "force-seekable" | "frames"
            | "length" | "end" | "ab-loop-a" | "ab-loop-b" | "speed"
            | "watch-later-directory" | "resume-playback" | "save-position-on-quit" => {
                if inline.is_none() && needs_value(&name) {
                    let _ = value(&name)?;
                }
            }
            _ => {}
        }
    }
    Ok(options)
}

fn needs_value(name: &str) -> bool {
    matches!(
        name,
        "msg-level"
            | "log-file"
            | "force-window"
            | "osd-level"
            | "config-dir"
            | "gpu-context"
            | "vulkan-swap-mode"
            | "demuxer-max-bytes"
            | "cache"
            | "cache-secs"
            | "sub-auto"
            | "audio-channels"
            | "audio-device"
            | "audio-client-name"
            | "title"
            | "autofit"
            | "frames"
            | "length"
            | "end"
            | "ab-loop-a"
            | "ab-loop-b"
            | "speed"
            | "watch-later-directory"
    )
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

pub fn help() -> &'static str {
    "Usage: player [options] [file|url]...\n\
     \n\
     --fs                  start fullscreen\n\
     --start=<time>        seek to the given time\n\
     --sub-file=<file>     load a subtitle file\n\
     --audio-file=<file>   load an external audio file\n\
     --playlist=<file>     load a playlist\n\
     --tui                 play in the terminal\n\
     --input-ipc-server=<path>  listen for IPC commands\n"
}
