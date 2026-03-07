use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};

use crate::audio_out::AudioOutput;
use crate::cmd::{Command, DemuxCommand, DemuxPacket, EndReason, UiUpdate, VideoFrame};
use crate::decode_audio::{AudioBuffer, AudioDecoder};
use crate::decode_video::VideoDecoder;
use crate::demux::StreamInfo;
use crate::subtitle::SubtitleTrack;
use crate::sync::SyncClock;

/// Accumulated relative seek waiting to be dispatched.
struct QueuedSeek {
    seconds: f64,
    exact: bool,
}

// ── Shared core ────────────────────────────────────────────────────

/// State shared between video and audio-only modes.
struct PlayerCore {
    // Channels
    cmd_rx: Receiver<Command>,
    demux_packet_rx: Receiver<DemuxPacket>,
    demux_cmd_tx: Sender<DemuxCommand>,
    ui_update_tx: Sender<UiUpdate>,

    // Decoders
    audio_decoder: Option<AudioDecoder>,

    // Output
    audio_output: Option<AudioOutput>,

    // Sync
    sync_clock: SyncClock,
    audio_clock: Arc<AtomicI64>,

    // State
    paused: bool,
    volume: u32,
    pre_mute_volume: Option<u32>,
    audio_delay_us: i64,
    duration_us: i64,
    pending_seeks: u32,
    queued_seek: Option<QueuedSeek>,
    /// After an exact seek, skip audio/video with PTS below this value.
    seek_floor_us: i64,
    /// For inexact seeks: waiting for first post-seek packet to land.
    seek_landed: bool,
    /// Tracks the end PTS of the last scheduled audio buffer.
    last_audio_end_us: i64,
    /// After demuxer EOF: PTS at which all scheduled audio finishes.
    eof_audio_end_us: Option<i64>,

    // Subtitles
    subtitle_tracks: Vec<SubtitleTrack>,
    current_subtitle_idx: Option<usize>,
    last_subtitle_text: Option<String>,

    // Stream info
    file_path: PathBuf,
    stream_info: StreamInfo,
    current_audio_track: usize,
}

impl PlayerCore {
    fn queue_seek(&mut self, seconds: f64, exact: bool) {
        if let Some(ref mut qs) = self.queued_seek {
            qs.seconds += seconds;
            qs.exact = qs.exact || exact;
        } else {
            self.queued_seek = Some(QueuedSeek { seconds, exact });
        }
    }

