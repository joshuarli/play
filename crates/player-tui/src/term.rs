use std::env;
use std::io::{self, Write};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

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
    let explicit = env::var("PLAYER_TUI_ART").ok();
    let in_tmux = env::var_os("TMUX").is_some();
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
    let passthrough = in_tmux && tmux_passthrough();
    resolve_art_mode(explicit.as_deref(), in_tmux, passthrough, known)
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

fn resolve_art_mode(
    explicit: Option<&str>,
    in_tmux: bool,
    passthrough: bool,
    known: bool,
) -> ArtMode {
    if let Some(value) = explicit {
        return match value {
            "kitty" | "pixels" => ArtMode::Kitty,
            _ => ArtMode::Blocks,
        };
    }
    if in_tmux && !passthrough {
        return ArtMode::Blocks;
    }
    if known { ArtMode::Kitty } else { ArtMode::Blocks }
}

fn transmit_header(placeholders: bool, id: u32, rect: Rect, more: usize) -> String {
    if placeholders {
        format!("\x1b_Ga=T,U=1,C=1,f=24,s={COVER_SIZE},v={COVER_SIZE},q=2,i={id},m={more};")
    } else {
        format!(
            "\x1b_Ga=T,f=24,s={COVER_SIZE},v={COVER_SIZE},c={},r={},C=1,z=1,q=2,i={id},m={more};",
            rect.w, rect.h
        )
    }
}

const PLACEHOLDER: char = '\u{10EEEE}';

const DIACRITICS: &[char] = &[
    '\u{0305}', '\u{030D}', '\u{030E}', '\u{0310}', '\u{0312}', '\u{033D}', '\u{033E}',
    '\u{033F}', '\u{0346}', '\u{034A}', '\u{034B}', '\u{034C}', '\u{0350}', '\u{0351}',
    '\u{0352}', '\u{0357}', '\u{035B}', '\u{0363}', '\u{0364}', '\u{0365}', '\u{0366}',
    '\u{0367}', '\u{0368}', '\u{0369}', '\u{036A}', '\u{036B}', '\u{036C}', '\u{036D}',
    '\u{036E}', '\u{036F}', '\u{0483}', '\u{0484}', '\u{0485}', '\u{0486}', '\u{0487}',
    '\u{0592}', '\u{0593}', '\u{0594}', '\u{0595}', '\u{0597}', '\u{0598}', '\u{0599}',
    '\u{059C}', '\u{059D}', '\u{059E}', '\u{059F}', '\u{05A0}', '\u{05A1}', '\u{05A8}',
    '\u{05A9}', '\u{05AB}', '\u{05AC}',
];

struct Kitty {
    enabled: bool,
    in_tmux: bool,
    placeholders: bool,
    id: u32,
    key: Option<(usize, usize, usize, usize)>,
}

impl Kitty {
    fn new(enabled: bool) -> Self {
        let in_tmux = env::var_os("TMUX").is_some();
        Self {
            enabled,
            placeholders: enabled && in_tmux,
            in_tmux,
            id: (std::process::id() & 0x00ff_ffff).max(1),
            key: None,
        }
    }

