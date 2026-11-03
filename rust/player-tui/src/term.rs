use std::env;
use std::io::{self, Write};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::{SigId, flag, low_level};

use crate::art::{COVER_SIZE, Cover};
use crate::canvas::{Canvas, Rect};

pub struct ShutdownFlag {
    requested: Arc<AtomicBool>,
    registrations: Vec<SigId>,
}

impl ShutdownFlag {
    pub fn install() -> io::Result<Self> {
        let requested = Arc::new(AtomicBool::new(false));
        let mut registrations = Vec::with_capacity(3);
        for signal in [SIGHUP, SIGINT, SIGTERM] {
            match flag::register(signal, Arc::clone(&requested)) {
                Ok(id) => registrations.push(id),
                Err(error) => {
                    for id in registrations.drain(..) {
                        low_level::unregister(id);
                    }
                    return Err(error);
                }
            }
        }
        Ok(Self {
            requested,
            registrations,
        })
    }

    pub fn requested(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }
}

impl Drop for ShutdownFlag {
    fn drop(&mut self) {
        for id in self.registrations.drain(..) {
            low_level::unregister(id);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtMode {
    Kitty,
    Blocks,
}

pub fn detect_art_mode() -> ArtMode {
    if let Ok(value) = env::var("PLAYER_TUI_ART") {
        return match value.as_str() {
            "blocks" | "half-block" | "halfblock" => ArtMode::Blocks,
            "kitty" | "pixels" => ArtMode::Kitty,
            _ => ArtMode::Blocks,
        };
    }
    let in_tmux = env::var_os("TMUX").is_some();
    if in_tmux && !tmux_passthrough() {
        return ArtMode::Blocks;
    }
    let known = env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
        || env::var_os("WEZTERM_EXECUTABLE").is_some()
        || env::var_os("KITTY_WINDOW_ID").is_some()
        || matches!(
            env::var("TERM_PROGRAM").as_deref(),
            Ok("ghostty") | Ok("WezTerm") | Ok("kitty")
        )
        || matches!(
            env::var("TERM").as_deref(),
            Ok("xterm-kitty") | Ok("xterm-ghostty")
        );
    if known { ArtMode::Kitty } else { ArtMode::Blocks }
}

fn tmux_passthrough() -> bool {
    for args in [
        ["show-options", "-gv", "allow-passthrough"],
        ["show-options", "-wv", "allow-passthrough"],
    ] {
        if let Ok(output) = Command::new("tmux").args(args).output()
            && output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim() == "on"
        {
            return true;
        }
    }
    false
}

struct Kitty {
    enabled: bool,
    in_tmux: bool,
    id: u32,
    key: Option<(usize, usize, usize, usize)>,
}

impl Kitty {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            in_tmux: env::var_os("TMUX").is_some(),
            id: (std::process::id() & 0x3fff_ffff).max(1),
            key: None,
        }
    }

    fn send(&self, sequence: &[u8]) {
        let mut stdout = io::stdout().lock();
        if self.in_tmux {
            let mut wrapped = Vec::with_capacity(sequence.len() + 16);
            wrapped.extend_from_slice(b"\x1bPtmux;");
            for byte in sequence {
                if *byte == 0x1b {
                    wrapped.push(0x1b);
                }
                wrapped.push(*byte);
            }
            wrapped.extend_from_slice(b"\x1b\\");
            let _ = stdout.write_all(&wrapped);
        } else {
            let _ = stdout.write_all(sequence);
        }
    }

    fn place(&mut self, cover: &Cover, rect: Rect) {
        if !self.enabled || rect.w == 0 || rect.h == 0 {
            return;
        }
        let key = (rect.x, rect.y, rect.w, rect.h);
        if self.key == Some(key) {
            return;
        }
        self.clear();
        let _ = write!(
            io::stdout().lock(),
            "\x1b[{};{}H",
            rect.y + 1,
            rect.x + 1
        );
        let data = STANDARD.encode(&cover.pixels);
        let mut remaining = data.as_bytes();
        let mut first = true;
        while !remaining.is_empty() {
            let take = remaining.len().min(3072);
            let (chunk, rest) = remaining.split_at(take);
            let more = usize::from(!rest.is_empty());
            let mut sequence = Vec::with_capacity(chunk.len() + 96);
            if first {
                let _ = write!(
                    sequence,
                    "\x1b_Ga=T,f=24,s={COVER_SIZE},v={COVER_SIZE},c={},r={},C=1,z=1,q=2,i={},m={more};",
                    rect.w, rect.h, self.id
                );
                first = false;
            } else {
                let _ = write!(sequence, "\x1b_Gm={more};");
            }
            sequence.extend_from_slice(chunk);
            sequence.extend_from_slice(b"\x1b\\");
            self.send(&sequence);
            remaining = rest;
        }
        self.key = Some(key);
    }

    fn clear(&mut self) {
        if self.key.is_none() {
            return;
        }
        self.send(format!("\x1b_Ga=d,d=i,i={},q=2\x1b\\", self.id).as_bytes());
        self.key = None;
    }
}

impl Drop for Kitty {
    fn drop(&mut self) {
        self.clear();
    }
}

pub struct Terminal {
    kitty: Kitty,
    pub mode: ArtMode,
    active: bool,
}

impl Terminal {
    pub fn enter(mode: ArtMode) -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout().lock();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        stdout.write_all(b"\x1b[2J")?;
        stdout.flush()?;
        Ok(Self {
            kitty: Kitty::new(mode == ArtMode::Kitty),
            mode,
            active: true,
        })
    }

