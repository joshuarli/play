// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::OsString;
use std::path::PathBuf;

use lexopt::{Arg, Parser};

use crate::cli::command::{self, OptionKind, OptionSpec, COMMAND, INPUT};
use crate::cli::error::Error;

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub input: PathBuf,
    pub seek: Option<f64>,
    pub seek_ts: Option<i64>,
    pub track: Option<usize>,
    pub decode_only: bool,
    pub probe_only: bool,
    pub verify_only: bool,
    pub verify: bool,
    pub no_progress: bool,
    pub no_gapless: bool,
    pub dump_visuals: bool,
}

#[derive(Debug)]
pub enum Invocation {
    Run(Args),
    Help,
    Version,
}

pub fn parse<I, T>(args: I) -> Result<Invocation, Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut parser = Parser::from_iter(args);
    let mut parsed = Args::default();
    let mut input = None;
    let mut seen = Vec::new();

    while let Some(arg) = parser.next().map_err(Error::from_lexopt)? {
        match arg {
            Arg::Short(short) => {
                let spec = COMMAND
                    .find_short(short)
                    .ok_or_else(|| Error::unknown_option(format!("-{short}")))?;

                let invocation =
                    handle_option(&mut parser, spec, &mut parsed, &mut seen)?;

                if let Some(invocation) = invocation {
                    return Ok(invocation);
                }
            }
            Arg::Long(long) => {
                let spec = COMMAND
                    .find_long(long)
                    .ok_or_else(|| Error::unknown_option(format!("--{long}")))?;

                let invocation =
                    handle_option(&mut parser, spec, &mut parsed, &mut seen)?;

                if let Some(invocation) = invocation {
                    return Ok(invocation);
                }
            }
            Arg::Value(value) => {
                if input.is_some() {
                    return Err(Error::unexpected_argument(value));
                }

                input = Some(PathBuf::from(value));
            }
        }
    }

    validate_conflicts(&seen)?;

    parsed.input = input.ok_or_else(|| Error::missing_argument(INPUT))?;

    Ok(Invocation::Run(parsed))
}

fn handle_option(
    parser: &mut Parser,
    spec: &OptionSpec,
    parsed: &mut Args,
    seen: &mut Vec<&'static str>,
) -> Result<Option<Invocation>, Error> {
    match &spec.kind {
        OptionKind::Help => return Ok(Some(Invocation::Help)),
        OptionKind::Version => return Ok(Some(Invocation::Version)),
        OptionKind::Flag | OptionKind::Value(_) => (),
    }

    if !spec.repeatable && seen.contains(&spec.id) {
        return Err(Error::duplicate_option(spec));
    }

    seen.push(spec.id);

    match &spec.kind {
        OptionKind::Flag => apply_flag(parsed, spec)?,
        OptionKind::Value(_) => {
            let value = parser.value().map_err(|_| Error::missing_value(spec))?;
            apply_value(parsed, spec, value)?;
        }
        OptionKind::Help | OptionKind::Version => return Err(Error::unhandled_option(spec)),
    }

    Ok(None)
}

fn apply_flag(args: &mut Args, spec: &OptionSpec) -> Result<(), Error> {
    match spec.id {
        command::DECODE_ONLY => args.decode_only = true,
        command::PROBE_ONLY => args.probe_only = true,
        command::VERIFY_ONLY => args.verify_only = true,
        command::VERIFY => args.verify = true,
        command::NO_PROGRESS => args.no_progress = true,
        command::NO_GAPLESS => args.no_gapless = true,
        command::DUMP_VISUALS => args.dump_visuals = true,
        _ => return Err(Error::unhandled_option(spec)),
    }

    Ok(())
}

fn apply_value(args: &mut Args, spec: &OptionSpec, value: OsString) -> Result<(), Error> {
    match spec.id {
        command::SEEK => args.seek = Some(parse_option_value::<f64>(value, spec)?),
        command::SEEK_TS => args.seek_ts = Some(parse_option_value::<i64>(value, spec)?),
        command::TRACK => args.track = Some(parse_option_value::<usize>(value, spec)?),
        _ => return Err(Error::unhandled_option(spec)),
    }

    Ok(())
}

fn parse_option_value<T>(value: OsString, spec: &OptionSpec) -> Result<T, Error>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let parsed = match value.to_str() {
        Some(text) => text.parse::<T>(),
        None => return Err(Error::non_unicode_value(spec)),
    };

    parsed.map_err(|error| Error::invalid_value(spec, value, error.to_string()))
}

