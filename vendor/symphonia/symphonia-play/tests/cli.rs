// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_symphonia-play"))
        .args(args)
        .output()
        .expect("failed to execute symphonia-play")
}

fn from_utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("command output is valid UTF-8")
}

#[test]
fn help_prints_to_stdout_and_exits_zero() {
    for args in [&["-h"][..], &["--help"][..]] {
        let output = run(args);

        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}");

        let stdout = from_utf8(output.stdout);
        assert!(stdout.contains("Usage: symphonia-play [OPTIONS] <INPUT>"));
        assert!(stdout.contains("Arguments:"));
        assert!(stdout.contains("<INPUT>"));
        assert!(stdout.contains("Options:"));
        assert!(stdout.contains("Examples:"));
    }
}

#[test]
fn version_prints_to_stdout_and_exits_zero() {
    for args in [&["-V"][..], &["--version"][..]] {
        let output = run(args);

        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}");

        let stdout = from_utf8(output.stdout);
        let version = stdout.trim_end();
        let prefix = format!("symphonia-play {} (git ", env!("CARGO_PKG_VERSION"));

        assert!(version.starts_with(&prefix), "unexpected version output '{version}'");

        let sha = version
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(')'))
            .expect("version output ends with the git SHA");

        assert!(!sha.is_empty());
        assert!(sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}

#[test]
fn unknown_option_goes_to_stderr() {
    let output = run(&["--bogus"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let stderr = from_utf8(output.stderr);
    assert!(stderr.contains("unknown option '--bogus' for 'symphonia-play'"));
    assert!(stderr.contains("Usage: symphonia-play [OPTIONS] <INPUT>"));
    assert!(stderr.contains("--help"));
}

#[test]
fn missing_input_goes_to_stderr() {
    let output = run(&[]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let stderr = from_utf8(output.stderr);
    assert!(stderr.contains("missing required argument 'INPUT' for 'symphonia-play'"));
}

#[test]
fn invalid_value_goes_to_stderr() {
    let output = run(&["--track", "nope", "input.wav"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let stderr = from_utf8(output.stderr);
    assert!(stderr.contains("invalid value"));
    assert!(stderr.contains("--track <TRACK>"));
}

#[test]
fn duplicate_and_conflicting_options_go_to_stderr() {
    let output = run(&["--seek", "1", "--seek", "2", "input.wav"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(from_utf8(output.stderr).contains("cannot be used multiple times"));

    let output = run(&["--probe-only", "--decode-only", "input.wav"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(from_utf8(output.stderr).contains("cannot be used with"));
}
