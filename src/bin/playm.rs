use std::collections::VecDeque;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

use play::audio_out::AudioOutput;
use play::cmd::{self, Command, DemuxCommand, DemuxPacket, EndReason, UiUpdate};
use play::decode_audio::{AudioBuffer, AudioDecoder};
use play::demux::{self, StreamInfo};
use play::sync::SyncClock;
use play::time::{format_time, now_ms, parse_time};

// ── Args ──────────────────────────────────────────────────────────

struct Args {
    files: Vec<PathBuf>,
    volume: u32,
    audio_track: usize,
    start: Option<String>,
    verbose: u8,
}

const USAGE: &str = "\
Usage: playm [OPTIONS] <FILE|DIR>...

Arguments:
  <FILE|DIR>...  One or more media files or directories

Options:
      --volume <N>          Initial volume percentage 0-100 [default: 100]
      --audio-track <N>     Audio track index, 1-based [default: 1]
      --start <TIME>        Start position (HH:MM:SS, MM:SS, or seconds)
  -v                        Verbose logging (-v info, -vv debug)
  -h, --help                Print help";

fn parse_args() -> Result<Args> {
    let mut files = Vec::new();
    let mut volume: u32 = 100;
    let mut audio_track: usize = 1;
    let mut start: Option<String> = None;
    let mut verbose: u8 = 0;

    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--volume" => {
                let val = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--volume requires a value"))?;
                volume = val.parse().map_err(|_| {
                    anyhow::anyhow!("invalid value '{val}' for --volume: expected integer")
                })?;
            }
            "--audio-track" => {
                let val = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--audio-track requires a value"))?;
                audio_track = val.parse().map_err(|_| {
                    anyhow::anyhow!("invalid value '{val}' for --audio-track: expected integer")
                })?;
            }
            "--start" => {
                let val = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--start requires a value"))?;
                start = Some(val);
            }
            s if s.starts_with("-v") && s.chars().skip(1).all(|c| c == 'v') => {
                verbose = (s.len() - 1).min(255) as u8;
            }
            s if s.starts_with('-') => {
                bail!("unknown option '{s}'\n\n{USAGE}");
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }

    if files.is_empty() {
        bail!("required arguments not provided: <FILE>...\n\n{USAGE}");
    }

    Ok(Args {
        files,
        volume,
        audio_track,
        start,
        verbose,
    })
}

// ── Main ──────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args = parse_args()?;

    let log_level = match args.verbose {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Info,
        _ => log::LevelFilter::Debug,
    };
    env_logger::Builder::new()
        .filter_level(log_level)
        .format_timestamp_millis()
        .init();

    ffmpeg_next::init().context("Failed to initialize ffmpeg")?;

    let files = cmd::expand_files(&args.files);
    if files.is_empty() {
        bail!("No media files found in the given paths");
    }

    for file in &files {
        if !file.exists() {
            bail!("File not found: {}", file.display());
        }
    }

    let mut term_keys = termion::async_stdin().keys();

    let mut index = 0;
    let mut first_run = true;
    while index < files.len() {
        let file = &files[index];
        match play_file(file, &args, first_run, index, files.len(), &mut term_keys) {
            Ok(reason) => match reason {
                EndReason::Quit => break,
                EndReason::Eof | EndReason::NextFile => index += 1,
                EndReason::PrevFile => index = index.saturating_sub(1),
            },
            Err(e) => {
                log::error!("Error playing {}: {e}", file.display());
                index += 1;
            }
        }
        first_run = false;
    }

    Ok(())
}

// ── Play File ─────────────────────────────────────────────────────