fn validate_conflicts(seen: &[&'static str]) -> Result<(), Error> {
    for option in COMMAND.options {
        if !seen.contains(&option.id) {
            continue;
        }

        for conflict in option.conflicts {
            if seen.contains(conflict) {
                if let Some(other) = COMMAND.option(conflict) {
                    return Err(Error::conflicting_options(option, other));
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Args, Invocation, parse};
    use crate::cli::command;
    use crate::cli::error::Error;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn parse_args(args: &[&str]) -> Result<Invocation, Error> {
        parse(std::iter::once("symphonia-play").chain(args.iter().copied()))
    }

    fn run_args(args: &[&str]) -> Args {
        match parse_args(args).expect("arguments should parse") {
            Invocation::Run(args) => args,
            other => panic!("expected run invocation, got {other:?}"),
        }
    }

    #[test]
    fn parses_all_options_and_the_input() {
        let args = run_args(&[
            "--seek=12.5",
            "-t",
            "2",
            "--no-progress",
            "--no-gapless",
            "--dump-visuals",
            "input.flac",
        ]);

        assert_eq!(args.seek, Some(12.5));
        assert_eq!(args.track, Some(2));
        assert!(args.no_progress);
        assert!(args.no_gapless);
        assert!(args.dump_visuals);
        assert_eq!(args.input, PathBuf::from("input.flac"));
    }

    #[test]
    fn parses_modes() {
        assert!(run_args(&["--decode-only", "input"]).decode_only);
        assert!(run_args(&["--probe-only", "input"]).probe_only);
        assert!(run_args(&["--verify-only", "input"]).verify_only);
        assert!(run_args(&["--verify", "input"]).verify);
        assert!(run_args(&["-v", "input"]).verify);
    }

    #[test]
    fn seek_ts_accepts_hyphen_values() {
        assert_eq!(run_args(&["--seek-ts", "-1234", "input"]).seek_ts, Some(-1234));
        assert_eq!(run_args(&["-S-1234", "input"]).seek_ts, Some(-1234));
        assert_eq!(run_args(&["--seek-ts=-1234", "input"]).seek_ts, Some(-1234));
    }

    #[test]
    fn supports_short_option_clusters() {
        let args = run_args(&["-vt2", "input"]);

        assert!(args.verify);
        assert_eq!(args.track, Some(2));

        assert!(matches!(parse_args(&["-vh"]), Ok(Invocation::Help)));
    }

    #[test]
    fn supports_attached_and_equals_values() {
        assert_eq!(run_args(&["-s1.25", "input"]).seek, Some(1.25));
        assert_eq!(run_args(&["-s=1.25", "input"]).seek, Some(1.25));
        assert_eq!(run_args(&["--seek=1.25", "input"]).seek, Some(1.25));
        assert_eq!(run_args(&["--track=3", "input"]).track, Some(3));
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let args = run_args(&["--", "--not-an-option"]);

        assert_eq!(args.input, PathBuf::from("--not-an-option"));

        let args = run_args(&["--", "--help"]);
        assert_eq!(args.input, PathBuf::from("--help"));

        let args = run_args(&["-"]);
        assert_eq!(args.input, PathBuf::from("-"));
    }

    #[test]
    fn help_and_version_are_successful_early_exits() {
        for args in [&["-h"][..], &["--help"][..], &["input", "--help"][..]] {
            assert!(matches!(parse_args(args), Ok(Invocation::Help)), "{args:?}");
        }

        for args in [&["-V"][..], &["--version"][..]] {
            assert!(matches!(parse_args(args), Ok(Invocation::Version)), "{args:?}");
        }
    }

    #[test]
    fn unknown_options_are_rejected() {
        let error = parse_args(&["--bogus"]).expect_err("unknown long option should fail");
        assert!(matches!(&error, Error::UnknownOption(option) if option == "--bogus"));
        assert_eq!(error.to_string(), "unknown option '--bogus' for 'symphonia-play'");

        let error = parse_args(&["-z"]).expect_err("unknown short option should fail");
        assert!(matches!(&error, Error::UnknownOption(option) if option == "-z"));
        assert!(error.to_string().contains("'-z'"));
    }

    #[test]
    fn missing_arguments_are_rejected() {
        let error = parse_args(&[]).expect_err("input is required");
        assert!(matches!(&error, Error::MissingArgument("INPUT")));
        assert!(error.to_string().contains("INPUT"));

        let error = parse_args(&["--seek"]).expect_err("seek requires a value");
        assert!(matches!(&error, Error::MissingValue(_)));
        assert!(error.to_string().contains("--seek"));
        assert!(error.to_string().contains("TIME"));
    }

    #[test]
    fn invalid_values_are_rejected() {
        let error = parse_args(&["--seek", "nope", "input"]).expect_err("seek must be a float");
        assert!(matches!(&error, Error::InvalidValue { .. }));
        assert!(error.to_string().contains("--seek <TIME>"));
        assert!(error.to_string().contains("nope"));

        let error =
            parse_args(&["--seek-ts", "nope", "input"]).expect_err("seek-ts must be an integer");
        assert!(matches!(&error, Error::InvalidValue { .. }));
        assert!(error.to_string().contains("--seek-ts <TIMESTAMP>"));

        let error = parse_args(&["--track", "nope", "input"]).expect_err("track must be a usize");
        assert!(matches!(&error, Error::InvalidValue { .. }));
        assert!(error.to_string().contains("--track <TRACK>"));
    }

    #[test]
    fn flags_reject_attached_values() {
        for args in [&["--verify=yes", "input"][..], &["-v=yes", "input"][..]] {
            let error = parse_args(args).expect_err("flags do not take values");
            assert!(matches!(&error, Error::UnexpectedValue { .. }), "{args:?}: {error}");
        }
    }

    #[test]
    fn duplicate_value_options_are_rejected() {
        let error = parse_args(&["--seek", "1", "--seek", "2", "input"])
            .expect_err("seek cannot be repeated");
        assert!(matches!(&error, Error::DuplicateOption(_)));
        assert!(error.to_string().contains("--seek <TIME>"));
        assert!(error.to_string().contains("multiple times"));
    }

    #[test]
    fn repeated_flags_are_allowed() {
        let args = run_args(&["-vv", "input"]);
        assert!(args.verify);
    }

    #[test]
    fn extra_positionals_are_rejected() {
        let error = parse_args(&["one", "two"]).expect_err("only one input is accepted");
        assert!(matches!(&error, Error::UnexpectedArgument(value) if value == "two"));
        assert!(error.to_string().contains("unexpected argument"));
    }

    #[test]
    fn declared_conflicts_are_rejected() {
        let cases: &[&[&str]] = &[
            &["--seek", "1", "--probe-only", "input"],
            &["--seek", "1", "--seek-ts", "2", "input"],
            &["--seek", "1", "--verify", "input"],
            &["--seek-ts", "1", "--decode-only", "input"],
            &["--decode-only", "--probe-only", "input"],
            &["--decode-only", "--verify-only", "input"],
            &["--decode-only", "--verify", "input"],
            &["--probe-only", "--verify-only", "input"],
            &["--verify-only", "--verify", "input"],
        ];

        for args in cases {
            let error = parse_args(args).expect_err("conflicting options should fail");
            assert!(
                matches!(&error, Error::ConflictingOptions { .. }),
                "{args:?}: {error}"
            );
        }
    }

    #[test]
    fn probe_only_may_be_combined_with_verify() {
        let args = run_args(&["--probe-only", "--verify", "input"]);

        assert!(args.probe_only);
        assert!(args.verify);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_input_is_preserved() {
        use std::os::unix::ffi::OsStringExt;

        let path = OsString::from_vec(vec![b'f', 0xff, b'.', b'w', b'a', b'v']);

        let invocation = parse([OsString::from("symphonia-play"), path.clone()])
            .expect("non-UTF-8 paths are valid input");

        match invocation {
            Invocation::Run(args) => assert_eq!(args.input.as_os_str(), path.as_os_str()),
            other => panic!("expected run invocation, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_option_values_are_rejected() {
        use std::os::unix::ffi::OsStringExt;

        let value = OsString::from_vec(vec![b'1', 0xff]);
        let error = parse([OsString::from("symphonia-play"), OsString::from("--seek"), value])
            .expect_err("seek requires UTF-8");

        assert!(matches!(&error, Error::NonUnicodeValue(_)));
        assert!(error.to_string().contains("--seek <TIME>"));
    }

    #[test]
    fn option_lookup_uses_the_schema() {
        assert!(command::COMMAND.find_short('s').is_some());
        assert!(command::COMMAND.find_long("seek-ts").is_some());
        assert!(command::COMMAND.find_short('S').is_some());
        assert!(command::COMMAND.find_long("verify").is_some());
    }
}
