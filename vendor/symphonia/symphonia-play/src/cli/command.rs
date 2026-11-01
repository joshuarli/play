// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub const SEEK: &str = "seek";
pub const SEEK_TS: &str = "seek-ts";
pub const TRACK: &str = "track";
pub const DECODE_ONLY: &str = "decode-only";
pub const PROBE_ONLY: &str = "probe-only";
pub const VERIFY_ONLY: &str = "verify-only";
pub const VERIFY: &str = "verify";
pub const NO_PROGRESS: &str = "no-progress";
pub const NO_GAPLESS: &str = "no-gapless";
pub const DUMP_VISUALS: &str = "dump-visuals";
pub const HELP: &str = "help";
pub const VERSION: &str = "version";

pub const INPUT: &str = "INPUT";

pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub options: &'static [OptionSpec],
    pub positionals: &'static [PositionalSpec],
    pub examples: &'static [&'static str],
}

impl CommandSpec {
    pub fn find_short(&self, short: char) -> Option<&OptionSpec> {
        self.options.iter().find(|option| option.short == Some(short))
    }

    pub fn find_long(&self, long: &str) -> Option<&OptionSpec> {
        self.options.iter().find(|option| option.long == long)
    }

    pub fn option(&self, id: &str) -> Option<&OptionSpec> {
        self.options.iter().find(|option| option.id == id)
    }
}

pub struct OptionSpec {
    pub id: &'static str,
    pub short: Option<char>,
    pub long: &'static str,
    pub kind: OptionKind,
    pub repeatable: bool,
    pub conflicts: &'static [&'static str],
    pub help: &'static str,
}

impl OptionSpec {
    pub fn label(&self) -> String {
        let mut label = match self.short {
            Some(short) => format!("-{short}, --{}", self.long),
            None => format!("--{}", self.long),
        };

        if let OptionKind::Value(value) = &self.kind {
            label.push_str(&format!(" <{}>", value.name));
        }

        label
    }
}

pub enum OptionKind {
    Flag,
    Value(ValueSpec),
    Help,
    Version,
}

pub struct ValueSpec {
    pub name: &'static str,
    pub default: Option<&'static str>,
}

pub struct PositionalSpec {
    pub name: &'static str,
    pub required: bool,
    pub repeatable: bool,
    pub help: &'static str,
}

pub static COMMAND: CommandSpec = CommandSpec {
    name: "symphonia-play",
    description: "Play audio with Symphonia",
    options: &[
        OptionSpec {
            id: SEEK,
            short: Some('s'),
            long: "seek",
            kind: OptionKind::Value(ValueSpec { name: "TIME", default: None }),
            repeatable: false,
            conflicts: &[SEEK_TS, DECODE_ONLY, PROBE_ONLY, VERIFY, VERIFY_ONLY],
            help: "Seek to the time in seconds",
        },
        OptionSpec {
            id: SEEK_TS,
            short: Some('S'),
            long: "seek-ts",
            kind: OptionKind::Value(ValueSpec { name: "TIMESTAMP", default: None }),
            repeatable: false,
            conflicts: &[SEEK, DECODE_ONLY, PROBE_ONLY, VERIFY, VERIFY_ONLY],
            help: "Seek to the timestamp in timebase units",
        },
        OptionSpec {
            id: TRACK,
            short: Some('t'),
            long: "track",
            kind: OptionKind::Value(ValueSpec { name: "TRACK", default: None }),
            repeatable: false,
            conflicts: &[],
            help: "The track to use",
        },
        OptionSpec {
            id: DECODE_ONLY,
            short: None,
            long: "decode-only",
            kind: OptionKind::Flag,
            repeatable: true,
            conflicts: &[PROBE_ONLY, VERIFY_ONLY, VERIFY],
            help: "Decode, but do not play the audio",
        },
        OptionSpec {
            id: PROBE_ONLY,
            short: None,
            long: "probe-only",
            kind: OptionKind::Flag,
            repeatable: true,
            conflicts: &[DECODE_ONLY, VERIFY_ONLY],
            help: "Only probe the input for metadata",
        },
        OptionSpec {
            id: VERIFY_ONLY,
            short: None,
            long: "verify-only",
            kind: OptionKind::Flag,
            repeatable: true,
            conflicts: &[VERIFY],
            help: "Verify the decoded audio is valid, but do not play the audio",
        },
        OptionSpec {
            id: VERIFY,
            short: Some('v'),
            long: "verify",
            kind: OptionKind::Flag,
            repeatable: true,
            conflicts: &[],
            help: "Verify the decoded audio is valid during playback",
        },
        OptionSpec {
            id: NO_PROGRESS,
            short: None,
            long: "no-progress",
            kind: OptionKind::Flag,
            repeatable: true,
            conflicts: &[],
            help: "Do not display playback progress",
        },
        OptionSpec {
            id: NO_GAPLESS,
            short: None,
            long: "no-gapless",
            kind: OptionKind::Flag,
            repeatable: true,
            conflicts: &[],
            help: "Disable gapless decoding and playback",
        },
        OptionSpec {
            id: DUMP_VISUALS,
            short: None,
            long: "dump-visuals",
            kind: OptionKind::Flag,
            repeatable: true,
            conflicts: &[],
            help: "Dump all visuals to the current working directory",
        },
        OptionSpec {
            id: HELP,
            short: Some('h'),
            long: "help",
            kind: OptionKind::Help,
            repeatable: true,
            conflicts: &[],
            help: "Print help",
        },
        OptionSpec {
            id: VERSION,
            short: Some('V'),
            long: "version",
            kind: OptionKind::Version,
            repeatable: true,
            conflicts: &[],
            help: "Print version",
        },
    ],
    positionals: &[PositionalSpec {
        name: INPUT,
        required: true,
        repeatable: false,
        help: "The input file path, or - to use standard input",
    }],
    examples: &[
        "symphonia-play path/to/audio.flac",
        "symphonia-play --probe-only path/to/audio.flac",
        "symphonia-play --verify path/to/audio.flac",
        "cat path/to/audio.flac | symphonia-play -",
    ],
};

