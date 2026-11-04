use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Pause(Toggle),
    Fullscreen(Toggle),
    Mute(Toggle),
    Seek { amount: f64, mode: SeekMode, exact: bool },
    FrameStep(i32),
    Volume(Adjust),
    Speed(Adjust),
    Stop,
    Quit,
    LoadFile { path: String, mode: LoadMode },
    PlaylistNext,
    PlaylistPrevious,
    Screenshot,
    SubtitleAdd(String),
    SelectTrack { kind: TrackKind, id: Option<i64> },
    SelectTrackAuto(TrackKind),
    CycleTrack(TrackKind),
    OscVisibility(OscVisibilityChange),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscVisibility {
    Auto,
    Always,
    Never,
}

impl OscVisibility {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(OscVisibility::Auto),
            "always" => Some(OscVisibility::Always),
            "never" => Some(OscVisibility::Never),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            OscVisibility::Auto => "auto",
            OscVisibility::Always => "always",
            OscVisibility::Never => "never",
        }
    }

    pub fn next(self) -> Self {
        match self {
            OscVisibility::Auto => OscVisibility::Always,
            OscVisibility::Always => OscVisibility::Never,
            OscVisibility::Never => OscVisibility::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscVisibilityChange {
    Cycle,
    Set(OscVisibility),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Toggle,
    Set(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Adjust {
    Add(f64),
    Set(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekMode {
    Relative,
    Absolute,
    AbsolutePercent,
    RelativePercent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadMode {
    Replace,
    Append,
    InsertNext,
    InsertAt(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Subtitle,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError(String);

impl CommandError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CommandError {}

pub fn parse_args(args: &[String]) -> Result<Option<Command>, CommandError> {
    let mut index = 0;
    while index < args.len() && is_flag(&args[index]) {
        index += 1;
    }
    if index >= args.len() {
        return Ok(None);
    }
    let name = &args[index];
    let rest = &args[index + 1..];
    match name.as_str() {
        "cycle" => parse_cycle(rest),
        "set" => parse_set(rest),
        "add" => parse_add(rest),
        "seek" => parse_seek(rest),
        "frame-step" => Ok(Some(Command::FrameStep(1))),
        "frame-back-step" => Ok(Some(Command::FrameStep(-1))),
        "stop" => Ok(Some(Command::Stop)),
        "quit" => Ok(Some(Command::Quit)),
        "loadfile" => parse_loadfile(rest),
        "playlist-next" => Ok(Some(Command::PlaylistNext)),
        "playlist-prev" => Ok(Some(Command::PlaylistPrevious)),
        "screenshot" => Ok(Some(Command::Screenshot)),
        "sub-add" => parse_sub_add(rest),
        _ => Ok(None),
    }
}

fn parse_cycle(args: &[String]) -> Result<Option<Command>, CommandError> {
    let Some(property) = args.first() else {
        return Err(CommandError::new("cycle requires a property"));
    };
    Ok(match property.as_str() {
        "pause" => Some(Command::Pause(Toggle::Toggle)),
        "fullscreen" => Some(Command::Fullscreen(Toggle::Toggle)),
        "mute" => Some(Command::Mute(Toggle::Toggle)),
        "audio" => Some(Command::CycleTrack(TrackKind::Audio)),
        "sub" => Some(Command::CycleTrack(TrackKind::Subtitle)),
        "video" => Some(Command::CycleTrack(TrackKind::Video)),
        "osc-visibility" => Some(Command::OscVisibility(OscVisibilityChange::Cycle)),
        _ => None,
    })
}

fn parse_set(args: &[String]) -> Result<Option<Command>, CommandError> {
    let Some(property) = args.first() else {
        return Err(CommandError::new("set requires a property"));
    };
    let value = args.get(1).map(String::as_str);
    match property.as_str() {
        "pause" => Ok(Some(Command::Pause(Toggle::Set(parse_bool(value)?)))),
        "fullscreen" => Ok(Some(Command::Fullscreen(Toggle::Set(parse_bool(value)?)))),
        "mute" => Ok(Some(Command::Mute(Toggle::Set(parse_bool(value)?)))),
        "volume" => Ok(Some(Command::Volume(Adjust::Set(parse_number(value)?)))),
        "speed" => Ok(Some(Command::Speed(Adjust::Set(parse_number(value)?)))),
        "aid" => parse_track(TrackKind::Audio, value),
        "sid" => parse_track(TrackKind::Subtitle, value),
        "vid" => parse_track(TrackKind::Video, value),
        "osc-visibility" => {
            let mode = value
                .and_then(OscVisibility::parse)
                .ok_or_else(|| CommandError::new("osc-visibility is auto, always, or never"))?;
            Ok(Some(Command::OscVisibility(OscVisibilityChange::Set(mode))))
        }
        _ => Ok(None),
    }
}

fn parse_add(args: &[String]) -> Result<Option<Command>, CommandError> {
    let Some(property) = args.first() else {
        return Err(CommandError::new("add requires a property"));
    };
    let value = args.get(1).map(String::as_str);
    match property.as_str() {
        "volume" => Ok(Some(Command::Volume(Adjust::Add(parse_number(value)?)))),
        "speed" => Ok(Some(Command::Speed(Adjust::Add(parse_number(value)?)))),
        _ => Ok(None),
    }
}

fn parse_seek(args: &[String]) -> Result<Option<Command>, CommandError> {
    let Some(amount) = args.first() else {
        return Err(CommandError::new("seek requires an amount"));
    };
    let amount = parse_time(amount)
        .ok_or_else(|| CommandError::new("invalid parameter"))?;
    let mut mode = SeekMode::Relative;
    let mut exact = false;
    if let Some(value) = args.get(1) {
        for part in value.split('+') {
            match part {
                "relative" => mode = SeekMode::Relative,
                "absolute" => mode = SeekMode::Absolute,
                "absolute-percent" => mode = SeekMode::AbsolutePercent,
                "relative-percent" => mode = SeekMode::RelativePercent,
                "exact" => exact = true,
                "keyframes" => exact = false,
                "default" | "" => {}
                _ => return Ok(None),
            }
        }
    }
    Ok(Some(Command::Seek {
        amount,
        mode,
        exact,
    }))
}

fn parse_loadfile(args: &[String]) -> Result<Option<Command>, CommandError> {
    let Some(path) = args.first() else {
        return Err(CommandError::new("loadfile requires a path"));
    };
    let mode = match args.get(1).map(String::as_str) {
        None | Some("replace") => LoadMode::Replace,
        Some("append") => LoadMode::Append,
        Some("insert-next") => LoadMode::InsertNext,
        Some(value) if value.starts_with("insert-at:") => {
            let index = value["insert-at:".len()..]
                .parse::<i64>()
                .map_err(|_| CommandError::new("invalid loadfile insert index"))?;
            LoadMode::InsertAt(index)
        }
        Some(_) => return Ok(None),
    };
    Ok(Some(Command::LoadFile {
        path: path.clone(),
        mode,
    }))
}

fn parse_sub_add(args: &[String]) -> Result<Option<Command>, CommandError> {
    let Some(path) = args.first() else {
        return Err(CommandError::new("sub-add requires a path"));
    };
    Ok(Some(Command::SubtitleAdd(path.clone())))
}

fn parse_track(
    kind: TrackKind,
    value: Option<&str>,
) -> Result<Option<Command>, CommandError> {
    let Some(value) = value else {
        return Err(CommandError::new("track selection requires a value"));
    };
    let id = if value == "no" {
        None
    } else if value == "auto" {
        return Ok(Some(Command::SelectTrackAuto(kind)));
    } else {
        Some(
            value
                .parse::<i64>()
                .map_err(|_| CommandError::new(format!("invalid track id '{value}'")))?,
        )
    };
    Ok(Some(Command::SelectTrack { kind, id }))
}

fn parse_time(value: &str) -> Option<f64> {
    let (sign, rest) = match value.as_bytes().first() {
        Some(b'-') => (-1.0, &value[1..]),
        Some(b'+') => (1.0, &value[1..]),
        _ => (1.0, value),
    };
    if rest.is_empty() {
        return None;
    }
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() > 3 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    let mut total = 0.0;
    for (index, part) in parts.iter().enumerate() {
        let number = part.parse::<f64>().ok()?;
        if !number.is_finite() || number < 0.0 {
            return None;
        }
        let scale = match parts.len() - index {
            3 => 3600.0,
            2 => 60.0,
            _ => 1.0,
        };
        total += number * scale;
    }
    Some(sign * total)
}

fn parse_bool(value: Option<&str>) -> Result<bool, CommandError> {
    match value {
        Some("yes") | Some("true") | Some("1") => Ok(true),
        Some("no") | Some("false") | Some("0") => Ok(false),
        Some(value) => Err(CommandError::new(format!("invalid boolean '{value}'"))),
        None => Err(CommandError::new("missing boolean value")),
    }
}

fn parse_number(value: Option<&str>) -> Result<f64, CommandError> {
    let Some(value) = value else {
        return Err(CommandError::new("missing numeric value"));
    };
    let number = value
        .parse::<f64>()
        .map_err(|_| CommandError::new(format!("invalid number '{value}'")))?;
    if !number.is_finite() {
        return Err(CommandError::new(format!("invalid number '{value}'")));
    }
    Ok(number)
}

fn is_flag(value: &str) -> bool {
    matches!(
        value,
        "no-osd"
            | "osd-bar"
            | "osd-msg"
            | "osd-msg-bar"
            | "osd-auto"
            | "expand-properties"
            | "raw"
            | "repeatable"
            | "nonrepeatable"
            | "nonscalable"
            | "async"
            | "sync"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(values: &[&str]) -> Option<Command> {
        parse_args(&values.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>())
            .unwrap()
    }

    #[test]
    fn command_parses_toggles_and_sets() {
        assert_eq!(command(&["cycle", "pause"]), Some(Command::Pause(Toggle::Toggle)));
        assert_eq!(
            command(&["set", "pause", "yes"]),
            Some(Command::Pause(Toggle::Set(true)))
        );
        assert_eq!(
            command(&["set", "fullscreen", "no"]),
            Some(Command::Fullscreen(Toggle::Set(false)))
        );
        assert_eq!(
            command(&["cycle", "mute"]),
            Some(Command::Mute(Toggle::Toggle))
        );
    }

    #[test]
    fn command_parses_adjustments_and_seeks() {
        assert_eq!(
            command(&["add", "volume", "5"]),
            Some(Command::Volume(Adjust::Add(5.0)))
        );
        assert_eq!(
            command(&["set", "speed", "1.5"]),
            Some(Command::Speed(Adjust::Set(1.5)))
        );
        assert_eq!(
            command(&["seek", "5"]),
            Some(Command::Seek {
                amount: 5.0,
                mode: SeekMode::Relative,
                exact: false
            })
        );
        assert_eq!(
            command(&["seek", "12.5", "absolute+exact"]),
            Some(Command::Seek {
                amount: 12.5,
                mode: SeekMode::Absolute,
                exact: true
            })
        );
        assert_eq!(
            command(&["seek", "50", "absolute-percent"]),
            Some(Command::Seek {
                amount: 50.0,
                mode: SeekMode::AbsolutePercent,
                exact: false
            })
        );
    }

    #[test]
    fn command_parses_osc_visibility() {
        assert_eq!(
            command(&["cycle", "osc-visibility"]),
            Some(Command::OscVisibility(OscVisibilityChange::Cycle))
        );
        assert_eq!(
            command(&["set", "osc-visibility", "never"]),
            Some(Command::OscVisibility(OscVisibilityChange::Set(OscVisibility::Never)))
        );
        assert!(parse_args(&["set".to_owned(), "osc-visibility".to_owned(), "sometimes".to_owned()]).is_err());
        assert_eq!(OscVisibility::Auto.next(), OscVisibility::Always);
        assert_eq!(OscVisibility::Always.next(), OscVisibility::Never);
        assert_eq!(OscVisibility::Never.next(), OscVisibility::Auto);
    }

    #[test]
    fn command_parses_actions_and_tracks() {
        assert_eq!(command(&["frame-step"]), Some(Command::FrameStep(1)));
        assert_eq!(command(&["frame-back-step"]), Some(Command::FrameStep(-1)));
        assert_eq!(command(&["stop"]), Some(Command::Stop));
        assert_eq!(command(&["quit"]), Some(Command::Quit));
        assert_eq!(command(&["playlist-next"]), Some(Command::PlaylistNext));
        assert_eq!(command(&["playlist-prev"]), Some(Command::PlaylistPrevious));
        assert_eq!(command(&["screenshot"]), Some(Command::Screenshot));
        assert_eq!(
            command(&["sub-add", "/tmp/a.srt"]),
            Some(Command::SubtitleAdd("/tmp/a.srt".to_owned()))
        );
        assert_eq!(
            command(&["set", "aid", "2"]),
            Some(Command::SelectTrack {
                kind: TrackKind::Audio,
                id: Some(2)
            })
        );
        assert_eq!(
            command(&["set", "sid", "no"]),
            Some(Command::SelectTrack {
                kind: TrackKind::Subtitle,
                id: None
            })
        );
    }

    #[test]
    fn command_parses_loadfile_modes() {
        assert_eq!(
            command(&["loadfile", "/tmp/a.mkv"]),
            Some(Command::LoadFile {
                path: "/tmp/a.mkv".to_owned(),
                mode: LoadMode::Replace
            })
        );
        assert_eq!(
            command(&["loadfile", "/tmp/a.mkv", "append"]),
            Some(Command::LoadFile {
                path: "/tmp/a.mkv".to_owned(),
                mode: LoadMode::Append
            })
        );
        assert_eq!(
            command(&["loadfile", "/tmp/a.mkv", "insert-at:2"]),
            Some(Command::LoadFile {
                path: "/tmp/a.mkv".to_owned(),
                mode: LoadMode::InsertAt(2)
            })
        );
    }

    #[test]
    fn command_parses_prefix_flags_and_rejects_bad_arguments() {
        let parse = |values: &[&str]| {
            parse_args(&values.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(command(&["no-osd", "cycle", "pause"]), Some(Command::Pause(Toggle::Toggle)));
        assert_eq!(
            command(&["loadfile", "/tmp/a b.mkv", "append"]),
            Some(Command::LoadFile {
                path: "/tmp/a b.mkv".to_owned(),
                mode: LoadMode::Append
            })
        );
        assert_eq!(command(&["unknown-command"]), None);
        assert!(parse(&["seek", "nope"]).is_err());
        assert_eq!(command(&["seek", "5", "weird-mode"]), None);
        assert_eq!(command(&["loadfile", "/tmp/a.mkv", "append-play"]), None);
        assert!(parse(&["set", "pause", "maybe"]).is_err());
        assert!(parse(&["loadfile"]).is_err());
    }

    #[test]
    fn command_passes_unsupported_properties_through() {
        assert_eq!(command(&["cycle", "chapter"]), None);
        assert_eq!(command(&["set", "chapter", "2"]), None);
        assert_eq!(command(&["add", "chapter", "1"]), None);
    }

    #[test]
    fn command_parses_track_cycling() {
        assert_eq!(
            command(&["cycle", "audio"]),
            Some(Command::CycleTrack(TrackKind::Audio))
        );
        assert_eq!(
            command(&["cycle", "sub"]),
            Some(Command::CycleTrack(TrackKind::Subtitle))
        );
        assert_eq!(
            command(&["cycle", "video"]),
            Some(Command::CycleTrack(TrackKind::Video))
        );
        assert_eq!(
            command(&["cycle", "audio"]),
            Some(Command::CycleTrack(TrackKind::Audio))
        );
    }

    #[test]
    fn command_accepts_time_seeks_and_auto_tracks() {
        assert_eq!(
            command(&["seek", "1:30"]),
            Some(Command::Seek {
                amount: 90.0,
                mode: SeekMode::Relative,
                exact: false
            })
        );
        assert_eq!(
            command(&["seek", "+0:10"]),
            Some(Command::Seek {
                amount: 10.0,
                mode: SeekMode::Relative,
                exact: false
            })
        );
        assert_eq!(
            command(&["seek", "1:02:03.5"]),
            Some(Command::Seek {
                amount: 3723.5,
                mode: SeekMode::Relative,
                exact: false
            })
        );
        assert_eq!(
            command(&["seek", "-0:05", "absolute"]),
            Some(Command::Seek {
                amount: -5.0,
                mode: SeekMode::Absolute,
                exact: false
            })
        );
        assert_eq!(
            command(&["set", "aid", "auto"]),
            Some(Command::SelectTrackAuto(TrackKind::Audio))
        );
        assert_eq!(
            command(&["set", "sid", "auto"]),
            Some(Command::SelectTrackAuto(TrackKind::Subtitle))
        );
        assert!(parse_args(&["seek".to_owned(), "1:2:3:4".to_owned()]).is_err());
    }
}
