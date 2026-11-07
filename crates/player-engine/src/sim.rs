#[cfg(test)]
mod tests {
    use crate::sched::{SeekPrecision, 
        AudioChunk, AudioDecoder, AudioOutput, Command, DecodedFrame, DemuxEvent, Demuxer, Packet,
        Scheduler, Stats, StreamKind, VideoDecoder, VideoOutput,
    };
    use crate::state::State;
    use crate::sub::EngineSub;
    use std::collections::VecDeque;

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
                for pair in video.chunks_exact_mut(2) {
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
                codec_name: "fake".to_owned(),
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
            let samples = vec![0.0f32; (self.chunk_seconds * 48000.0 * 2.0) as usize];
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

    struct FakeVideoDecoder {
        buffer: Vec<DecodedFrame>,
        next_id: u64,
        reorder: bool,
        max_seen: f64,
        sends: usize,
        fail_at_send: Option<usize>,
        delay: usize,
    }

    const REORDER_WINDOW: f64 = 0.1;

    impl FakeVideoDecoder {
        fn new(reorder: bool) -> Self {
            FakeVideoDecoder {
                buffer: Vec::new(),
                next_id: 0,
                reorder,
                max_seen: f64::NEG_INFINITY,
                sends: 0,
                fail_at_send: None,
                delay: 0,
            }
        }

        fn release_ready(&mut self) -> Vec<DecodedFrame> {
            if !self.reorder {
                let ready = self.buffer.len().saturating_sub(self.delay);
                return self.buffer.drain(..ready).collect();
            }
            self.buffer.sort_by(|a, b| a.pts.total_cmp(&b.pts));
            let split = self
                .buffer
                .iter()
                .position(|frame| frame.pts >= self.max_seen - REORDER_WINDOW)
                .unwrap_or(self.buffer.len());
            self.buffer.drain(..split).collect()
        }
    }

    impl VideoDecoder for FakeVideoDecoder {
        fn send(&mut self, packet: Packet) -> Result<(), String> {
            self.sends += 1;
            if self.fail_at_send == Some(self.sends) {
                return Err("video decode failed".to_string());
            }
            let id = self.next_id;
            self.next_id += 1;
            let pts = packet.pts.expect("scheduler stamps timestamps");
            self.max_seen = self.max_seen.max(pts);
            self.buffer.push(DecodedFrame { pts, id });
            Ok(())
        }

        fn drain(&mut self) -> Result<Vec<DecodedFrame>, String> {
            Ok(self.release_ready())
        }

        fn finish(&mut self) -> Result<Vec<DecodedFrame>, String> {
            self.buffer.sort_by(|a, b| a.pts.total_cmp(&b.pts));
            Ok(self.buffer.drain(..).collect())
        }

        fn flush(&mut self) {
            self.buffer.clear();
            self.max_seen = f64::NEG_INFINITY;
        }
    }

    struct FakeAudioOutput {
        log: Vec<(f64, usize)>,
        queued: f64,
        position: f64,
        paused: bool,
        pushes: usize,
        fail_at_push: Option<usize>,
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
            self.queued += duration;
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

        fn flush(&mut self) {
            self.queued = 0.0;
        }

        fn drained(&self) -> bool {
            self.queued == 0.0
        }
    }

    struct FakeVideoOutput {
        presented: Vec<(f64, u64)>,
        presents: usize,
        fail_at_present: Option<usize>,
    }

    impl FakeVideoOutput {
        fn new() -> Self {
            FakeVideoOutput {
                presented: Vec::new(),
                presents: 0,
                fail_at_present: None,
            }
        }
    }

    impl VideoOutput for FakeVideoOutput {
        fn present(&mut self, pts: f64, id: u64) -> Result<(), String> {
            self.presents += 1;
            if self.fail_at_present == Some(self.presents) {
                return Err("presentation failed".to_string());
            }
            self.presented.push((pts, id));
            Ok(())
        }
    }

    type Sim = Scheduler<FakeDemux, FakeAudioDecoder, FakeVideoDecoder, FakeAudioOutput, FakeVideoOutput>;

    struct Harness {
        sim: Sim,
        now: f64,
    }

    impl Harness {
        fn new(demux: FakeDemux, reorder_video: bool) -> Self {
            let sim = Sim::new(
                demux,
                FakeAudioDecoder::new(0.04),
                FakeVideoDecoder::new(reorder_video),
                FakeAudioOutput::new(),
                FakeVideoOutput::new(),
            );
            Harness { sim, now: 0.0 }
        }

        fn with_video_delay(demux: FakeDemux, delay: usize) -> Self {
            let mut video = FakeVideoDecoder::new(false);
            video.delay = delay;
            let sim = Sim::new(
                demux,
                FakeAudioDecoder::new(0.04),
                video,
                FakeAudioOutput::new(),
                FakeVideoOutput::new(),
            );
            Harness { sim, now: 0.0 }
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

        fn stats(&self) -> Stats {
            self.sim.stats()
        }
    }

    const DT: f64 = 0.01;

    #[test]
    fn normal_av_plays_to_ended_with_no_drops() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.run(600, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        let stats = h.stats();
        assert_eq!(stats.presented, 50);
        assert_eq!(stats.dropped_late, 0);
        assert_eq!(stats.audio_chunks, 50);
        let presented: Vec<f64> = h.sim.video_output().presented.iter().map(|(pts, _)| *pts).collect();
        let mut sorted = presented.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(presented, sorted);
    }

    #[test]
    fn video_only_runs_on_system_clock() {
        let mut h = Harness::new(FakeDemux::build(1.0, 25.0, 0.0, 0, false, false), false);
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().presented, 25);
        assert_eq!(h.stats().audio_chunks, 0);
    }

    fn fed_sub() -> EngineSub {
        let info = player_media::StreamInfo {
            index: 7,
            kind: player_media::StreamKind::Subtitle,
            codec_name: "subrip".to_owned(),
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
        assert_eq!(h.stats().presented, 10);
    }

    #[test]
    fn audio_only_plays_to_ended() {
        let mut h = Harness::new(FakeDemux::build(1.0, 0.0, 0.04, 1, false, false), false);
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().audio_chunks, 25);
        assert_eq!(h.stats().presented, 0);
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
        let presented: Vec<f64> = h.sim.video_output().presented.iter().map(|(pts, _)| *pts).collect();
        let mut sorted = presented.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(presented, sorted);
    }

    #[test]
    fn reordered_dts_presents_in_pts_order() {
        let mut h = Harness::new(FakeDemux::build(1.0, 25.0, 0.04, 1, true, false), true);
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().presented, 25);
        let presented: Vec<f64> = h.sim.video_output().presented.iter().map(|(pts, _)| *pts).collect();
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
            &[(30, Command::Seek(3.0, SeekPrecision::Exact)), (120, Command::Seek(0.5, SeekPrecision::Exact))],
        );
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().seeks_completed, 2);
        let after_first: Vec<f64> = h.sim.video_output().presented.iter().map(|(pts, _)| *pts).collect();
        assert!(after_first.iter().any(|pts| *pts >= 3.0));
        assert!(after_first.iter().any(|pts| *pts < 1.0 && *pts >= 0.5));
    }

    #[test]
    fn seek_between_keyframes_resumes_at_the_target() {
        let mut h = Harness::new(FakeDemux::av(14.0, 25.0, 0.04).with_keyframes(5.0), false);
        h.run(25, DT, &[]);
        let presented_before = h.sim.video_output().presented.len();
        let pushes_before = h.sim.audio_output().log.len();
        h.sim.send(Command::Seek(9.0, SeekPrecision::Exact));
        h.run(50, DT, &[]);
        let presented: Vec<f64> = h.sim.video_output().presented[presented_before..]
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
        assert!(stats.presented >= 95, "presented {}", stats.presented);
    }

    #[test]
    fn keyframe_seek_resumes_at_the_keyframe_without_decoding_through() {
        let mut h = Harness::new(FakeDemux::av(14.0, 25.0, 0.04).with_keyframes(5.0), false);
        h.run(25, DT, &[]);
        let presented_before = h.sim.video_output().presented.len();
        h.sim.send(Command::Seek(9.0, SeekPrecision::Keyframe));
        h.run(30, DT, &[]);
        let presented: Vec<f64> = h.sim.video_output().presented[presented_before..]
            .iter()
            .map(|(pts, _)| *pts)
            .collect();
        assert!((presented[0] - 5.0).abs() < 1e-9, "keyframe seek resumes at 5.0: {presented:?}");
        assert_eq!(h.sim.stats().dropped_late, 0);
        let position = h.sim.clock_position(h.now);
        assert!((5.0..6.0).contains(&position), "clock follows the keyframe: {position}");
    }

    #[test]
    fn repeated_rapid_seeks_land_on_last_target() {
        let mut h = Harness::new(FakeDemux::av(4.0, 25.0, 0.04), false);
        h.run(60, DT, &[(10, Command::Seek(3.5, SeekPrecision::Exact))]);
        h.sim.send(Command::Seek(0.2, SeekPrecision::Exact));
        h.sim.send(Command::Seek(2.8, SeekPrecision::Exact));
        h.sim.send(Command::Seek(1.1, SeekPrecision::Exact));
        h.sim.send(Command::Seek(3.9, SeekPrecision::Exact));
        h.sim.send(Command::Seek(0.7, SeekPrecision::Exact));
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
            h.sim.video_output().presented.iter().map(|(pts, _)| *pts).collect();
        assert!(presented.iter().any(|pts| *pts >= 0.7));
    }

    #[test]
    fn seek_while_paused_stays_paused() {
        let mut h = Harness::new(FakeDemux::av(2.0, 25.0, 0.04), false);
        h.run(
            400,
            DT,
            &[(10, Command::Pause(true)), (20, Command::Seek(1.0, SeekPrecision::Exact))],
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
        let frozen_video = h.sim.video_output().presented.len();
        let frozen_position = h.sim.clock_position(h.now);
        for _ in 0..50 {
            h.sim.audio_output_mut().consume(DT);
            h.sim.step(h.now);
            h.now += DT;
        }
        assert_eq!(h.sim.audio_output().log.len(), frozen_audio);
        assert_eq!(h.sim.video_output().presented.len(), frozen_video);
        assert_eq!(h.sim.clock_position(h.now), frozen_position);
        assert!(matches!(h.sim.state(), State::Paused { .. }));
        h.sim.send(Command::Pause(false));
        h.run(400, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().presented, 50);
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
        assert_eq!(h.stats().presented, 25);
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
        let presented = h.sim.video_output().presented.len();
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
        assert!(h.sim.video_output().presented.len() > presented);
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
        let presented: Vec<f64> = h.sim.video_output().presented.iter().map(|(pts, _)| *pts).collect();
        let mut sorted = presented.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(presented, sorted);
    }

    #[test]
    fn trailing_frames_present_after_eof_before_ended() {
        let mut h = Harness::new(FakeDemux::av(0.4, 25.0, 0.04), false);
        h.run(300, DT, &[]);
        assert_eq!(h.sim.state(), State::Ended);
        assert_eq!(h.stats().presented, 10);
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
}