    pub fn size() -> io::Result<(usize, usize)> {
        let (cols, rows) = crossterm::terminal::size()?;
        Ok((cols as usize, rows as usize))
    }

    pub fn clear(&mut self) {
        let _ = io::stdout().lock().write_all(b"\x1b[2J");
    }

    pub fn invalidate_art(&mut self) {
        self.kitty.clear();
    }

    pub fn draw(&mut self, canvas: &Canvas, cover: Option<&Cover>, rect: Rect) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        stdout.write_all(canvas.render().as_bytes())?;
        stdout.flush()?;
        drop(stdout);
        if self.mode == ArtMode::Kitty {
            if let Some(cover) = cover {
                self.kitty.place(cover, rect);
            } else {
                self.kitty.clear();
            }
        }
        Ok(())
    }

    pub fn leave(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.kitty.clear();
        let mut stdout = io::stdout().lock();
        let _ = execute!(stdout, Show, LeaveAlternateScreen);
        let _ = stdout.flush();
        let _ = disable_raw_mode();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.leave();
    }
}

pub enum Key {
    Pause,
    Seek(i32),
    Volume(i32),
    Mute,
    Next,
    Previous,
    Restart,
    Quit,
    Resize,
}

pub fn read_key(timeout: Duration) -> io::Result<Option<Key>> {
    if !event::poll(timeout)? {
        return Ok(None);
    }
    let event = event::read()?;
    if matches!(event, Event::Resize(_, _)) {
        return Ok(Some(Key::Resize));
    }
    let Event::Key(KeyEvent {
        code,
        modifiers,
        kind,
        ..
    }) = event
    else {
        return Ok(None);
    };
    if kind == KeyEventKind::Release {
        return Ok(None);
    }
    let control = modifiers.contains(KeyModifiers::CONTROL);
    let key = match code {
        KeyCode::Char(' ') => Key::Pause,
        KeyCode::Left => Key::Seek(-5),
        KeyCode::Right => Key::Seek(5),
        KeyCode::Up => Key::Volume(5),
        KeyCode::Down => Key::Volume(-5),
        KeyCode::Char('m') | KeyCode::Char('M') => Key::Mute,
        KeyCode::Char('n') | KeyCode::Char('N') => Key::Next,
        KeyCode::Char('p') | KeyCode::Char('P') => Key::Previous,
        KeyCode::Char('r') | KeyCode::Char('R') => Key::Restart,
        KeyCode::Char('q') | KeyCode::Char('Q') => Key::Quit,
        KeyCode::Char('c') | KeyCode::Char('C') if control => Key::Quit,
        _ => return Ok(None),
    };
    Ok(Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_art_mode_wins() {
        unsafe {
            env::set_var("PLAYER_TUI_ART", "blocks");
        }
        assert_eq!(detect_art_mode(), ArtMode::Blocks);
        unsafe {
            env::set_var("PLAYER_TUI_ART", "kitty");
        }
        assert_eq!(detect_art_mode(), ArtMode::Kitty);
        unsafe {
            env::remove_var("PLAYER_TUI_ART");
        }
    }
}
