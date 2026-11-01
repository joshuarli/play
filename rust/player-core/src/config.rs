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

#[derive(Debug, Default, Clone, PartialEq)]
pub struct StartupConfig {
    pub files: Vec<String>,
    pub fullscreen: Option<bool>,
    pub start: Option<StartTime>,
    pub audio_ops: Vec<PathListOp>,
    pub sub_ops: Vec<PathListOp>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommandLine {
    pub config: StartupConfig,
    pub remaining: Vec<String>,
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

pub fn parse_startup(args: &[String]) -> Result<ParsedCommandLine, ConfigError> {
    let mut parsed = ParsedCommandLine {
        config: StartupConfig::default(),
        remaining: Vec::new(),
    };
    let mut positional_only = false;
    for arg in args {
        if positional_only {
            parsed.config.files.push(arg.clone());
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
            parsed.config.files.push(arg.clone());
            continue;
        }
        let (name, value) = split_option(arg);
        match name {
            "--fullscreen" | "--fs" => {
                parsed.config.fullscreen = Some(parse_bool(name, value)?);
            }
            "--no-fullscreen" | "--no-fs" => {
                if value.is_some() {
                    return Err(ConfigError::InvalidValue(
                        name.to_owned(),
                        value.unwrap_or_default().to_owned(),
                    ));
                }
                parsed.config.fullscreen = Some(false);
            }
            "--start" => {
                let value = value.ok_or_else(|| {
                    ConfigError::MissingValue("--start".to_owned())
                })?;
                parsed.config.start = Some(parse_start_time(value)?);
            }
            "--audio-file" => {
                let value = required_value(name, value)?;
                parsed.config.audio_ops.push(PathListOp::Append(value.to_owned()));
            }
            "--audio-files" => {
                let value = required_value(name, value)?;
                parsed.config.audio_ops.push(PathListOp::Set(split_path_list(name, value)?));
            }
            "--sub-file" => {
                let value = required_value(name, value)?;
                parsed.config.sub_ops.push(PathListOp::Append(value.to_owned()));
            }
            "--sub-files" => {
                let value = required_value(name, value)?;
                parsed.config.sub_ops.push(PathListOp::Set(split_path_list(name, value)?));
            }
            _ => parsed.remaining.push(arg.clone()),
        }
    }
    Ok(parsed)
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

    #[test]
    fn config_parses_files_and_supported_options() {
        let parsed = parse_startup(&args(&[
            "--fullscreen",
            "one.mp4",
            "--start=0.5",
            "--audio-file=audio.m4a",
            "--sub-file=subs.srt",
            "two.mkv",
        ]))
        .unwrap();
        assert_eq!(parsed.config.files, vec!["one.mp4", "two.mkv"]);
        assert_eq!(parsed.config.fullscreen, Some(true));
        assert_eq!(parsed.config.start, Some(StartTime::Absolute(0.5)));
        assert_eq!(
            parsed.config.audio_ops,
            vec![PathListOp::Append("audio.m4a".to_owned())]
        );
        assert_eq!(
            parsed.config.sub_ops,
            vec![PathListOp::Append("subs.srt".to_owned())]
        );
        assert!(parsed.remaining.is_empty());
    }

    #[test]
    fn config_passes_engine_options_through() {
        let parsed = parse_startup(&args(&[
            "--no-config", "--vo=null", "--idle=yes", "--help", "--script=x.lua",
        ]))
        .unwrap();
        assert!(parsed.config.files.is_empty());
        assert_eq!(
            parsed.remaining,
            vec!["--no-config", "--vo=null", "--idle=yes", "--help", "--script=x.lua"]
        );
    }

    #[test]
    fn config_handles_positional_separator() {
        let parsed = parse_startup(&args(&["--", "-file.mp4", "--start=1"])).unwrap();
        assert_eq!(parsed.config.files, vec!["-file.mp4", "--start=1"]);
        assert!(parsed.config.start.is_none());
        assert!(parsed.remaining.is_empty());
    }

    #[test]
    fn config_parses_booleans() {
        for (value, expected) in [
            ("--fullscreen", Some(true)),
            ("--fs", Some(true)),
            ("--fullscreen=no", Some(false)),
            ("--fs=0", Some(false)),
            ("--no-fullscreen", Some(false)),
            ("--no-fs", Some(false)),
        ] {
            let parsed = parse_startup(&args(&[value])).unwrap();
            assert_eq!(parsed.config.fullscreen, expected, "{value}");
        }
        assert!(parse_startup(&args(&["--fullscreen=maybe"])).is_err());
        assert!(parse_startup(&args(&["--no-fullscreen=yes"])).is_err());
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
            assert_eq!(parsed.config.start, Some(expected), "{value}");
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
            parsed.config.audio_ops,
            vec![
                PathListOp::Set(vec!["a.m4a".to_owned(), "b.m4a".to_owned()]),
                PathListOp::Append("c.m4a".to_owned()),
            ]
        );
        assert_eq!(
            parsed.config.sub_ops,
            vec![
                PathListOp::Set(vec!["s1.srt".to_owned(), "s2.srt".to_owned()]),
                PathListOp::Append("s3.srt".to_owned()),
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
