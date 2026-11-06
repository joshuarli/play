use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum StartTime {
    Unset,
    Absolute(f64),
    Relative(f64),
    Percent(f64),
    Chapter(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PathListOp {
    Set(Vec<String>),
    Append(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StartupOp {
    AddFile(String),
    AddPlaylist(String),
    SetFullscreen(bool),
    SetTui(bool),
    SetStart(StartTime),
    AudioFiles(PathListOp),
    SubFiles(PathListOp),
    Forward(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    MissingValue(String),
    InvalidValue(String, String),
    Unsupported(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingValue(option) => {
                write!(formatter, "option {option} requires a value")
            }
            ConfigError::InvalidValue(option, value) => {
                write!(formatter, "invalid value for {option}: '{value}'")
            }
            ConfigError::Unsupported(option) => {
                write!(formatter, "unsupported option {option}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn parse_startup(args: &[String]) -> Result<Vec<StartupOp>, ConfigError> {
    let mut operations = Vec::with_capacity(args.len());
    let mut positional_only = false;
    for arg in args {
        if positional_only {
            operations.push(StartupOp::AddFile(arg.clone()));
            continue;
        }
        if arg == "--" {
            positional_only = true;
            continue;
        }
        if arg == "--{" || arg == "--}" {
            return Err(ConfigError::Unsupported(arg.clone()));
        }
        if arg == "-" || !arg.starts_with('-') {
            operations.push(StartupOp::AddFile(arg.clone()));
            continue;
        }
        let (name, value) = split_option(arg);
        let operation = match name {
            "--fullscreen" | "--fs" => StartupOp::SetFullscreen(parse_bool(name, value)?),
            "--tui" => StartupOp::SetTui(parse_bool(name, value)?),
            "--no-tui" => {
                if let Some(value) = value {
                    return Err(ConfigError::InvalidValue(name.to_owned(), value.to_owned()));
                }
                StartupOp::SetTui(false)
            }
            "--no-fullscreen" | "--no-fs" => {
                if let Some(value) = value {
                    return Err(ConfigError::InvalidValue(name.to_owned(), value.to_owned()));
                }
                StartupOp::SetFullscreen(false)
            }
            "--start" => {
                let value = value.ok_or_else(|| ConfigError::MissingValue("--start".to_owned()))?;
                StartupOp::SetStart(parse_start_time(value)?)
            }
            "--playlist" => StartupOp::AddPlaylist(required_value(name, value)?.to_owned()),
            "--audio-file" => {
                StartupOp::AudioFiles(PathListOp::Append(required_value(name, value)?.to_owned()))
            }
            "--audio-files" => StartupOp::AudioFiles(PathListOp::Set(split_path_list(
                name,
                required_value(name, value)?,
            )?)),
            "--sub-file" => {
                StartupOp::SubFiles(PathListOp::Append(required_value(name, value)?.to_owned()))
            }
            "--sub-files" => StartupOp::SubFiles(PathListOp::Set(split_path_list(
                name,
                required_value(name, value)?,
            )?)),
            _ => StartupOp::Forward(arg.clone()),
        };
        operations.push(operation);
    }
    Ok(operations)
}

fn split_option(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (arg, None),
    }
}

fn required_value<'a>(name: &str, value: Option<&'a str>) -> Result<&'a str, ConfigError> {
    match value {
        Some("") | None => Err(ConfigError::MissingValue(name.to_owned())),
        Some(value) => Ok(value),
    }
}

fn parse_bool(name: &str, value: Option<&str>) -> Result<bool, ConfigError> {
    match value {
        None => Ok(true),
        Some("yes") | Some("true") | Some("1") => Ok(true),
        Some("no") | Some("false") | Some("0") => Ok(false),
        Some(value) => Err(ConfigError::InvalidValue(name.to_owned(), value.to_owned())),
    }
}

fn split_path_list(name: &str, value: &str) -> Result<Vec<String>, ConfigError> {
    let paths: Vec<String> = value
        .split(',')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    if paths.is_empty() {
        Err(ConfigError::MissingValue(name.to_owned()))
    } else {
        Ok(paths)
    }
}

fn parse_start_time(value: &str) -> Result<StartTime, ConfigError> {
    if value == "none" {
        return Ok(StartTime::Unset);
    }
    if let Some(percent) = value.strip_suffix('%') {
        let percent = percent
            .parse::<f64>()
            .map_err(|_| invalid_start(value))?;
        if !(0.0..=100.0).contains(&percent) {
            return Err(invalid_start(value));
        }
        return Ok(StartTime::Percent(percent));
    }
    if let Some(chapter) = value.strip_prefix('#') {
        let chapter = chapter
            .parse::<i64>()
            .map_err(|_| invalid_start(value))?;
        if chapter < 1 {
            return Err(invalid_start(value));
        }
        return Ok(StartTime::Chapter(chapter - 1));
    }
    if let Some(time) = value.strip_prefix('+') {
        return Ok(StartTime::Relative(parse_time_string(time)?));
    }
    if let Some(time) = value.strip_prefix('-') {
        return Ok(StartTime::Relative(-parse_time_string(time)?));
    }
    Ok(StartTime::Absolute(parse_time_string(value)?))
}

fn parse_time_string(value: &str) -> Result<f64, ConfigError> {
    let parts: Vec<&str> = value.split(':').collect();
    let seconds = match parts.as_slice() {
        [seconds] => parse_seconds(seconds)?,
        [minutes, seconds] => {
            parse_whole(minutes)? as f64 * 60.0 + parse_seconds(seconds)?
        }
        [hours, minutes, seconds] => {
            parse_whole(hours)? as f64 * 3600.0
                + parse_whole(minutes)? as f64 * 60.0
                + parse_seconds(seconds)?
        }
        _ => return Err(invalid_start(value)),
    };
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(invalid_start(value));
    }
    Ok(seconds)
}

fn parse_whole(value: &str) -> Result<u64, ConfigError> {
    if value.is_empty() {
        return Err(invalid_start(value));
    }
    value.parse::<u64>().map_err(|_| invalid_start(value))
}

fn parse_seconds(value: &str) -> Result<f64, ConfigError> {
    if value.is_empty() {
        return Err(invalid_start(value));
    }
    let seconds = value.parse::<f64>().map_err(|_| invalid_start(value))?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(invalid_start(value));
    }
    Ok(seconds)
}

fn invalid_start(value: &str) -> ConfigError {
    ConfigError::InvalidValue("--start".to_owned(), value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn file(value: &str) -> StartupOp {
        StartupOp::AddFile(value.to_owned())
    }

    fn forward(value: &str) -> StartupOp {
        StartupOp::Forward(value.to_owned())
    }

    #[test]
    fn config_keeps_files_and_supported_options_in_argument_order() {
        let parsed = parse_startup(&args(&[
            "--fullscreen",
            "one.mp4",
            "--start=0.5",
            "--audio-file=audio.m4a",
            "--sub-file=subs.srt",
            "two.mkv",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            vec![
                StartupOp::SetFullscreen(true),
                file("one.mp4"),
                StartupOp::SetStart(StartTime::Absolute(0.5)),
                StartupOp::AudioFiles(PathListOp::Append("audio.m4a".to_owned())),
                StartupOp::SubFiles(PathListOp::Append("subs.srt".to_owned())),
                file("two.mkv"),
            ]
        );
    }

    #[test]
    fn config_orders_playlists_with_positional_files() {
        let parsed = parse_startup(&args(&[
            "first.mp4",
            "--playlist=list.m3u",
            "last.mp4",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            vec![
                file("first.mp4"),
                StartupOp::AddPlaylist("list.m3u".to_owned()),
                file("last.mp4"),
            ]
        );
        assert!(parse_startup(&args(&["--playlist"])).is_err());
        assert!(parse_startup(&args(&["--playlist="])).is_err());
    }

    #[test]
    fn config_passes_engine_options_through() {
        let parsed = parse_startup(&args(&[
            "--no-config", "--vo=null", "--idle=yes", "--help", "--script=x.lua",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            vec![
                forward("--no-config"),
                forward("--vo=null"),
                forward("--idle=yes"),
                forward("--help"),
                forward("--script=x.lua"),
            ]
        );
    }

    #[test]
    fn config_handles_positional_separator() {
        let parsed = parse_startup(&args(&["--", "-file.mp4", "--start=1"])).unwrap();
        assert_eq!(parsed, vec![file("-file.mp4"), file("--start=1")]);
    }

    #[test]
    fn config_parses_booleans() {
        for (value, expected) in [
            ("--fullscreen", true),
            ("--fs", true),
            ("--fullscreen=no", false),
            ("--fs=0", false),
            ("--no-fullscreen", false),
            ("--no-fs", false),
        ] {
            let parsed = parse_startup(&args(&[value])).unwrap();
            assert_eq!(parsed, vec![StartupOp::SetFullscreen(expected)], "{value}");
        }
        assert!(parse_startup(&args(&["--fullscreen=maybe"])).is_err());
        assert!(parse_startup(&args(&["--no-fullscreen=yes"])).is_err());
    }

    #[test]
    fn config_parses_tui() {
        for (value, expected) in [
            ("--tui", true),
            ("--tui=yes", true),
            ("--tui=no", false),
            ("--no-tui", false),
        ] {
            let parsed = parse_startup(&args(&[value])).unwrap();
            assert_eq!(parsed, vec![StartupOp::SetTui(expected)], "{value}");
        }
        assert!(parse_startup(&args(&["--tui=maybe"])).is_err());
        assert!(parse_startup(&args(&["--no-tui=yes"])).is_err());
    }

    #[test]
    fn config_parses_start_times() {
        for (value, expected) in [
            ("--start=none", StartTime::Unset),
            ("--start=5", StartTime::Absolute(5.0)),
            ("--start=01:02:03.5", StartTime::Absolute(3723.5)),
            ("--start=+5", StartTime::Relative(5.0)),
            ("--start=-2.5", StartTime::Relative(-2.5)),
            ("--start=50%", StartTime::Percent(50.0)),
            ("--start=#3", StartTime::Chapter(2)),
        ] {
            let parsed = parse_startup(&args(&[value])).unwrap();
            assert_eq!(parsed, vec![StartupOp::SetStart(expected)], "{value}");
        }
        for value in ["--start", "--start=", "--start=abc", "--start=101%",
                      "--start=#0", "--start=+x"] {
            assert!(parse_startup(&args(&[value])).is_err(), "{value}");
        }
    }

    #[test]
    fn config_parses_path_lists() {
        let parsed = parse_startup(&args(&[
            "--audio-files=a.m4a,b.m4a",
            "--audio-file=c.m4a",
            "--sub-files=s1.srt,s2.srt",
            "--sub-file=s3.srt",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            vec![
                StartupOp::AudioFiles(PathListOp::Set(vec![
                    "a.m4a".to_owned(),
                    "b.m4a".to_owned()
                ])),
                StartupOp::AudioFiles(PathListOp::Append("c.m4a".to_owned())),
                StartupOp::SubFiles(PathListOp::Set(vec![
                    "s1.srt".to_owned(),
                    "s2.srt".to_owned()
                ])),
                StartupOp::SubFiles(PathListOp::Append("s3.srt".to_owned())),
            ]
        );
        assert!(parse_startup(&args(&["--audio-file="])).is_err());
        assert!(parse_startup(&args(&["--sub-files=,,"])).is_err());
    }

    #[test]
    fn config_rejects_per_file_option_groups() {
        assert!(parse_startup(&args(&["--{"])).is_err());
        assert!(parse_startup(&args(&["--}"])).is_err());
    }
}