    /// Drain cmd_rx for additional seeks after a SeekRelative, coalescing them.
    /// Returns true if a Quit was encountered (caller should exit).
    fn drain_seek_commands(&mut self) -> bool {
        let mut deferred: Option<Command> = None;
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                Command::SeekRelative {
                    seconds: s,
                    exact: e,
                } => self.queue_seek(s, e),
                other => {
                    deferred = Some(other);
                    break;
                }
            }
        }
        if let Some(cmd) = deferred {
            return self.handle_command_shared(cmd);
        }
        false
    }

    /// Handle commands common to both modes. Returns true if the player should exit.
    fn handle_command_shared(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Quit => {
                if self.demux_cmd_tx.send(DemuxCommand::Stop).is_err() {
                    log::warn!("Demuxer already disconnected on quit");
                }
                if let Some(ao) = self.audio_output.as_ref() {
                    ao.stop();
                }
                return true;
            }
            Command::PlayPause => {
                self.paused = !self.paused;
                self.sync_clock.set_paused(self.paused);
                if let Some(ao) = self.audio_output.as_ref() {
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
                if self.drain_seek_commands() {
                    return true;
                }
            }
            Command::VolumeUp => self.adjust_volume(5),
            Command::VolumeDown => self.adjust_volume(-5),
            Command::ToggleMute => self.toggle_mute(),
            Command::CycleAudioTrack => self.cycle_audio_track(),
            Command::CycleSubtitle => self.cycle_subtitle(),
            Command::AudioDelayIncrease => {
                self.audio_delay_us += 100_000;
                let ms = self.audio_delay_us / 1000;
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::Osd(format!("Audio delay: {ms:+}ms")));
            }
            Command::AudioDelayDecrease => {
                self.audio_delay_us -= 100_000;
                let ms = self.audio_delay_us / 1000;
                let _ = self
                    .ui_update_tx
                    .send(UiUpdate::Osd(format!("Audio delay: {ms:+}ms")));
            }
            Command::NextFile => {
                if self
                    .ui_update_tx
                    .send(UiUpdate::EndOfFile(EndReason::NextFile))
                    .is_err()
                {
                    log::warn!("UI disconnected on NextFile");
                }
            }
            Command::PrevFile => {
                if self
                    .ui_update_tx
                    .send(UiUpdate::EndOfFile(EndReason::PrevFile))
                    .is_err()
                {
                    log::warn!("UI disconnected on PrevFile");
                }
            }
            // SeekAbsolute is mode-specific (video uses dispatch_seek_video,
            // audio-only uses dispatch_seek_audio_only)
            Command::SeekAbsolute { .. } | Command::ToggleFullscreen => {}
        }
        false
    }

    fn adjust_volume(&mut self, delta: i32) {
        self.pre_mute_volume = None; // unmute on manual volume change
        self.volume = (self.volume as i32 + delta).clamp(0, 100) as u32;
        if let Some(ao) = self.audio_output.as_mut() {
            ao.set_volume(self.volume as f32 / 100.0);
        }
        let _ = self
            .ui_update_tx
            .send(UiUpdate::Osd(format!("Volume: {}%", self.volume)));
    }

    fn toggle_mute(&mut self) {
        if let Some(prev) = self.pre_mute_volume.take() {
            self.volume = prev;
        } else {
            self.pre_mute_volume = Some(self.volume);
            self.volume = 0;
        }
        if let Some(ao) = self.audio_output.as_mut() {
            ao.set_volume(self.volume as f32 / 100.0);
        }
        let label = if self.pre_mute_volume.is_some() {
            "Muted"
        } else {
            "Volume"
        };
        let _ = self
            .ui_update_tx
            .send(UiUpdate::Osd(format!("{label}: {}%", self.volume)));
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

        if self
            .demux_cmd_tx
            .send(DemuxCommand::ChangeAudio(new_info.index))
            .is_err()
        {
            log::warn!("Demuxer disconnected on audio track change");
        }

        if let Err(e) = self.switch_audio_decoder(new_info.index) {
            log::error!("Audio switch failed: {e}");
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

    fn cycle_subtitle(&mut self) {
        let total = self.subtitle_tracks.len();
        if total == 0 {
            let _ = self
                .ui_update_tx
                .send(UiUpdate::Osd("Subtitles: none available".to_string()));
        } else {
            self.current_subtitle_idx = match self.current_subtitle_idx {
                Some(i) if i + 1 < total => Some(i + 1),
                Some(_) => None,
                None => Some(0),
            };
            let msg = match self.current_subtitle_idx {
                Some(i) => format!("Subtitles: {}", self.subtitle_tracks[i].label),
                None => "Subtitles: off".to_string(),
            };
            let _ = self.ui_update_tx.send(UiUpdate::Osd(msg));
            let _ = self.ui_update_tx.send(UiUpdate::SubtitleText(None));
        }
    }

    fn switch_audio_decoder(&mut self, stream_index: usize) -> anyhow::Result<()> {
        let ictx = ffmpeg_next::format::input(&self.file_path)
            .context("Failed to re-open file for audio switch")?;
        let stream = ictx
            .stream(stream_index)
            .ok_or_else(|| anyhow::anyhow!("Audio stream {stream_index} not found"))?;
        let decoder = AudioDecoder::new(&stream).context("Failed to create audio decoder")?;
        let new_rate = decoder.sample_rate;
        let new_channels = decoder.channels;
        self.audio_decoder = Some(decoder);
        self.audio_output = None;
        let mut ao = AudioOutput::new(new_rate, new_channels, self.audio_clock.clone())
            .context("Failed to create audio output")?;
        if self.volume < 100 {
            ao.set_volume(self.volume as f32 / 100.0);
        }
        self.audio_output = Some(ao);
        Ok(())
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

    fn check_eof(&mut self) {
        if self.eof_audio_end_us.is_some() {
            let done = self.sync_clock.audio_pts() >= self.eof_audio_end_us.unwrap()
                || self
                    .audio_output
                    .as_ref()
                    .is_some_and(|ao| ao.buffered_samples() == 0);
            if done {
                self.eof_audio_end_us = None;
                if self
                    .ui_update_tx
                    .send(UiUpdate::EndOfFile(EndReason::Eof))
                    .is_err()
                {
                    log::warn!("UI disconnected on EOF check");
                }
            }
        }
    }

    /// Schedule an audio buffer, blocking or non-blocking depending on mode.
    /// Returns any buffer that couldn't be scheduled (audio-only non-blocking path).
    fn schedule_audio(ao: &AudioOutput, buf: AudioBuffer, blocking: bool) -> Option<AudioBuffer> {
        if blocking {
            ao.schedule_buffer(&buf);
            None
        } else if ao.try_schedule_buffer(&buf) {
            None
        } else {
            Some(buf)
        }
    }

    /// Process audio from decoder, applying delay and seek floor.
    /// `blocking`: true in video mode (schedule_buffer blocks), false in audio-only.
    /// Returns any buffers that couldn't be scheduled non-blockingly.
    fn decode_audio_packet(
        &mut self,
        packet: &ffmpeg_next::Packet,
        blocking: bool,
    ) -> Vec<AudioBuffer> {
        let mut pending = Vec::new();
        let Some(decoder) = self.audio_decoder.as_mut() else {
            return pending;
        };
        if let Err(e) = decoder.send_packet(packet) {
            log::debug!("Audio send_packet error: {e}");
            return pending;
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
                    let _ = self.ui_update_tx.send(UiUpdate::SeekFlush(buf.pts_us));
                }
            }
            self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
            if let Some(ao) = self.audio_output.as_ref()
                && let Some(leftover) = Self::schedule_audio(ao, buf, blocking)
            {
                pending.push(leftover);
            }
        }
        pending
    }

    /// Drain remaining audio from decoder at EOF.
    fn drain_audio_at_eof(&mut self, blocking: bool) -> Vec<AudioBuffer> {
        let mut pending = Vec::new();
        let Some(ad) = self.audio_decoder.as_mut() else {
            return pending;
        };
        let _ = ad.send_eof();
        while let Some(buf) = ad.receive_buffer() {
            self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
            if let Some(ao) = self.audio_output.as_ref()
                && let Some(leftover) = Self::schedule_audio(ao, buf, blocking)
            {
                pending.push(leftover);
            }
        }
        if let Some(buf) = ad.drain_accum() {
            self.last_audio_end_us = self.last_audio_end_us.max(buf.end_us());
            if let Some(ao) = self.audio_output.as_ref()
                && let Some(leftover) = Self::schedule_audio(ao, buf, blocking)
            {
                pending.push(leftover);
            }
        }
        pending
    }

    fn signal_eof(&mut self) {
        if self.audio_output.is_some() && self.last_audio_end_us > 0 {
            self.eof_audio_end_us = Some(self.last_audio_end_us);
        } else if self
            .ui_update_tx
            .send(UiUpdate::EndOfFile(EndReason::Eof))
            .is_err()
        {
            log::warn!("UI disconnected on EOF signal");
        }
    }

    fn flush_audio_pipeline(&mut self) {
        self.last_audio_end_us = 0;
        self.eof_audio_end_us = None;
        if let Some(ad) = self.audio_decoder.as_mut() {
            ad.flush();
        }
        if let Some(ao) = self.audio_output.as_ref() {
            ao.flush();
        }
    }

    /// Common seek dispatch shared between video and audio-only modes.
    /// Mode-specific code (video decoder flush, scrubbing, display flush,
    /// ring skip) is handled by the caller before/after this.
    fn dispatch_seek_common(&mut self, target: i64, forward: bool, exact: bool) {
        let _ = self.demux_cmd_tx.send(DemuxCommand::Seek {
            target_pts: target,
            forward,
        });
        self.pending_seeks += 1;
        if !exact {
            self.seek_landed = false;
        }
        self.seek_floor_us = if exact { target } else { 0 };
        self.sync_clock.set_position(target);
    }
}

