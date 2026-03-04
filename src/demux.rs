use std::path::Path;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::media::Type;

use crate::cmd::{DemuxCommand, DemuxPacket};

/// Metadata about streams in the file.
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub video_stream: Option<VideoStreamInfo>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
    pub duration_us: i64,
}

#[derive(Debug, Clone)]
pub struct VideoStreamInfo {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub codec_name: String,
}

#[derive(Debug, Clone)]
pub struct AudioStreamInfo {
    pub index: usize,
    pub codec_name: String,
    pub sample_rate: u32,
    pub channel_layout_desc: String,
}

#[derive(Debug, Clone)]
pub struct SubtitleStreamInfo {
    pub index: usize,
    pub codec_name: String,
    pub language: Option<String>,
}

/// Probe a file and return stream info without starting playback.
pub fn probe(path: &Path) -> Result<StreamInfo> {
    let ictx = ffmpeg::format::input(path)
        .with_context(|| format!("Failed to open: {}", path.display()))?;

    let duration_us = if ictx.duration() >= 0 {
        // ffmpeg duration is in AV_TIME_BASE units (microseconds)
        ictx.duration()
    } else {
        0
    };

    let video_stream = ictx
        .streams()
        .best(Type::Video)
        .map(|s| {
            let params = s.parameters();
            let codec = ffmpeg::codec::context::Context::from_parameters(params).ok();
            let codec_name = codec
                .as_ref()
                .map(|c| c.id().name().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let (width, height) = codec
                .and_then(|c| c.decoder().video().ok())
                .map(|v| (v.width(), v.height()))
                .unwrap_or((0, 0));
            VideoStreamInfo {
                index: s.index(),
                width,
                height,
                codec_name,
            }
        });

    let audio_streams: Vec<AudioStreamInfo> = ictx
        .streams()
        .filter(|s| s.parameters().medium() == Type::Audio)
        .map(|s| {
            let params = s.parameters();
            let codec = ffmpeg::codec::context::Context::from_parameters(params).ok();
            let codec_name = codec
                .as_ref()
                .map(|c| c.id().name().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let (sample_rate, channels) = codec
                .and_then(|c| c.decoder().audio().ok())
                .map(|a| (a.rate(), a.channel_layout().channels() as u16))
                .unwrap_or((0, 0));
            AudioStreamInfo {
                index: s.index(),
                codec_name,
                sample_rate,
                channel_layout_desc: format!("{channels}ch"),
            }
        })
        .collect();

    let subtitle_streams: Vec<SubtitleStreamInfo> = ictx
        .streams()
        .filter(|s| s.parameters().medium() == Type::Subtitle)
        .map(|s| {
            let metadata = s.metadata();
            let language = metadata.get("language").map(|s| s.to_string());
            let codec_name = ffmpeg::codec::context::Context::from_parameters(s.parameters())
                .ok()
                .map(|c| c.id().name().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            SubtitleStreamInfo {
                index: s.index(),
                codec_name,
                language,
            }
        })
        .collect();

    Ok(StreamInfo {
        video_stream,
        audio_streams,
        subtitle_streams,
        duration_us,
    })
}

/// Run the demuxer read loop on a dedicated thread.
/// Reads packets from the file and sends them to the player thread.
/// Listens for seek/flush/stop commands from the player.
pub fn run_demuxer(
    path: &Path,
    video_idx: Option<usize>,
    audio_idx: Option<usize>,
    subtitle_idx: Option<usize>,
    cmd_rx: Receiver<DemuxCommand>,
    packet_tx: Sender<DemuxPacket>,
) -> Result<()> {
    let mut ictx = ffmpeg::format::input(path)
        .with_context(|| format!("Failed to open: {}", path.display()))?;

    loop {
        // Check for commands (non-blocking)
        match cmd_rx.try_recv() {
            Ok(DemuxCommand::Stop) => {
                log::debug!("Demuxer: stop received");
                return Ok(());
            }
            Ok(DemuxCommand::Seek { target_pts, forward }) => {
                log::debug!("Demuxer: seek to {target_pts}us (forward={forward})");
                let ts = target_pts;
                if forward {
                    // Keyframe at or after target
                    let _ = ictx.seek(ts, ts..);
                } else {
                    // Keyframe at or before target
                    let _ = ictx.seek(ts, ..ts);
                }
                let _ = packet_tx.send(DemuxPacket::Flush);
                continue;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => return Ok(()),
        }

        // Read next packet
        match read_next_packet(&mut ictx) {
            Some((stream_idx, packet)) => {
                let demux_pkt = if Some(stream_idx) == video_idx {
                    DemuxPacket::Video(packet)
                } else if Some(stream_idx) == audio_idx {
                    DemuxPacket::Audio(packet)
                } else if Some(stream_idx) == subtitle_idx {
                    DemuxPacket::Subtitle(packet)
                } else {
                    continue; // skip unwanted streams
                };

                if packet_tx.send(demux_pkt).is_err() {
                    return Ok(()); // player disconnected
                }
            }
            None => {
                // End of file
                let _ = packet_tx.send(DemuxPacket::Eof);
                log::debug!("Demuxer: EOF");

                // Wait for a seek command or stop
                match cmd_rx.recv() {
                    Ok(DemuxCommand::Stop) => return Ok(()),
                    Ok(DemuxCommand::Seek { target_pts, .. }) => {
                        let _ = ictx.seek(target_pts, ..target_pts);
                        continue;
                    }
                    Err(_) => return Ok(()),
                }
            }
        }
    }
}

fn read_next_packet(ictx: &mut Input) -> Option<(usize, ffmpeg::Packet)> {
    ictx.packets()
        .next()
        .map(|(stream, packet)| (stream.index(), packet))
}
