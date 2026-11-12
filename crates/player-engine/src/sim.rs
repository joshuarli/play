#[cfg(test)]
mod tests {
    use crate::sched::{SeekPrecision, 
        AudioChunk, AudioDecoder, AudioOutput, Command, DecodedFrame, DemuxEvent, Demuxer, Packet,
        PresentOutcome, Scheduler, Stats, StreamKind, VideoDecoder, VideoOutput,
    };
    use crate::state::State;
    use crate::sub::EngineSub;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    const MAX_QUEUED_FRAMES: usize = 64;

    struct FakeDemux {
        script: Vec<Packet>,
        cursor: usize,
        has_audio: bool,
        has_video: bool,
        audio_streams: usize,
        selected: usize,
        reads: usize,
        fail_at_read: Option<usize>,
        keyframe_interval: Option<f64>,
    }

    impl FakeDemux {
        fn with_keyframes(mut self, interval: f64) -> Self {
            self.keyframe_interval = Some(interval);
            self
        }

        fn av(duration: f64, fps: f64, audio_every: f64) -> Self {
            FakeDemux::build(duration, fps, audio_every, 1, false, false)
        }

        fn build(
            duration: f64,
            fps: f64,
            audio_every: f64,
            audio_streams: usize,
            reorder_video: bool,
            strip_pts: bool,
        ) -> Self {
            let mut video: Vec<Packet> = Vec::new();
            let mut index = 0usize;
            while fps > 0.0 && (index as f64) / fps < duration {
                let pts = if strip_pts {
                    None
                } else {
                    Some(index as f64 / fps)
                };
                video.push(Packet {
                    stream: 100,
                    kind: StreamKind::Video,
                    pts,
                    duration: None,
                    payload: vec![index as u8],
                });
                index += 1;
            }
            if reorder_video {
                for pair in video.as_chunks_mut::<2>().0.iter_mut() {
                    pair.swap(0, 1);
                }
            }
            let mut audio: Vec<Packet> = Vec::new();
            let mut at = 0.0;
            let mut tick = 0usize;
            while audio_every > 0.0 && at < duration {
                for stream in 0..audio_streams {
                    audio.push(Packet {
                        stream,
                        kind: StreamKind::Audio,
                        pts: if strip_pts { None } else { Some(at) },
                        duration: Some(audio_every),
                        payload: vec![tick as u8],
                    });
                }
                tick += 1;
                at += audio_every;
            }
            let mut script = Vec::with_capacity(video.len() + audio.len());
            let (mut vi, mut ai) = (0usize, 0usize);
            while vi < video.len() || ai < audio.len() {
                if ai < audio.len()
                    && (vi >= video.len()
                        || audio[ai].pts.unwrap_or(0.0) <= video[vi].pts.unwrap_or(0.0))
                {
                    script.push(audio[ai].clone());
                    ai += 1;
                } else {
                    script.push(video[vi].clone());
                    vi += 1;
                }
            }
            FakeDemux {
                script,
                cursor: 0,
                has_audio: audio_every > 0.0,
                has_video: fps > 0.0,
                audio_streams,
                selected: 0,
                reads: 0,
                fail_at_read: None,
                keyframe_interval: None,
            }
        }
    }

    impl Demuxer for FakeDemux {
        fn has_audio(&self) -> bool {
            self.has_audio
        }

        fn has_video(&self) -> bool {
            self.has_video
        }

        fn read(&mut self) -> DemuxEvent {
            self.reads += 1;
            if self.fail_at_read == Some(self.reads) {
                return DemuxEvent::Error("demux lost".to_string());
            }
            match self.script.get(self.cursor) {
                Some(packet) => {
                    self.cursor += 1;
                    DemuxEvent::Packet(packet.clone())
                }
                None => DemuxEvent::Eof,
            }
        }

        fn seek(&mut self, seconds: f64) {
            let seconds = match self.keyframe_interval {
                Some(interval) => (seconds / interval).floor() * interval,
                None => seconds,
            };
            self.cursor = self
                .script
                .iter()
                .position(|packet| packet.pts.unwrap_or(f64::NEG_INFINITY) >= seconds)
                .unwrap_or(self.script.len());
        }

        fn select_audio(&mut self, stream: usize) {
            self.selected = stream;
        }

        fn deselect_audio(&mut self) {
            self.selected = usize::MAX;
        }

        fn selected_audio(&self) -> usize {
            self.selected
        }

        fn audio_info(&self, stream: usize) -> Option<player_media::StreamInfo> {
            (stream < self.audio_streams).then(|| player_media::StreamInfo {
                index: stream,
                kind: player_media::StreamKind::Audio,
                codec_name: "fake".to_owned(), profile: -99,
                width: 0,
                height: 0,
                sample_rate: 48_000,
                channels: 2,
                time_base: (1, 48_000),
                duration_seconds: None,
                extradata: Vec::new(),
                channel_mask: 0,
                attached: false,
                default: false,
                forced: false,
                language: None,
                title: None,
            })
        }
    }

    struct FakeAudioDecoder {
        pending: VecDeque<AudioChunk>,
        sends: usize,
        fail_at_send: Option<usize>,
        chunk_seconds: f64,
    }

    impl FakeAudioDecoder {
        fn new(chunk_seconds: f64) -> Self {
            FakeAudioDecoder {
                pending: VecDeque::new(),
                sends: 0,
                fail_at_send: None,
                chunk_seconds,
            }
        }
    }

    impl AudioDecoder for FakeAudioDecoder {
        fn send(&mut self, packet: Packet) -> Result<(), String> {
            self.sends += 1;
            if self.fail_at_send == Some(self.sends) {
                return Err("audio decode failed".to_string());
            }
            let pts = packet.pts.expect("scheduler stamps timestamps");
            let samples = vec![1.0f32; (self.chunk_seconds * 48000.0 * 2.0) as usize];
            self.pending.push_back(AudioChunk {
                pts,
                stream: packet.stream,
                channels: 2,
                rate: 48000,
                samples,
            });
            Ok(())
        }

