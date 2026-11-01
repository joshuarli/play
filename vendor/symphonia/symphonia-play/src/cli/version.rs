// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::cli::command::COMMAND;

pub const GIT_SHA: &str = env!("SYMPHONIA_PLAY_GIT_SHA");

pub fn line() -> String {
    format!("{} {} (git {GIT_SHA})", COMMAND.name, env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::{GIT_SHA, line};
    use crate::cli::command::COMMAND;

    #[test]
    fn version_names_the_command_and_package() {
        let version = line();
        let prefix = format!("{} {} (git ", COMMAND.name, env!("CARGO_PKG_VERSION"));

        assert!(version.starts_with(&prefix), "unexpected version output '{version}'");
        assert!(version.ends_with(')'), "unexpected version output '{version}'");
    }

    #[test]
    fn git_sha_is_hexadecimal() {
        assert!(!GIT_SHA.is_empty());
        assert!(GIT_SHA.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(!GIT_SHA.eq_ignore_ascii_case("unknown"));
        assert!(!GIT_SHA.eq_ignore_ascii_case("dirty"));
    }
}