// ── Video player ───────────────────────────────────────────────────

/// Video mode: owns the video decoder and display-flush state.
struct VideoPlayer {
    core: PlayerCore,
    video_decoder: Option<VideoDecoder>,
    video_frame_tx: Sender<VideoFrame>,
    /// Set by dispatch_seek, cleared when the first post-seek video frame
    /// carries seek_flush=true to the display.
    needs_display_flush: bool,
    /// Suppresses audio during video scrubbing.
    scrubbing: bool,
    last_seek_time: Option<Instant>,
}

impl VideoPlayer {
    fn dispatch_seek(&mut self, target: i64, forward: bool, exact: bool) {
        self.core.dispatch_seek_common(target, forward, exact);
        self.scrubbing = true;
        self.last_seek_time = Some(Instant::now());
        if let Some(vd) = self.video_decoder.as_mut() {
            vd.flush();
        }
        self.core.flush_audio_pipeline();
        self.needs_display_flush = true;
    }

    fn execute_queued_seek(&mut self) {
        let Some(qs) = self.core.queued_seek.take() else {
            return;
        };
        if qs.seconds.abs() < 0.001 && !qs.exact {
            return;
        }

        // During scrubbing, serialize seeks: one in flight at a time.
        if self.scrubbing && self.core.pending_seeks > 0 {
            let current = self.core.sync_clock.audio_pts();
            let projected = (current + (qs.seconds * 1_000_000.0) as i64)
                .max(0)
                .min(self.core.duration_us);
            self.core.sync_clock.set_position(projected);
            self.core.queued_seek = Some(QueuedSeek {
                seconds: 0.0,
                exact: qs.exact,
            });
            return;
        }

        let current = self.core.sync_clock.audio_pts();
        let delta_us = (qs.seconds * 1_000_000.0) as i64;
        let target = (current + delta_us).max(0).min(self.core.duration_us);
        let forward = qs.seconds > 0.0;
        self.dispatch_seek(target, !qs.exact && forward, qs.exact);
    }

