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
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::thread;

use anyhow::{bail, Context, Result};
use clap::Parser;

use cmd::{Args, Command, DemuxCommand, DemuxPacket, UiUpdate, VideoFrame};
use player::Player;

fn main() -> Result<()> {
    let args = Args::parse();

    // Set up logging
    let log_level = match args.verbose {
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

    // Validate files
    for file in &args.files {
        if !file.exists() {
            bail!("File not found: {}", file.display());
        }
    }

    // Play first file (playlist support later)
    let file = &args.files[0];
    play_file(file, &args)?;

    Ok(())
}

fn play_file(path: &Path, args: &Args) -> Result<()> {
    // Probe the file
    let info = demux::probe(path)?;

    // Always print stream info
    log_stream_info(&info);

    let has_video = info.video_stream.is_some();
    let has_audio = !info.audio_streams.is_empty();

    if !has_video && !has_audio {
        bail!("No playable streams found in {}", path.display());
    }

    // Determine stream indices
    let video_idx = info.video_stream.as_ref().map(|s| s.index);
    let audio_idx = info
        .audio_streams
        .get(args.audio_track.saturating_sub(1))
        .or(info.audio_streams.first())
        .map(|s| s.index);
    let subtitle_idx = info.subtitle_streams.first().map(|s| s.index);

    // Load external subtitles
    let mut subtitle_tracks = Vec::new();

    // Auto-detect SRT files
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

    // --sub-file flag
    if let Some(ref sub_path) = args.sub_file {
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

    // Create channels
    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<Command>(32);
    let (demux_packet_tx, demux_packet_rx) = crossbeam_channel::bounded::<DemuxPacket>(64);
    let (demux_cmd_tx, demux_cmd_rx) = crossbeam_channel::bounded::<DemuxCommand>(4);
    let (video_frame_tx, video_frame_rx) = crossbeam_channel::bounded::<VideoFrame>(8);
    let (ui_update_tx, ui_update_rx) = crossbeam_channel::unbounded::<UiUpdate>();

    // Audio clock shared between player (writer) and main thread (reader for timebase sync)
    let audio_clock = Arc::new(AtomicI64::new(0));

    let video_width = info
        .video_stream
        .as_ref()
        .map(|s| s.width)
        .unwrap_or(640);
    let video_height = info
        .video_stream
        .as_ref()
        .map(|s| s.height)
        .unwrap_or(480);

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
    let initial_volume = args.volume;
    let initial_audio_delay = args.audio_delay;
    let start_pos = args
        .start
        .as_ref()
        .and_then(|s| time::parse_time(s).ok())
        .unwrap_or(0);

    let player_clock = audio_clock.clone();
    let player_thread = thread::Builder::new()
        .name("player".into())
        .spawn(move || {
            let mut player = match Player::new(
                cmd_rx,
                demux_packet_rx,
                demux_cmd_tx,
                video_frame_tx,
                ui_update_tx,
                player_info,
                initial_volume,
                initial_audio_delay,
                subtitle_tracks,
                player_clock,
            ) {
                Ok(p) => p,
                Err(e) => {
                    log::error!("Failed to create player: {e}");
                    return;
                }
            };

            // Initialize decoders (need access to the input context for stream params)
            let ictx = match ffmpeg_next::format::input(&player_path) {
                Ok(ctx) => ctx,
                Err(e) => {
                    log::error!("Failed to open file for decoding: {e}");
                    return;
                }
            };
            if let Err(e) = player.init_decoders(&ictx) {
                log::error!("Failed to init decoders: {e}");
                return;
            }
            drop(ictx); // close the second context, decoders are initialized

            // Seek to start position if specified
            if start_pos > 0 {
                let _ = player.seek_to(start_pos);
            }

            player.run();
        })
        .context("Failed to spawn player thread")?;

    if has_video {
        window::run_app(
            cmd_tx,
            video_frame_rx,
            ui_update_rx,
            video_width,
            video_height,
            args.fullscreen,
            audio_clock,
        );
    } else {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        terminal::run_terminal(cmd_tx, ui_update_rx, filename, info.duration_us);
    }

    player_thread.join().ok();
    demux_thread.join().ok();

    Ok(())
}

fn log_stream_info(info: &demux::StreamInfo) {
    if let Some(ref v) = info.video_stream {
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
