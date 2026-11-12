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
        match name.as_str() {
            "version" | "v" => return Err("__version__".to_owned()),
            "help" | "h" => return Err("__help__".to_owned()),
            "no-config" | "no-load-scripts" | "terminal" | "input-terminal" => {}
            "load-scripts" => {
                if flag(&inline)? {
                    return Err("option '--load-scripts' is unsupported: this player has no scripting".to_owned());
                }
            }
            "idle" => options.idle = flag(&inline)?,
            "no-idle" => options.idle = false,
            "pause" => options.pause = flag(&inline)?,
            "no-pause" => options.pause = false,
            "keep-open" => options.keep_open = flag(&inline)?,
            "no-keep-open" => options.keep_open = false,
            "fullscreen" | "fs" => options.fullscreen = Some(flag(&inline)?),
            "no-fullscreen" | "no-fs" => options.fullscreen = Some(false),
            "tui" => options.tui = flag(&inline)?,
            "no-tui" => options.tui = false,
            "gui" => options.gui = flag(&inline)?,
            "no-gui" => options.gui = false,
            "gate" => options.gui_gate = true,
            "list-protocols" => options.list_protocols = true,
            "frames" => {
                let count: u64 = value(&name)?
                    .parse()
                    .map_err(|_| "invalid --frames".to_owned())?;
                options.frames = (count > 0).then_some(count);
            }
            "audio-device" => options.audio_device = Some(value(&name)?),
            "term-playing-msg" => options.term_playing_msg = Some(value(&name)?),
            "log-file" => options.log_file = Some(PathBuf::from(value(&name)?)),
            "osc" => options.osc = flag(&inline)?,
            "no-osc" => options.osc = false,
            "audio" => options.no_audio = !flag(&inline)?,
            "no-audio" => options.no_audio = true,
            "video" => options.no_video = !flag(&inline)?,
            "no-video" => options.no_video = true,
            "vo" => options.null_video = value(&name)? == "null",
            "ao" => options.null_audio = value(&name)? == "null",
            "hwdec" => {
                let requested = value(&name)?;
                hardware_request(&requested)?;
                options.hwdec = requested;
            }
            "gpu-api" => options.gpu_api = parse_gpu_api(&value(&name)?)?,
            "hwdec-codecs" => {
                let _ = value(&name)?;
            }
            "input-ipc-server" | "input-ipc-server-raw" => {
                options.ipc = Some(PathBuf::from(value(&name)?));
            }
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
            "sid" => options.sid = Some(value(&name)?),
            "audio-file" => options.audio_files.push(value(&name)?),
            "geometry" => options.geometry = parse_geometry(&value(&name)?),
            "really-quiet" | "quiet" => options.quiet = true,
            "msg-level" | "input-default-bindings"
            | "input-vo-keyboard" | "force-window" | "osd-level" | "screenshot-sw"
            | "config-dir" | "gpu-context" | "gpu-sw" | "vulkan-swap-mode"
            | "demuxer-max-bytes" | "cache" | "cache-secs" | "sub-auto"
            | "audio-channels" | "audio-client-name" | "title"
            | "no-border" | "border" | "autofit" | "force-seekable"
            | "length" | "end" | "ab-loop-a" | "ab-loop-b" | "speed"
            | "watch-later-directory" | "resume-playback" | "save-position-on-quit"
                if inline.is_none() && needs_value(&name) =>
            {
                let _ = value(&name)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
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
