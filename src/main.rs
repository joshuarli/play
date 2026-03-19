mod audio_out;
mod cmd;
mod decode_audio;
mod decode_video;
mod demux;
mod input;
mod osd;
mod player;
mod subtitle;
mod sync;
mod terminal;
mod time;
mod video_out;
mod window;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::thread;

use anyhow::{Context, Result, bail};
use termion::input::TermRead;

use cmd::{Args, Command, DemuxCommand, DemuxPacket, EndReason, UiUpdate, VideoFrame};
use player::Player;

fn main() -> Result<()> {
    let args = cmd::parse_args()?;

    let log_level = match args.verbose {
        0 if args.files.is_empty() => log::LevelFilter::Info,
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Info,
        _ => log::LevelFilter::Debug,
    };
    env_logger::Builder::new()
        .filter_level(log_level)
        .format_timestamp_millis()
        .init();

    // Initialize ffmpeg
    ffmpeg_next::init().context("Failed to initialize ffmpeg")?;
    // SAFETY: av_log_set_level is thread-safe and accepts any valid log level.
    // Called once before any ffmpeg operations.
    unsafe {
        let ff_log = match args.verbose {
            0 => ffmpeg_sys_next::AV_LOG_ERROR,
            1 => ffmpeg_sys_next::AV_LOG_WARNING,
            _ => ffmpeg_sys_next::AV_LOG_VERBOSE,
        };
        ffmpeg_sys_next::av_log_set_level(ff_log);
    }

    // Finder mode: no CLI files, wait for Finder to deliver file-open events
    if args.files.is_empty() {
        log::info!("No CLI files, entering Finder mode...");
        let finder_args = args.clone();
        window::run_finder_mode(move |files| {
            if files.is_empty() {
                return;
            }
            if let Err(e) = start_finder_playback(&files[0], &finder_args) {
                log::error!("Finder playback error: {e}");
            }
        });
        return Ok(());
    }

    let files = {
        let expanded = cmd::expand_files(&args.files);
        if expanded.is_empty() {
            bail!("No media files found in the given paths");
        }
        expanded
    };

    // Validate files exist
    for file in &files {
        if !file.exists() {
            bail!("File not found: {}", file.display());
        }
    }

    // Single async stdin reader for the entire playlist (avoids spawning
    // a new background thread per file, which eats keystrokes during transitions).
    let mut term_keys = termion::async_stdin().keys();

    // Playlist loop
    let mut index = 0;
    let mut first_run = true;
    while index < files.len() {
        let file = &files[index];
        match play_file(file, &args, first_run, index, files.len(), &mut term_keys) {
            Ok(reason) => {
                match reason {
                    EndReason::Quit => break,
                    EndReason::Eof | EndReason::NextFile => {
                        index += 1;
                        // Eof past last file: done
                    }
                    EndReason::PrevFile => {
                        index = index.saturating_sub(1);
                    }
                }
            }
            Err(e) => {
                log::error!("Error playing {}: {e}", file.display());
                index += 1; // skip to next
            }
        }
        first_run = false;
    }

    Ok(())
}

// ── Shared playback setup ──────────────────────────────────────────

/// State returned by `start_playback` for the UI layer.
struct PlaybackHandle {
    info: demux::StreamInfo,
    cmd_tx: crossbeam_channel::Sender<Command>,
    video_frame_rx: crossbeam_channel::Receiver<VideoFrame>,
    ui_update_rx: crossbeam_channel::Receiver<UiUpdate>,
    ui_update_tx: crossbeam_channel::Sender<UiUpdate>,
    audio_clock: Arc<AtomicI64>,
    video_width: u32,
    video_height: u32,
    filename: String,
    demux_thread: thread::JoinHandle<()>,
    player_thread: thread::JoinHandle<()>,
}

