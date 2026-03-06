use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::thread;

use anyhow::{Context, Result, bail};

use play::cmd::{self, Command, DemuxCommand, DemuxPacket, EndReason, UiUpdate, VideoFrame};
use play::player::Player;
use play::{demux, subtitle, time, window};

fn main() -> Result<()> {
    let args = cmd::parse_args()?;

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

    let mut index = 0;
    let mut first_run = true;
    while index < files.len() {
        let file = &files[index];
        match play_file(file, &args, first_run, index, files.len()) {
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

fn play_file(
    path: &Path,
    args: &cmd::Args,
    first_run: bool,
    file_index: usize,
    file_count: usize,
) -> Result<EndReason> {
    let info = demux::probe(path)?;

    if info.video_stream.is_none() {
        bail!(
            "No video stream in {}. Use playm for audio-only files.",
            path.display()
        );
    }

    let video_idx = info.video_stream.as_ref().map(|s| s.index);
    let audio_idx = info
        .audio_streams
        .get(args.audio_track.saturating_sub(1))
        .or(info.audio_streams.first())
        .map(|s| s.index);
    let subtitle_idx = info.subtitle_streams.first().map(|s| s.index);

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

    // Create channels
    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<Command>(32);
    let (demux_packet_tx, demux_packet_rx) = crossbeam_channel::bounded::<DemuxPacket>(64);
    let (demux_cmd_tx, demux_cmd_rx) = crossbeam_channel::bounded::<DemuxCommand>(4);
    let (video_frame_tx, video_frame_rx) = crossbeam_channel::bounded::<VideoFrame>(8);
    let (ui_update_tx, ui_update_rx) = crossbeam_channel::unbounded::<UiUpdate>();

    let audio_clock = Arc::new(AtomicI64::new(0));

    let video_width = info.video_stream.as_ref().map(|s| s.width).unwrap_or(640);
    let video_height = info.video_stream.as_ref().map(|s| s.height).unwrap_or(480);

    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    if file_count > 1 {
        let _ = ui_update_tx.send(UiUpdate::Osd(format!(
            "({}/{}) {filename}",
            file_index + 1,
            file_count
        )));
    }

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
    let start_pos = if first_run {
        args.start
            .as_ref()
            .and_then(|s| time::parse_time(s).ok())
            .unwrap_or(0)
    } else {
        0
    };

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
                player_path,
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

    let title = format!("playv - {filename}");
    log_stream_info(&info);

    let reason = window::run_app(
        cmd_tx,
        video_frame_rx,
        ui_update_rx,
        video_width,
        video_height,
        args.fullscreen,
        audio_clock,
        info.duration_us,
        &title,
        first_run,
        file_index,
        file_count,
    );

    player_thread.join().ok();
    demux_thread.join().ok();

    Ok(reason)
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
    eprintln!("Duration: {}", time::format_time(info.duration_us));
}