#[cfg(test)]
mod tests {
    use super::{
        COMMAND, INPUT, OptionKind, OptionSpec, PositionalSpec,
    };

    fn options() -> Vec<&'static OptionSpec> {
        COMMAND.options.iter().collect()
    }

    #[test]
    fn command_metadata_is_complete() {
        assert!(!COMMAND.name.is_empty());
        assert!(!COMMAND.description.is_empty());
        assert!(!COMMAND.options.is_empty());
        assert!(!COMMAND.positionals.is_empty());
        assert!(!COMMAND.examples.is_empty());

        for option in options() {
            assert!(!option.id.is_empty(), "option with empty id");
            assert!(!option.long.is_empty(), "option '{}' has an empty name", option.id);
            assert!(!option.help.is_empty(), "option '{}' has no help", option.id);
        }

        for positional in COMMAND.positionals {
            assert!(!positional.name.is_empty());
            assert!(!positional.help.is_empty(), "positional '{}' has no help", positional.name);
        }
    }

    #[test]
    fn names_and_spellings_are_unique() {
        let options = options();

        for (index, option) in options.iter().enumerate() {
            for other in &options[index + 1..] {
                assert_ne!(option.id, other.id, "duplicate option id '{}'", option.id);
                assert_ne!(
                    option.long, other.long,
                    "duplicate long option '--{}'",
                    option.long
                );

                if let (Some(short), Some(other_short)) = (option.short, other.short) {
                    assert_ne!(short, other_short, "duplicate short option '-{short}'");
                }
            }
        }

        for (index, positional) in COMMAND.positionals.iter().enumerate() {
            for other in &COMMAND.positionals[index + 1..] {
                assert_ne!(positional.name, other.name, "duplicate positional");
            }
        }
    }

    #[test]
    fn conflicts_reference_registered_options() {
        for option in options() {
            for conflict in option.conflicts {
                assert!(
                    COMMAND.option(conflict).is_some(),
                    "option '{}' conflicts with unregistered '{}'",
                    option.id,
                    conflict
                );
                assert_ne!(option.id, *conflict, "option '{}' conflicts with itself", option.id);
            }
        }
    }

    #[test]
    fn value_options_have_value_names_and_flags_have_none() {
        for option in options() {
            match &option.kind {
                OptionKind::Value(value) => {
                    assert!(!value.name.is_empty(), "option '{}' has no value name", option.id);
                }
                OptionKind::Flag | OptionKind::Help | OptionKind::Version => (),
            }
        }
    }

    #[test]
    fn input_positional_is_required_and_not_repeated() {
        let positional: &PositionalSpec = COMMAND
            .positionals
            .iter()
            .find(|positional| positional.name == INPUT)
            .expect("INPUT positional is registered");
        assert!(positional.required);
        assert!(!positional.repeatable);
    }
}