fn play_file(
    path: &Path,
    args: &Args,
    first_run: bool,
    file_index: usize,
    file_count: usize,
    term_keys: &mut termion::input::Keys<termion::AsyncReader>,
) -> Result<EndReason> {
    let info = demux::probe(path)?;

    if info.audio_streams.is_empty() {
        bail!("No audio streams found in {}", path.display());
    }

    let audio_idx = info
        .audio_streams
        .get(args.audio_track.saturating_sub(1))
        .or(info.audio_streams.first())
        .map(|s| s.index);

    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<Command>(32);
    let (demux_packet_tx, demux_packet_rx) = crossbeam_channel::bounded::<DemuxPacket>(64);
    let (demux_cmd_tx, demux_cmd_rx) = crossbeam_channel::bounded::<DemuxCommand>(4);
    let (ui_update_tx, ui_update_rx) = crossbeam_channel::unbounded::<UiUpdate>();

    let audio_clock = Arc::new(AtomicI64::new(0));

    let start_pos = if first_run {
        args.start
            .as_ref()
            .and_then(|s| parse_time(s).ok())
            .unwrap_or(0)
    } else {
        0
    };

    // Spawn demuxer thread
    let demux_path = path.to_path_buf();
    let demux_thread = thread::Builder::new()
        .name("demuxer".into())
        .spawn(move || {
            if let Err(e) = demux::run_demuxer(
                &demux_path,
                None,
                audio_idx,
                None,
                demux_cmd_rx,
                demux_packet_tx,
            ) {
                log::error!("Demuxer error: {e}");
            }
        })
        .context("Failed to spawn demuxer thread")?;

    // Spawn audio player thread
    let player_path = path.to_path_buf();
    let player_info = info.clone();
    let player_clock = audio_clock.clone();
    let initial_volume = args.volume;
    let player_thread = thread::Builder::new()
        .name("audio-player".into())
        .spawn(move || {
            let mut player = AudioPlayer::new(
                cmd_rx,
                demux_packet_rx,
                demux_cmd_tx,
                ui_update_tx,
                player_clock,
                player_path,
                player_info,
                initial_volume,
            );
            if let Err(e) = player.init_decoders() {
                log::error!("Failed to init decoders: {e}");
                return;
            }
            if start_pos > 0 {
                player.seek_to(start_pos);
            }
            player.run();
        })
        .context("Failed to spawn player thread")?;

    // Terminal UI on main thread
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let reason = run_terminal(
        cmd_tx,
        ui_update_rx,
        audio_clock,
        term_keys,
        filename,
        &info,
        file_index,
        file_count,
    );

    player_thread.join().ok();
    demux_thread.join().ok();

    Ok(reason)
}

// ── Audio Player ──────────────────────────────────────────────────

struct AudioPlayer {
    cmd_rx: Receiver<Command>,
    demux_packet_rx: Receiver<DemuxPacket>,
    demux_cmd_tx: Sender<DemuxCommand>,
    ui_update_tx: Sender<UiUpdate>,

    audio_decoder: Option<AudioDecoder>,
    audio_output: Option<AudioOutput>,
    audio_clock: Arc<AtomicI64>,
    clock: SyncClock,

    paused: bool,
    volume: u32,
    duration_us: i64,

    pending_seeks: u32,
    queued_seek: Option<f64>,
    last_audio_end_us: i64,
    eof_audio_end_us: Option<i64>,
    pending_audio: VecDeque<AudioBuffer>,

    stream_info: StreamInfo,
    current_audio_track: usize,
    file_path: PathBuf,
}

impl AudioPlayer {
    #[allow(clippy::too_many_arguments)]
    fn new(
        cmd_rx: Receiver<Command>,
        demux_packet_rx: Receiver<DemuxPacket>,
        demux_cmd_tx: Sender<DemuxCommand>,
        ui_update_tx: Sender<UiUpdate>,
        audio_clock: Arc<AtomicI64>,
        file_path: PathBuf,
        stream_info: StreamInfo,
        initial_volume: u32,
    ) -> Self {
        let clock = SyncClock::new(audio_clock.clone());
        Self {
            cmd_rx,
            demux_packet_rx,
            demux_cmd_tx,
            ui_update_tx,
            audio_decoder: None,
            audio_output: None,
            audio_clock,
            clock,
            paused: false,
            volume: initial_volume,
            duration_us: stream_info.duration_us,
            pending_seeks: 0,
            queued_seek: None,
            last_audio_end_us: 0,
            eof_audio_end_us: None,
            pending_audio: VecDeque::new(),
            stream_info,
            current_audio_track: 0,
            file_path,
        }
    }