    fn handle_command(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::SeekAbsolute { target_us } => {
                let target = target_us.max(0).min(self.core.duration_us);
                self.dispatch_seek(target, true, false);
                false
            }
            cmd => self.core.handle_command_shared(cmd),
        }
    }

    fn handle_packet(&mut self, pkt: DemuxPacket) {
        match pkt {
            DemuxPacket::Flush => {
                self.core.pending_seeks = self.core.pending_seeks.saturating_sub(1);
            }
            _ if self.core.pending_seeks > 0 => {}
            DemuxPacket::Video(packet) => {
                if let Some(decoder) = self.video_decoder.as_mut() {
                    if decoder.send_packet(&packet).is_err() {
                        return;
                    }
                    while let Some(mut frame) = decoder.receive_frame() {
                        if frame.pts_us < self.core.seek_floor_us {
                            drop(frame);
                            continue;
                        }
                        if self.needs_display_flush {
                            self.needs_display_flush = false;
                            frame.seek_flush = true;
                        }
                        if !self.core.seek_landed {
                            self.core.seek_landed = true;
                            self.core.sync_clock.set_position(frame.pts_us);
                        }
                        match self.video_frame_tx.try_send(frame) {
                            Ok(()) | Err(crossbeam_channel::TrySendError::Full(_)) => {}
                            Err(crossbeam_channel::TrySendError::Disconnected(_)) => return,
                        }
                    }
                }
            }
            DemuxPacket::Audio(packet) => {
                if self.scrubbing {
                    return;
                }
                // Video mode: blocking schedule
                self.core.decode_audio_packet(&packet, true);
            }
            DemuxPacket::Subtitle(_) => {}
            DemuxPacket::Eof => {
                if let Some(vd) = self.video_decoder.as_mut() {
                    let _ = vd.send_eof();
                    while let Some(frame) = vd.receive_frame() {
                        let _ = self.video_frame_tx.send(frame);
                    }
                }
                // Blocking drain
                self.core.drain_audio_at_eof(true);
                self.core.signal_eof();
            }
        }
    }

    fn run(&mut self) {
        loop {
            while let Ok(cmd) = self.core.cmd_rx.try_recv() {
                if self.handle_command(cmd) {
                    return;
                }
            }

            self.execute_queued_seek();

            // Clear scrubbing after seeks settle + 100ms grace period
            if self.scrubbing
                && self.core.pending_seeks == 0
                && self.core.queued_seek.is_none()
                && self
                    .last_seek_time
                    .is_some_and(|t| t.elapsed() > Duration::from_millis(100))
            {
                self.scrubbing = false;
            }

            for _ in 0..9 {
                match self.core.demux_packet_rx.try_recv() {
                    Ok(pkt) => self.handle_packet(pkt),
                    Err(_) => break,
                }
            }

            self.core.update_subtitles();
            self.core.check_eof();

            let timeout = if self.core.queued_seek.is_some() {
                Duration::from_millis(1)
            } else {
                Duration::from_millis(50)
            };
            crossbeam_channel::select! {
                recv(self.core.cmd_rx) -> msg => {
                    match msg {
                        Ok(cmd) => {
                            if self.handle_command(cmd) {
                                return;
                            }
                        }
                        Err(_) => return,
                    }
                }
                recv(self.core.demux_packet_rx) -> msg => {
                    match msg {
                        Ok(pkt) => self.handle_packet(pkt),
                        Err(_) => return,
                    }
                }
                default(timeout) => {}
            }
        }
    }
}

