use std::io::{self, BufWriter, Write};
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
use crate::visualizer::{self, SpectrumAnalyzer, VizRing};

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
    viz_ring: Option<Arc<VizRing>>,
) -> EndReason {
    let raw = io::stdout().into_raw_mode().expect("failed to enter raw mode");
    let mut stdout = BufWriter::with_capacity(8192, raw);

    // Enter alternate screen, clear, hide cursor
    write!(stdout, "\x1b[?1049h\x1b[H\x1b[2J\x1b[?25l").ok();

    let duration_us = info.duration_us;
    let dur = format_time(duration_us);

    let sample_rate = info
        .audio_streams
        .first()
        .map(|a| a.sample_rate)
        .unwrap_or(48000);

    // Count header lines for layout
    let header_lines = count_header_lines(info);

    draw_header(&mut stdout, filename, info, file_index, file_count);

    // Timestamp line (will be continuously updated)
    write!(stdout, "00:00:00 -> {dur}").ok();
    stdout.flush().ok();

    let mut end_reason = EndReason::Quit;
    let mut osd_message: Option<(String, u64)> = None;
    let mut paused = false;

    // Visualizer state
    let mut viz_enabled = false;
    let mut analyzer = SpectrumAnalyzer::new(64);
    let mut fft_buf = vec![0.0f32; visualizer::FFT_SIZE];
    let mut render_buf = String::with_capacity(4096);
    let mut viz_generation: u64 = 0;
    // Track previous viz dimensions so we can clear on resize
    let mut prev_viz_rows: usize = 0;
    let mut prev_viz_top: u16 = 0;

    loop {
        // Drain all pending keys (avoid lag buildup)
        loop {
            let key = match keys.next() {
                Some(Ok(k)) => k,
                _ => break,
            };
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
                Key::Char('v') if viz_ring.is_some() => {
                    viz_enabled = !viz_enabled;
                    if !viz_enabled {
                        // Clear screen and redraw header
                        write!(stdout, "\x1b[H\x1b[2J").ok();
                        draw_header(&mut stdout, filename, info, file_index, file_count);
                        prev_viz_rows = 0;
                    }
                    Action::None
                }
                _ => Action::None,
            };
            match action {
                Action::End(reason) => {
                    end_reason = reason;
                    let _ = cmd_tx.send(Command::Quit);
                    // Show cursor, leave alternate screen
                    write!(stdout, "\x1b[?25h").ok();
                    if !matches!(end_reason, EndReason::NextFile | EndReason::PrevFile) {
                        write!(stdout, "\x1b[?1049l").ok();
                    }
                    stdout.flush().ok();
                    return end_reason;
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

        // Get terminal size
        let (cols, rows) = termion::terminal_size().unwrap_or((80, 24));

        // timestamp_row is right after the header
        let timestamp_row = header_lines + 1;

        // Update the timestamp line at a fixed row
        let current = audio_clock.load(Ordering::Relaxed);
        let pos = format_time(current);
        let icon = if paused { "\u{23f8}" } else { "\u{25b6}" };
        write!(
            stdout,
            "\x1b[{};1H\x1b[2K{icon} {pos} -> {dur}",
            timestamp_row
        )
        .ok();

        // Show OSD message after timestamp if active
        if let Some((ref text, _)) = osd_message {
            write!(stdout, "  {text}").ok();
        }

        // Render visualizer
        if viz_enabled {
            if let Some(ref vr) = viz_ring {
                let n = vr.read_recent(&mut fft_buf, &mut viz_generation);

                // Layout — compute every frame (terminal may resize)
                let bar_count = visualizer::bar_count_for_width(cols);
                let viz_top = timestamp_row + 2;
                let viz_rows = if rows > viz_top + 1 {
                    (rows - viz_top - 1) as usize
                } else {
                    2
                };
                let viz_rows = viz_rows.min(16);

                // If dimensions changed, clear old rows
                if viz_top != prev_viz_top || viz_rows != prev_viz_rows {
                    for r in prev_viz_top..(prev_viz_top + prev_viz_rows as u16) {
                        write!(stdout, "\x1b[{};1H\x1b[2K", r).ok();
                    }
                    prev_viz_top = viz_top;
                    prev_viz_rows = viz_rows;
                }

                // Center horizontally
                let viz_width = bar_count * 2 + bar_count.saturating_sub(1);
                let start_col = if (cols as usize) > viz_width {
                    ((cols as usize - viz_width) / 2 + 1) as u16
                } else {
                    1
                };

                if n > 0 {
                    // New audio data — recompute spectrum
                    let bars = analyzer.compute(&fft_buf[..n], bar_count, sample_rate);
                    visualizer::render_bars(
                        &mut render_buf,
                        bars,
                        viz_rows,
                        viz_top,
                        start_col,
                        cols,
                    );
                }
                // Always redraw (render_buf holds last computed frame)
                if !render_buf.is_empty() {
                    write!(stdout, "{render_buf}").ok();
                }
            }
        }

        stdout.flush().ok();

        thread::sleep(Duration::from_millis(50));
    }

    // Show cursor. Stay in alternate screen for next/prev.
    write!(stdout, "\x1b[?25h").ok();
    if !matches!(end_reason, EndReason::NextFile | EndReason::PrevFile) {
        write!(stdout, "\x1b[?1049l").ok();
    }
    stdout.flush().ok();

    drop(stdout);
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

fn draw_header(
    stdout: &mut impl Write,
    filename: &str,
    info: &StreamInfo,
    file_index: usize,
    file_count: usize,
) {
    if file_count > 1 {
        write!(stdout, "({}/{}) {filename}\r\n", file_index + 1, file_count).ok();
    } else {
        write!(stdout, "{filename}\r\n").ok();
    }
    print_metadata(stdout, info);
    if let Some(a) = info.audio_streams.first() {
        write!(stdout, "{}\r\n", format_audio_info(a)).ok();
    }
}

/// Count the number of header lines printed before the timestamp.
fn count_header_lines(info: &StreamInfo) -> u16 {
    let mut lines: u16 = 1; // filename line

    let keys = ["title", "artist", "album_artist", "album", "date", "genre"];
    for key in &keys {
        if info
            .metadata
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            lines += 1;
        }
    }

    if !info.audio_streams.is_empty() {
        lines += 1; // audio info line
    }

    lines
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

use crate::time::now_ms;