    fn init_decoders(&mut self) -> Result<()> {
        let ictx = ffmpeg_next::format::input(&self.file_path)
            .with_context(|| format!("Failed to open: {}", self.file_path.display()))?;
        let audio = self
            .stream_info
            .audio_streams
            .first()
            .context("No audio stream")?;
        let stream = ictx.stream(audio.index).context("Audio stream not found")?;
        let decoder = AudioDecoder::new(&stream)?;
        let sample_rate = decoder.sample_rate;
        let channels = decoder.channels;
        self.audio_decoder = Some(decoder);

        let mut ao = AudioOutput::new(sample_rate, channels, self.audio_clock.clone())?;
        if self.volume < 100 {
            ao.set_volume(self.volume as f32 / 100.0);
        }
        self.audio_output = Some(ao);
        Ok(())
    }

    fn seek_to(&mut self, target_us: i64) {
        self.pending_seeks += 1;
        let _ = self.demux_cmd_tx.send(DemuxCommand::Seek {
            target_pts: target_us,
            forward: false,
        });
        if let Some(ad) = self.audio_decoder.as_mut() {
            ad.flush();
        }
        if let Some(ao) = self.audio_output.as_ref() {
            ao.flush();
        }
        self.clock.set_position(target_us);
    }

    fn run(&mut self) {
        loop {
            // 1. Drain all pending commands first.
            while let Ok(cmd) = self.cmd_rx.try_recv() {
                if self.handle_command(cmd) {
                    return;
                }
            }

            self.execute_queued_seek();

            // 2. Drain pending audio to ring (non-blocking).
            self.drain_pending_audio();

            // 3. Process packets if room in pending queue.
            if self.pending_audio.len() < 16 {
                for _ in 0..8 {
                    match self.demux_packet_rx.try_recv() {
                        Ok(pkt) => self.handle_packet(pkt),
                        Err(_) => break,
                    }
                }
            }

            // 4. Check EOF.
            if let Some(end_us) = self.eof_audio_end_us {
                if self.clock.audio_pts() >= end_us {
                    self.eof_audio_end_us = None;
                    let _ = self.ui_update_tx.send(UiUpdate::EndOfFile(EndReason::Eof));
                }
            }

            // 5. Wait for new events when idle.
            if self.pending_audio.is_empty() && self.queued_seek.is_none() {
                crossbeam_channel::select! {
                    recv(self.cmd_rx) -> msg => {
                        match msg {
                            Ok(cmd) => {
                                if self.handle_command(cmd) {
                                    return;
                                }
                            }
                            Err(_) => return,
                        }
                    }
                    recv(self.demux_packet_rx) -> msg => {
                        match msg {
                            Ok(pkt) => self.handle_packet(pkt),
                            Err(_) => return,
                        }
                    }
                    default(Duration::from_millis(4)) => {}
                }
            } else if !self.pending_audio.is_empty() {
                std::thread::sleep(Duration::from_micros(100));
            }
        }
    }

