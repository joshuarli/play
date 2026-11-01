// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fmt::Write;

use crate::cli::command::{CommandSpec, OptionKind, OptionSpec};

pub fn usage(command: &CommandSpec) -> String {
    let mut usage = format!("Usage: {}", command.name);

    if !command.options.is_empty() {
        usage.push_str(" [OPTIONS]");
    }

    for positional in command.positionals {
        usage.push(' ');

        if positional.required {
            write!(usage, "<{}>", positional.name).expect("writing to a String cannot fail");
        }
        else {
            write!(usage, "[{}]", positional.name).expect("writing to a String cannot fail");
        }

        if positional.repeatable {
            usage.push_str("...");
        }
    }

    usage
}

pub fn render(command: &CommandSpec) -> String {
    let mut output = String::new();

    writeln!(output, "{}", command.description).expect("writing to a String cannot fail");
    writeln!(output).expect("writing to a String cannot fail");
    writeln!(output, "{}", usage(command)).expect("writing to a String cannot fail");

    if !command.positionals.is_empty() {
        writeln!(output).expect("writing to a String cannot fail");
        writeln!(output, "Arguments:").expect("writing to a String cannot fail");

        let width = command
            .positionals
            .iter()
            .map(|positional| positional.name.len() + 2)
            .max()
            .unwrap_or(0);

        for positional in command.positionals {
            let label = if positional.required {
                format!("<{}>", positional.name)
            }
            else {
                format!("[{}]", positional.name)
            };

            let mut details = positional.help.to_owned();
            if positional.required {
                details.push_str(" [required]");
            }
            if positional.repeatable {
                details.push_str(" [repeatable]");
            }

            writeln!(output, "  {label:<width$}  {details}")
                .expect("writing to a String cannot fail");
        }
    }

    if !command.options.is_empty() {
        writeln!(output).expect("writing to a String cannot fail");
        writeln!(output, "Options:").expect("writing to a String cannot fail");

        let width = command.options.iter().map(|option| option.label().len()).max().unwrap_or(0);

        for option in command.options {
            writeln!(output, "  {:<width$}  {}", option.label(), option_details(option))
                .expect("writing to a String cannot fail");
        }
    }

    if !command.examples.is_empty() {
        writeln!(output).expect("writing to a String cannot fail");
        writeln!(output, "Examples:").expect("writing to a String cannot fail");

        for example in command.examples {
            writeln!(output, "  {example}").expect("writing to a String cannot fail");
        }
    }

    output
}

fn option_details(option: &OptionSpec) -> String {
    match &option.kind {
        OptionKind::Flag => format!("{} [default: false]", option.help),
        OptionKind::Value(value) => {
            format!("{} [default: {}]", option.help, value.default.unwrap_or("none"))
        }
        OptionKind::Help | OptionKind::Version => option.help.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{render, usage};
    use crate::cli::command::{COMMAND, OptionKind};

    #[test]
    fn usage_is_rendered_from_the_schema() {
        assert_eq!(usage(&COMMAND), "Usage: symphonia-play [OPTIONS] <INPUT>");
    }

    #[test]
    fn help_contains_every_option_and_positional() {
        let help = render(&COMMAND);

        assert!(help.contains(COMMAND.description));
        assert!(help.contains(&usage(&COMMAND)));
        assert!(help.contains("Arguments:"));
        assert!(help.contains("Options:"));
        assert!(help.contains("Examples:"));

        for option in COMMAND.options {
            let label = option.label();
            assert!(help.contains(&label), "help does not contain option label '{label}'");
            assert!(
                help.contains(option.help),
                "help does not contain description for '{}'",
                option.id
            );
        }

        for positional in COMMAND.positionals {
            assert!(
                help.contains(&format!("<{}>", positional.name)),
                "help does not contain positional '{}'",
                positional.name
            );
            assert!(help.contains(positional.help));
        }

        for example in COMMAND.examples {
            assert!(help.contains(example), "help does not contain example '{example}'");
        }
    }

    #[test]
    fn help_shows_value_names_and_defaults() {
        let help = render(&COMMAND);

        for option in COMMAND.options {
            match &option.kind {
                OptionKind::Value(value) => {
                    assert!(help.contains(&format!("<{}>", value.name)));
                    assert!(help.contains(&format!(
                        "[default: {}]",
                        value.default.unwrap_or("none")
                    )));
                }
                OptionKind::Flag => assert!(help.contains("[default: false]")),
                OptionKind::Help | OptionKind::Version => (),
            }
        }
    }

    #[test]
    fn help_keeps_schema_order() {
        let help = render(&COMMAND);
        let mut previous = 0;

        for option in COMMAND.options {
            let position = help.find(&option.label()).expect("option is rendered");
            assert!(position >= previous, "option '{}' is out of order", option.id);
            previous = position;
        }
    }
}