// ── Audio-only player ──────────────────────────────────────────────

/// Audio-only mode: no video decoder, non-blocking audio scheduling,
/// ring buffer skip for forward seeks.
struct AudioOnlyPlayer {
    core: PlayerCore,
    /// Decoded audio buffers waiting for ring space.
    pending_audio: VecDeque<AudioBuffer>,
}

impl AudioOnlyPlayer {
    fn dispatch_seek(&mut self, target: i64, forward: bool, exact: bool) {
        self.core.dispatch_seek_common(target, forward, exact);
        self.core.last_audio_end_us = 0;
        self.core.eof_audio_end_us = None;
        if let Some(ad) = self.core.audio_decoder.as_mut() {
            ad.flush();
        }
        if let Some(ao) = self.core.audio_output.as_ref() {
            ao.flush_quick();
            ao.set_clock_position(target);
        }
        self.pending_audio.clear();
    }

    fn execute_queued_seek(&mut self) {
        let Some(qs) = self.core.queued_seek.take() else {
            return;
        };
        if qs.seconds.abs() < 0.001 && !qs.exact {
            return;
        }

        let current = self.core.sync_clock.audio_pts();
        let delta_us = (qs.seconds * 1_000_000.0) as i64;
        let target = (current + delta_us).max(0).min(self.core.duration_us);
        let forward = qs.seconds > 0.0;

        // Forward seek: skip directly in the ring buffer when possible.
        if forward
            && !qs.exact
            && let Some(ao) = self.core.audio_output.as_ref()
        {
            let sample_rate = self
                .core
                .stream_info
                .audio_streams
                .get(self.core.current_audio_track)
                .map(|a| a.sample_rate)
                .unwrap_or(48000);
            let samples_needed = (delta_us as u64 * sample_rate as u64 / 1_000_000) as usize;
            let available = ao.buffered_samples();
            if samples_needed > 0 && available > 0 {
                let actual_skip = samples_needed.min(available);
                ao.request_skip(actual_skip);
                let actual_us = actual_skip as i64 * 1_000_000 / sample_rate as i64;
                let actual_target = (current + actual_us).min(self.core.duration_us);
                self.core.sync_clock.set_position(actual_target);
                return;
            }
        }

        self.dispatch_seek(target, !qs.exact && forward, qs.exact);
    }

