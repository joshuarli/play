use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

use crate::cmd::{Command, EndReason, UiUpdate};
use crate::time::format_time;

/// Run audio-only terminal mode. Blocks until quit or EOF.
/// Returns the EndReason so the playlist loop knows what to do.
pub fn run_terminal(
    cmd_tx: Sender<Command>,
    ui_update_rx: Receiver<UiUpdate>,
    filename: &str,
    duration_us: i64,
) -> EndReason {
    let mut stdout = io::stdout().into_raw_mode().expect("failed to enter raw mode");
    let mut keys = termion::async_stdin().keys();

    let dur = format_time(duration_us);
    write!(stdout, "\r\x1b[K\u{25b6} 00:00:00 / {dur}  {filename}").ok();
    stdout.flush().ok();

    let mut end_reason = EndReason::Quit;

    loop {
        if let Some(Ok(key)) = keys.next() {
            let cmd = match key {
                Key::Char('q') => Some(Command::Quit),
                Key::Char(' ') => Some(Command::PlayPause),
                Key::Left => Some(Command::SeekRelative {
                    seconds: -5.0,
                    exact: false,
                }),
                Key::Right => Some(Command::SeekRelative {
                    seconds: 5.0,
                    exact: false,
                }),
                Key::Up => Some(Command::VolumeUp),
                Key::Down => Some(Command::VolumeDown),
                Key::Char('a') => Some(Command::CycleAudioTrack),
                Key::Char('+') | Key::Char('=') => Some(Command::AudioDelayIncrease),
                Key::Char('-') => Some(Command::AudioDelayDecrease),
                Key::Char('>') | Key::Char('.') => Some(Command::NextFile),
                Key::Char('<') | Key::Char(',') => Some(Command::PrevFile),
                _ => None,
            };
            if let Some(cmd) = cmd {
                let quit = matches!(cmd, Command::Quit);
                let _ = cmd_tx.send(cmd);
                if quit {
                    end_reason = EndReason::Quit;
                    break;
                }
            }
        }

        let mut should_break = false;
        while let Ok(update) = ui_update_rx.try_recv() {
            match update {
                UiUpdate::Osd(text) => {
                    write!(stdout, "\r\x1b[K{text}").ok();
                    stdout.flush().ok();
                }
                UiUpdate::EndOfFile(reason) => {
                    end_reason = reason;
                    should_break = true;
                }
                _ => {}
            }
        }
        if should_break {
            let _ = cmd_tx.send(Command::Quit);
            break;
        }

        thread::sleep(Duration::from_millis(50));
    }

    drop(stdout); // restores terminal via Drop
    let mut out = io::stdout();
    writeln!(out).ok();

    end_reason
}
