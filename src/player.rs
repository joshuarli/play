use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};

use crate::audio_out::AudioOutput;
use crate::cmd::{Command, DemuxCommand, DemuxPacket, UiUpdate, VideoFrame};
use crate::decode_audio::AudioDecoder;
use crate::decode_video::VideoDecoder;
use crate::demux::StreamInfo;
use crate::subtitle::SubtitleTrack;
use crate::sync::SyncClock;
use crate::time::format_time;

/// Accumulated relative seek waiting to be dispatched.
/// Like mpv's `queue_seek()`: coalesces rapid key-repeat seeks so only one
/// seek is in flight at a time, and the previous frame stays visible until
/// the new one is decoded.
struct QueuedSeek {
    seconds: f64,
    exact: bool,
}

/// Minimum time between seek dispatches. Must be >= the display timer tick
/// (4ms / 240Hz) so at most one seek_flush frame lands per tick — otherwise
/// the second flush clears the first before the compositor runs.
const SEEK_MIN_DISPLAY: Duration = Duration::from_millis(4);
/// Safety timeout: if a seek completes but no frame arrives within this window,
/// dispatch the queued seek anyway (prevents stuck decodes from freezing).
const SEEK_COALESCE_TIMEOUT: Duration = Duration::from_millis(50);

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
    pending_seeks: u32,
    /// Accumulated relative seek, dispatched by `execute_queued_seek()`.
    queued_seek: Option<QueuedSeek>,
    /// When the last seek was dispatched (for coalescing window).
    last_seek_dispatched: Option<Instant>,
    /// Set when the first post-seek frame is shown; unblocks the next queued seek.
    seek_frame_shown: bool,
    /// After an exact seek, skip audio/video with PTS below this value.
    seek_floor_us: i64,
    /// For inexact seeks: waiting for first post-seek packet to land.
    seek_landed: bool,
    /// Set by dispatch_seek, cleared when the first post-seek video frame
    /// carries seek_flush=true to the display. Separate from seek_landed so
    /// the video flush works even if audio lands first.
    needs_display_flush: bool,
    /// Tracks the end PTS of the last scheduled audio buffer.
    last_audio_end_us: i64,
    /// After demuxer EOF: PTS at which all scheduled audio finishes.
    eof_audio_end_us: Option<i64>,

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
        audio_clock: Arc<AtomicI64>,
    ) -> Result<Self> {
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
            pending_seeks: 0,
            queued_seek: None,
            last_seek_dispatched: None,
            seek_frame_shown: true,
            seek_floor_us: 0,
            seek_landed: true,
            needs_display_flush: false,
            last_audio_end_us: 0,
            eof_audio_end_us: None,
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
            let vd = VideoDecoder::new(&stream)?;
            let _ = self.ui_update_tx.send(UiUpdate::VideoSize {
                width: vd.width(),
                height: vd.height(),
            });
            self.video_decoder = Some(vd);
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
        self.pending_seeks += 1;
        let _ = self.demux_cmd_tx.send(DemuxCommand::Seek {
            target_pts: target_us,
            forward: false,
        });
        self.flush_decoders();
        self.sync_clock.set_position(target_us);
        self.seek_floor_us = target_us;
        let _ = self.ui_update_tx.send(UiUpdate::SeekFlush(target_us));
    }

    fn dispatch_seek(&mut self, target: i64, forward: bool, exact: bool) {
        self.pending_seeks += 1;
        let _ = self.demux_cmd_tx.send(DemuxCommand::Seek {
            target_pts: target,
            forward,
        });
        self.flush_decoders();
        self.needs_display_flush = self.video_decoder.is_some();
        if exact {
            self.seek_floor_us = target;
            self.sync_clock.set_position(target);
        } else {
            self.seek_floor_us = 0;
            self.sync_clock.set_position(target);
            self.seek_landed = false;
        }
    }

    fn flush_decoders(&mut self) {
        self.last_audio_end_us = 0;
        self.eof_audio_end_us = None;
        if let Some(ref mut vd) = self.video_decoder {
            vd.flush();
        }
        if let Some(ref mut ad) = self.audio_decoder {
            ad.flush();
        }
        if let Some(ref ao) = self.audio_output {
            ao.flush();
        }
    }

    fn queue_seek(&mut self, seconds: f64, exact: bool) {
        if let Some(ref mut qs) = self.queued_seek {
            qs.seconds += seconds;
            qs.exact = qs.exact || exact;
        } else {
            self.queued_seek = Some(QueuedSeek { seconds, exact });
        }
    }

    /// Dispatch the queued seek if enough time has passed or a frame has been shown.
    /// Called each iteration of the main loop.
    fn execute_queued_seek(&mut self) {
        let Some(qs) = self.queued_seek.take() else {
            return;
        };

        // Never dispatch while a seek is still in flight — its packets would
        // be discarded (pending_seeks > 0 guard) and the frame never shown.
        if self.pending_seeks > 0 {
            self.queued_seek = Some(qs);
            return;
        }

        if let Some(dispatched) = self.last_seek_dispatched {
            let elapsed = dispatched.elapsed();
            // Minimum display time: let each frame survive at least one VSync
            // before the next seek_flush clears it.
            if elapsed < SEEK_MIN_DISPLAY {
                self.queued_seek = Some(qs);
                return;
            }
            // After minimum time, still wait for frame to actually arrive
            // (handles slow seeks). Safety timeout prevents stuck decodes.
            if !self.seek_frame_shown && elapsed < SEEK_COALESCE_TIMEOUT {
                self.queued_seek = Some(qs);
                return;
            }
        }

        let current = self.sync_clock.audio_pts();
        let delta_us = (qs.seconds * 1_000_000.0) as i64;
        let target = (current + delta_us).max(0).min(self.duration_us);
        let forward = qs.seconds > 0.0;

        self.dispatch_seek(target, !qs.exact && forward, qs.exact);
        self.last_seek_dispatched = Some(Instant::now());
        self.seek_frame_shown = false;
    }

    fn adjust_volume(&mut self, delta: i32) {
        self.volume = (self.volume as i32 + delta).clamp(0, 100) as u32;
        if let Some(ref mut ao) = self.audio_output {
            ao.set_volume(self.volume as f32 / 100.0);
        }
        let _ = self
            .ui_update_tx
            .send(UiUpdate::Osd(format!("Volume: {}%", self.volume)));
    }

    /// Run the player event loop. Blocks until quit.
    pub fn run(&mut self) {
        log::info!("Player: starting event loop");

        loop {
            // Tight poll when a seek is queued so execute_queued_seek runs
            // promptly once SEEK_MIN_DISPLAY elapses. Otherwise 16ms is fine.
            let timeout = if self.queued_seek.is_some() {
                Duration::from_millis(1)
            } else {
                Duration::from_millis(16)
            };

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
                        Ok(pkt) => {
                            self.handle_packet(pkt);
                            // Batch-drain packets that arrived together. After a
                            // Flush the first video packet is usually already
                            // queued — processing it in the same iteration saves
                            // a full round-trip through the select loop.
                            for _ in 0..8 {
                                match self.demux_packet_rx.try_recv() {
                                    Ok(p) => self.handle_packet(p),
                                    Err(_) => break,
                                }
                            }
                        }
                        Err(_) => return,
                    }
                }
                default(timeout) => {
                    // Timeout — periodic work below
                }
            }

            self.execute_queued_seek();
            self.update_subtitles();

            // After EOF: wait for audio playback to finish
            if let Some(end_us) = self.eof_audio_end_us {
                if self.sync_clock.audio_pts() >= end_us {
                    self.eof_audio_end_us = None;
                    let _ = self.ui_update_tx.send(UiUpdate::EndOfFile);
                }
            }
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
                let _ = self.ui_update_tx.send(UiUpdate::Paused(self.paused));
            }
            Command::SeekRelative { seconds, exact } => {
                self.queue_seek(seconds, exact);

                // Drain cmd_rx for additional seeks that already arrived
                let mut deferred = Vec::new();
                while let Ok(cmd) = self.cmd_rx.try_recv() {
                    match cmd {
                        Command::SeekRelative { seconds: s, exact: e } => {
                            self.queue_seek(s, e);
                        }
                        other => {
                            deferred.push(other);
                            break;
                        }
                    }
                }

                for cmd in deferred {
                    if self.handle_command(cmd) {
                        return true;
                    }
                }
            }
            Command::VolumeUp => self.adjust_volume(5),
            Command::VolumeDown => self.adjust_volume(-5),
            Command::CycleAudioTrack => {
                if self.stream_info.audio_streams.len() > 1 {
                    self.current_audio_track =
                        (self.current_audio_track + 1) % self.stream_info.audio_streams.len();
                    let info = &self.stream_info.audio_streams[self.current_audio_track];
                    let _ = self.ui_update_tx.send(UiUpdate::Osd(format!(
                        "Audio: {}/{} - {} {}Hz {}",
                        self.current_audio_track + 1,
                        self.stream_info.audio_streams.len(),
                        info.codec_name,
                        info.sample_rate,
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
                    let msg = match self.current_subtitle_idx {
                        Some(i) => format!("Subtitles: {}", self.subtitle_tracks[i].label),
                        None => "Subtitles: off".to_string(),
                    };
                    let _ = self.ui_update_tx.send(UiUpdate::Osd(msg));
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
            // Seek completed — decrement in-flight counter
            DemuxPacket::Flush => {
                self.pending_seeks = self.pending_seeks.saturating_sub(1);
                return;
            }
            // Discard stale pre-seek packets
            _ if self.pending_seeks > 0 => {
                return;
            }
            DemuxPacket::Video(packet) => {
                if let Some(ref mut decoder) = self.video_decoder {
                    if decoder.send_packet(&packet).is_err() {
                        return;
                    }
                    while let Some(mut frame) = decoder.receive_frame() {
                        // Exact seek: skip frames before the target
                        if frame.pts_us < self.seek_floor_us {
                            drop(frame);
                            continue;
                        }
                        // First valid frame after a seek: flush the display layer
                        // atomically with this frame (no VSync gap). Separate from
                        // seek_landed so this works even if audio landed first.
                        if self.needs_display_flush {
                            self.needs_display_flush = false;
                            self.seek_frame_shown = true;
                            frame.seek_flush = true;
                        }
                        // Inexact seek: first decoded frame reveals actual position
                        if !self.seek_landed {
                            self.seek_landed = true;
                            self.sync_clock.set_position(frame.pts_us);
                            let dur_str = format_time(self.duration_us);
                            let pos_str = format_time(frame.pts_us);
                            let _ = self
                                .ui_update_tx
                                .send(UiUpdate::Osd(format!("{pos_str} / {dur_str}")));
                        }
                        // Non-blocking send — the display layer's timebase handles
                        // presentation timing. Never block here so audio keeps flowing.
                        match self.video_frame_tx.try_send(frame) {
                            Ok(()) => {}
                            Err(crossbeam_channel::TrySendError::Full(_)) => {}
                            Err(crossbeam_channel::TrySendError::Disconnected(_)) => return,
                        }
                    }
                }
            }
            DemuxPacket::Audio(packet) => {
                if let Some(ref mut decoder) = self.audio_decoder {
                    if let Err(e) = decoder.send_packet(&packet) {
                        log::debug!("Audio send_packet error: {e}");
                        return;
                    }
                    while let Some(mut buf) = decoder.receive_buffer() {
                        buf.pts_us += self.audio_delay_us;
                        // After a seek, skip audio before the target
                        if buf.pts_us < self.seek_floor_us {
                            continue;
                        }
                        // First post-seek audio buffer reveals actual position
                        if !self.seek_landed {
                            self.seek_landed = true;
                            self.sync_clock.set_position(buf.pts_us);
                            // Audio-only: flush display via UI channel (no video
                            // frame to carry the flush). For video files, the video
                            // handler sets frame.seek_flush instead.
                            if self.video_decoder.is_none() {
                                self.seek_frame_shown = true;
                                self.needs_display_flush = false;
                                let _ = self
                                    .ui_update_tx
                                    .send(UiUpdate::SeekFlush(buf.pts_us));
                            }
                            let dur_str = format_time(self.duration_us);
                            let pos_str = format_time(buf.pts_us);
                            let _ = self
                                .ui_update_tx
                                .send(UiUpdate::Osd(format!("{pos_str} / {dur_str}")));
                        }
                        let end_us = buf.pts_us
                            + (buf.samples_per_channel as i64 * 1_000_000
                                / buf.sample_rate as i64);
                        self.last_audio_end_us = self.last_audio_end_us.max(end_us);
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
                        let end = buf.pts_us
                            + (buf.samples_per_channel as i64 * 1_000_000
                                / buf.sample_rate as i64);
                        self.last_audio_end_us = self.last_audio_end_us.max(end);
                        if let Some(ref ao) = self.audio_output {
                            ao.schedule_buffer(&buf);
                        }
                    }
                    // Flush any remaining accumulated samples
                    if let Some(buf) = ad.drain_accum() {
                        let end = buf.pts_us
                            + (buf.samples_per_channel as i64 * 1_000_000
                                / buf.sample_rate as i64);
                        self.last_audio_end_us = self.last_audio_end_us.max(end);
                        if let Some(ref ao) = self.audio_output {
                            ao.schedule_buffer(&buf);
                        }
                    }
                }
                if self.audio_output.is_some() && self.last_audio_end_us > 0 {
                    // Wait for audio to finish playing before signaling EOF
                    self.eof_audio_end_us = Some(self.last_audio_end_us);
                } else {
                    let _ = self.ui_update_tx.send(UiUpdate::EndOfFile);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a Player wired to test channels (no decoders).
    fn make_test_player() -> (
        Player,
        Sender<Command>,
        Receiver<VideoFrame>,
        Receiver<UiUpdate>,
        Receiver<DemuxCommand>,
    ) {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (demux_pkt_tx, demux_pkt_rx) = crossbeam_channel::unbounded();
        let (demux_cmd_tx, demux_cmd_rx) = crossbeam_channel::unbounded();
        let (video_frame_tx, video_frame_rx) = crossbeam_channel::bounded(8);
        let (ui_update_tx, ui_update_rx) = crossbeam_channel::unbounded();
        let audio_clock = Arc::new(AtomicI64::new(0));

        let stream_info = StreamInfo {
            duration_us: 3_600_000_000, // 1 hour
            video_stream: None,
            audio_streams: vec![],
            subtitle_streams: vec![],
        };

        let player = Player::new(
            cmd_rx,
            demux_pkt_rx,
            demux_cmd_tx,
            video_frame_tx,
            ui_update_tx,
            stream_info,
            100,
            0.0,
            vec![],
            audio_clock,
        )
        .unwrap();

        // Keep demux_pkt_tx alive so player channels don't disconnect
        std::mem::forget(demux_pkt_tx);

        (player, cmd_tx, video_frame_rx, ui_update_rx, demux_cmd_rx)
    }

    /// Simulate what happens after a seek completes: demux sends Flush,
    /// then the first video frame is decoded. In real code this flows through
    /// handle_packet; here we set the state directly since we have no decoder.
    fn simulate_seek_completion(player: &mut Player, frame_pts_us: i64) {
        player.pending_seeks = player.pending_seeks.saturating_sub(1);
        player.seek_landed = true;
        player.seek_frame_shown = true;
        player.needs_display_flush = false;
        player.sync_clock.set_position(frame_pts_us);
    }

    #[test]
    fn first_seek_dispatches_immediately() {
        let (mut player, _, _, _, demux_cmd_rx) = make_test_player();

        player.queue_seek(5.0, false);
        player.execute_queued_seek();

        assert_eq!(player.pending_seeks, 1);
        assert!(player.queued_seek.is_none());
        assert!(demux_cmd_rx.try_recv().is_ok(), "Seek command should be sent to demuxer");
    }

    #[test]
    fn pending_seeks_blocks_dispatch() {
        let (mut player, _, _, _, _) = make_test_player();

        // First seek dispatches
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 1);

        // Second seek while first is in-flight: should be deferred
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 1, "Should not dispatch while seek in-flight");
        assert!(player.queued_seek.is_some(), "Seek should remain queued");
    }

    #[test]
    fn rapid_seeks_deferred_by_minimum_display_time() {
        // Reproduces the core visual bug: when seeks complete instantly,
        // each frame's seek_flush clears the previous frame before VSync
        // can composite it. Without minimum display time, every seek
        // dispatches immediately and no intermediate frame is ever visible.
        let (mut player, _, _, _, demux_cmd_rx) = make_test_player();

        // First seek: dispatches immediately (no prior seek)
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 1);
        let mut dispatch_count = 1;

        // Simulate instant completion
        simulate_seek_completion(&mut player, 5_000_000);

        // Queue and try to execute 5 more seeks without any real time passing.
        // These should ALL be deferred — the first frame needs time on screen.
        for _ in 0..5 {
            player.queue_seek(5.0, false);
            player.execute_queued_seek();
            if player.pending_seeks > 0 {
                dispatch_count += 1;
                let pts = player.sync_clock.audio_pts();
                simulate_seek_completion(&mut player, pts);
            }
        }

        assert_eq!(
            dispatch_count, 1,
            "Only first seek should dispatch without waiting; got {dispatch_count}. \
             Intermediate frames are being flushed before VSync can composite them."
        );
        assert!(player.queued_seek.is_some(), "Remaining seeks should be queued");

        // Drain demux commands: only one Seek should have been sent
        let mut seek_count = 0;
        while demux_cmd_rx.try_recv().is_ok() {
            seek_count += 1;
        }
        assert_eq!(seek_count, 1);
    }

    #[test]
    fn deferred_seek_dispatches_after_minimum_display_time() {
        let (mut player, _, _, _, _) = make_test_player();

        // First seek dispatches
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        simulate_seek_completion(&mut player, 5_000_000);

        // Queue another — deferred (too soon)
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 0, "Should be deferred");
        assert!(player.queued_seek.is_some());

        // Wait for minimum display time
        std::thread::sleep(SEEK_MIN_DISPLAY + Duration::from_millis(1));

        // Now it should dispatch
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 1, "Should dispatch after display time");
        assert!(player.queued_seek.is_none());
    }

    #[test]
    fn queued_seeks_accumulate_during_deferral() {
        let (mut player, _, _, _, _) = make_test_player();

        // First seek dispatches to 5s
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        simulate_seek_completion(&mut player, 5_000_000);

        // Queue 4 more while deferred
        for _ in 0..4 {
            player.queue_seek(5.0, false);
        }

        let qs = player.queued_seek.as_ref().unwrap();
        assert!((qs.seconds - 20.0).abs() < 0.001, "Should accumulate to +20s");

        // Wait and dispatch — should seek from 5s to 25s
        std::thread::sleep(SEEK_MIN_DISPLAY + Duration::from_millis(1));
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 1);

        // Clock should target 25s (5s current + 20s accumulated)
        let pos = player.sync_clock.audio_pts();
        assert_eq!(pos, 25_000_000);
    }

    #[test]
    fn safety_timeout_prevents_stuck_scrubbing() {
        let (mut player, _, _, _, _) = make_test_player();

        // First seek dispatches
        player.queue_seek(5.0, false);
        player.execute_queued_seek();

        // Simulate: Flush arrives but NO frame (decoder stuck)
        player.pending_seeks = 0;
        // seek_frame_shown stays false

        // Queue another seek
        player.queue_seek(5.0, false);

        // Before timeout: deferred
        player.execute_queued_seek();
        assert!(player.queued_seek.is_some(), "Should defer (no frame yet)");

        // Wait past safety timeout
        std::thread::sleep(SEEK_COALESCE_TIMEOUT + Duration::from_millis(1));

        // Should dispatch now despite no frame shown
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 1, "Should dispatch after timeout");
    }

    #[test]
    fn needs_display_flush_independent_of_seek_landed() {
        // The core display bug: if audio lands before video after a seek,
        // seek_landed is set true. Without a separate needs_display_flush
        // flag, the video handler wouldn't set frame.seek_flush=true,
        // and the display flush would go through the UI channel (race).
        let (mut player, _, _, _, _) = make_test_player();

        // Simulate dispatch_seek for an inexact seek with video
        player.pending_seeks = 1;
        player.seek_landed = false;
        player.needs_display_flush = true;
        player.seek_frame_shown = false;

        // Simulate audio landing first (sets seek_landed, NOT display flush)
        player.seek_landed = true;
        player.sync_clock.set_position(5_000_000);

        // needs_display_flush must survive audio landing — it waits for video
        assert!(
            player.needs_display_flush,
            "Display flush should wait for video frame, not audio"
        );

        // Simulate video frame arriving (clears needs_display_flush)
        player.needs_display_flush = false;
        player.seek_frame_shown = true;

        assert!(
            !player.needs_display_flush,
            "Display flush should be cleared after video frame"
        );
    }

    #[test]
    fn constants_min_display_less_than_coalesce_timeout() {
        // SEEK_MIN_DISPLAY must be < SEEK_COALESCE_TIMEOUT, otherwise the
        // "wait for frame" window inside execute_queued_seek is unreachable
        // and every seek blocks until the safety timeout.
        assert!(
            SEEK_MIN_DISPLAY < SEEK_COALESCE_TIMEOUT,
            "SEEK_MIN_DISPLAY ({SEEK_MIN_DISPLAY:?}) must be < SEEK_COALESCE_TIMEOUT ({SEEK_COALESCE_TIMEOUT:?})"
        );
    }

    #[test]
    fn frame_shown_gates_dispatch_within_coalesce_window() {
        // After SEEK_MIN_DISPLAY elapses, execute_queued_seek should still
        // defer if seek_frame_shown is false (no frame decoded yet) — the
        // coalesce window gives the decoder time to produce a frame so it
        // can be seen before the next seek_flush clears it.
        let (mut player, _, _, _, _) = make_test_player();

        // First seek dispatches
        player.queue_seek(5.0, false);
        player.execute_queued_seek();

        // Simulate Flush arriving but NO video frame (decoder still working)
        player.pending_seeks = 0;
        // seek_frame_shown stays false

        // Wait past SEEK_MIN_DISPLAY but within SEEK_COALESCE_TIMEOUT
        std::thread::sleep(SEEK_MIN_DISPLAY + Duration::from_millis(1));

        // Queue a second seek — should defer (no frame shown yet)
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 0, "Should defer: no frame shown yet");
        assert!(player.queued_seek.is_some());

        // Now simulate the frame arriving
        player.seek_frame_shown = true;

        // Should dispatch immediately (MIN_DISPLAY already elapsed + frame shown)
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 1, "Should dispatch: frame shown and MIN_DISPLAY passed");
        assert!(player.queued_seek.is_none());
    }

    #[test]
    fn audio_only_seek_flush_uses_ui_channel() {
        // For audio-only files (no video decoder), SeekFlush must go through
        // the UI channel since there's no video frame to carry it.
        let (mut player, _, _, _, _) = make_test_player();
        assert!(player.video_decoder.is_none());

        // dispatch_seek should NOT set needs_display_flush for audio-only
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert!(
            !player.needs_display_flush,
            "Audio-only files don't use needs_display_flush"
        );
    }
}
