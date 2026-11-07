use crate::clock::{judge_frame, ClockMode, FrameVerdict, MasterClock};
use crate::state::{Event, Machine, SeekResume, State};
use crate::sub::EngineSub;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    Audio,
    Video,
    Subtitle,
}

#[derive(Clone, Debug)]
pub struct Packet {
    pub stream: usize,
    pub kind: StreamKind,
    pub pts: Option<f64>,
    pub duration: Option<f64>,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct AudioChunk {
    pub pts: f64,
    pub stream: usize,
    pub channels: u16,
    pub rate: u32,
    pub samples: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct DecodedFrame {
    pub pts: f64,
    pub id: u64,
}

#[derive(Debug)]
pub enum DemuxEvent {
    Packet(Packet),
    Eof,
    Error(String),
}

pub trait Demuxer {
    fn has_audio(&self) -> bool;
    fn has_video(&self) -> bool;
    fn read(&mut self) -> DemuxEvent;
    fn seek(&mut self, seconds: f64);
    fn select_audio(&mut self, stream: usize);
    fn deselect_audio(&mut self) {}
    fn selected_audio(&self) -> usize;
    fn audio_info(&self, _stream: usize) -> Option<player_media::StreamInfo> {
        None
    }
}

pub trait AudioDecoder {
    fn send(&mut self, packet: Packet) -> Result<(), String>;
    fn drain(&mut self) -> Result<Vec<AudioChunk>, String>;
    fn finish(&mut self) -> Result<Vec<AudioChunk>, String>;
    fn flush(&mut self);
    fn reconfigure(&mut self, _info: &player_media::StreamInfo) -> Result<(), String> {
        Ok(())
    }
}

pub trait VideoDecoder {
    fn send(&mut self, packet: Packet) -> Result<(), String>;
    fn drain(&mut self) -> Result<Vec<DecodedFrame>, String>;
    fn finish(&mut self) -> Result<Vec<DecodedFrame>, String>;
    fn flush(&mut self);
    fn release(&mut self, _id: u64) {}
    fn decoded(&self) -> u64 {
        0
    }
    fn evicted(&self) -> u64 {
        0
    }
    fn undrained(&self) -> u64 {
        0
    }
}

pub trait AudioOutput {
    fn push(&mut self, chunk: AudioChunk) -> Result<(), String>;
    fn queued_seconds(&self) -> f64;
    fn device_position(&self) -> f64;
    fn set_paused(&mut self, paused: bool);
    fn flush(&mut self);
    fn finish(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn drained(&self) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentOutcome {
    Delivered,
    Missing,
    Discarded,
}

pub trait VideoOutput {
    fn present(&mut self, pts: f64, id: u64) -> Result<PresentOutcome, String>;
}

#[derive(Clone, Debug)]
pub enum Command {
    Pause(bool),
    Seek(f64, SeekPrecision, u64),
    SetSpeed(f64),
    SwitchAudio(usize),
    DisableAudio,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekPrecision {
    Exact,
    Keyframe,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub decoded: u64,
    pub delivered: u64,
    pub dropped_late: u64,
    pub evicted: u64,
    pub missing: u64,
    pub discarded: u64,
    pub flushed: u64,
    pub frames_pending: u64,
    pub audio_chunks: u64,
    pub seeks_completed: u64,
    pub seek_generation: u64,
    pub seek_completed_generation: u64,
    pub commands_ignored: u64,
    pub failures: u64,
    pub steps: u64,
    pub last_presented_pts_ns: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct InFlightSeek {
    generation: u64,
    target: f64,
}

const PUMP_PACKETS_PER_STEP: usize = 8;
const SEEK_PUMP_LIMIT: usize = 16384;
const SEEK_FRAME_TOLERANCE: f64 = 0.04;
const SEEK_AUDIO_PRIMING: f64 = 0.2;
const SEEK_PREROLL_CHUNKS: u64 = 8;
const SUBTITLE_PRIME_WINDOW: f64 = 30.0;
const SUBTITLE_PRIME_PACKETS: usize = 8192;
const AUDIO_HIGH_WATER: f64 = 0.12;
const AUDIO_LOW_WATER: f64 = AUDIO_HIGH_WATER * 0.5;
const PREROLL_AUDIO_LIMIT: f64 = 3.0;
const AUDIO_BACKLOG_LIMIT: f64 = 1.0;

pub struct Scheduler<D, A, V, AO, VO> {
    machine: Machine,
    clock: MasterClock,
    demux: D,
    audio: A,
    video: V,
    audio_out: AO,
    video_out: VO,
    commands: Vec<Command>,
    pending_frames: std::collections::VecDeque<DecodedFrame>,
    audio_backlog: std::collections::VecDeque<AudioChunk>,
    startup_audio: std::collections::VecDeque<AudioChunk>,
    prerolling_video: bool,
    audio_cursor: f64,
    video_cursor: f64,
    eof_seen: bool,
    stats: Stats,
    failure: Option<String>,
    subs: Vec<EngineSub>,
    selected_sub: Option<usize>,
    subs_visible: bool,
    audio_disabled: bool,
    sub_cursor: f64,
    in_flight_seek: Option<InFlightSeek>,
}

impl<D, A, V, AO, VO> Scheduler<D, A, V, AO, VO>
where
    D: Demuxer,
    A: AudioDecoder,
    V: VideoDecoder,
    AO: AudioOutput,
    VO: VideoOutput,
{
    pub fn new(demux: D, audio: A, video: V, audio_out: AO, video_out: VO) -> Self {
        let mode = if demux.has_audio() {
            ClockMode::AudioDriven
        } else {
            ClockMode::SystemDriven
        };
        Scheduler {
            machine: Machine::new(),
            clock: MasterClock::new(mode),
            demux,
            audio,
            video,
            audio_out,
            video_out,
            commands: Vec::new(),
            pending_frames: std::collections::VecDeque::new(),
            audio_backlog: std::collections::VecDeque::new(),
            startup_audio: std::collections::VecDeque::new(),
            prerolling_video: false,
            audio_cursor: 0.0,
            video_cursor: 0.0,
            eof_seen: false,
            stats: Stats::default(),
            failure: None,
            subs: Vec::new(),
            selected_sub: None,
            subs_visible: true,
            audio_disabled: false,
            sub_cursor: 0.0,
            in_flight_seek: None,
        }
    }

    pub fn send(&mut self, command: Command) {
        if let Command::Seek(_, _, generation) = &command {
            self.stats.seek_generation = *generation;
            if matches!(self.commands.last(), Some(Command::Seek(..))) {
                *self.commands.last_mut().expect("seek already queued") = command;
                return;
            }
        }
        self.commands.push(command);
    }

    pub fn enable_video_preroll(&mut self) {
        self.prerolling_video = self.demux.has_video();
    }

    pub fn state(&self) -> State {
        *self.machine.state()
    }

    pub fn stats(&self) -> Stats {
        Stats {
            decoded: self.video.decoded(),
            evicted: self.video.evicted(),
            frames_pending: self.pending_frames.len() as u64 + self.video.undrained(),
            ..self.stats
        }
    }

    fn present_frame(&mut self, frame: DecodedFrame) -> Result<(), String> {
        let pts = frame.pts;
        match self.video_out.present(frame.pts, frame.id)? {
            PresentOutcome::Delivered => self.stats.delivered += 1,
            PresentOutcome::Missing => self.stats.missing += 1,
            PresentOutcome::Discarded => self.stats.discarded += 1,
        }
        self.stats.last_presented_pts_ns = Some((pts * 1e9).round() as i64);
        if self
            .in_flight_seek
            .is_some_and(|seek| pts >= seek.target - SEEK_FRAME_TOLERANCE)
        {
            self.complete_seek();
        }
        Ok(())
    }

    fn complete_seek(&mut self) {
        if let Some(seek) = self.in_flight_seek.take() {
            self.stats.seek_completed_generation = seek.generation;
        }
    }

    fn note_audio_resumed(&mut self) {
        if self.in_flight_seek.is_some()
            && !self.demux.has_video()
            && self.audio_out.queued_seconds() > 0.0
        {
            self.complete_seek();
        }
    }

    fn drop_late(&mut self, frame: DecodedFrame) {
        self.video.release(frame.id);
        self.stats.dropped_late += 1;
    }

    fn flush_video(&mut self) {
        let outstanding = self.pending_frames.len() as u64 + self.video.undrained();
        self.stats.flushed += outstanding;
        self.video.flush();
        self.pending_frames.clear();
    }

    pub fn speed(&self) -> f64 {
        self.clock.speed()
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn demux(&self) -> &D {
        &self.demux
    }

    pub fn set_subs(&mut self, subs: Vec<EngineSub>) {
        self.subs = subs;
        for sub in &mut self.subs {
            sub.set_visible(self.subs_visible);
        }
        self.selected_sub = None;
        self.sub_cursor = 0.0;
    }

    pub fn subs(&self) -> &[EngineSub] {
        &self.subs
    }

    pub fn selected_sub(&self) -> Option<usize> {
        self.selected_sub
    }

    pub fn select_sub(&mut self, selected: Option<usize>) {
        if selected != self.selected_sub {
            self.selected_sub = selected.filter(|index| *index < self.subs.len());
            self.sub_cursor = 0.0;
        }
    }

    pub fn add_sub(&mut self, sub: EngineSub) -> usize {
        let position = self.subs.len();
        let mut sub = sub;
        sub.set_visible(self.subs_visible);
        self.subs.push(sub);
        self.select_sub(Some(position));
        position
    }

    pub fn set_sub_visible(&mut self, visible: bool) {
        self.subs_visible = visible;
        for sub in &mut self.subs {
            sub.set_visible(visible);
        }
    }

    fn selected_sub_stream(&self) -> Option<usize> {
        self.selected_sub
            .and_then(|index| self.subs.get(index))
            .and_then(|sub| sub.source().stream)
    }

    fn feed_sub(&mut self, packet: Packet) {
        let pts = stamp(packet.pts, &mut self.sub_cursor);
        let duration = packet.duration.unwrap_or(0.0).max(0.0);
        if let Some(sub) = self.selected_sub.and_then(|index| self.subs.get_mut(index)) {
            sub.feed(&packet.payload, pts, duration);
        }
    }

    pub fn clock_position(&self, now: f64) -> f64 {
        self.clock.position(now)
    }

    pub fn subtitle_text(&self, now: f64) -> String {
        let position_ms = (self.clock.position(now) * 1000.0).round() as i64;
        self.selected_sub
            .and_then(|index| self.subs.get(index))
            .map(|sub| sub.text_at(position_ms))
            .unwrap_or_default()
    }

    pub fn audio_output(&self) -> &AO {
        &self.audio_out
    }

    pub fn audio_output_mut(&mut self) -> &mut AO {
        &mut self.audio_out
    }

    pub fn video_output(&self) -> &VO {
        &self.video_out
    }

    #[cfg(test)]
    pub fn test_handles(&mut self) -> (&mut D, &mut A, &mut V, &mut VO) {
        (
            &mut self.demux,
            &mut self.audio,
            &mut self.video,
            &mut self.video_out,
        )
    }

    pub fn open(&mut self) {
        let _ = self.machine.apply(Event::Open);
        let _ = self.machine.apply(Event::Opened);
        let _ = self.machine.apply(Event::Buffered);
    }

    pub fn step(&mut self, now: f64) {
        self.stats.steps += 1;
        self.run_commands(now);
        self.observe(now);
        match *self.machine.state() {
            State::Playing => self.pump_playing(now),
            State::Draining => self.pump_draining(now),
            _ => self.observe(now),
        }
    }

    fn fail(&mut self, message: String) {
        self.stats.failures += 1;
        self.failure = Some(message.clone());
        self.machine.set_error(message);
        let _ = self.machine.apply(Event::Fail);
    }

    fn run_commands(&mut self, now: f64) {
        let commands = std::mem::take(&mut self.commands);
        for command in commands {
            match command {
                Command::Pause(pause) => {
                    let event = if pause { Event::Pause } else { Event::Resume };
                    if self.machine.apply(event).is_ok() {
                        self.clock.set_paused(pause, now);
                        self.audio_out.set_paused(pause);
                    } else {
                        self.stats.commands_ignored += 1;
                    }
                }
                Command::Seek(target, precision, generation) => {
                    self.command_seek(target, precision, generation, now)
                }
                Command::SetSpeed(speed) => {
                    if matches!(
                        *self.machine.state(),
                        State::Playing | State::Paused { .. } | State::Seeking { .. }
                    ) {
                        self.clock.set_speed(speed, now);
                    } else {
                        self.stats.commands_ignored += 1;
                    }
                }
                Command::SwitchAudio(stream) => {
                    if matches!(
                        *self.machine.state(),
                        State::Playing | State::Paused { .. }
                    ) {
                        let info = self.demux.audio_info(stream);
                        match info {
                            Some(info) if self.audio.reconfigure(&info).is_ok() => {
                                self.demux.select_audio(stream);
                                if self.audio_disabled {
                                    self.audio_disabled = false;
                                    self.clock.use_audio_time(now);
                                }
                                self.audio.flush();
                                self.audio_out.flush();
                                self.audio_backlog.clear();
                                self.audio_cursor = self.clock.position(now);
                            }
                            _ => self.stats.commands_ignored += 1,
                        }
                    } else {
                        self.stats.commands_ignored += 1;
                    }
                }
                Command::DisableAudio => {
                    if matches!(
                        *self.machine.state(),
                        State::Playing | State::Paused { .. }
                    ) {
                        self.audio_disabled = true;
                        self.demux.deselect_audio();
                        self.audio.flush();
                        self.audio_out.flush();
                        self.audio_backlog.clear();
                        self.startup_audio.clear();
                        self.clock.use_system_time(now);
                    } else {
                        self.stats.commands_ignored += 1;
                    }
                }
                Command::Close => {
                    self.flush_video();
                    let _ = self.machine.apply(Event::Close);
                    let _ = self.machine.apply(Event::Closed);
                }
            }
        }
    }

    fn command_seek(&mut self, target: f64, precision: SeekPrecision, generation: u64, now: f64) {
        if self.machine.apply(Event::Seek).is_err() {
            self.stats.commands_ignored += 1;
            return;
        }
        self.in_flight_seek = None;
        let resume = match *self.machine.state() {
            State::Seeking { resume } => resume,
            _ => SeekResume::Playing,
        };
        self.audio.flush();
        self.flush_video();
        if let Some(stream) = self.selected_sub_stream() {
            if let Err(message) = self.prime_subtitles(stream, target) {
                self.fail(message);
                return;
            }
        }
        self.demux.seek(target);
        self.audio_out.flush();
        self.audio_backlog.clear();
        self.startup_audio.clear();
        self.prerolling_video = false;
        self.audio_cursor = target;
        self.video_cursor = target;
        self.eof_seen = false;
        self.clock.seek_rebase(target, now);

        let mut target = target;
        let mut keyframe_pending = precision == SeekPrecision::Keyframe && self.demux.has_video();
        let mut have_audio = !self.demux.has_audio();
        let mut have_video = !self.demux.has_video();
        let preroll_start = Instant::now();
        for _ in 0..SEEK_PUMP_LIMIT {
            if have_audio && have_video {
                break;
            }
            if have_video && self.audio_backlog.len() >= SEEK_PREROLL_CHUNKS as usize {
                break;
            }
            match self.demux.read() {
                DemuxEvent::Packet(packet) => {
                    if keyframe_pending && packet.kind == StreamKind::Video {
                        keyframe_pending = false;
                        if let Some(pts) = packet.pts {
                            target = pts.max(0.0);
                            self.audio_cursor = target;
                            self.video_cursor = target;
                        }
                    }
                    if let Err(message) =
                        self.route(packet, target, &mut have_audio, &mut have_video)
                    {
                        self.fail(message);
                        return;
                    }
                    if have_audio && have_video {
                        break;
                    }
                }
                DemuxEvent::Eof => {
                    self.eof_seen = true;
                    break;
                }
                DemuxEvent::Error(message) => {
                    self.fail(message);
                    return;
                }
            }
        }
        self.stats.seeks_completed += 1;
        self.in_flight_seek = Some(InFlightSeek { generation, target });
        self.clock
            .seek_rebase(target, now + preroll_start.elapsed().as_secs_f64());
        let _ = self.machine.apply(Event::SeekDone);
        if matches!(self.machine.state(), State::Paused { .. }) {
            if let Err(message) = self.present_paused(now) {
                self.fail(message);
                return;
            }
            self.complete_seek();
        }
        if self.eof_seen {
            self.complete_seek();
        }
        if self.eof_seen && resume == SeekResume::Playing {
            let _ = self.machine.apply(Event::Eof);
        }
    }

    fn present_paused(&mut self, now: f64) -> Result<(), String> {
        let position = self.clock.position(now);
        let chosen = self
            .pending_frames
            .iter()
            .position(|frame| frame.pts > position + 0.001)
            .map(|index| index.saturating_sub(1))
            .unwrap_or_else(|| self.pending_frames.len().saturating_sub(1));
        if self.pending_frames.is_empty() {
            return Ok(());
        }
        let stale: Vec<DecodedFrame> = self.pending_frames.drain(..chosen).collect();
        for frame in stale {
            self.drop_late(frame);
        }
        let frame = self.pending_frames.pop_front().expect("chosen frame");
        self.present_frame(frame)
    }

    fn startup_audio_seconds(&self) -> f64 {
        self.startup_audio
            .iter()
            .map(chunk_duration)
            .sum()
    }

    fn begin_playback(&mut self, now: f64) -> Result<(), String> {
        self.prerolling_video = false;
        self.audio_backlog.extend(std::mem::take(&mut self.startup_audio));
        if self.feed_output() {
            return Ok(());
        }
        if let Some(frame) = self.pending_frames.pop_front() {
            let pts = frame.pts;
            self.present_frame(frame)?;
            self.clock.seek_rebase(pts, now);
        }
        Ok(())
    }

    fn prime_subtitles(&mut self, stream: usize, target: f64) -> Result<(), String> {
        let start = (target - SUBTITLE_PRIME_WINDOW).max(0.0);
        self.demux.seek(start);
        self.sub_cursor = start;
        let mut seen = 0usize;
        while seen < SUBTITLE_PRIME_PACKETS {
            match self.demux.read() {
                DemuxEvent::Packet(packet) => {
                    seen += 1;
                    if packet.kind == StreamKind::Subtitle && packet.stream == stream {
                        let pts = stamp(packet.pts, &mut self.sub_cursor);
                        if pts > target {
                            break;
                        }
                        self.feed_sub(packet);
                    }
                }
                DemuxEvent::Eof | DemuxEvent::Error(_) => break,
            }
        }
        Ok(())
    }

    fn route(
        &mut self,
        packet: Packet,
        target: f64,
        have_audio: &mut bool,
        have_video: &mut bool,
    ) -> Result<(), String> {
        match packet.kind {
            StreamKind::Audio if !self.audio_disabled && packet.stream == self.demux.selected_audio() => {
                let pts = stamp(packet.pts, &mut self.audio_cursor);
                if pts + packet.duration.unwrap_or(0.0) < target - SEEK_AUDIO_PRIMING {
                    return Ok(());
                }
                let mut packet = packet;
                packet.pts = Some(pts);
                self.audio.send(packet)?;
                for chunk in self.audio.finish()? {
                    let end = chunk.pts + chunk_duration(&chunk);
                    self.audio_cursor = end;
                    if end <= target {
                        continue;
                    }
                    *have_audio = true;
                    self.audio_backlog.push_back(chunk);
                }
            }
            StreamKind::Subtitle if self.selected_sub_stream() == Some(packet.stream) => {
                self.feed_sub(packet);
            }
            StreamKind::Video => {
                let pts = stamp(packet.pts, &mut self.video_cursor);
                let mut packet = packet;
                packet.pts = Some(pts);
                self.video.send(packet)?;
                for frame in self.video.drain()? {
                    self.video_cursor = self.video_cursor.max(frame.pts);
                    if frame.pts < target - SEEK_FRAME_TOLERANCE {
                        self.drop_late(frame);
                        continue;
                    }
                    *have_video = true;
                    self.pending_frames.push_back(frame);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn feed_output(&mut self) -> bool {
        while self.audio_out.queued_seconds() < AUDIO_HIGH_WATER {
            let Some(chunk) = self.audio_backlog.pop_front() else {
                break;
            };
            if let Err(message) = self.audio_out.push(chunk) {
                self.fail(message);
                return true;
            }
        }
        self.note_audio_resumed();
        false
    }

    fn backlog_seconds(&self) -> f64 {
        self.audio_backlog.iter().map(chunk_duration).sum()
    }

    fn video_wanted(&self, now: f64) -> bool {
        if !self.demux.has_video() {
            return false;
        }
        let position = self.clock.position(now);
        !self.pending_frames.iter().any(|frame| frame.pts > position)
    }

    fn has_active_audio(&self) -> bool {
        !self.audio_disabled
            && self.demux.has_audio()
            && self.demux.selected_audio() != usize::MAX
    }

    fn audio_wanted(&self) -> bool {
        self.has_active_audio()
            && self.audio_backlog.is_empty()
            && self.audio_out.queued_seconds() < AUDIO_HIGH_WATER
    }

    pub fn next_wakeup(&self, now: f64) -> Option<f64> {
        let state = *self.machine.state();
        if !matches!(state, State::Playing | State::Draining) {
            return None;
        }
        let position = self.clock.position(now);
        let speed = self.clock.speed();
        let mut wait = f64::INFINITY;
        if let Some(frame) = self.pending_frames.front() {
            if frame.pts <= position {
                return Some(0.0);
            }
            if speed > 0.0 {
                wait = (frame.pts - position) / speed;
            }
        }
        if self.prerolling_video {
            return Some(0.0);
        }
        if state == State::Playing && (self.audio_wanted() || self.video_wanted(now)) {
            return Some(0.0);
        }
        if self.has_active_audio() {
            let queued = self.audio_out.queued_seconds();
            wait = wait.min(if queued > AUDIO_LOW_WATER {
                queued - AUDIO_LOW_WATER
            } else {
                queued
            });
        }
        wait.is_finite().then_some(wait.max(0.0))
    }

    fn pump_playing(&mut self, now: f64) {
        if self.feed_output() {
            return;
        }
        let audio_wanted = self.audio_wanted();
        let backlog_full = self.backlog_seconds() >= AUDIO_BACKLOG_LIMIT;
        if !self.prerolling_video && (backlog_full || !(audio_wanted || self.video_wanted(now))) {
            self.present_due(now);
            self.observe(now);
            return;
        }
        for _ in 0..PUMP_PACKETS_PER_STEP {
            let audio_before = self.stats.audio_chunks;
            let video_before = self.pending_frames.len();
            match self.demux.read() {
                DemuxEvent::Packet(packet) => {
                    if self.feed(packet) {
                        return;
                    }
                }
                DemuxEvent::Eof => {
                    self.eof_seen = true;
                    if self.prerolling_video {
                        self.prerolling_video = false;
                        self.audio_backlog.extend(std::mem::take(&mut self.startup_audio));
                    }
                    match self.audio.finish() {
                        Ok(chunks) => {
                            for chunk in chunks {
                                self.audio_backlog.push_back(chunk);
                                self.stats.audio_chunks += 1;
                            }
                        }
                        Err(message) => {
                            self.fail(message);
                            return;
                        }
                    }
                    match self.video.finish() {
                        Ok(frames) => {
                            for frame in frames {
                                self.pending_frames.push_back(frame);
                            }
                        }
                        Err(message) => {
                            self.fail(message);
                            return;
                        }
                    }
                    let _ = self.machine.apply(Event::Eof);
                    self.pump_draining(now);
                    return;
                }
                DemuxEvent::Error(message) => {
                    self.fail(message);
                    return;
                }
            }
            if self.pending_frames.len() != video_before
                || (!self.demux.has_video() && self.stats.audio_chunks != audio_before)
            {
                break;
            }
        }
        if self.feed_output() {
            return;
        }
        if self.prerolling_video {
            if !self.pending_frames.is_empty()
                || self.startup_audio_seconds() >= PREROLL_AUDIO_LIMIT
            {
                if let Err(message) = self.begin_playback(now) {
                    self.fail(message);
                    return;
                }
            }
            self.observe(now);
            return;
        }
        self.present_due(now);
        self.observe(now);
    }

    fn feed(&mut self, packet: Packet) -> bool {
        match packet.kind {
            StreamKind::Audio if !self.audio_disabled && packet.stream == self.demux.selected_audio() => {
                let pts = stamp(packet.pts, &mut self.audio_cursor);
                let mut packet = packet;
                packet.pts = Some(pts);
                if let Err(message) = self.audio.send(packet) {
                    self.fail(message);
                    return true;
                }
                match self.audio.drain() {
                    Ok(chunks) => {
                        for chunk in chunks {
                            self.audio_cursor = chunk.pts + chunk_duration(&chunk);
                            if self.prerolling_video {
                                self.startup_audio.push_back(chunk);
                            } else {
                                self.audio_backlog.push_back(chunk);
                            }
                            self.stats.audio_chunks += 1;
                        }
                    }
                    Err(message) => {
                        self.fail(message);
                        return true;
                    }
                }
            }
            StreamKind::Video => {
                let pts = stamp(packet.pts, &mut self.video_cursor);
                let mut packet = packet;
                packet.pts = Some(pts);
                if let Err(message) = self.video.send(packet) {
                    self.fail(message);
                    return true;
                }
                match self.video.drain() {
                    Ok(frames) => {
                        for frame in frames {
                            self.video_cursor = self.video_cursor.max(frame.pts);
                            self.pending_frames.push_back(frame);
                        }
                    }
                    Err(message) => {
                        self.fail(message);
                        return true;
                    }
                }
            }
            StreamKind::Subtitle if self.selected_sub_stream() == Some(packet.stream) => {
                self.feed_sub(packet);
            }
            _ => {}
        }
        false
    }

    fn present_due(&mut self, now: f64) {
        let position = self.clock.position(now);
        while let Some(frame) = self.pending_frames.front() {
            match judge_frame(frame.pts, position) {
                FrameVerdict::Wait => break,
                FrameVerdict::Present => {
                    let frame = self.pending_frames.pop_front().expect("peeked");
                    if let Err(message) = self.present_frame(frame) {
                        self.fail(message);
                        return;
                    }
                }
                FrameVerdict::DropLate => {
                    let frame = self.pending_frames.pop_front().expect("peeked");
                    self.drop_late(frame);
                }
            }
        }
    }

    fn pump_draining(&mut self, now: f64) {
        match self.audio.drain() {
            Ok(chunks) => {
                for chunk in chunks {
                    self.audio_backlog.push_back(chunk);
                    self.stats.audio_chunks += 1;
                }
            }
            Err(message) => {
                self.fail(message);
                return;
            }
        }
        match self.video.drain() {
            Ok(frames) => {
                for frame in frames {
                    self.pending_frames.push_back(frame);
                }
            }
            Err(message) => {
                self.fail(message);
                return;
            }
        }
        if self.feed_output() {
            return;
        }
        if self.audio_backlog.is_empty()
            && let Err(message) = self.audio_out.finish()
        {
            self.fail(message);
            return;
        }
        let audio_drained = self.audio_backlog.is_empty() && self.audio_out.drained();
        if self.clock.is_audio_driven() && audio_drained && !self.pending_frames.is_empty() {
            self.clock.use_system_time(now);
        }
        self.present_due(now);
        self.observe(now);
        if self.pending_frames.is_empty() && audio_drained {
            let _ = self.machine.apply(Event::Drained);
        }
    }

    fn observe(&mut self, now: f64) {
        self.clock.observe_device(
            self.audio_out.device_position(),
            self.audio_out.queued_seconds(),
            now,
        );
    }
}

fn stamp(pts: Option<f64>, cursor: &mut f64) -> f64 {
    let stamped = pts.unwrap_or(*cursor);
    *cursor = stamped.max(*cursor);
    stamped
}

fn chunk_duration(chunk: &AudioChunk) -> f64 {
    if chunk.rate == 0 || chunk.channels == 0 {
        return 0.0;
    }
    chunk.samples.len() as f64 / (chunk.channels as f64 * chunk.rate as f64)
}
