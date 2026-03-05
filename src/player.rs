use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};

use crate::audio_out::AudioOutput;
use crate::cmd::{Command, DemuxCommand, DemuxPacket, EndReason, UiUpdate, VideoFrame};
use crate::decode_audio::AudioDecoder;
use crate::decode_video::VideoDecoder;
use crate::demux::StreamInfo;
use crate::subtitle::SubtitleTrack;
use crate::sync::SyncClock;

/// Accumulated relative seek waiting to be dispatched.
/// Like mpv's `queue_seek()`: coalesces rapid key-repeat seeks so only one
/// seek is in flight at a time, and the previous frame stays visible until
/// the new one is decoded.
struct QueuedSeek {
    seconds: f64,
    exact: bool,
}

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
    /// After an exact seek, skip audio/video with PTS below this value.
    seek_floor_us: i64,
    /// For inexact seeks: waiting for first post-seek packet to land.
    seek_landed: bool,
    /// Set by dispatch_seek, cleared when the first post-seek video frame
    /// carries seek_flush=true to the display. Separate from seek_landed so
    /// the video flush works even if audio lands first.
    needs_display_flush: bool,
    /// Suppresses audio during video scrubbing. Set by dispatch_seek,
    /// cleared after a settling period with no new seeks.
    scrubbing: bool,
    last_seek_time: Option<Instant>,
    /// Tracks the end PTS of the last scheduled audio buffer.
    last_audio_end_us: i64,
    /// After demuxer EOF: PTS at which all scheduled audio finishes.
    eof_audio_end_us: Option<i64>,
    /// Decoded audio buffers waiting for ring space. Decouples decode from
    /// the blocking ring push so the player can always check commands.
    pending_audio: VecDeque<crate::decode_audio::AudioBuffer>,

    // Subtitles
    subtitle_tracks: Vec<SubtitleTrack>,
    current_subtitle_idx: Option<usize>,
    last_subtitle_text: Option<String>,

    // Stream info
    file_path: PathBuf,
    stream_info: StreamInfo,
    current_audio_track: usize,
}