    fn handle_command(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Quit => {
                let _ = self.demux_cmd_tx.send(DemuxCommand::Stop);
                if let Some(ao) = self.audio_output.as_ref() {
                    ao.stop();
                }
                return true;
            }
            Command::PlayPause => {
                self.paused = !self.paused;
                self.clock.set_paused(self.paused);
                if let Some(ao) = self.audio_output.as_ref() {
                    if self.paused {
                        ao.pause();
                    } else {
                        ao.play();
                    }
                }
                let _ = self.ui_update_tx.send(UiUpdate::Paused(self.paused));
            }
            Command::SeekRelative { seconds, .. } => {
                self.queue_seek(seconds);
                // Drain additional seeks that already arrived
                while let Ok(cmd) = self.cmd_rx.try_recv() {
                    match cmd {
                        Command::SeekRelative { seconds: s, .. } => self.queue_seek(s),
                        other => return self.handle_command(other),
                    }
                }
            }
            Command::VolumeUp => self.adjust_volume(5),
            Command::VolumeDown => self.adjust_volume(-5),
            Command::CycleAudioTrack => self.cycle_audio_track(),
            Command::NextFile => {
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::EndOfFile(EndReason::NextFile));
            }
            Command::PrevFile => {
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::EndOfFile(EndReason::PrevFile));
            }
            _ => {}
        }
        false
    }

    fn handle_packet(&mut self, pkt: DemuxPacket) {
        match pkt {
            DemuxPacket::Flush => {
                self.pending_seeks = self.pending_seeks.saturating_sub(1);
                if let Some(ad) = self.audio_decoder.as_mut() {
                    ad.flush();
                }
                if let Some(ao) = self.audio_output.as_ref() {
                    ao.flush_quick();
                }
                self.pending_audio.clear();
            }
            _ if self.pending_seeks > 0 => {}
            DemuxPacket::Audio(packet) => {
                if let Some(decoder) = self.audio_decoder.as_mut() {
                    if decoder.send_packet(&packet).is_err() {
                        return;
                    }
                    while let Some(buf) = decoder.receive_buffer() {
                        self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
                        if let Some(ao) = self.audio_output.as_ref() {
                            if !ao.try_schedule_buffer(&buf) {
                                self.pending_audio.push_back(buf);
                            }
                        }
                    }
                }
            }
            DemuxPacket::Eof => {
                if let Some(ad) = self.audio_decoder.as_mut() {
                    let _ = ad.send_eof();
                    while let Some(buf) = ad.receive_buffer() {
                        self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
                        if let Some(ao) = self.audio_output.as_ref() {
                            if !ao.try_schedule_buffer(&buf) {
                                self.pending_audio.push_back(buf);
                            }
                        }
                    }
                    if let Some(buf) = ad.drain_accum() {
                        self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
                        if let Some(ao) = self.audio_output.as_ref() {
                            if !ao.try_schedule_buffer(&buf) {
                                self.pending_audio.push_back(buf);
                            }
                        }
                    }
                }
                if self.audio_output.is_some() && self.last_audio_end_us > 0 {
                    self.eof_audio_end_us = Some(self.last_audio_end_us);
                } else {
                    let _ = self.ui_update_tx.send(UiUpdate::EndOfFile(EndReason::Eof));
                }
            }
            _ => {}
        }
    }

    fn queue_seek(&mut self, seconds: f64) {
        match &mut self.queued_seek {
            Some(qs) => *qs += seconds,
            None => self.queued_seek = Some(seconds),
        }
    }

    fn execute_queued_seek(&mut self) {
        let Some(seconds) = self.queued_seek.take() else {
            return;
        };
        if seconds.abs() < 0.001 {
            return;
        }

        let current = self.clock.audio_pts();
        let delta_us = (seconds * 1_000_000.0) as i64;
        let target = (current + delta_us).max(0).min(self.duration_us);
        let forward = seconds > 0.0;

        // Try ring buffer skip for forward seeks (instant, no I/O)
        if forward {
            if let Some(ao) = self.audio_output.as_ref() {
                let sample_rate = self
                    .stream_info
                    .audio_streams
                    .get(self.current_audio_track)
                    .map(|a| a.sample_rate)
                    .unwrap_or(48000);
                let samples_needed = (delta_us as u64 * sample_rate as u64 / 1_000_000) as usize;
                let available = ao.buffered_samples();
                if samples_needed > 0 && available > 0 {
                    let actual_skip = samples_needed.min(available);
                    ao.request_skip(actual_skip);
                    let actual_us = actual_skip as i64 * 1_000_000 / sample_rate as i64;
                    let actual_target = (current + actual_us).min(self.duration_us);
                    self.clock.set_position(actual_target);
                    return;
                }
            }
        }

        self.dispatch_seek(target, forward);
    }

    fn dispatch_seek(&mut self, target: i64, forward: bool) {
        let _ = self.demux_cmd_tx.send(DemuxCommand::Seek {
            target_pts: target,
            forward,
        });
        self.pending_seeks += 1;
        self.last_audio_end_us = 0;
        self.eof_audio_end_us = None;
        if let Some(ad) = self.audio_decoder.as_mut() {
            ad.flush();
        }
        if let Some(ao) = self.audio_output.as_ref() {
            ao.flush_quick();
            ao.set_clock_position(target);
        }
        self.pending_audio.clear();
        self.clock.set_position(target);
    }

    fn drain_pending_audio(&mut self) {
        let Some(ao) = self.audio_output.as_ref() else {
            self.pending_audio.clear();
            return;
        };
        while let Some(buf) = self.pending_audio.front() {
            if ao.try_schedule_buffer(buf) {
                self.pending_audio.pop_front();
            } else {
                break;
            }
        }
    }

    fn adjust_volume(&mut self, delta: i32) {
        self.volume = (self.volume as i32 + delta).clamp(0, 100) as u32;
        if let Some(ao) = self.audio_output.as_mut() {
            ao.set_volume(self.volume as f32 / 100.0);
        }
        let _ = self
            .ui_update_tx
            .send(UiUpdate::Osd(format!("Volume: {}%", self.volume)));
    }

    fn cycle_audio_track(&mut self) {
        if self.stream_info.audio_streams.len() <= 1 {
            return;
        }

        self.current_audio_track =
            (self.current_audio_track + 1) % self.stream_info.audio_streams.len();
        let new_info = self.stream_info.audio_streams[self.current_audio_track].clone();

        if let Some(ad) = self.audio_decoder.as_mut() {
            ad.flush();
        }
        if let Some(ao) = self.audio_output.as_ref() {
            ao.flush();
        }
        let _ = self
            .demux_cmd_tx
            .send(DemuxCommand::ChangeAudio(new_info.index));

        // Re-open input for new stream parameters
        if let Ok(ictx) = ffmpeg_next::format::input(&self.file_path) {
            if let Some(stream) = ictx.stream(new_info.index) {
                if let Ok(decoder) = AudioDecoder::new(&stream) {
                    let new_rate = decoder.sample_rate;
                    let new_channels = decoder.channels;
                    self.audio_decoder = Some(decoder);
                    self.audio_output = None;
                    match AudioOutput::new(new_rate, new_channels, self.audio_clock.clone()) {
                        Ok(mut ao) => {
                            if self.volume < 100 {
                                ao.set_volume(self.volume as f32 / 100.0);
                            }
                            self.audio_output = Some(ao);
                        }
                        Err(e) => log::error!("Failed to create audio output: {e}"),
                    }
                }
            }
        }

        let _ = self.ui_update_tx.send(UiUpdate::Osd(format!(
            "Audio: {}/{} - {} {}Hz {}",
            self.current_audio_track + 1,
            self.stream_info.audio_streams.len(),
            new_info.codec_name,
            new_info.sample_rate,
            new_info.channel_layout_desc,
        )));
    }
}

