// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::OsString;
use std::fmt;

use crate::cli::command::{COMMAND, OptionSpec};

#[derive(Debug)]
pub enum Error {
    UnknownOption(String),
    UnexpectedArgument(OsString),
    UnexpectedValue { option: String, value: OsString },
    MissingValue(String),
    InvalidValue { option: String, value: OsString, reason: String },
    NonUnicodeValue(String),
    DuplicateOption(String),
    ConflictingOptions { option: String, other: String },
    MissingArgument(&'static str),
    UnhandledOption(String),
    Parse(String),
}

impl Error {
    pub fn unknown_option(option: String) -> Self {
        Error::UnknownOption(option)
    }

    pub fn unexpected_argument(value: OsString) -> Self {
        Error::UnexpectedArgument(value)
    }

    pub fn missing_value(option: &OptionSpec) -> Self {
        Error::MissingValue(option.label())
    }

    pub fn invalid_value(option: &OptionSpec, value: OsString, reason: String) -> Self {
        Error::InvalidValue { option: option.label(), value, reason }
    }

    pub fn non_unicode_value(option: &OptionSpec) -> Self {
        Error::NonUnicodeValue(option.label())
    }

    pub fn duplicate_option(option: &OptionSpec) -> Self {
        Error::DuplicateOption(option.label())
    }

    pub fn conflicting_options(option: &OptionSpec, other: &OptionSpec) -> Self {
        Error::ConflictingOptions { option: option.label(), other: other.label() }
    }

    pub fn missing_argument(name: &'static str) -> Self {
        Error::MissingArgument(name)
    }

    pub fn unhandled_option(option: &OptionSpec) -> Self {
        Error::UnhandledOption(option.label())
    }

    pub fn from_lexopt(error: lexopt::Error) -> Self {
        match error {
            lexopt::Error::UnexpectedOption(option) => Error::UnknownOption(option),
            lexopt::Error::UnexpectedArgument(value) => Error::UnexpectedArgument(value),
            lexopt::Error::UnexpectedValue { option, value } => {
                Error::UnexpectedValue { option, value }
            }
            other => Error::Parse(other.to_string()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownOption(option) => {
                write!(f, "unknown option '{option}' for '{}'", COMMAND.name)
            }
            Error::UnexpectedArgument(value) => {
                write!(f, "unexpected argument {value:?} for '{}'", COMMAND.name)
            }
            Error::UnexpectedValue { option, value } => {
                write!(f, "unexpected value {value:?} for '{option}'")
            }
            Error::MissingValue(option) => write!(f, "missing value for '{option}'"),
            Error::InvalidValue { option, value, reason } => {
                write!(f, "invalid value {value:?} for '{option}': {reason}")
            }
            Error::NonUnicodeValue(option) => {
                write!(f, "invalid value for '{option}': value is not valid UTF-8")
            }
            Error::DuplicateOption(option) => {
                write!(f, "'{option}' cannot be used multiple times")
            }
            Error::ConflictingOptions { option, other } => {
                write!(f, "'{option}' cannot be used with '{other}'")
            }
            Error::MissingArgument(name) => {
                write!(f, "missing required argument '{name}' for '{}'", COMMAND.name)
            }
            Error::UnhandledOption(option) => {
                write!(f, "internal error: option '{option}' has no handler")
            }
            Error::Parse(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}