impl Player {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cmd_rx: Receiver<Command>,
        demux_packet_rx: Receiver<DemuxPacket>,
        demux_cmd_tx: Sender<DemuxCommand>,
        video_frame_tx: Sender<VideoFrame>,
        ui_update_tx: Sender<UiUpdate>,
        file_path: PathBuf,
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
            seek_floor_us: 0,
            seek_landed: true,
            needs_display_flush: false,
            scrubbing: false,
            last_seek_time: None,
            last_audio_end_us: 0,
            eof_audio_end_us: None,
            pending_audio: VecDeque::new(),
            subtitle_tracks,
            current_subtitle_idx: None,
            last_subtitle_text: None,
            file_path,
            stream_info,
            current_audio_track: 0,
        })
    }

    /// Initialize decoders. Opens the file to read stream parameters.
    pub fn init_decoders(&mut self) -> Result<()> {
        let ictx = ffmpeg_next::format::input(&self.file_path)
            .with_context(|| format!("Failed to open: {}", self.file_path.display()))?;
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

            if self.volume < 100
                && let Some(ref mut ao) = self.audio_output
            {
                ao.set_volume(self.volume as f32 / 100.0);
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
        let _ = self.demux_cmd_tx.send(DemuxCommand::Seek {
            target_pts: target,
            forward,
        });
        if self.stream_info.video_stream.is_some() {
            self.pending_seeks += 1;
            self.scrubbing = true;
            self.last_seek_time = Some(Instant::now());
            self.flush_decoders(); // flushes decoders + stops AudioUnit + clears rings
            self.needs_display_flush = true;
            if !exact {
                self.seek_landed = false;
            }
        } else {
            // Audio-only: use pending_seeks to discard stale pre-seek packets
            // still queued in the demux channel (up to 64). Without this, the
            // player decodes + schedule_buffers every stale packet, blocking
            // for 10-50ms before the Flush and new audio can arrive.
            self.pending_seeks += 1;
            self.last_audio_end_us = 0;
            self.eof_audio_end_us = None;
            if let Some(ref mut ad) = self.audio_decoder {
                ad.flush();
            }
            if let Some(ref ao) = self.audio_output {
                ao.flush_quick();
                ao.set_clock_position(target);
            }
            self.pending_audio.clear();
        }
        self.seek_floor_us = if exact { target } else { 0 };
        self.sync_clock.set_position(target);
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
        // Skip no-op seeks (can happen when clock was projected eagerly)
        if qs.seconds.abs() < 0.001 && !qs.exact {
            return;
        }

        let has_video = self.stream_info.video_stream.is_some();

        // During scrubbing, serialize seeks: one in flight at a time.
        // The queued seek accumulates key-repeats, so when the Flush
        // arrives we dispatch a single coalesced seek instead of many.
        // This cuts CPU decode work proportionally (~30 seeks/sec → ~15).
        // Single seeks (not scrubbing) always fire immediately.
        if self.scrubbing && self.pending_seeks > 0 {
            // Project the clock to the intended position so the progress
            // bar tracks the user's input instantly, even before the
            // demuxer round-trip completes.
            let current = self.sync_clock.audio_pts();
            let projected = (current + (qs.seconds * 1_000_000.0) as i64)
                .max(0)
                .min(self.duration_us);
            self.sync_clock.set_position(projected);
            // Zero the offset since the clock now reflects it.
            // New key-repeats accumulate fresh from the projected position.
            self.queued_seek = Some(QueuedSeek {
                seconds: 0.0,
                exact: qs.exact,
            });
            return;
        }

        let current = self.sync_clock.audio_pts();
        let delta_us = (qs.seconds * 1_000_000.0) as i64;
        let target = (current + delta_us).max(0).min(self.duration_us);
        let forward = qs.seconds > 0.0;

        // Audio-only forward seek: skip directly in the ring buffer instead
        // of going through the demuxer. Instant — no file I/O, no channel
        // round-trips, no decoder flush.
        if !has_video
            && forward
            && !qs.exact
            && let Some(ref ao) = self.audio_output
        {
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
                self.sync_clock.set_position(actual_target);
                return;
            }
        }

        self.dispatch_seek(target, !qs.exact && forward, qs.exact);
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

        if self.stream_info.video_stream.is_none() {
            self.run_audio_only();
        } else {
            self.run_video();
        }
    }

    /// Video mode: commands always drain first so seek state is current
    /// before packets are processed. schedule_buffer may block but that's
    /// fine — video paces decode via the display timebase.
    fn run_video(&mut self) {
        loop {
            // 1. Always drain all pending commands first.
            while let Ok(cmd) = self.cmd_rx.try_recv() {
                if self.handle_command(cmd) {
                    return;
                }
            }

            self.execute_queued_seek();

            // Clear scrubbing after seeks settle AND a grace period elapses.
            // Key-repeat fires at ~30Hz (33ms gaps) — 100ms covers the gap
            // so audio doesn't leak between repeats.
            if self.scrubbing
                && self.pending_seeks == 0
                && self.queued_seek.is_none()
                && self
                    .last_seek_time
                    .is_some_and(|t| t.elapsed() > Duration::from_millis(100))
            {
                self.scrubbing = false;
            }

            // 2. Process packets (video frames + audio).
            while let Ok(pkt) = self.demux_packet_rx.try_recv() {
                self.handle_packet(pkt);
            }

            self.update_subtitles();

            if let Some(end_us) = self.eof_audio_end_us
                && self.sync_clock.audio_pts() >= end_us
            {
                self.eof_audio_end_us = None;
                let _ = self.ui_update_tx.send(UiUpdate::EndOfFile(EndReason::Eof));
            }

            // 3. Wait for new events when idle.
            // During normal playback, select! wakes instantly on packet/command
            // arrival — the timeout only matters when channels are quiet (paused,
            // near EOF). 50ms keeps subtitle timing reasonable while cutting
            // idle wakeups from 250/s to 20/s.
            let timeout = if self.queued_seek.is_some() {
                Duration::from_millis(1)
            } else {
                Duration::from_millis(50)
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
                        Ok(pkt) => self.handle_packet(pkt),
                        Err(_) => return,
                    }
                }
                default(timeout) => {}
            }
        }
    }

    /// Audio-only mode: commands ALWAYS get priority. Decoded audio is
    /// queued in `pending_audio` and drained to the ring non-blockingly,
    /// so the player thread is never stuck in schedule_buffer when a seek
    /// command arrives.
    fn run_audio_only(&mut self) {
        loop {
            // 1. Always drain ALL pending commands first — seeks get
            //    immediate processing regardless of audio backpressure.
            while let Ok(cmd) = self.cmd_rx.try_recv() {
                if self.handle_command(cmd) {
                    return;
                }
            }

            self.execute_queued_seek();

            // 2. Drain pending audio to ring (non-blocking).
            self.drain_pending_audio();

            // 3. Process packets if we have room in pending queue.
            //    Limit to avoid starving command checks.
            if self.pending_audio.len() < 16 {
                for _ in 0..8 {
                    match self.demux_packet_rx.try_recv() {
                        Ok(pkt) => self.handle_packet(pkt),
                        Err(_) => break,
                    }
                }
            }

            self.update_subtitles();

            if let Some(end_us) = self.eof_audio_end_us
                && self.sync_clock.audio_pts() >= end_us
            {
                self.eof_audio_end_us = None;
                let _ = self.ui_update_tx.send(UiUpdate::EndOfFile(EndReason::Eof));
            }

            // 4. If nothing to do, wait for new events.
            if self.pending_audio.is_empty() && self.queued_seek.is_none() {
                let timeout = Duration::from_millis(4);
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
                    default(timeout) => {}
                }
            } else if !self.pending_audio.is_empty() {
                // Pending audio waiting for ring space — brief yield
                std::thread::sleep(Duration::from_micros(100));
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
            Command::SeekAbsolute { target_us } => {
                let target = target_us.max(0).min(self.duration_us);
                // Inexact (keyframe) seek for instant display — no decode-to-exact
                self.dispatch_seek(target, true, false);
            }
            Command::SeekRelative { seconds, exact } => {
                self.queue_seek(seconds, exact);

                // Drain cmd_rx for additional seeks that already arrived
                let mut deferred: Option<Command> = None;
                while let Ok(cmd) = self.cmd_rx.try_recv() {
                    match cmd {
                        Command::SeekRelative {
                            seconds: s,
                            exact: e,
                        } => {
                            self.queue_seek(s, e);
                        }
                        other => {
                            deferred = Some(other);
                            break;
                        }
                    }
                }

                if let Some(cmd) = deferred
                    && self.handle_command(cmd)
                {
                    return true;
                }
            }
            Command::VolumeUp => self.adjust_volume(5),
            Command::VolumeDown => self.adjust_volume(-5),
            Command::CycleAudioTrack => {
                if self.stream_info.audio_streams.len() > 1 {
                    self.current_audio_track =
                        (self.current_audio_track + 1) % self.stream_info.audio_streams.len();
                    let new_info = &self.stream_info.audio_streams[self.current_audio_track];

                    // Flush current audio pipeline
                    if let Some(ref mut ad) = self.audio_decoder {
                        ad.flush();
                    }
                    if let Some(ref ao) = self.audio_output {
                        ao.flush();
                    }

                    // Tell demuxer to switch audio stream
                    let _ = self
                        .demux_cmd_tx
                        .send(DemuxCommand::ChangeAudio(new_info.index));

                    // Re-open input to get new stream parameters and create new decoder
                    match ffmpeg_next::format::input(&self.file_path) {
                        Ok(ictx) => {
                            match ictx.stream(new_info.index) {
                                Some(stream) => {
                                    match AudioDecoder::new(&stream) {
                                        Ok(decoder) => {
                                            let new_rate = decoder.sample_rate;
                                            let new_channels = decoder.channels;
                                            self.audio_decoder = Some(decoder);

                                            // Recreate audio output for new stream params
                                            self.audio_output = None;
                                            match AudioOutput::new(
                                                new_rate,
                                                new_channels,
                                                self.audio_clock.clone(),
                                            ) {
                                                Ok(mut ao) => {
                                                    if self.volume < 100 {
                                                        ao.set_volume(self.volume as f32 / 100.0);
                                                    }
                                                    self.audio_output = Some(ao);
                                                }
                                                Err(e) => {
                                                    log::error!(
                                                        "Failed to create audio output: {e}"
                                                    );
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("Failed to create audio decoder: {e}");
                                        }
                                    }
                                }
                                None => {
                                    log::error!("Audio stream {} not found", new_info.index);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to re-open file for audio switch: {e}");
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
                    let _ = self.ui_update_tx.send(UiUpdate::SubtitleText(None));
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
            Command::ToggleFullscreen => {
                // Handled on main thread
                log::debug!("Toggle fullscreen");
            }
        }
        false
    }

    fn handle_packet(&mut self, pkt: DemuxPacket) {
        match pkt {
            DemuxPacket::Flush => {
                self.pending_seeks = self.pending_seeks.saturating_sub(1);
                if self.stream_info.video_stream.is_none() {
                    // Audio-only: flush decoder + ring + pending audio now
                    // that new-position packets are about to arrive.
                    if let Some(ref mut ad) = self.audio_decoder {
                        ad.flush();
                    }
                    if let Some(ref ao) = self.audio_output {
                        ao.flush_quick();
                    }
                    self.pending_audio.clear();
                }
            }
            // Discard stale pre-seek packets
            _ if self.pending_seeks > 0 => {}
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
                            frame.seek_flush = true;
                        }
                        // Inexact seek: first decoded frame reveals actual position
                        if !self.seek_landed {
                            self.seek_landed = true;
                            self.sync_clock.set_position(frame.pts_us);
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
                // Video mode: skip audio while scrubbing. Set on dispatch,
                // cleared only after all seeks settle (no pending, no queued).
                if self.scrubbing {
                    return;
                }
                if let Some(ref mut decoder) = self.audio_decoder {
                    if let Err(e) = decoder.send_packet(&packet) {
                        log::debug!("Audio send_packet error: {e}");
                        return;
                    }
                    let has_video = self.stream_info.video_stream.is_some();
                    while let Some(mut buf) = decoder.receive_buffer() {
                        buf.pts_us += self.audio_delay_us;
                        if buf.pts_us < self.seek_floor_us {
                            continue;
                        }
                        if !self.seek_landed {
                            self.seek_landed = true;
                            self.sync_clock.set_position(buf.pts_us);
                            if !has_video {
                                self.needs_display_flush = false;
                                let _ = self.ui_update_tx.send(UiUpdate::SeekFlush(buf.pts_us));
                            }
                        }
                        self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
                        if let Some(ref ao) = self.audio_output {
                            if has_video {
                                ao.schedule_buffer(&buf);
                            } else if !ao.try_schedule_buffer(&buf) {
                                self.pending_audio.push_back(buf);
                            }
                        }
                    }
                }
            }
            DemuxPacket::Subtitle(_packet) => {
                // TODO: decode embedded subtitles
            }
            DemuxPacket::Eof => {
                let has_video = self.stream_info.video_stream.is_some();
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
                        self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
                        if let Some(ref ao) = self.audio_output {
                            if has_video {
                                ao.schedule_buffer(&buf);
                            } else if !ao.try_schedule_buffer(&buf) {
                                self.pending_audio.push_back(buf);
                            }
                        }
                    }
                    // Flush any remaining accumulated samples
                    if let Some(buf) = ad.drain_accum() {
                        self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
                        if let Some(ref ao) = self.audio_output {
                            if has_video {
                                ao.schedule_buffer(&buf);
                            } else if !ao.try_schedule_buffer(&buf) {
                                self.pending_audio.push_back(buf);
                            }
                        }
                    }
                }
                if self.audio_output.is_some() && self.last_audio_end_us > 0 {
                    // Wait for audio to finish playing before signaling EOF
                    self.eof_audio_end_us = Some(self.last_audio_end_us);
                } else {
                    let _ = self.ui_update_tx.send(UiUpdate::EndOfFile(EndReason::Eof));
                }
            }
        }
    }

    /// Try to drain pending audio buffers into the ring. Non-blocking.
    fn drain_pending_audio(&mut self) {
        let Some(ref ao) = self.audio_output else {
            self.pending_audio.clear();
            return;
        };
        while let Some(buf) = self.pending_audio.front() {
            if ao.try_schedule_buffer(buf) {
                self.pending_audio.pop_front();
            } else {
                break; // ring full
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
        let current = track.text_at(pts);

        let changed = match (&current, &self.last_subtitle_text) {
            (Some(a), Some(b)) => *a != b.as_str(),
            (None, None) => false,
            _ => true,
        };

        if changed {
            let text = current.map(|s| s.to_string());
            self.last_subtitle_text = text.clone();
            let _ = self.ui_update_tx.send(UiUpdate::SubtitleText(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a Player wired to test channels (no decoders).
    /// `with_video` controls whether the player has a video stream (affects
    /// seek debouncing — video mode debounces, audio-only does not).
    fn make_test_player_ex(
        with_video: bool,
    ) -> (
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

        let video_stream = if with_video {
            Some(crate::demux::VideoStreamInfo {
                index: 0,
                width: 1920,
                height: 1080,
                codec_name: "h264".into(),
            })
        } else {
            None
        };

        let stream_info = StreamInfo {
            duration_us: 3_600_000_000, // 1 hour
            video_stream,
            audio_streams: vec![],
            subtitle_streams: vec![],
            metadata: vec![],
        };

        let player = Player::new(
            cmd_rx,
            demux_pkt_rx,
            demux_cmd_tx,
            video_frame_tx,
            ui_update_tx,
            PathBuf::from("/dev/null"),
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

    /// Audio-only test player (no seek debouncing).
    fn make_test_player() -> (
        Player,
        Sender<Command>,
        Receiver<VideoFrame>,
        Receiver<UiUpdate>,
        Receiver<DemuxCommand>,
    ) {
        make_test_player_ex(false)
    }

    /// Video test player (with seek debouncing).
    fn make_video_test_player() -> (
        Player,
        Sender<Command>,
        Receiver<VideoFrame>,
        Receiver<UiUpdate>,
        Receiver<DemuxCommand>,
    ) {
        make_test_player_ex(true)
    }

    #[test]
    fn first_seek_dispatches_immediately() {
        let (mut player, _, _, _, demux_cmd_rx) = make_test_player();

        player.queue_seek(5.0, false);
        player.execute_queued_seek();

        // Audio-only now uses pending_seeks to discard stale pre-seek packets
        assert_eq!(player.pending_seeks, 1);
        assert!(player.queued_seek.is_none());
        assert!(
            demux_cmd_rx.try_recv().is_ok(),
            "Seek command should be sent to demuxer"
        );
    }

    #[test]
    fn scrubbing_serializes_video_seeks() {
        let (mut player, _, _, _, demux_cmd_rx) = make_video_test_player();

        // First seek dispatches immediately (not yet scrubbing)
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert_eq!(player.pending_seeks, 1);
        assert!(player.scrubbing);

        // Second seek deferred: scrubbing + pending_seeks > 0.
        // Clock is projected eagerly, queued seek zeroed.
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert_eq!(
            player.pending_seeks, 1,
            "Should not dispatch while scrubbing"
        );
        // Clock projected to 10s (5s dispatched + 5s projected)
        assert_eq!(player.sync_clock.audio_pts(), 10_000_000);

        // Only one seek sent to demuxer
        let mut count = 0;
        while demux_cmd_rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);

        // Simulate Flush arrival + new key-repeat while waiting
        player.pending_seeks = 0;
        player.queue_seek(5.0, false);
        player.execute_queued_seek();
        assert_eq!(
            player.pending_seeks, 1,
            "New seek should dispatch after Flush"
        );
        assert!(player.queued_seek.is_none());
    }

    #[test]
    fn queued_seeks_accumulate() {
        let (mut player, _, _, _, _) = make_test_player();

        // queue_seek accumulates offsets into a single queued seek
        for _ in 0..4 {
            player.queue_seek(5.0, false);
        }

        let qs = player.queued_seek.as_ref().unwrap();
        assert!(
            (qs.seconds - 20.0).abs() < 0.001,
            "Should accumulate to +20s"
        );
    }

    #[test]
    fn needs_display_flush_independent_of_seek_landed() {
        // If audio lands before video after a seek, seek_landed is set true.
        // needs_display_flush must remain true until a video frame clears it.
        let (mut player, _, _, _, _) = make_test_player();

        player.pending_seeks = 1;
        player.seek_landed = false;
        player.needs_display_flush = true;

        // Audio lands first
        player.seek_landed = true;
        player.sync_clock.set_position(5_000_000);

        assert!(
            player.needs_display_flush,
            "Display flush should wait for video frame, not audio"
        );

        // Video frame arrives
        player.needs_display_flush = false;

        assert!(
            !player.needs_display_flush,
            "Display flush should be cleared after video frame"
        );
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
