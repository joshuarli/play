use crate::clock::{judge_frame, ClockMode, FrameVerdict, MasterClock};
use crate::state::{Event, Machine, SeekResume, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    Audio,
    Video,
}

#[derive(Clone, Debug)]
pub struct Packet {
    pub stream: usize,
    pub kind: StreamKind,
    pub pts: Option<f64>,
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
    fn selected_audio(&self) -> usize;
}

pub trait AudioDecoder {
    fn send(&mut self, packet: Packet) -> Result<(), String>;
    fn drain(&mut self) -> Result<Vec<AudioChunk>, String>;
    fn finish(&mut self) -> Result<Vec<AudioChunk>, String>;
    fn flush(&mut self);
}

pub trait VideoDecoder {
    fn send(&mut self, packet: Packet) -> Result<(), String>;
    fn drain(&mut self) -> Result<Vec<DecodedFrame>, String>;
    fn finish(&mut self) -> Result<Vec<DecodedFrame>, String>;
    fn flush(&mut self);
}

pub trait AudioOutput {
    fn push(&mut self, chunk: AudioChunk) -> Result<(), String>;
    fn queued_seconds(&self) -> f64;
    fn device_position(&self) -> f64;
    fn set_paused(&mut self, paused: bool);
    fn flush(&mut self);
    fn drained(&self) -> bool;
}

pub trait VideoOutput {
    fn present(&mut self, pts: f64, id: u64) -> Result<(), String>;
}

#[derive(Clone, Debug)]
pub enum Command {
    Pause(bool),
    Seek(f64),
    SetSpeed(f64),
    SwitchAudio(usize),
    Close,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub presented: u64,
    pub dropped_late: u64,
    pub audio_chunks: u64,
    pub seeks_completed: u64,
    pub commands_ignored: u64,
    pub failures: u64,
}

const PUMP_PACKETS_PER_STEP: usize = 64;
const SEEK_PUMP_LIMIT: usize = 1024;

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
    audio_cursor: f64,
    video_cursor: f64,
    eof_seen: bool,
    stats: Stats,
    failure: Option<String>,
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
            audio_cursor: 0.0,
            video_cursor: 0.0,
            eof_seen: false,
            stats: Stats::default(),
            failure: None,
        }
    }

    pub fn send(&mut self, command: Command) {
        self.commands.push(command);
    }

    pub fn state(&self) -> State {
        *self.machine.state()
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn clock_position(&self, now: f64) -> f64 {
        self.clock.position(now)
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
        self.run_commands(now);
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
                        self.audio_out.set_paused(pause);
                    } else {
                        self.stats.commands_ignored += 1;
                    }
                }
                Command::Seek(target) => self.command_seek(target, now),
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
                        self.demux.select_audio(stream);
                        self.audio.flush();
                        self.audio_cursor = self.clock.position(now);
                    } else {
                        self.stats.commands_ignored += 1;
                    }
                }
                Command::Close => {
                    let _ = self.machine.apply(Event::Close);
                    let _ = self.machine.apply(Event::Closed);
                }
            }
        }
    }

    fn command_seek(&mut self, target: f64, now: f64) {
        if self.machine.apply(Event::Seek).is_err() {
            self.stats.commands_ignored += 1;
            return;
        }
        let resume = match *self.machine.state() {
            State::Seeking { resume } => resume,
            _ => SeekResume::Playing,
        };
        self.demux.seek(target);
        self.audio.flush();
        self.video.flush();
        self.audio_out.flush();
        self.pending_frames.clear();
        self.audio_cursor = target;
        self.video_cursor = target;
        self.eof_seen = false;
        self.clock.seek_rebase(target, now);

        let mut have_audio = !self.demux.has_audio();
        let mut have_video = !self.demux.has_video();
        for _ in 0..SEEK_PUMP_LIMIT {
            if have_audio && have_video {
                break;
            }
            match self.demux.read() {
                DemuxEvent::Packet(packet) => {
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
        let _ = self.machine.apply(Event::SeekDone);
        if self.eof_seen && resume == SeekResume::Playing {
            let _ = self.machine.apply(Event::Eof);
        }
    }

    fn route(
        &mut self,
        packet: Packet,
        target: f64,
        have_audio: &mut bool,
        have_video: &mut bool,
    ) -> Result<(), String> {
        match packet.kind {
            StreamKind::Audio if packet.stream == self.demux.selected_audio() => {
                let pts = stamp(packet.pts, &mut self.audio_cursor);
                let mut packet = packet;
                packet.pts = Some(pts);
                self.audio.send(packet)?;
                for chunk in self.audio.finish()? {
                    self.audio_cursor = chunk.pts + chunk_duration(&chunk);
                    if chunk.pts >= target {
                        *have_audio = true;
                    }
                    self.audio_out.push(chunk)?;
                    self.stats.audio_chunks += 1;
                }
            }
            StreamKind::Video => {
                let pts = stamp(packet.pts, &mut self.video_cursor);
                let mut packet = packet;
                packet.pts = Some(pts);
                self.video.send(packet)?;
                for frame in self.video.finish()? {
                    self.video_cursor = self.video_cursor.max(frame.pts);
                    if frame.pts >= target {
                        *have_video = true;
                    }
                    self.pending_frames.push_back(frame);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn pump_playing(&mut self, now: f64) {
        for _ in 0..PUMP_PACKETS_PER_STEP {
            match self.demux.read() {
                DemuxEvent::Packet(packet) => {
                    if self.feed(packet) {
                        return;
                    }
                }
                DemuxEvent::Eof => {
                    self.eof_seen = true;
                    match self.audio.finish() {
                        Ok(chunks) => {
                            for chunk in chunks {
                                if let Err(message) = self.audio_out.push(chunk) {
                                    self.fail(message);
                                    return;
                                }
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
        }
        self.present_due(now);
        self.observe(now);
    }

    fn feed(&mut self, packet: Packet) -> bool {
        match packet.kind {
            StreamKind::Audio if packet.stream == self.demux.selected_audio() => {
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
                            if let Err(message) = self.audio_out.push(chunk) {
                                self.fail(message);
                                return true;
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
                    if let Err(message) = self.video_out.present(frame.pts, frame.id) {
                        self.fail(message);
                        return;
                    }
                    self.stats.presented += 1;
                }
                FrameVerdict::DropLate => {
                    self.pending_frames.pop_front();
                    self.stats.dropped_late += 1;
                }
            }
        }
    }

    fn pump_draining(&mut self, now: f64) {
        match self.audio.drain() {
            Ok(chunks) => {
                for chunk in chunks {
                    if let Err(message) = self.audio_out.push(chunk) {
                        self.fail(message);
                        return;
                    }
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
        self.present_due(now);
        self.observe(now);
        if self.pending_frames.is_empty() && self.audio_out.drained() {
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