        fn drain(&mut self) -> Result<Vec<AudioChunk>, String> {
            Ok(self.pending.drain(..).collect())
        }

        fn finish(&mut self) -> Result<Vec<AudioChunk>, String> {
            Ok(self.pending.drain(..).collect())
        }

        fn flush(&mut self) {
            self.pending.clear();
        }
    }

    #[derive(Default)]
    struct FakeStorage {
        frames: VecDeque<(u64, f64)>,
        next_id: u64,
        drained: u64,
        decoded: u64,
        evicted: u64,
    }

    impl FakeStorage {
        fn push(&mut self, pts: f64) {
            let id = self.next_id;
            self.next_id += 1;
            self.decoded += 1;
            self.frames.push_back((id, pts));
            while self.frames.len() > MAX_QUEUED_FRAMES {
                let (id, _) = self.frames.pop_front().expect("over capacity");
                if id >= self.drained {
                    self.evicted += 1;
                }
            }
        }

        fn collect(&mut self) -> Vec<DecodedFrame> {
            let frames = self
                .frames
                .iter()
                .filter(|(id, _)| *id >= self.drained)
                .map(|(id, pts)| DecodedFrame { pts: *pts, id: *id })
                .collect();
            self.drained = self.next_id;
            frames
        }

        fn undrained(&self) -> u64 {
            self.frames
                .iter()
                .filter(|(id, _)| *id >= self.drained)
                .count() as u64
        }

        fn take(&mut self, id: u64) -> bool {
            match self.frames.iter().position(|(queued, _)| *queued == id) {
                Some(position) => {
                    self.frames.remove(position);
                    true
                }
                None => false,
            }
        }

        fn release(&mut self, id: u64) {
            if let Some(position) = self.frames.iter().position(|(queued, _)| *queued == id) {
                self.frames.remove(position);
            }
        }

        fn flush(&mut self) {
            self.frames.clear();
            self.drained = self.next_id;
        }
    }

    struct FakeVideoDecoder {
        storage: Rc<RefCell<FakeStorage>>,
        buffer: Vec<f64>,
        reorder: bool,
        max_seen: f64,
        sends: usize,
        fail_at_send: Option<usize>,
        delay: usize,
    }

    const REORDER_WINDOW: f64 = 0.1;

    impl FakeVideoDecoder {
        fn new(reorder: bool, storage: Rc<RefCell<FakeStorage>>) -> Self {
            FakeVideoDecoder {
                storage,
                buffer: Vec::new(),
                reorder,
                max_seen: f64::NEG_INFINITY,
                sends: 0,
                fail_at_send: None,
                delay: 0,
            }
        }

        fn release_ready(&mut self) -> Vec<f64> {
            if !self.reorder {
                let ready = self.buffer.len().saturating_sub(self.delay);
                return self.buffer.drain(..ready).collect();
            }
            self.buffer.sort_by(|a, b| a.total_cmp(b));
            let split = self
                .buffer
                .iter()
                .position(|pts| *pts >= self.max_seen - REORDER_WINDOW)
                .unwrap_or(self.buffer.len());
            self.buffer.drain(..split).collect()
        }

        fn release_into_storage(&mut self) {
            let ready = self.release_ready();
            let mut storage = self.storage.borrow_mut();
            for pts in ready {
                storage.push(pts);
            }
        }
    }

    impl VideoDecoder for FakeVideoDecoder {
        fn send(&mut self, packet: Packet) -> Result<(), String> {
            self.sends += 1;
            if self.fail_at_send == Some(self.sends) {
                return Err("video decode failed".to_string());
            }
            let pts = packet.pts.expect("scheduler stamps timestamps");
            self.max_seen = self.max_seen.max(pts);
            self.buffer.push(pts);
            Ok(())
        }

        fn drain(&mut self) -> Result<Vec<DecodedFrame>, String> {
            self.release_into_storage();
            Ok(self.storage.borrow_mut().collect())
        }

        fn finish(&mut self) -> Result<Vec<DecodedFrame>, String> {
            self.buffer.sort_by(|a, b| a.total_cmp(b));
            let all: Vec<f64> = std::mem::take(&mut self.buffer);
            {
                let mut storage = self.storage.borrow_mut();
                for pts in all {
                    storage.push(pts);
                }
            }
            Ok(self.storage.borrow_mut().collect())
        }

        fn flush(&mut self) {
            self.buffer.clear();
            self.max_seen = f64::NEG_INFINITY;
            self.storage.borrow_mut().flush();
        }

        fn release(&mut self, id: u64) {
            self.storage.borrow_mut().release(id);
        }

        fn decoded(&self) -> u64 {
            self.storage.borrow().decoded
        }

        fn evicted(&self) -> u64 {
            self.storage.borrow().evicted
        }

        fn undrained(&self) -> u64 {
            self.storage.borrow().undrained()
        }
    }

    struct FakeAudioOutput {
        log: Vec<(f64, usize)>,
        queued: f64,
        position: f64,
        paused: bool,
        pushes: usize,
        fail_at_push: Option<usize>,
        segments: Vec<(usize, bool)>,
        speed: f64,
    }

    impl FakeAudioOutput {
        fn new() -> Self {
            FakeAudioOutput {
                log: Vec::new(),
                queued: 0.0,
                position: 0.0,
                paused: false,
                pushes: 0,
                fail_at_push: None,
                segments: Vec::new(),
                speed: 1.0,
            }
        }

        fn consume(&mut self, dt: f64) {
            if self.paused {
                return;
            }
            let played = self.queued.min(dt);
            self.queued -= played;
            self.position += played;
        }
    }

