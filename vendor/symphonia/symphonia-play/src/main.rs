// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![warn(rust_2018_idioms)]
// Justification: Fields on DecoderOptions and FormatOptions may change at any time, but
// symphonia-play doesn't want to be updated every time those fields change, therefore always fill
// in the remaining fields with default values.
#![allow(clippy::needless_update)]

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("symphonia-play supports only Linux and macOS");

use std::process::ExitCode;

mod cli;
mod logger;
mod output;
mod player;
mod resampler;
mod ui;

fn main() -> ExitCode {
    cli::run(std::env::args_os())
}
