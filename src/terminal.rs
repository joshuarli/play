use std::io::{self, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use termion::event::Key;
use termion::input::Keys;
use termion::raw::IntoRawMode;
use termion::AsyncReader;

use crate::cmd::{Command, EndReason, UiUpdate};
use crate::demux::{AudioStreamInfo, StreamInfo};
use crate::time::format_time;

/// Run audio-only terminal mode. Blocks until quit or EOF.
/// `keys` is a shared async stdin reader, created once for the whole playlist.
pub fn run_terminal(
    cmd_tx: Sender<Command>,
    ui_update_rx: Receiver<UiUpdate>,
    audio_clock: Arc<AtomicI64>,
    keys: &mut Keys<AsyncReader>,
    filename: &str,
    info: &StreamInfo,
    file_index: usize,
    file_count: usize,
) -> EndReason {
    let mut stdout = io::stdout().into_raw_mode().expect("failed to enter raw mode");

    // Enter alternate screen and clear it
    write!(stdout, "\x1b[?1049h\x1b[H\x1b[2J").ok();

    let duration_us = info.duration_us;
    let dur = format_time(duration_us);

    // Print header
    if file_count > 1 {
        write!(stdout, "({}/{}) {filename}\r\n", file_index + 1, file_count).ok();
    } else {
        write!(stdout, "{filename}\r\n").ok();
    }
    print_metadata(&mut stdout, info);

    // Audio stream info
    if let Some(a) = info.audio_streams.first() {
        write!(stdout, "{}\r\n", format_audio_info(a)).ok();
    }

    // Timestamp line (will be continuously updated)
    write!(stdout, "00:00:00 -> {dur}").ok();
    stdout.flush().ok();

    let mut end_reason = EndReason::Quit;
    let mut osd_message: Option<(String, u64)> = None;
    let mut paused = false;

    loop {
        if let Some(Ok(key)) = keys.next() {
            // Quit/Next/Prev are handled directly — no player round-trip needed.
            // We just tell the player to stop and return the reason to the playlist loop.
            let action = match key {
                Key::Char('q') => Action::End(EndReason::Quit),
                Key::Char('>') | Key::Char('.') | Key::Char('\n') => {
                    Action::End(EndReason::NextFile)
                }
                Key::Char('<') | Key::Char(',') => Action::End(EndReason::PrevFile),
                Key::Char(' ') => Action::Send(Command::PlayPause),
                Key::Left => Action::Send(Command::SeekRelative {
                    seconds: -5.0,
                    exact: false,
                }),
                Key::Right => Action::Send(Command::SeekRelative {
                    seconds: 5.0,
                    exact: false,
                }),
                Key::Up => Action::Send(Command::VolumeUp),
                Key::Down => Action::Send(Command::VolumeDown),
                Key::Char('a') => Action::Send(Command::CycleAudioTrack),
                Key::Char('+') | Key::Char('=') => Action::Send(Command::AudioDelayIncrease),
                Key::Char('-') => Action::Send(Command::AudioDelayDecrease),
                _ => Action::None,
            };
            match action {
                Action::End(reason) => {
                    end_reason = reason;
                    let _ = cmd_tx.send(Command::Quit);
                    break;
                }
                Action::Send(cmd) => {
                    let _ = cmd_tx.send(cmd);
                }
                Action::None => {}
            }
        }

        let mut should_break = false;
        while let Ok(update) = ui_update_rx.try_recv() {
            match update {
                UiUpdate::Osd(text) => {
                    osd_message = Some((text, now_ms() + 2000));
                }
                UiUpdate::Paused(is_paused) => {
                    paused = is_paused;
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

        // Clear expired OSD message
        if let Some((_, deadline)) = &osd_message {
            if now_ms() >= *deadline {
                osd_message = None;
            }
        }

        // Update the timestamp line
        let current = audio_clock.load(Ordering::Relaxed);
        let pos = format_time(current);
        let icon = if paused { "\u{23f8}" } else { "\u{25b6}" };
        write!(stdout, "\r\x1b[K{icon} {pos} -> {dur}").ok();

        // Show OSD message after timestamp if active
        if let Some((ref text, _)) = osd_message {
            write!(stdout, "  {text}").ok();
        }
        stdout.flush().ok();

        thread::sleep(Duration::from_millis(50));
    }

    // Stay in alternate screen for next/prev (next call clears and redraws).
    // Exit on quit or EOF — for EOF mid-playlist, main.rs will re-enter.
    if !matches!(end_reason, EndReason::NextFile | EndReason::PrevFile) {
        write!(stdout, "\x1b[?1049l").ok();
        stdout.flush().ok();
    }

    drop(stdout); // restores raw mode via Drop
    end_reason
}

enum Action {
    End(EndReason),
    Send(Command),
    None,
}

fn format_audio_info(a: &AudioStreamInfo) -> String {
    format!("{} {}Hz {}", a.codec_name, a.sample_rate, a.channel_layout_desc)
}

/// Print metadata lines.
fn print_metadata(stdout: &mut impl Write, info: &StreamInfo) {
    let keys = ["title", "artist", "album_artist", "album", "date", "genre"];
    for key in &keys {
        if let Some(val) = info.metadata.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            write!(stdout, "{}: {}\r\n", capitalize(key), val.1).ok();
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().replace('_', " "),
    }
}

fn now_ms() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