    impl AudioOutput for FakeAudioOutput {
        fn push(&mut self, chunk: AudioChunk) -> Result<(), String> {
            self.pushes += 1;
            if self.fail_at_push == Some(self.pushes) {
                return Err("output device lost".to_string());
            }
            let duration = chunk.samples.len() as f64 / (chunk.channels as f64 * chunk.rate as f64);
            self.log.push((chunk.pts, chunk.stream));
            self.segments.push((chunk.samples.len() / chunk.channels as usize, chunk.samples.iter().all(|sample| *sample == 0.0)));
            self.queued += duration / self.speed;
            Ok(())
        }

        fn queued_seconds(&self) -> f64 {
            self.queued
        }

        fn device_position(&self) -> f64 {
            self.position
        }

        fn set_paused(&mut self, paused: bool) {
            self.paused = paused;
        }

        fn set_speed(&mut self, speed: f64) -> Result<(), String> {
            self.speed = speed;
            Ok(())
        }

        fn flush(&mut self) {
            self.queued = 0.0;
        }

        fn drained(&self) -> bool {
            self.queued == 0.0
        }
    }

    struct FakeVideoOutput {
        storage: Rc<RefCell<FakeStorage>>,
        discard: bool,
        delivered: Vec<(f64, u64)>,
        presents: usize,
        fail_at_present: Option<usize>,
    }

    impl FakeVideoOutput {
        fn new(storage: Rc<RefCell<FakeStorage>>) -> Self {
            FakeVideoOutput {
                storage,
                discard: false,
                delivered: Vec::new(),
                presents: 0,
                fail_at_present: None,
            }
        }
    }

    impl VideoOutput for FakeVideoOutput {
        fn present(&mut self, pts: f64, id: u64) -> Result<PresentOutcome, String> {
            self.presents += 1;
            if self.fail_at_present == Some(self.presents) {
                return Err("presentation failed".to_string());
            }
            if !self.storage.borrow_mut().take(id) {
                return Ok(PresentOutcome::Missing);
            }
            if self.discard {
                return Ok(PresentOutcome::Discarded);
            }
            self.delivered.push((pts, id));
            Ok(PresentOutcome::Delivered)
        }
    }

    type Sim = Scheduler<FakeDemux, FakeAudioDecoder, FakeVideoDecoder, FakeAudioOutput, FakeVideoOutput>;

    struct Harness {
        sim: Sim,
        now: f64,
        storage: Rc<RefCell<FakeStorage>>,
    }

    impl Harness {
        fn new(demux: FakeDemux, reorder_video: bool) -> Self {
            Self::build(demux, FakeVideoDecoder::new(reorder_video, Rc::new(RefCell::new(FakeStorage::default()))))
        }

        fn with_video_delay(demux: FakeDemux, delay: usize) -> Self {
            let storage = Rc::new(RefCell::new(FakeStorage::default()));
            let mut video = FakeVideoDecoder::new(false, storage.clone());
            video.delay = delay;
            Self::build(demux, video)
        }

        fn build(demux: FakeDemux, video: FakeVideoDecoder) -> Self {
            let storage = video.storage.clone();
            let sim = Sim::new(
                demux,
                FakeAudioDecoder::new(0.04),
                video,
                FakeAudioOutput::new(),
                FakeVideoOutput::new(storage.clone()),
            );
            Harness { sim, now: 0.0, storage }
        }

        fn run(&mut self, steps: usize, dt: f64, at: &[(usize, Command)]) {
            self.sim.open();
            for step in 0..steps {
                for (at_step, command) in at {
                    if *at_step == step {
                        self.sim.send(command.clone());
                    }
                }
                self.sim.audio_output_mut().consume(dt);
                self.sim.step(self.now);
                self.now += dt;
                if matches!(
                    self.sim.state(),
                    State::Ended | State::Error | State::Idle
                ) {
                    break;
                }
            }
        }

        fn step_checked(&mut self, dt: f64) {
            self.sim.audio_output_mut().consume(dt);
            self.sim.step(self.now);
            self.now += dt;
            self.assert_accounting();
        }

        fn paced(&mut self, deadline: f64, max_wait: f64) -> Paced {
            self.sim.open();
            let mut run = Paced {
                steps: 0,
                starved: false,
                audio_started: false,
                progressed: false,
            };
            while self.now < deadline {
                self.sim.step(self.now);
                run.steps += 1;
                if matches!(
                    self.sim.state(),
                    State::Ended | State::Error | State::Idle
                ) {
                    break;
                }
                run.audio_started = !self.sim.audio_output().log.is_empty();
                if run.audio_started
                    && matches!(self.sim.state(), State::Playing)
                    && self.sim.audio_output().queued_seconds() <= 0.0
                {
                    run.starved = true;
                }
                let Some(wait) = self.sim.next_wakeup(self.now) else {
                    break;
                };
                assert!(
                    wait.is_finite() && (0.0..=max_wait).contains(&wait),
                    "next wakeup {wait} is outside 0..={max_wait}"
                );
                run.progressed = true;
                self.sim.audio_output_mut().consume(wait);
                self.now += wait;
            }
            run
        }

        fn assert_accounting(&self) {
            let stats = self.stats();
            assert_eq!(
                stats.decoded,
                stats.dropped_late
                    + stats.evicted
                    + stats.missing
                    + stats.discarded
                    + stats.delivered
                    + stats.flushed
                    + stats.frames_pending,
                "frame accounting broke: {stats:?}"
            );
        }

        fn stats(&self) -> Stats {
            self.sim.stats()
        }
    }

    const DT: f64 = 0.01;

    struct Paced {
        steps: usize,
        starved: bool,
        audio_started: bool,
        progressed: bool,
    }