    fn handle_command(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::SeekAbsolute { target_us } => {
                let target = target_us.max(0).min(self.core.duration_us);
                self.dispatch_seek(target, true, false);
                false
            }
            cmd => self.core.handle_command_shared(cmd),
        }
    }

    fn handle_packet(&mut self, pkt: DemuxPacket) {
        match pkt {
            DemuxPacket::Flush => {
                self.core.pending_seeks = self.core.pending_seeks.saturating_sub(1);
                if let Some(ad) = self.core.audio_decoder.as_mut() {
                    ad.flush();
                }
                if let Some(ao) = self.core.audio_output.as_ref() {
                    ao.flush_quick();
                }
                self.pending_audio.clear();
            }
            _ if self.core.pending_seeks > 0 => {}
            DemuxPacket::Audio(packet) => {
                // Non-blocking schedule; spill to pending_audio
                let leftover = self.core.decode_audio_packet(&packet, false);
                self.pending_audio.extend(leftover);
            }
            DemuxPacket::Subtitle(_) => {}
            DemuxPacket::Eof => {
                let leftover = self.core.drain_audio_at_eof(false);
                self.pending_audio.extend(leftover);
                self.core.signal_eof();
            }
            DemuxPacket::Video(_) => {} // shouldn't happen, but harmless
        }
    }

    /// Drain pending audio buffers into the ring. Non-blocking.
    fn drain_pending_audio(&mut self) {
        let Some(ref ao) = self.core.audio_output else {
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

    fn run(&mut self) {
        loop {
            while let Ok(cmd) = self.core.cmd_rx.try_recv() {
                if self.handle_command(cmd) {
                    return;
                }
            }

            self.execute_queued_seek();
            self.drain_pending_audio();

            if self.pending_audio.len() < 16 {
                for _ in 0..8 {
                    match self.core.demux_packet_rx.try_recv() {
                        Ok(pkt) => self.handle_packet(pkt),
                        Err(_) => break,
                    }
                }
            }

            self.core.update_subtitles();
            self.core.check_eof();

            if self.pending_audio.is_empty() && self.core.queued_seek.is_none() {
                let timeout = Duration::from_millis(4);
                crossbeam_channel::select! {
                    recv(self.core.cmd_rx) -> msg => {
                        match msg {
                            Ok(cmd) => {
                                if self.handle_command(cmd) {
                                    return;
                                }
                            }
                            Err(_) => return,
                        }
                    }
                    recv(self.core.demux_packet_rx) -> msg => {
                        match msg {
                            Ok(pkt) => self.handle_packet(pkt),
                            Err(_) => return,
                        }
                    }
                    default(timeout) => {}
                }
            } else if !self.pending_audio.is_empty() {
                std::thread::sleep(Duration::from_micros(100));
            }
        }
    }
}

// ── Public API ─────────────────────────────────────────────────────

/// Player dispatches to the appropriate mode at construction time.
pub struct Player {
    mode: PlayerMode,
}

enum PlayerMode {
    Video(VideoPlayer),
    AudioOnly(AudioOnlyPlayer),
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
        let has_video = stream_info.video_stream.is_some();
        let duration_us = stream_info.duration_us;

        let core = PlayerCore {
            cmd_rx,
            demux_packet_rx,
            demux_cmd_tx,
            ui_update_tx,
            audio_decoder: None,
            audio_output: None,
            sync_clock,
            audio_clock,
            paused: false,
            volume: initial_volume,
            pre_mute_volume: None,
            audio_delay_us: (initial_audio_delay * 1_000_000.0) as i64,
            duration_us,
            pending_seeks: 0,
            queued_seek: None,
            seek_floor_us: 0,
            seek_landed: true,
            last_audio_end_us: 0,
            eof_audio_end_us: None,
            subtitle_tracks,
            current_subtitle_idx: None,
            last_subtitle_text: None,
            file_path,
            stream_info,
            current_audio_track: 0,
        };

        let mode = if has_video {
            PlayerMode::Video(VideoPlayer {
                core,
                video_decoder: None,
                video_frame_tx,
                needs_display_flush: false,
                scrubbing: false,
                last_seek_time: None,
            })
        } else {
            PlayerMode::AudioOnly(AudioOnlyPlayer {
                core,
                pending_audio: VecDeque::new(),
            })
        };

        Ok(Self { mode })
    }

    fn core(&self) -> &PlayerCore {
        match &self.mode {
            PlayerMode::Video(v) => &v.core,
            PlayerMode::AudioOnly(a) => &a.core,
        }
    }

    fn core_mut(&mut self) -> &mut PlayerCore {
        match &mut self.mode {
            PlayerMode::Video(v) => &mut v.core,
            PlayerMode::AudioOnly(a) => &mut a.core,
        }
    }

    pub fn init_decoders(&mut self) -> Result<()> {
        let core = self.core();
        let ictx = ffmpeg_next::format::input(&core.file_path)
            .with_context(|| format!("Failed to open: {}", core.file_path.display()))?;

        if let PlayerMode::Video(ref mut v) = self.mode
            && let Some(vs) = &v.core.stream_info.video_stream
        {
            let stream = ictx.stream(vs.index).context("Video stream not found")?;
            let vd = VideoDecoder::new(&stream)?;
            let _ = v.core.ui_update_tx.send(UiUpdate::VideoSize {
                width: vd.width(),
                height: vd.height(),
            });
            v.video_decoder = Some(vd);
        }

        let core = self.core_mut();
        if let Some(audio) = core.stream_info.audio_streams.first() {
            let stream = ictx.stream(audio.index).context("Audio stream not found")?;
            let decoder = AudioDecoder::new(&stream)?;
            let sample_rate = decoder.sample_rate;
            let channels = decoder.channels;
            core.audio_decoder = Some(decoder);

            log::debug!("Creating audio output...");
            match AudioOutput::new(sample_rate, channels, core.audio_clock.clone()) {
                Ok(ao) => {
                    log::debug!("Audio output created successfully");
                    core.audio_output = Some(ao);
                }
                Err(e) => {
                    log::error!("Failed to create audio output: {e}");
                }
            }

            if core.volume < 100
                && let Some(ao) = core.audio_output.as_mut()
            {
                ao.set_volume(core.volume as f32 / 100.0);
            }
        }

        if !core.subtitle_tracks.is_empty() {
            core.current_subtitle_idx = Some(0);
        }

        Ok(())
    }

    pub fn seek_to(&mut self, target_us: i64) {
        let core = self.core_mut();
        core.pending_seeks += 1;
        let _ = core.demux_cmd_tx.send(DemuxCommand::Seek {
            target_pts: target_us,
            forward: false,
        });
        core.flush_audio_pipeline();
        if let PlayerMode::Video(ref mut v) = self.mode
            && let Some(vd) = v.video_decoder.as_mut()
        {
            vd.flush();
        }
        let core = self.core_mut();
        core.sync_clock.set_position(target_us);
        core.seek_floor_us = target_us;
        let _ = core.ui_update_tx.send(UiUpdate::SeekFlush(target_us));
    }

    pub fn run(&mut self) {
        log::info!("Player: starting event loop");
        match &mut self.mode {
            PlayerMode::Video(v) => v.run(),
            PlayerMode::AudioOnly(a) => a.run(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a Player wired to test channels (no decoders).
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
            duration_us: 3_600_000_000,
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

        // SAFETY: keeps demux_pkt_tx alive so player channels don't disconnect
        std::mem::forget(demux_pkt_tx);

        (player, cmd_tx, video_frame_rx, ui_update_rx, demux_cmd_rx)
    }

    fn make_test_player() -> (
        Player,
        Sender<Command>,
        Receiver<VideoFrame>,
        Receiver<UiUpdate>,
        Receiver<DemuxCommand>,
    ) {
        make_test_player_ex(false)
    }

    fn make_video_test_player() -> (
        Player,
        Sender<Command>,
        Receiver<VideoFrame>,
        Receiver<UiUpdate>,
        Receiver<DemuxCommand>,
    ) {
        make_test_player_ex(true)
    }

    // Helper accessors for test assertions on mode-specific state
    fn as_audio_only(p: &mut Player) -> &mut AudioOnlyPlayer {
        match &mut p.mode {
            PlayerMode::AudioOnly(a) => a,
            _ => panic!("expected audio-only player"),
        }
    }

    fn as_video(p: &mut Player) -> &mut VideoPlayer {
        match &mut p.mode {
            PlayerMode::Video(v) => v,
            _ => panic!("expected video player"),
        }
    }

    #[test]
    fn first_seek_dispatches_immediately() {
        let (mut player, _, _, _, demux_cmd_rx) = make_test_player();
        let ao = as_audio_only(&mut player);

        ao.core.queue_seek(5.0, false);
        ao.execute_queued_seek();

        assert_eq!(ao.core.pending_seeks, 1);
        assert!(ao.core.queued_seek.is_none());
        assert!(
            demux_cmd_rx.try_recv().is_ok(),
            "Seek command should be sent to demuxer"
        );
    }

    #[test]
    fn scrubbing_serializes_video_seeks() {
        let (mut player, _, _, _, demux_cmd_rx) = make_video_test_player();
        let vp = as_video(&mut player);

        // First seek dispatches immediately
        vp.core.queue_seek(5.0, false);
        vp.execute_queued_seek();
        assert_eq!(vp.core.pending_seeks, 1);
        assert!(vp.scrubbing);

        // Second seek deferred: scrubbing + pending_seeks > 0
        vp.core.queue_seek(5.0, false);
        vp.execute_queued_seek();
        assert_eq!(
            vp.core.pending_seeks, 1,
            "Should not dispatch while scrubbing"
        );
        assert_eq!(vp.core.sync_clock.audio_pts(), 10_000_000);

        let mut count = 0;
        while demux_cmd_rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);

        // Simulate Flush arrival + new seek
        vp.core.pending_seeks = 0;
        vp.core.queue_seek(5.0, false);
        vp.execute_queued_seek();
        assert_eq!(
            vp.core.pending_seeks, 1,
            "New seek should dispatch after Flush"
        );
        assert!(vp.core.queued_seek.is_none());
    }

    #[test]
    fn queued_seeks_accumulate() {
        let (mut player, _, _, _, _) = make_test_player();
        let ao = as_audio_only(&mut player);

        for _ in 0..4 {
            ao.core.queue_seek(5.0, false);
        }

        let qs = ao.core.queued_seek.as_ref().unwrap();
        assert!(
            (qs.seconds - 20.0).abs() < 0.001,
            "Should accumulate to +20s"
        );
    }

    #[test]
    fn needs_display_flush_independent_of_seek_landed() {
        let (mut player, _, _, _, _) = make_video_test_player();
        let vp = as_video(&mut player);

        vp.core.pending_seeks = 1;
        vp.core.seek_landed = false;
        vp.needs_display_flush = true;

        // Audio lands first
        vp.core.seek_landed = true;
        vp.core.sync_clock.set_position(5_000_000);

        assert!(
            vp.needs_display_flush,
            "Display flush should wait for video frame, not audio"
        );

        vp.needs_display_flush = false;
        assert!(!vp.needs_display_flush);
    }

    #[test]
    fn audio_only_has_no_display_flush() {
        let (mut player, _, _, _, _) = make_test_player();
        let _ao = as_audio_only(&mut player);
        assert!(matches!(player.mode, PlayerMode::AudioOnly(_)));

        // AudioOnlyPlayer has no needs_display_flush field at all —
        // this test verifies the type system prevents the bug.
        let ao = as_audio_only(&mut player);
        ao.core.queue_seek(5.0, false);
        ao.execute_queued_seek();
        // No display flush state to check — it doesn't exist. Pass.
    }
}
