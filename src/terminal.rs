use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use termion::AsyncReader;
use termion::event::Key;
use termion::input::Keys;
use termion::raw::IntoRawMode;

use crate::cmd::{Command, EndReason, UiUpdate};
use crate::demux::{AudioStreamInfo, StreamInfo};
use crate::time::format_time;

/// Run audio-only terminal mode. Blocks until quit or EOF.
/// `keys` is a shared async stdin reader, created once for the whole playlist.
#[allow(clippy::too_many_arguments)]
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
    let mut stdout = io::stdout()
        .into_raw_mode()
        .expect("failed to enter raw mode");

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
        // Drain ALL available keys so rapid key-repeat during seeking is
        // batched into a single accumulated seek command per iteration.
        let mut seek_accum: f64 = 0.0;
        let mut end_from_keys = false;
        while let Some(Ok(key)) = keys.next() {
            let action = match key {
                Key::Char('q') => Action::End(EndReason::Quit),
                Key::Char('>') | Key::Char('.') | Key::Char('\n')
                    if file_index + 1 < file_count =>
                {
                    Action::End(EndReason::NextFile)
                }
                Key::Char('<') | Key::Char(',') if file_index > 0 => {
                    Action::End(EndReason::PrevFile)
                }
                Key::Char(' ') => Action::Send(Command::PlayPause),
                Key::Left => {
                    seek_accum -= 3.0;
                    Action::None
                }
                Key::Right => {
                    seek_accum += 3.0;
                    Action::None
                }
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
                    end_from_keys = true;
                    break;
                }
                Action::Send(cmd) => {
                    let _ = cmd_tx.send(cmd);
                }
                Action::None => {}
            }
        }
        if end_from_keys {
            break;
        }
        if seek_accum != 0.0 {
            let _ = cmd_tx.send(Command::SeekRelative {
                seconds: seek_accum,
                exact: false,
            });
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
        if let Some((_, deadline)) = &osd_message
            && now_ms() >= *deadline
        {
            osd_message = None;
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

        thread::sleep(Duration::from_millis(10));
    }

    // Stay in alternate screen when advancing within a playlist.
    let advancing = matches!(end_reason, EndReason::NextFile | EndReason::PrevFile)
        || (end_reason == EndReason::Eof && file_index + 1 < file_count);
    if !advancing {
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
    format!(
        "{} {}Hz {}",
        a.codec_name, a.sample_rate, a.channel_layout_desc
    )
}

/// Print metadata lines.
fn print_metadata(stdout: &mut impl Write, info: &StreamInfo) {
    let keys = ["title", "artist", "album_artist", "album", "date", "genre"];
    for key in &keys {
        if let Some(val) = info
            .metadata
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
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

use crate::time::now_ms;
