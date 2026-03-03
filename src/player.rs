use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};

use crate::audio_out::AudioOutput;
use crate::cmd::{Command, DemuxCommand, DemuxPacket, UiUpdate, VideoFrame};
use crate::decode_audio::AudioDecoder;
use crate::decode_video::VideoDecoder;
use crate::demux::StreamInfo;
use crate::subtitle::SubtitleTrack;
use crate::sync::{SyncAction, SyncClock};
use crate::time::format_time;

/// Player state machine.
pub struct Player {
    // Channels
    cmd_rx: Receiver<Command>,
    demux_packet_rx: Receiver<DemuxPacket>,
    demux_cmd_tx: Sender<DemuxCommand>,
    video_frame_tx: Sender<VideoFrame>,
    ui_update_tx: Sender<UiUpdate>,

    // Decoders
    video_decoder: Option<VideoDecoder>,
    audio_decoder: Option<AudioDecoder>,

    // Output
    audio_output: Option<AudioOutput>,

    // Sync
    sync_clock: SyncClock,
    audio_clock: Arc<AtomicI64>,

    // State
    paused: bool,
    volume: u32,
    audio_delay_us: i64,
    duration_us: i64,

    // Subtitles
    subtitle_tracks: Vec<SubtitleTrack>,
    current_subtitle_idx: Option<usize>,
    last_subtitle_text: Option<String>,

    // Stream info
    stream_info: StreamInfo,
    current_audio_track: usize,
}

impl Player {
    pub fn new(
        cmd_rx: Receiver<Command>,
        demux_packet_rx: Receiver<DemuxPacket>,
        demux_cmd_tx: Sender<DemuxCommand>,
        video_frame_tx: Sender<VideoFrame>,
        ui_update_tx: Sender<UiUpdate>,
        stream_info: StreamInfo,
        initial_volume: u32,
        initial_audio_delay: f64,
        subtitle_tracks: Vec<SubtitleTrack>,
    ) -> Result<Self> {
        let audio_clock = Arc::new(AtomicI64::new(0));
        let sync_clock = SyncClock::new(audio_clock.clone());

        Ok(Self {
            cmd_rx,
            demux_packet_rx,
            demux_cmd_tx,
            video_frame_tx,
            ui_update_tx,
            video_decoder: None,
            audio_decoder: None,
            audio_output: None,
            sync_clock,
            audio_clock,
            paused: false,
            volume: initial_volume,
            audio_delay_us: (initial_audio_delay * 1_000_000.0) as i64,
            duration_us: stream_info.duration_us,
            subtitle_tracks,
            current_subtitle_idx: None,
            last_subtitle_text: None,
            stream_info,
            current_audio_track: 0,
        })
    }

    /// Initialize decoders. Must be called after construction, with access to the ffmpeg streams.
    pub fn init_decoders(&mut self, ictx: &ffmpeg_next::format::context::Input) -> Result<()> {
        if let Some(ref vs) = self.stream_info.video_stream {
            let stream = ictx.stream(vs.index).context("Video stream not found")?;
            self.video_decoder = Some(VideoDecoder::new(&stream)?);

            let vd = self.video_decoder.as_ref().unwrap();
            let _ = self.ui_update_tx.send(UiUpdate::VideoSize {
                width: vd.width(),
                height: vd.height(),
            });
        }

        if let Some(audio) = self.stream_info.audio_streams.first() {
            let stream = ictx.stream(audio.index).context("Audio stream not found")?;
            let decoder = AudioDecoder::new(&stream)?;
            let sample_rate = decoder.sample_rate;
            let channels = decoder.channels;
            self.audio_decoder = Some(decoder);

            log::debug!("Creating audio output...");
            match AudioOutput::new(sample_rate, channels, self.audio_clock.clone()) {
                Ok(ao) => {
                    log::debug!("Audio output created successfully");
                    self.audio_output = Some(ao);
                }
                Err(e) => {
                    log::error!("Failed to create audio output: {e}");
                }
            }

            if self.volume < 100 {
                if let Some(ref mut ao) = self.audio_output {
                    ao.set_volume(self.volume as f32 / 100.0);
                }
            }
        }

        // Enable first subtitle track if available
        if !self.subtitle_tracks.is_empty() {
            self.current_subtitle_idx = Some(0);
        }

        Ok(())
    }