    #[test]
    fn video_only_paced_steps_track_frames_not_the_clock() {
        let mut h = Harness::new(FakeDemux::build(5.0, 24.0, 0.0, 0, false, false), false);
        let run = h.paced(5.0, 0.05);
        assert_eq!(h.sim.state(), State::Ended);
        let stats = h.stats();
        assert_eq!(stats.delivered, 120, "every frame is delivered: {stats:?}");
        assert_eq!(stats.dropped_late, 0, "pacing dropped frames: {stats:?}");
        assert!(
            run.steps <= 120 + 40,
            "video-only spun: {} steps for 120 frames",
            run.steps
        );
        assert_eq!(stats.steps as usize, run.steps, "step counter tracks iterations");
    }

    #[test]
    fn pacing_never_spins_while_a_frame_is_pending() {
        let mut h = Harness::with_video_delay(FakeDemux::build(4.0, 25.0, 0.0, 0, false, false), 10);
        h.sim.open();
        let mut now = 0.0;
        let mut steps = 0;
        while now < 4.0 {
            h.sim.step(now);
            steps += 1;
            if matches!(h.sim.state(), State::Ended | State::Error) {
                break;
            }
            let wait = h.sim.next_wakeup(now).expect("a playing scheduler has a wakeup");
            assert!((0.0..=0.05).contains(&wait), "wakeup {wait} out of bounds");
            assert!(
                wait >= DT / 2.0 || wait == 0.0,
                "the scheduler asked to be woken after {wait}s without due work"
            );
            h.sim.audio_output_mut().consume(wait);
            now += wait;
        }
        assert_eq!(h.sim.state(), State::Ended);
        assert!(steps < 200, "{steps} steps for a 4 s video-only clip");
        assert!(h.stats().delivered >= 95, "{:?}", h.stats());
        assert!(h.stats().dropped_late <= 2, "{:?}", h.stats());
    }

    #[test]
    fn paced_av_playback_keeps_the_audio_queue_fed() {
        let mut h = Harness::new(FakeDemux::av(4.0, 25.0, 0.04), false);
        let run = h.paced(8.0, 0.05);
        assert_eq!(h.sim.state(), State::Ended);
        assert!(run.progressed, "the scheduler never asked for a wakeup");
        assert!(!run.starved, "the audio output underran mid-playback");
        assert_eq!(h.stats().delivered, 100, "{:?}", h.stats());
        assert_eq!(h.stats().dropped_late, 0, "{:?}", h.stats());
    }

    #[test]
    fn a_command_is_applied_without_waiting_out_the_deadline() {
        let mut h = Harness::new(FakeDemux::build(5.0, 24.0, 0.0, 0, false, false), false);
        h.sim.open();
        let mut waited = false;
        for _ in 0..400 {
            h.sim.step(h.now);
            let Some(wait) = h.sim.next_wakeup(h.now) else {
                break;
            };
            if wait > 0.0 {
                waited = true;
                break;
            }
            h.sim.audio_output_mut().consume(wait);
            h.now += wait;
        }
        assert!(waited, "the video-only scheduler never reached a future deadline");
        let steps = h.sim.stats().steps;
        h.sim.send(Command::Seek(2.0, SeekPrecision::Exact, 5));
        h.sim.step(h.now);
        assert_eq!(
            h.sim.stats().steps,
            steps + 1,
            "the queued command needed an extra step"
        );
        assert_eq!(
            h.sim.stats().seek_completed_generation,
            5,
            "the command was not applied at the head of the step"
        );
        let wait = h
            .sim
            .next_wakeup(h.now)
            .expect("the seek left the scheduler playing");
        assert!(
            (0.0..=0.05).contains(&wait),
            "the deadline after a seek is not bounded: {wait}"
        );
    }

    #[test]
    fn normal_av_plays_to_ended_with_no_drops() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.run(600, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        let stats = h.stats();
        assert_eq!(stats.delivered, 50);
        assert_eq!(stats.dropped_late, 0);
        assert_eq!(stats.audio_chunks, 50);
        let presented: Vec<f64> = h.sim.video_output().delivered.iter().map(|(pts, _)| *pts).collect();
        let mut sorted = presented.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(presented, sorted);
    }

