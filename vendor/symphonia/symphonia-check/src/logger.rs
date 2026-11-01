// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::env;
use std::io::{IsTerminal, Write};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use log::{Level, LevelFilter, Log, Metadata, Record};
use regex_lite::Regex;

const RUST_LOG_ENV: &str = "RUST_LOG";
const DEFAULT_LEVEL: LevelFilter = LevelFilter::Info;

static LOGGER: OnceLock<Logger> = OnceLock::new();

pub fn init() {
    let logger = LOGGER.get_or_init(Logger::from_env);

    if log::set_logger(logger).is_ok() {
        log::set_max_level(logger.filter.max_level());
    }
}

struct Logger {
    filter: Filter,
    color: bool,
}

impl Logger {
    fn from_env() -> Self {
        let filter = match env::var(RUST_LOG_ENV) {
            Ok(spec) => Filter::parse(&spec),
            Err(_) => Filter::default(),
        };

        Logger { filter, color: std::io::stderr().is_terminal() }
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.filter.enabled(metadata.level().to_level_filter(), metadata.target())
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        if !self.filter.message_matches(&record.args().to_string()) {
            return;
        }

        let target = record.target();
        let width = max_target_width(target);
        let level = level_label(record.level());
        let stderr = std::io::stderr();
        let mut stderr = stderr.lock();

        let _ = if self.color {
            let color = level_color(record.level());
            writeln!(
                stderr,
                " {color}{level}\x1b[0m \x1b[1m{target:<width$}\x1b[0m > {}",
                record.args()
            )
        }
        else {
            writeln!(stderr, " {level} {target:<width$} > {}", record.args())
        };
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

#[derive(Debug)]
struct Filter {
    directives: Vec<Directive>,
    message: Option<Regex>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter::global(DEFAULT_LEVEL)
    }
}

impl Filter {
    fn global(level: LevelFilter) -> Self {
        Filter { directives: vec![Directive { target: None, level }], message: None }
    }

    fn parse(spec: &str) -> Self {
        let mut parts = spec.split('/');
        let directives = parts.next().unwrap_or_default();
        let message = parts.next();

        if parts.next().is_some() {
            eprintln!("warning: invalid logging spec '{spec}', ignoring it (too many '/'s)");
            return Filter::global(LevelFilter::Error);
        }

        let mut filter = Filter { directives: Vec::new(), message: None };

        for directive in directives.split(',').map(str::trim_ascii) {
            if directive.is_empty() {
                continue;
            }

            if let Some(directive) = parse_directive(directive) {
                filter.insert(directive);
            }
        }

        if filter.directives.is_empty() {
            filter.directives.push(Directive { target: None, level: LevelFilter::Error });
        }
        else {
            filter.directives.sort_by_key(|directive| directive.target_width());
        }

        if let Some(pattern) = message {
            match Regex::new(pattern) {
                Ok(regex) => filter.message = Some(regex),
                Err(err) => eprintln!("warning: invalid regex filter - {err}"),
            }
        }

        filter
    }

    fn insert(&mut self, directive: Directive) {
        if let Some(existing) =
            self.directives.iter_mut().find(|existing| existing.target == directive.target)
        {
            *existing = directive;
        }
        else {
            self.directives.push(directive);
        }
    }

    fn enabled(&self, level: LevelFilter, target: &str) -> bool {
        self.directives
            .iter()
            .rev()
            .find(|directive| {
                directive.target.as_deref().is_none_or(|prefix| target.starts_with(prefix))
            })
            .is_some_and(|directive| level <= directive.level)
    }

    fn max_level(&self) -> LevelFilter {
        self.directives.iter().map(|directive| directive.level).max().unwrap_or(LevelFilter::Off)
    }

    fn message_matches(&self, message: &str) -> bool {
        self.message.as_ref().is_none_or(|regex| regex.is_match(message))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Directive {
    target: Option<String>,
    level: LevelFilter,
}

impl Directive {
    fn target_width(&self) -> usize {
        self.target.as_ref().map_or(0, String::len)
    }
}

fn parse_directive(spec: &str) -> Option<Directive> {
    let mut parts = spec.split('=');
    let target = parts.next().unwrap_or_default().trim_ascii();

    match (parts.next().map(str::trim_ascii), parts.next()) {
        (None, None) => match target.parse() {
            Ok(level) => Some(Directive { target: None, level }),
            Err(_) => {
                Some(Directive { target: Some(target.to_owned()), level: LevelFilter::Trace })
            }
        },
        (Some(""), None) => {
            Some(Directive { target: Some(target.to_owned()), level: LevelFilter::Trace })
        }
        (Some(level), None) => match level.parse() {
            Ok(level) => Some(Directive { target: Some(target.to_owned()), level }),
            Err(_) => {
                eprintln!("warning: invalid logging spec '{spec}', ignoring it");
                None
            }
        },
        _ => {
            eprintln!("warning: invalid logging spec '{spec}', ignoring it");
            None
        }
    }
}

static MAX_TARGET_WIDTH: AtomicUsize = AtomicUsize::new(0);

fn max_target_width(target: &str) -> usize {
    let width = MAX_TARGET_WIDTH.load(Ordering::Relaxed);

    if width < target.len() {
        MAX_TARGET_WIDTH.store(target.len(), Ordering::Relaxed);
        target.len()
    }
    else {
        width
    }
}

fn level_label(level: Level) -> &'static str {
    match level {
        Level::Trace => "TRACE",
        Level::Debug => "DEBUG",
        Level::Info => "INFO ",
        Level::Warn => "WARN ",
        Level::Error => "ERROR",
    }
}

fn level_color(level: Level) -> &'static str {
    match level {
        Level::Trace => "\x1b[35m",
        Level::Debug => "\x1b[34m",
        Level::Info => "\x1b[32m",
        Level::Warn => "\x1b[33m",
        Level::Error => "\x1b[31m",
    }
}