    /// Seek to a specific position (microseconds). Used for --start.
    pub fn seek_to(&mut self, target_us: i64) {
        let _ = self.demux_cmd_tx.send(DemuxCommand::Seek {
            target_pts: target_us,
            exact: false,
        });
        if let Some(ref mut vd) = self.video_decoder {
            vd.flush();
        }
        if let Some(ref mut ad) = self.audio_decoder {
            ad.flush();
        }
        if let Some(ref ao) = self.audio_output {
            ao.flush();
        }
        self.sync_clock.set_position(target_us);
    }

    /// Run the player event loop. Blocks until quit.
    pub fn run(&mut self) {
        log::info!("Player: starting event loop");

        loop {
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
                default(Duration::from_millis(16)) => {
                    // Timeout — periodic work below
                }
            }

            self.update_subtitles();
        }
    }

    fn handle_command(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Quit => {
                let _ = self.demux_cmd_tx.send(DemuxCommand::Stop);
                if let Some(ref ao) = self.audio_output {
                    ao.stop();
                }
                return true;
            }
            Command::PlayPause => {
                self.paused = !self.paused;
                self.sync_clock.set_paused(self.paused);
                if let Some(ref ao) = self.audio_output {
                    if self.paused {
                        ao.pause();
                    } else {
                        ao.play();
                    }
                }
            }
            Command::SeekRelative { seconds, exact } => {
                let current = self.sync_clock.audio_pts();
                let delta_us = (seconds * 1_000_000.0) as i64;
                let target = (current + delta_us).max(0).min(self.duration_us);

                // Show OSD immediately
                let dur_str = format_time(self.duration_us);
                let pos_str = format_time(target);
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::Osd(format!("{pos_str} / {dur_str}")));

                // Send seek to demuxer
                let _ = self.demux_cmd_tx.send(DemuxCommand::Seek {
                    target_pts: target,
                    exact,
                });

                // Flush decoders
                if let Some(ref mut vd) = self.video_decoder {
                    vd.flush();
                }
                if let Some(ref mut ad) = self.audio_decoder {
                    ad.flush();
                }
                if let Some(ref ao) = self.audio_output {
                    ao.flush();
                }

                self.sync_clock.set_position(target);
            }
            Command::VolumeUp => {
                self.volume = (self.volume + 5).min(100);
                if let Some(ref mut ao) = self.audio_output {
                    ao.set_volume(self.volume as f32 / 100.0);
                }
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::Osd(format!("Volume: {}%", self.volume)));
            }
            Command::VolumeDown => {
                self.volume = self.volume.saturating_sub(5);
                if let Some(ref mut ao) = self.audio_output {
                    ao.set_volume(self.volume as f32 / 100.0);
                }
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::Osd(format!("Volume: {}%", self.volume)));
            }
            Command::CycleAudioTrack => {
                if self.stream_info.audio_streams.len() > 1 {
                    self.current_audio_track =
                        (self.current_audio_track + 1) % self.stream_info.audio_streams.len();
                    let info = &self.stream_info.audio_streams[self.current_audio_track];
                    let _ = self.ui_update_tx.send(UiUpdate::Osd(format!(
                        "Audio: {}/{} - {} ({} {})",
                        self.current_audio_track + 1,
                        self.stream_info.audio_streams.len(),
                        info.channel_layout_desc,
                        info.codec_name,
                        info.channel_layout_desc,
                    )));
                }
            }
            Command::CycleSubtitle => {
                let total = self.subtitle_tracks.len();
                if total == 0 {
                    let _ = self
                        .ui_update_tx
                        .send(UiUpdate::Osd("Subtitles: none available".to_string()));
                } else {
                    self.current_subtitle_idx = match self.current_subtitle_idx {
                        Some(i) if i + 1 < total => Some(i + 1),
                        Some(_) => None, // turn off
                        None => Some(0), // turn back on
                    };
                    let label = match self.current_subtitle_idx {
                        Some(i) => self.subtitle_tracks[i].label.clone(),
                        None => "off".to_string(),
                    };
                    let _ = self
                        .ui_update_tx
                        .send(UiUpdate::Osd(format!("Subtitles: {label}")));
                    let _ = self
                        .ui_update_tx
                        .send(UiUpdate::SubtitleText(None));
                }
            }
            Command::AudioDelayIncrease => {
                self.audio_delay_us += 100_000; // +100ms
                let ms = self.audio_delay_us / 1000;
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::Osd(format!("Audio delay: {ms:+}ms")));
            }
            Command::AudioDelayDecrease => {
                self.audio_delay_us -= 100_000; // -100ms
                let ms = self.audio_delay_us / 1000;
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::Osd(format!("Audio delay: {ms:+}ms")));
            }
            Command::NextFile | Command::PrevFile => {
                let _ = self.ui_update_tx.send(UiUpdate::EndOfFile);
            }
            Command::ToggleFullscreen => {
                // Handled on main thread
                log::debug!("Toggle fullscreen");
            }
        }
        false
    }

    fn handle_packet(&mut self, pkt: DemuxPacket) {
        match pkt {
            DemuxPacket::Video(packet) => {
                if let Some(ref mut decoder) = self.video_decoder {
                    if decoder.send_packet(&packet).is_err() {
                        return;
                    }
                    while let Some(frame) = decoder.receive_frame() {
                        let action = self.sync_clock.decide(frame.pts_us);
                        match action {
                            SyncAction::Display => {
                                let _ = self.video_frame_tx.send(frame);
                            }
                            SyncAction::Drop => {
                                // Release pixel buffer
                                if !frame.pixel_buffer.is_null() {
                                    unsafe {
                                        crate::decode_video::release_pixel_buffer(
                                            frame.pixel_buffer,
                                        );
                                    }
                                }
                            }
                            SyncAction::Wait(_us) => {
                                // For simplicity, just display it (the layer handles timing)
                                let _ = self.video_frame_tx.send(frame);
                            }
                        }
                    }
                }
            }
            DemuxPacket::Audio(packet) => {
                if let Some(ref mut decoder) = self.audio_decoder {
                    if decoder.send_packet(&packet).is_err() {
                        return;
                    }
                    while let Some(mut buf) = decoder.receive_buffer() {
                        // Apply audio delay
                        buf.pts_us += self.audio_delay_us;
                        if let Some(ref ao) = self.audio_output {
                            ao.schedule_buffer(&buf);
                        }
                    }
                }
            }
            DemuxPacket::Subtitle(_packet) => {
                // TODO: decode embedded subtitles
            }
            DemuxPacket::Eof => {
                // Drain decoders
                if let Some(ref mut vd) = self.video_decoder {
                    let _ = vd.send_eof();
                    while let Some(frame) = vd.receive_frame() {
                        let _ = self.video_frame_tx.send(frame);
                    }
                }
                if let Some(ref mut ad) = self.audio_decoder {
                    let _ = ad.send_eof();
                    while let Some(buf) = ad.receive_buffer() {
                        if let Some(ref ao) = self.audio_output {
                            ao.schedule_buffer(&buf);
                        }
                    }
                }
                let _ = self.ui_update_tx.send(UiUpdate::EndOfFile);
            }
        }
    }

    fn update_subtitles(&mut self) {
        let Some(idx) = self.current_subtitle_idx else {
            return;
        };
        let Some(track) = self.subtitle_tracks.get(idx) else {
            return;
        };
        let pts = self.sync_clock.audio_pts();
        let text = track.text_at(pts).map(|s| s.to_string());

        if text != self.last_subtitle_text {
            self.last_subtitle_text = text.clone();
            let _ = self.ui_update_tx.send(UiUpdate::SubtitleText(text));
        }
    }
}
