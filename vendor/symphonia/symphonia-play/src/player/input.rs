// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Command {
    SeekBy(i64),
    Next,
    Previous,
    Quit,
}

pub struct Keyboard {
    raw: Option<RawMode>,
}

struct RawMode;

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

impl Keyboard {
    pub fn new(enabled: bool) -> io::Result<Self> {
        if !enabled {
            return Ok(Self::disabled());
        }

        enable_raw_mode()?;

        Ok(Self { raw: Some(RawMode) })
    }

    pub fn disabled() -> Self {
        Self { raw: None }
    }

    pub fn is_raw(&self) -> bool {
        self.raw.is_some()
    }

    pub fn poll(&self) -> Option<Command> {
        self.raw.as_ref()?;

        loop {
            match event::poll(Duration::ZERO) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(key)) => {
                        if let Some(command) = map_key(key) {
                            return Some(command);
                        }
                    }
                    Ok(_) => (),
                    Err(_) => return None,
                },
                _ => return None,
            }
        }
    }
}

fn map_key(event: KeyEvent) -> Option<Command> {
    if event.kind == KeyEventKind::Release {
        return None;
    }

    let control = event.modifiers.contains(KeyModifiers::CONTROL);

    match event.code {
        KeyCode::Left if control => Some(Command::Previous),
        KeyCode::Right if control => Some(Command::Next),
        KeyCode::Left => Some(Command::SeekBy(-5)),
        KeyCode::Right => Some(Command::SeekBy(5)),
        KeyCode::Char('[') => Some(Command::Previous),
        KeyCode::Char(']') => Some(Command::Next),
        KeyCode::Char('q') => Some(Command::Quit),
        KeyCode::Char('c') if control => Some(Command::Quit),
        KeyCode::Esc => Some(Command::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, map_key};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    fn event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn maps_arrows_to_five_second_seeks() {
        assert_eq!(map_key(event(KeyCode::Left, KeyModifiers::NONE)), Some(Command::SeekBy(-5)));
        assert_eq!(map_key(event(KeyCode::Right, KeyModifiers::NONE)), Some(Command::SeekBy(5)));
    }

    #[test]
    fn maps_control_arrows_and_brackets_to_track_changes() {
        for modifiers in [KeyModifiers::CONTROL, KeyModifiers::CONTROL | KeyModifiers::SHIFT] {
            assert_eq!(map_key(event(KeyCode::Left, modifiers)), Some(Command::Previous));
            assert_eq!(map_key(event(KeyCode::Right, modifiers)), Some(Command::Next));
        }

        assert_eq!(map_key(event(KeyCode::Char('['), KeyModifiers::NONE)), Some(Command::Previous));
        assert_eq!(map_key(event(KeyCode::Char(']'), KeyModifiers::NONE)), Some(Command::Next));
    }

    #[test]
    fn maps_quit_keys() {
        assert_eq!(map_key(event(KeyCode::Char('q'), KeyModifiers::NONE)), Some(Command::Quit));
        assert_eq!(map_key(event(KeyCode::Esc, KeyModifiers::NONE)), Some(Command::Quit));
        assert_eq!(
            map_key(event(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Command::Quit)
        );
    }

    #[test]
    fn ignores_key_releases() {
        for code in [KeyCode::Left, KeyCode::Right, KeyCode::Char('q'), KeyCode::Esc] {
            let mut release = event(code, KeyModifiers::NONE);
            release.kind = KeyEventKind::Release;
            assert_eq!(map_key(release), None);
        }
    }

    #[test]
    fn accepts_key_repeats() {
        let mut repeat = event(KeyCode::Right, KeyModifiers::NONE);
        repeat.kind = KeyEventKind::Repeat;
        assert_eq!(map_key(repeat), Some(Command::SeekBy(5)));
    }

    #[test]
    fn ignores_unmapped_keys() {
        assert_eq!(map_key(event(KeyCode::Char('x'), KeyModifiers::NONE)), None);
        assert_eq!(map_key(event(KeyCode::Up, KeyModifiers::NONE)), None);
    }
}