// ── Terminal UI ───────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn run_terminal(
    cmd_tx: Sender<Command>,
    ui_update_rx: Receiver<UiUpdate>,
    audio_clock: Arc<AtomicI64>,
    keys: &mut termion::input::Keys<termion::AsyncReader>,
    filename: &str,
    info: &StreamInfo,
    file_index: usize,
    file_count: usize,
) -> EndReason {
    let mut stdout = io::stdout()
        .into_raw_mode()
        .expect("failed to enter raw mode");

    // Enter alternate screen and clear
    write!(stdout, "\x1b[?1049h\x1b[H\x1b[2J").ok();

    let duration_us = info.duration_us;
    let dur = format_time(duration_us);

    // Header
    if file_count > 1 {
        write!(stdout, "({}/{}) {filename}\r\n", file_index + 1, file_count).ok();
    } else {
        write!(stdout, "{filename}\r\n").ok();
    }
    print_metadata(&mut stdout, info);
    if let Some(a) = info.audio_streams.first() {
        write!(
            stdout,
            "{} {}Hz {}\r\n",
            a.codec_name, a.sample_rate, a.channel_layout_desc
        )
        .ok();
    }
    write!(stdout, "00:00:00 -> {dur}").ok();
    stdout.flush().ok();

    let mut end_reason = EndReason::Quit;
    let mut osd_message: Option<(String, u64)> = None;
    let mut paused = false;

    loop {
        // Batch key events
        let mut seek_accum: f64 = 0.0;
        let mut end_from_keys = false;
        while let Some(Ok(key)) = keys.next() {
            match key {
                Key::Char('q') => {
                    end_reason = EndReason::Quit;
                    end_from_keys = true;
                    break;
                }
                Key::Char('>') | Key::Char('.') | Key::Char('\n')
                    if file_index + 1 < file_count =>
                {
                    end_reason = EndReason::NextFile;
                    end_from_keys = true;
                    break;
                }
                Key::Char('<') | Key::Char(',') if file_index > 0 => {
                    end_reason = EndReason::PrevFile;
                    end_from_keys = true;
                    break;
                }
                Key::Char(' ') => {
                    let _ = cmd_tx.send(Command::PlayPause);
                }
                Key::Left => seek_accum -= 1.0,
                Key::Right => seek_accum += 1.0,
                Key::Up => {
                    let _ = cmd_tx.send(Command::VolumeUp);
                }
                Key::Down => {
                    let _ = cmd_tx.send(Command::VolumeDown);
                }
                Key::Char('a') => {
                    let _ = cmd_tx.send(Command::CycleAudioTrack);
                }
                _ => {}
            }
        }
        if end_from_keys {
            let _ = cmd_tx.send(Command::Quit);
            break;
        }
        if seek_accum != 0.0 {
            let _ = cmd_tx.send(Command::SeekRelative {
                seconds: seek_accum,
                exact: false,
            });
        }

        // Drain UI updates
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

        // Clear expired OSD
        if let Some((_, deadline)) = &osd_message {
            if now_ms() >= *deadline {
                osd_message = None;
            }
        }

        // Update display
        let current = audio_clock.load(Ordering::Relaxed);
        let pos = format_time(current);
        let icon = if paused { "\u{23f8}" } else { "\u{25b6}" };
        write!(stdout, "\r\x1b[K{icon} {pos} -> {dur}").ok();
        if let Some((ref text, _)) = osd_message {
            write!(stdout, "  {text}").ok();
        }
        stdout.flush().ok();

        thread::sleep(Duration::from_millis(10));
    }

    // Stay in alt screen for next/prev; exit on quit/EOF
    if !matches!(end_reason, EndReason::NextFile | EndReason::PrevFile) {
        write!(stdout, "\x1b[?1049l").ok();
        stdout.flush().ok();
    }

    drop(stdout);
    end_reason
}

fn print_metadata(stdout: &mut impl io::Write, info: &StreamInfo) {
    let keys = ["title", "artist", "album_artist", "album", "date", "genre"];
    for key in &keys {
        if let Some(val) = info
            .metadata
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            let label = {
                let mut c = key.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().replace('_', " "),
                }
            };
            write!(stdout, "{label}: {}\r\n", val.1).ok();
        }
    }
}
