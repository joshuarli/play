// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub(crate) mod args;
mod command;
mod error;
mod help;
mod version;

use std::ffi::OsString;
use std::process::ExitCode;

use args::{Invocation, parse};
use command::COMMAND;

pub fn run<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    crate::logger::init();

    match parse(args) {
        Ok(Invocation::Help) => {
            println!("{}", help::render(&COMMAND));
            ExitCode::SUCCESS
        }
        Ok(Invocation::Version) => {
            println!("{}", version::line());
            ExitCode::SUCCESS
        }
        Ok(Invocation::Run(args)) => match crate::player::run(&args) {
            Ok(code) => ExitCode::from(code as u8),
            Err(err) => {
                log::error!("{}", err.to_string().to_ascii_lowercase());
                ExitCode::from(255)
            }
        },
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!();
            eprintln!("{}", help::usage(&COMMAND));
            eprintln!("Try '{} --help' for more information.", COMMAND.name);
            ExitCode::from(2)
        }
    }
}