/// Probe a file, load subtitles, create channels, and spawn demuxer + player
/// threads. Shared between CLI playlist mode and Finder file-open mode.
fn start_playback(path: &Path, args: &Args, start_pos: i64) -> Result<PlaybackHandle> {
    let info = demux::probe(path)?;

    if info.video_stream.is_none() && info.audio_streams.is_empty() {
        bail!("No playable streams found in {}", path.display());
    }

    // Stream indices
    let video_idx = info.video_stream.as_ref().map(|s| s.index);
    let audio_idx = info
        .audio_streams
        .get(args.audio_track.saturating_sub(1))
        .or(info.audio_streams.first())
        .map(|s| s.index);
    // Subtitles are pre-decoded at startup, not streamed through the demuxer.
    let subtitle_idx = None;

    // Load external subtitles
    let mut subtitle_tracks = Vec::new();
    for srt_path in subtitle::find_srt_files(path) {
        match subtitle::parse_srt(&srt_path) {
            Ok(entries) => {
                let label = srt_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("external")
                    .to_string();
                subtitle_tracks.push(subtitle::SubtitleTrack { label, entries });
            }
            Err(e) => log::warn!("Failed to parse {}: {e}", srt_path.display()),
        }
    }
    if let Some(sub_path) = &args.sub_file {
        match subtitle::parse_srt(sub_path) {
            Ok(entries) => {
                subtitle_tracks.insert(
                    0,
                    subtitle::SubtitleTrack {
                        label: sub_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("external")
                            .to_string(),
                        entries,
                    },
                );
            }
            Err(e) => log::warn!("Failed to parse subtitle file: {e}"),
        }
    }

    // Decode embedded text subtitles
    for sub_info in &info.subtitle_streams {
        match subtitle::decode_embedded_subtitles(path, sub_info.index, &sub_info.codec_name) {
            Ok(entries) if !entries.is_empty() => {
                let label = sub_info
                    .language
                    .as_deref()
                    .unwrap_or(&sub_info.codec_name)
                    .to_string();
                subtitle_tracks.push(subtitle::SubtitleTrack { label, entries });
            }
            Ok(_) => {}
            Err(e) => log::warn!(
                "Failed to decode embedded subtitle stream {}: {e}",
                sub_info.index
            ),
        }
    }

    // Channels
    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<Command>(32);
    let (demux_packet_tx, demux_packet_rx) = crossbeam_channel::bounded::<DemuxPacket>(64);
    let (demux_cmd_tx, demux_cmd_rx) = crossbeam_channel::bounded::<DemuxCommand>(4);
    let (video_frame_tx, video_frame_rx) = crossbeam_channel::bounded::<VideoFrame>(8);
    let (ui_update_tx, ui_update_rx) = crossbeam_channel::unbounded::<UiUpdate>();
    let ui_update_tx_clone = ui_update_tx.clone();

    let audio_clock = Arc::new(AtomicI64::new(0));
    let video_width = info.video_stream.as_ref().map(|s| s.width).unwrap_or(640);
    let video_height = info.video_stream.as_ref().map(|s| s.height).unwrap_or(480);
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Spawn demuxer thread
    let demux_path = path.to_path_buf();
    let demux_thread = thread::Builder::new()
        .name("demuxer".into())
        .spawn(move || {
            if let Err(e) = demux::run_demuxer(
                &demux_path,
                video_idx,
                audio_idx,
                subtitle_idx,
                demux_cmd_rx,
                demux_packet_tx,
            ) {
                log::error!("Demuxer error: {e}");
            }
        })
        .context("Failed to spawn demuxer thread")?;

    // Spawn player thread
    let player_path = path.to_path_buf();
    let player_info = info.clone();
    let player_clock = audio_clock.clone();
    let initial_volume = args.volume;
    let initial_audio_delay = args.audio_delay;
    let player_thread = thread::Builder::new()
        .name("player".into())
        .spawn(move || {
            let mut player = match Player::new(player::PlayerConfig {
                cmd_rx,
                demux_packet_rx,
                demux_cmd_tx,
                video_frame_tx,
                ui_update_tx,
                file_path: player_path,
                stream_info: player_info,
                initial_volume,
                initial_audio_delay,
                subtitle_tracks,
                audio_clock: player_clock,
            }) {
                Ok(p) => p,
                Err(e) => {
                    log::error!("Failed to create player: {e}");
                    return;
                }
            };

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

    Ok(PlaybackHandle {
        info,
        cmd_tx,
        video_frame_rx,
        ui_update_rx,
        ui_update_tx: ui_update_tx_clone,
        audio_clock,
        video_width,
        video_height,
        filename,
        demux_thread,
        player_thread,
    })
}

// ── Playback entry points ──────────────────────────────────────────

fn play_file(
    path: &Path,
    args: &Args,
    first_run: bool,
    file_index: usize,
    file_count: usize,
    term_keys: &mut termion::input::Keys<termion::AsyncReader>,
) -> Result<EndReason> {
    let start_pos = if first_run {
        args.start
            .as_ref()
            .and_then(|s| time::parse_time(s).ok())
            .unwrap_or(0)
    } else {
        0
    };

    let handle = start_playback(path, args, start_pos)?;
    let has_video = handle.info.video_stream.is_some();

    // Playlist OSD (video mode only; terminal prints its own header)
    if file_count > 1 && has_video {
        let _ = handle.ui_update_tx.send(UiUpdate::Osd(format!(
            "({}/{}) {}",
            file_index + 1,
            file_count,
            handle.filename
        )));
    }

    let title = format!("play - {}", handle.filename);
    if has_video {
        log_stream_info(&handle.info);
    }

    let reason = if has_video {
        window::run_app(
            handle.cmd_tx,
            handle.video_frame_rx,
            handle.ui_update_rx,
            handle.video_width,
            handle.video_height,
            args.fullscreen,
            handle.audio_clock,
            handle.info.duration_us,
            &title,
            first_run,
            file_index,
            file_count,
        )
    } else {
        terminal::run_terminal(
            handle.cmd_tx,
            handle.ui_update_rx,
            handle.audio_clock,
            term_keys,
            &handle.filename,
            &handle.info,
            file_index,
            file_count,
        )
    };

    handle.player_thread.join().ok();
    handle.demux_thread.join().ok();

    Ok(reason)
}

/// Set up playback for a file opened via Finder. Called on the main thread
/// during `app.run()` from the `openURLs:` callback.
fn start_finder_playback(path: &Path, args: &cmd::Args) -> Result<()> {
    if !path.exists() {
        bail!("File not found: {}", path.display());
    }

    let handle = start_playback(path, args, 0)?;

    if handle.info.video_stream.is_none() {
        bail!("Audio-only playback from Finder not yet supported");
    }

    let title = format!("play - {}", handle.filename);

    let mtm = objc2_foundation::MainThreadMarker::new().expect("must be on main thread");
    window::set_file_state(
        handle.cmd_tx,
        handle.video_frame_rx,
        handle.ui_update_rx,
        handle.audio_clock,
        handle.info.duration_us,
        0,
        1,
    );
    window::create_window(
        mtm,
        handle.video_width,
        handle.video_height,
        args.fullscreen,
        &title,
    );

    log_stream_info(&handle.info);
    Ok(())
}

fn log_stream_info(info: &demux::StreamInfo) {
    if let Some(v) = &info.video_stream {
        eprintln!(
            "Video: {} {}x{} [stream {}]",
            v.codec_name, v.width, v.height, v.index
        );
    }
    for (i, a) in info.audio_streams.iter().enumerate() {
        eprintln!(
            "Audio #{}: {} {}Hz {} [stream {}]",
            i + 1,
            a.codec_name,
            a.sample_rate,
            a.channel_layout_desc,
            a.index
        );
    }
    for (i, s) in info.subtitle_streams.iter().enumerate() {
        eprintln!(
            "Subtitle #{}: {} {} [stream {}]",
            i + 1,
            s.codec_name,
            s.language.as_deref().unwrap_or("unknown"),
            s.index
        );
    }
    eprintln!("Duration: {}", crate::time::format_time(info.duration_us));
}