    #[test]
    fn video_only_runs_on_system_clock() {
        let mut h = Harness::new(FakeDemux::build(1.0, 25.0, 0.0, 0, false, false), false);
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().delivered, 25);
        assert_eq!(h.stats().audio_chunks, 0);
    }

    fn fed_sub() -> EngineSub {
        let info = player_media::StreamInfo {
            index: 7,
            kind: player_media::StreamKind::Subtitle,
            codec_name: "subrip".to_owned(), profile: -99,
            width: 0,
            height: 0,
            sample_rate: 0,
            channels: 0,
            time_base: (1, 1000),
            duration_seconds: None,
            extradata: Vec::new(),
            channel_mask: 0,
            attached: false,
            default: false,
            forced: false,
            language: None,
            title: None,
        };
        let mut sub = EngineSub::open_embedded(&info).expect("subrip opens");
        sub.feed(b"Hello", 0.0, 1.0);
        sub
    }

    #[test]
    fn a_subtitle_added_while_hidden_stays_hidden() {
        let mut h = Harness::new(FakeDemux::av(1.0, 25.0, 0.04), false);
        h.sim.open();
        h.sim.set_sub_visible(false);
        let position = h.sim.add_sub(fed_sub());
        assert_eq!(h.sim.subs()[position].text_at(500), "");
        assert_eq!(h.sim.subtitle_text(0.5), "");
        h.sim.set_sub_visible(true);
        assert_eq!(h.sim.subs()[position].text_at(500), "Hello");
    }

    #[test]
    fn preroll_audio_survives_eof_before_the_first_video_frame() {
        let mut h = Harness::with_video_delay(FakeDemux::av(0.4, 25.0, 0.04), 10);
        h.sim.enable_video_preroll();
        h.run(300, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.sim.audio_output().log.len(), 11, "preroll audio was orphaned");
        assert_eq!(h.stats().delivered, 10);
    }

    #[test]
    fn audio_only_plays_to_ended() {
        let mut h = Harness::new(FakeDemux::build(1.0, 0.0, 0.04, 1, false, false), false);
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().audio_chunks, 25);
        assert_eq!(h.stats().delivered, 0);
    }

    #[test]
    fn variable_frame_rate_presents_in_order() {
        let mut demux = FakeDemux::av(0.5, 25.0, 0.04);
        for (i, packet) in demux.script.iter_mut().enumerate() {
            if packet.kind == StreamKind::Video {
                packet.pts = Some(((i * 37) % 500) as f64 / 1000.0);
            }
        }
        demux.script.sort_by(|a, b| {
            a.pts
                .unwrap_or(0.0)
                .total_cmp(&b.pts.unwrap_or(0.0))
        });
        let mut h = Harness::new(demux, false);
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        let presented: Vec<f64> = h.sim.video_output().delivered.iter().map(|(pts, _)| *pts).collect();
        let mut sorted = presented.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(presented, sorted);
    }

    #[test]
    fn reordered_dts_presents_in_pts_order() {
        let mut h = Harness::new(FakeDemux::build(1.0, 25.0, 0.04, 1, true, false), true);
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().delivered, 25);
        let presented: Vec<f64> = h.sim.video_output().delivered.iter().map(|(pts, _)| *pts).collect();
        let mut sorted = presented.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(presented, sorted);
    }

    #[test]
    fn forward_and_backward_seek_restart_from_target() {
        let mut h = Harness::new(FakeDemux::av(4.0, 25.0, 0.04), false);
        h.run(
            900,
            DT,
            &[
                (30, Command::Seek(3.0, SeekPrecision::Exact, 1)),
                (120, Command::Seek(0.5, SeekPrecision::Exact, 2)),
            ],
        );
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().seeks_completed, 2);
        let after_first: Vec<f64> = h.sim.video_output().delivered.iter().map(|(pts, _)| *pts).collect();
        assert!(after_first.iter().any(|pts| *pts >= 3.0));
        assert!(after_first.iter().any(|pts| *pts < 1.0 && *pts >= 0.5));
    }

    #[test]
    fn seek_between_keyframes_resumes_at_the_target() {
        let mut h = Harness::new(FakeDemux::av(14.0, 25.0, 0.04).with_keyframes(5.0), false);
        h.run(25, DT, &[]);
        let presented_before = h.sim.video_output().delivered.len();
        let pushes_before = h.sim.audio_output().log.len();
        h.sim.send(Command::Seek(9.0, SeekPrecision::Exact, 1));
        h.run(50, DT, &[]);
        let presented: Vec<f64> = h.sim.video_output().delivered[presented_before..]
            .iter()
            .map(|(pts, _)| *pts)
            .collect();
        assert!(!presented.is_empty(), "video resumes after the seek");
        assert!(presented[0] >= 9.0 - 0.04, "first frame after the seek is at {}", presented[0]);
        assert!(
            presented.len() >= 10 && *presented.last().unwrap() >= 9.4,
            "video keeps playing after the seek: {presented:?}"
        );
        let audio: Vec<f64> = h.sim.audio_output().log[pushes_before..].iter().map(|(pts, _)| *pts).collect();
        assert!(audio.iter().all(|pts| *pts >= 9.0 - 0.04), "audio before the target played: {:?}", &audio[..audio.len().min(5)]);
    }

    #[test]
    fn decoder_delay_longer_than_the_audio_lead_still_presents_on_time() {
        let mut h = Harness::with_video_delay(FakeDemux::av(4.0, 25.0, 0.04), 10);
        h.run(600, DT, &[]);
        let stats = h.sim.stats();
        assert_eq!(h.sim.state(), State::Ended);
        assert!(stats.dropped_late <= 2, "late frames with a delayed decoder: {stats:?}");
        assert!(stats.delivered >= 95, "presented {}", stats.delivered);
    }

    #[test]
    fn keyframe_seek_resumes_at_the_keyframe_without_decoding_through() {
        let mut h = Harness::new(FakeDemux::av(14.0, 25.0, 0.04).with_keyframes(5.0), false);
        h.run(25, DT, &[]);
        let presented_before = h.sim.video_output().delivered.len();
        let audio_before = h.sim.audio_output().log.len();
        h.sim.send(Command::Seek(9.0, SeekPrecision::Keyframe, 1));
        h.run(30, DT, &[]);
        let presented: Vec<f64> = h.sim.video_output().delivered[presented_before..]
            .iter()
            .map(|(pts, _)| *pts)
            .collect();
        assert!((presented[0] - 5.0).abs() < 1e-9, "keyframe seek resumes at 5.0: {presented:?}");
        let first_audio = h.sim.audio_output().log[audio_before].0;
        assert!((first_audio - presented[0]).abs() < 0.04,
            "first video {}, first audio {first_audio}", presented[0]);
        assert_eq!(h.sim.stats().dropped_late, 0);
        let position = h.sim.clock_position(h.now);
        assert!((5.0..6.0).contains(&position), "clock follows the keyframe: {position}");
    }

    #[test]
    fn repeated_rapid_seeks_land_on_last_target() {
        let mut h = Harness::new(FakeDemux::av(4.0, 25.0, 0.04), false);
        h.run(60, DT, &[(10, Command::Seek(3.5, SeekPrecision::Exact, 1))]);
        h.sim.send(Command::Seek(0.2, SeekPrecision::Exact, 2));
        h.sim.send(Command::Seek(2.8, SeekPrecision::Exact, 3));
        h.sim.send(Command::Seek(1.1, SeekPrecision::Exact, 4));
        h.sim.send(Command::Seek(3.9, SeekPrecision::Exact, 5));
        h.sim.send(Command::Seek(0.7, SeekPrecision::Exact, 6));
        h.sim.step(h.now);
        h.now += DT;
        assert_eq!(h.sim.stats().seeks_completed, 2, "queued seeks collapse to the last target");
        for _ in 0..600 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
            if matches!(h.sim.state(), State::Ended) {
                break;
            }
        }
        assert_eq!(h.sim.state(), State::Ended);
        let presented: Vec<f64> =
            h.sim.video_output().delivered.iter().map(|(pts, _)| *pts).collect();
        assert!(presented.iter().any(|pts| *pts >= 0.7));
    }

    #[test]
    fn back_to_back_seeks_complete_only_the_last_generation() {
        let mut h = Harness::new(FakeDemux::av(4.0, 25.0, 0.04), false);
        h.sim.open();
        for _ in 0..30 {
            h.step_checked(DT);
        }
        h.sim.send(Command::Seek(3.0, SeekPrecision::Exact, 1));
        h.sim.send(Command::Seek(1.0, SeekPrecision::Exact, 2));
        let mut completed = vec![0u64];
        for _ in 0..400 {
            h.step_checked(DT);
            completed.push(h.stats().seek_completed_generation);
            if h.sim.state() == State::Ended {
                break;
            }
        }
        assert_eq!(h.stats().seeks_completed, 1, "the queued first seek must not execute");
        assert!(
            completed.iter().all(|generation| *generation != 1),
            "a superseded seek completed: {completed:?}"
        );
        assert_eq!(h.stats().seek_completed_generation, 2);
    }

    #[test]
    fn paused_seek_completes_when_the_frame_is_presented() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.sim.open();
        for _ in 0..30 {
            h.step_checked(DT);
        }
        h.sim.send(Command::Pause(true));
        h.step_checked(DT);
        let presented = h.sim.video_output().delivered.len();
        assert_eq!(h.stats().seek_completed_generation, 0);
        h.sim.send(Command::Seek(1.0, SeekPrecision::Exact, 7));
        h.step_checked(DT);
        assert_eq!(
            h.sim.video_output().delivered.len(),
            presented + 1,
            "the paused seek presents exactly one frame"
        );
        assert_eq!(h.stats().seek_completed_generation, 7);
        assert!(matches!(h.sim.state(), State::Paused { .. }));
    }

    #[test]
    fn audio_only_seek_completes_when_audio_resumes() {
        let mut h = Harness::new(FakeDemux::build(1.0, 0.0, 0.04, 1, false, false), false);
        h.sim.open();
        for _ in 0..10 {
            h.step_checked(DT);
        }
        h.sim.send(Command::Seek(0.5, SeekPrecision::Exact, 3));
        let mut completed = false;
        for _ in 0..100 {
            h.step_checked(DT);
            if h.stats().seek_completed_generation == 3 {
                completed = true;
                break;
            }
        }
        assert!(completed, "audio-only seek never completed: {:?}", h.stats());
    }

    #[test]
    fn seek_at_eof_completes_once() {
        let mut h = Harness::new(FakeDemux::av(1.0, 25.0, 0.04), false);
        h.sim.open();
        for _ in 0..5 {
            h.step_checked(DT);
        }
        h.sim.send(Command::Seek(5.0, SeekPrecision::Exact, 2));
        let mut completions = 0;
        let mut previous = h.stats().seek_completed_generation;
        for _ in 0..300 {
            h.step_checked(DT);
            let current = h.stats().seek_completed_generation;
            if current == 2 && previous != 2 {
                completions += 1;
            }
            previous = current;
            if h.sim.state() == State::Ended {
                break;
            }
        }
        assert_eq!(h.stats().seek_completed_generation, 2);
        assert_eq!(completions, 1, "the eof seek completed more than once");
    }

    #[test]
    fn seek_while_paused_stays_paused() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.run(
            400,
            DT,
            &[
                (10, Command::Pause(true)),
                (20, Command::Seek(1.0, SeekPrecision::Exact, 1)),
            ],
        );
        assert!(matches!(h.sim.state(), State::Paused { .. }));
        assert_eq!(h.stats().seeks_completed, 1);
    }

    #[test]
    fn pause_freezes_progress_and_resume_continues() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.sim.open();
        for _ in 0..30 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
        }
        h.sim.send(Command::Pause(true));
        h.sim.step(h.now);
        let frozen_audio = h.sim.audio_output().log.len();
        let frozen_video = h.sim.video_output().delivered.len();
        let frozen_position = h.sim.clock_position(h.now);
        for _ in 0..50 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
        }
        assert_eq!(h.sim.audio_output().log.len(), frozen_audio);
        assert_eq!(h.sim.video_output().delivered.len(), frozen_video);
        assert_eq!(h.sim.clock_position(h.now), frozen_position);
        assert!(matches!(h.sim.state(), State::Paused { .. }));
        h.sim.send(Command::Pause(false));
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().delivered, 50);
    }

    #[test]
    fn pause_freezes_a_video_only_position_and_resume_continues() {
        let mut h = Harness::new(FakeDemux::build(1.0, 25.0, 0.0, 0, false, false), false);
        h.sim.open();
        for _ in 0..30 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
        }
        h.sim.send(Command::Pause(true));
        h.sim.step(h.now);
        let frozen_position = h.sim.clock_position(h.now);
        for _ in 0..50 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
        }
        assert_eq!(h.sim.clock_position(h.now), frozen_position);
        assert!(matches!(h.sim.state(), State::Paused { .. }));
        h.sim.send(Command::Pause(false));
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().delivered, 25);
    }

    #[test]
    fn disabling_audio_stops_it_while_video_continues() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.sim.open();
        for _ in 0..30 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
        }
        let pushed = h.sim.audio_output().log.len();
        let presented = h.sim.video_output().delivered.len();
        assert!(pushed > 0 && presented > 0);
        h.sim.send(Command::DisableAudio);
        h.sim.step(h.now);
        h.now += DT;
        for _ in 0..10 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
        }
        assert_eq!(h.sim.audio_output().log.len(), pushed, "disabled audio kept playing");
        assert!(h.sim.video_output().delivered.len() > presented);
        h.sim.send(Command::SwitchAudio(0));
        for _ in 0..20 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
            if h.sim.audio_output().log.len() > pushed {
                break;
            }
        }
        assert!(h.sim.audio_output().log.len() > pushed, "re-selected audio never resumed");
    }

    #[test]
    fn timestamp_gaps_and_overlaps_reach_the_sample_timeline() {
        let mut demux = FakeDemux::build(0.3, 0.0, 0.04, 1, false, false);
        demux.script.truncate(3);
        for (packet, pts) in demux.script.iter_mut().zip([0.10, 0.20, 0.22]) {
            packet.pts = Some(pts);
        }
        let mut h = Harness::new(demux, false);
        h.run(300, 0.01, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        let segments = &h.sim.audio_output().segments;
        let mut runs: Vec<(usize, bool)> = Vec::new();
        for &(frames, silent) in segments {
            if let Some(last) = runs.last_mut()
                && last.1 == silent
            {
                last.0 += frames;
                continue;
            }
            runs.push((frames, silent));
        }
        assert_eq!(runs, vec![(4800, true), (1920, false), (2880, true), (2880, false)]);
    }

    #[test]
    fn seek_and_track_switch_reset_audio_alignment() {
        let mut h = Harness::new(FakeDemux::build(3.0, 25.0, 0.04, 2, false, false), false);
        h.sim.open();
        for _ in 0..35 { h.step_checked(0.01); }
        let before_seek = h.sim.audio_output().log.len();
        h.sim.send(Command::Seek(1.0, SeekPrecision::Exact, 12));
        h.sim.step(h.now);
        for _ in 0..10 { h.step_checked(0.01); }
        let output = h.sim.audio_output();
        let first = output.log.iter().zip(&output.segments).skip(before_seek)
            .find(|(_, (_, silent))| !*silent).unwrap().0;
        assert!((first.0 - 1.0).abs() <= 0.041, "seek resumed at {}", first.0);
        let position = h.sim.clock_position(h.now);
        let before_switch = output.log.len();
        h.sim.send(Command::SwitchAudio(1));
        for _ in 0..20 { h.step_checked(0.01); }
        let output = h.sim.audio_output();
        let first = output.log.iter().zip(&output.segments).skip(before_switch)
            .find(|(_, (_, silent))| !*silent).unwrap().0;
        assert_eq!(first.1, 1);
        assert!((first.0 - position).abs() <= 0.081, "switch resumed at {}, position {position}", first.0);
    }

    #[test]
    fn delayed_audio_starts_after_video_preroll() {
        let mut demux = FakeDemux::build(0.4, 25.0, 0.04, 1, false, false);
        demux.script.retain(|packet| packet.kind != StreamKind::Audio || packet.pts.unwrap() < 0.12);
        for packet in &mut demux.script {
            if packet.kind == StreamKind::Audio { packet.pts = packet.pts.map(|pts| pts + 0.1); }
        }
        demux.script.sort_by(|a, b| a.pts.unwrap().total_cmp(&b.pts.unwrap()));
        let mut h = Harness::new(demux, false);
        h.sim.enable_video_preroll();
        h.run(300, 0.01, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        let segments = &h.sim.audio_output().segments;
        let silence: usize = segments.iter().take_while(|(_, silent)| *silent).map(|(frames, _)| frames).sum();
        assert_eq!(silence, 4800);
        assert!(segments.iter().any(|(_, silent)| !silent));
        assert_eq!(h.sim.video_output().delivered.first().unwrap().0, 0.0);
    }

    #[test]
    fn speed_transition_restarts_at_audible_position_and_stays_paused() {
        let mut h = Harness::new(FakeDemux::build(3.0, 25.0, 0.04, 1, false, false), false);
        h.sim.open();
        for _ in 0..40 { h.step_checked(0.01); }
        let before = h.sim.clock_position(h.now);
        let pushed = h.sim.audio_output().log.len();
        h.sim.send(Command::SetSpeed(2.0));
        h.sim.step(h.now);
        let after = h.sim.clock_position(h.now);
        assert!((after - before).abs() <= 0.011, "{before} -> {after}");
        let output = h.sim.audio_output();
        let resumed = output.log.iter().zip(&output.segments).skip(pushed)
            .find(|(_, (_, silent))| !*silent).unwrap().0.0;
        assert!((resumed - after).abs() <= 0.041, "resumed {resumed}, position {after}");
        h.sim.send(Command::Pause(true));
        h.sim.step(h.now);
        let paused = h.sim.clock_position(h.now);
        h.sim.send(Command::SetSpeed(0.5));
        h.sim.step(h.now);
        h.now += 1.0;
        h.sim.step(h.now);
        assert!((h.sim.clock_position(h.now) - paused).abs() < 1e-9);
        h.sim.send(Command::Pause(false));
        h.sim.step(h.now);
        for _ in 0..20 { h.step_checked(0.01); }
        let advance = h.sim.clock_position(h.now) - paused;
        assert!((advance - 0.1).abs() < 0.02, "advance {advance}");
    }

    #[test]
    fn speed_change_preserves_pending_seek_completion() {
        let mut h = Harness::new(FakeDemux::av(3.0, 25.0, 0.04), false);
        h.sim.open();
        h.sim.send(Command::Seek(1.0, SeekPrecision::Exact, 9));
        h.sim.send(Command::SetSpeed(2.0));
        for _ in 0..30 {
            h.step_checked(0.01);
            if h.stats().seek_completed_generation == 9 { break; }
        }
        assert_eq!(h.stats().seek_completed_generation, 9);
        assert_eq!(h.sim.speed(), 2.0);
    }

    #[test]
    fn speed_change_advances_system_clock_faster() {
        let mut h = Harness::new(FakeDemux::build(1.0, 25.0, 0.0, 0, false, false), false);
        h.sim.open();
        h.sim.send(Command::SetSpeed(2.0));
        h.sim.step(h.now);
        let at_start = h.sim.clock_position(h.now);
        h.now += 1.0;
        assert!((h.sim.clock_position(h.now) - at_start - 2.0).abs() < 1e-9);
    }

    #[test]
    fn audio_track_switch_routes_later_chunks_to_new_stream() {
        let mut h = Harness::new(FakeDemux::build(2.0, 25.0, 0.04, 2, false, false), false);
        h.run(600, DT, &[(1, Command::SwitchAudio(1))]);
        assert_eq!(h.sim.state(), State::Ended);
        let log = &h.sim.audio_output().log;
        assert!(!log.is_empty());
        let switch_at = log.iter().position(|(_, stream)| *stream == 1).expect("switched");
        assert!(log[..switch_at].iter().all(|(_, stream)| *stream == 0));
        assert!(log[switch_at..].iter().all(|(_, stream)| *stream == 1));
    }

    #[test]
    fn missing_timestamps_still_progress_to_ended() {
        let mut h = Harness::new(FakeDemux::build(1.0, 25.0, 0.04, 1, false, true), false);
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        let audio: Vec<f64> = h.sim.audio_output().log.iter().map(|(pts, _)| *pts).collect();
        let mut sorted = audio.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(audio, sorted);
        let presented: Vec<f64> = h.sim.video_output().delivered.iter().map(|(pts, _)| *pts).collect();
        let mut sorted = presented.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(presented, sorted);
    }

    #[test]
    fn trailing_frames_present_after_eof_before_ended() {
        let mut h = Harness::new(FakeDemux::av(0.4, 25.0, 0.04), false);
        h.run(300, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().delivered, 10);
    }

    #[test]
    fn decode_failure_mid_playback_reaches_error() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.sim.test_handles().2.fail_at_send = Some(10);
        h.run(600, DT, &[]);
        assert_eq!(h.sim.state(), State::Error);
        assert!(h.sim.failure().is_some());
        assert_eq!(h.stats().failures, 1);
    }

    #[test]
    fn output_failure_reaches_error() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.sim.test_handles().3.fail_at_present = Some(1);
        h.run(600, DT, &[]);
        assert_eq!(h.sim.state(), State::Error);
    }

    #[test]
    fn demux_error_reaches_error() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.sim.test_handles().0.fail_at_read = Some(20);
        h.run(600, DT, &[]);
        assert_eq!(h.sim.state(), State::Error);
    }

    #[test]
    fn close_during_playback_returns_to_idle() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.run(600, DT, &[(10, Command::Close)]);
        assert_eq!(h.sim.state(), State::Idle);
    }

    #[test]
    fn stale_commands_are_ignored_not_fatal() {
        let mut h = Harness::new(FakeDemux::av(0.4, 25.0, 0.04), false);
        h.run(300, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        h.sim.send(Command::SetSpeed(2.0));
        h.sim.send(Command::SwitchAudio(3));
        for _ in 0..5 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
        }
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().commands_ignored, 2);
    }

    #[test]
    fn frame_accounting_holds_through_overflow_and_late_frames() {
        let mut h = Harness::with_video_delay(
            FakeDemux::build(4.0, 25.0, 0.0, 0, false, false),
            70,
        );
        h.sim.open();
        let mut overflow = false;
        let mut late = false;
        let mut missing = false;
        for step in 0..600 {
            if step == 120 {
                h.now += 2.0;
            }
            h.step_checked(DT);
            overflow |= h.stats().evicted > 0;
            late |= h.stats().dropped_late > 0;
            missing |= h.stats().missing > 0;
            if h.sim.state() == State::Ended {
                break;
            }
        }
        assert_eq!(h.sim.state(), State::Ended);
        assert!(overflow, "the 64-frame cap never evicted a frame: {:?}", h.stats());
        assert!(late, "no frame was dropped late: {:?}", h.stats());
        assert!(missing, "a native frame evicted after draining was never counted: {:?}", h.stats());
        assert_eq!(
            h.stats().delivered,
            h.sim.video_output().delivered.len() as u64
        );
    }

    #[test]
    fn frame_accounting_holds_across_a_seek() {
        let mut h = Harness::new(FakeDemux::av(4.0, 25.0, 0.04), false);
        h.sim.open();
        for _ in 0..40 {
            h.step_checked(DT);
        }
        h.sim.send(Command::Seek(3.0, SeekPrecision::Exact, 1));
        for _ in 0..600 {
            h.step_checked(DT);
            if h.sim.state() == State::Ended {
                break;
            }
        }
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().seeks_completed, 1);
        assert!(
            h.stats().flushed > 0,
            "the seek discarded nothing: {:?}",
            h.stats()
        );
        assert_eq!(
            h.stats().delivered,
            h.sim.video_output().delivered.len() as u64
        );
    }

    #[test]
    fn a_late_frame_releases_its_native_frame() {
        let mut h = Harness::new(FakeDemux::build(0.04, 25.0, 0.0, 0, false, false), false);
        h.sim.open();
        h.now = 10.0;
        h.sim.step(h.now);
        assert_eq!(h.stats().dropped_late, 1);
        assert_eq!(
            h.storage.borrow().frames.len(),
            0,
            "the late frame stayed in native storage"
        );
        h.assert_accounting();
    }
}
