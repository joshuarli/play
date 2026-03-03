use std::io::{self, Write};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal;

use crate::cmd::{Command, UiUpdate};
use crate::time::format_time;

/// Run audio-only terminal mode. Blocks until quit or EOF.
pub fn run_terminal(
    cmd_tx: Sender<Command>,
    ui_update_rx: Receiver<UiUpdate>,
    filename: &str,
    duration_us: i64,
) {
    terminal::enable_raw_mode().ok();
    let mut stdout = io::stdout();

    let dur = format_time(duration_us);
    write!(stdout, "\r\x1b[K\u{25b6} 00:00:00 / {dur}  {filename}").ok();
    stdout.flush().ok();

    loop {
        if event::poll(Duration::from_millis(100)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                let cmd = match key.code {
                    KeyCode::Char('q') => Some(Command::Quit),
                    KeyCode::Char(' ') => Some(Command::PlayPause),
                    KeyCode::Left => Some(Command::SeekRelative {
                        seconds: -5.0,
                        exact: false,
                    }),
                    KeyCode::Right => Some(Command::SeekRelative {
                        seconds: 5.0,
                        exact: false,
                    }),
                    KeyCode::Up => Some(Command::VolumeUp),
                    KeyCode::Down => Some(Command::VolumeDown),
                    KeyCode::Char('a') => Some(Command::CycleAudioTrack),
                    KeyCode::Char('+') | KeyCode::Char('=') => {
                        Some(Command::AudioDelayIncrease)
                    }
                    KeyCode::Char('-') => Some(Command::AudioDelayDecrease),
                    _ => None,
                };
                if let Some(cmd) = cmd {
                    let quit = matches!(cmd, Command::Quit);
                    let _ = cmd_tx.send(cmd);
                    if quit {
                        break;
                    }
                }
            }
        }

        let mut should_quit = false;
        while let Ok(update) = ui_update_rx.try_recv() {
            match update {
                UiUpdate::Osd(text) => {
                    write!(stdout, "\r\x1b[K{text}").ok();
                    stdout.flush().ok();
                }
                UiUpdate::EndOfFile => {
                    should_quit = true;
                }
                _ => {}
            }
        }
        if should_quit {
            let _ = cmd_tx.send(Command::Quit);
            break;
        }
    }

    terminal::disable_raw_mode().ok();
    writeln!(stdout).ok();
}