    fn send(&self, sequence: &[u8]) {
        let _ = io::stdout().lock().write_all(&for_terminal(sequence, self.in_tmux));
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
        if !self.placeholders {
            let _ = write!(io::stdout().lock(), "\x1b[{};{}H", rect.y + 1, rect.x + 1);
        }
        let data = STANDARD.encode(&cover.pixels);
        let mut remaining = data.as_bytes();
        let mut first = true;
        while !remaining.is_empty() {
            let take = remaining.len().min(3072);
            let (chunk, rest) = remaining.split_at(take);
            let more = usize::from(!rest.is_empty());
            let mut sequence = Vec::with_capacity(chunk.len() + 96);
            if first {
                let header = transmit_header(self.placeholders, self.id, rect, more);
                sequence.extend_from_slice(header.as_bytes());
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

    fn placeholder_grid(&self, rect: Rect) -> Vec<u8> {
        let mut out = Vec::with_capacity(rect.w * rect.h * 12 + 64);
        let r = (self.id >> 16) as u8;
        let g = (self.id >> 8) as u8;
        let b = self.id as u8;
        let _ = write!(out, "\x1b[38;2;{r};{g};{b}m\x1b[48;2;10;10;14m");
        for (row, y) in (rect.y..rect.y + rect.h).enumerate() {
            let _ = write!(out, "\x1b[{};{}H", y + 1, rect.x + 1);
            let row_mark = DIACRITICS[row.min(DIACRITICS.len() - 1)];
            for col in 0..rect.w {
                let col_mark = DIACRITICS[col.min(DIACRITICS.len() - 1)];
                let _ = write!(out, "{PLACEHOLDER}{row_mark}{col_mark}");
            }
        }
        out.extend_from_slice(b"\x1b[0m");
        out
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

fn for_terminal(sequence: &[u8], in_tmux: bool) -> Vec<u8> {
    if !in_tmux {
        return sequence.to_vec();
    }
    let mut wrapped = Vec::with_capacity(sequence.len() + 16);
    wrapped.extend_from_slice(b"\x1bPtmux;");
    for byte in sequence {
        if *byte == 0x1b {
            wrapped.push(0x1b);
        }
        wrapped.push(*byte);
    }
    wrapped.extend_from_slice(b"\x1b\\");
    wrapped
}

const KITTY_PROBE_ID: u32 = 31;

fn kitty_answers(in_tmux: bool) -> io::Result<bool> {
    let query = format!("\x1b_Gi={KITTY_PROBE_ID},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
    let mut stdout = io::stdout().lock();
    stdout.write_all(&for_terminal(query.as_bytes(), in_tmux))?;
    stdout.write_all(b"\x1b[c")?;
    stdout.flush()?;
    drop(stdout);
    let stdin = io::stdin();
    let fd = stdin.as_fd();
    let deadline = Instant::now() + Duration::from_millis(if in_tmux { 400 } else { 1000 });
    let mut reply = Vec::new();
    loop {
        if let Some(answer) = kitty_verdict(&reply, in_tmux) {
            return Ok(answer);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        let timeout = rustix::event::Timespec::try_from(remaining).map_err(io::Error::other)?;
        let mut fds = [rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN)];
        if rustix::event::poll(&mut fds, Some(&timeout))? == 0 {
            continue;
        }
        let mut chunk = [0u8; 256];
        let read = rustix::io::read(fd, &mut chunk)?;
        if read == 0 {
            return Ok(false);
        }
        reply.extend_from_slice(&chunk[..read]);
    }
}

/// Some(answer) once the reply settles whether kitty graphics work. Outside
/// tmux the terminal answers in order, so its DA1 reply means no kitty reply
/// is coming. tmux answers DA1 itself ahead of the terminal's kitty reply,
/// so there only an OK, or the caller's deadline, decides.
fn kitty_verdict(reply: &[u8], in_tmux: bool) -> Option<bool> {
    let ok = format!("\x1b_Gi={KITTY_PROBE_ID};OK");
    let has_ok = reply.windows(ok.len()).any(|window| window == ok.as_bytes());
    let has_da1 = reply
        .windows(3)
        .position(|window| window == b"\x1b[?")
        .is_some_and(|start| reply[start..].contains(&b'c'));
    match (has_ok, has_da1, in_tmux) {
        (true, true, _) => Some(true),
        (true, false, true) => Some(true),
        (false, true, false) => Some(false),
        _ => None,
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
        let in_tmux = env::var_os("TMUX").is_some();
        let mode = if mode == ArtMode::Kitty
            && env::var_os("PLAYER_TUI_ART").is_none()
            && !kitty_answers(in_tmux)?
        {
            ArtMode::Blocks
        } else {
            mode
        };
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
        let skip = if self.mode == ArtMode::Kitty && cover.is_some() {
            Some(rect)
        } else {
            None
        };
        let mut stdout = io::stdout().lock();
        stdout.write_all(canvas.render(skip).as_bytes())?;
        stdout.flush()?;
        drop(stdout);
        if self.mode == ArtMode::Kitty {
            if let Some(cover) = cover {
                self.kitty.place(cover, rect);
                if self.kitty.placeholders {
                    let grid = self.kitty.placeholder_grid(rect);
                    let mut stdout = io::stdout().lock();
                    stdout.write_all(&grid)?;
                    stdout.flush()?;
                }
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
    fn kitty_verdict_outside_tmux_waits_for_the_ordered_da1() {
        assert_eq!(kitty_verdict(b"", false), None);
        assert_eq!(kitty_verdict(b"\x1b_Gi=31;OK\x1b\\", false), None);
        assert_eq!(kitty_verdict(b"\x1b_Gi=31;OK\x1b\\\x1b[?62;22;52c", false), Some(true));
        assert_eq!(kitty_verdict(b"\x1b[?62;22;52c", false), Some(false));
        assert_eq!(kitty_verdict(b"\x1b_Gi=31;ENOENT\x1b\\\x1b[?62c", false), Some(false));
    }

    #[test]
    fn kitty_verdict_in_tmux_ignores_tmux_answering_da1_first() {
        assert_eq!(kitty_verdict(b"\x1b[?1;2;4c", true), None);
        assert_eq!(kitty_verdict(b"\x1b[?1;2;4c\x1b_Gi=31;OK\x1b\\", true), Some(true));
        assert_eq!(kitty_verdict(b"\x1b_Gi=31;OK", true), Some(true));
    }

    #[test]
    fn explicit_art_mode_wins() {
        for in_tmux in [false, true] {
            for passthrough in [false, true] {
                for known in [false, true] {
                    assert_eq!(resolve_art_mode(Some("blocks"), in_tmux, passthrough, known), ArtMode::Blocks);
                    assert_eq!(resolve_art_mode(Some("halfblock"), in_tmux, passthrough, known), ArtMode::Blocks);
                    assert_eq!(resolve_art_mode(Some("kitty"), in_tmux, passthrough, known), ArtMode::Kitty);
                    assert_eq!(resolve_art_mode(Some("pixels"), in_tmux, passthrough, known), ArtMode::Kitty);
                    assert_eq!(resolve_art_mode(Some("bogus"), in_tmux, passthrough, known), ArtMode::Blocks);
                }
            }
        }
    }

    #[test]
    fn auto_mode_requires_passthrough_inside_tmux() {
        assert_eq!(resolve_art_mode(None, false, false, true), ArtMode::Kitty);
        assert_eq!(resolve_art_mode(None, false, false, false), ArtMode::Blocks);
        assert_eq!(resolve_art_mode(None, true, false, true), ArtMode::Blocks);
        assert_eq!(resolve_art_mode(None, true, true, true), ArtMode::Kitty);
        assert_eq!(resolve_art_mode(None, true, true, false), ArtMode::Blocks);
    }

    #[test]
    fn placeholder_grid_encodes_the_image_id_and_cell_positions() {
        let kitty = Kitty {
            enabled: true,
            in_tmux: true,
            placeholders: true,
            id: 0x01_02_03,
            key: None,
        };
        let rect = Rect { x: 2, y: 3, w: 3, h: 2 };
        let grid = String::from_utf8(kitty.placeholder_grid(rect)).unwrap();
        assert!(grid.contains("\x1b[38;2;1;2;3m"));
        assert!(grid.contains("\x1b[48;2;10;10;14m"));
        assert!(grid.contains("\x1b[4;3H"));
        assert!(grid.contains("\x1b[5;3H"));
        assert_eq!(grid.matches(PLACEHOLDER).count(), 6);
        assert!(grid.contains(&format!("{PLACEHOLDER}{}{}", DIACRITICS[0], DIACRITICS[0])));
        assert!(grid.contains(&format!("{PLACEHOLDER}{}{}", DIACRITICS[0], DIACRITICS[2])));
        assert!(grid.contains(&format!("{PLACEHOLDER}{}{}", DIACRITICS[1], DIACRITICS[0])));
        assert!(grid.ends_with("\x1b[0m"));
    }

    #[test]
    fn placeholder_transmission_uses_a_virtual_placement() {
        let rect = Rect { x: 0, y: 0, w: 10, h: 5 };
        let virtual_placement = transmit_header(true, 7, rect, 1);
        assert!(virtual_placement.contains("a=T,U=1"));
        assert!(virtual_placement.contains("i=7,m=1;"));
        assert!(!virtual_placement.contains("c=10"));

        let real_placement = transmit_header(false, 7, rect, 0);
        assert!(real_placement.contains("c=10,r=5"));
        assert!(!real_placement.contains("U=1"));
        assert!(real_placement.contains("i=7,m=0;"));
    }
}
