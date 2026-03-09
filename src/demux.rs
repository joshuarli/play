//! Demuxer thread: reads packets from a media file and sends them to the player.
//!
//! The demuxer owns the ffmpeg `Input` context and a [`PacketCache`] that stores
//! recently-read packets (up to 150 MB).  Seeks check the cache first via binary
//! search ([`PacketCache::seek_position`]); on a cache hit the demuxer replays
//! packets from memory without any file I/O.
//!
//! Multiple rapid seeks are coalesced: the demuxer drains the command channel and
//! keeps only the last seek target, sending one `Flush` per consumed seek so the
//! player can decrement its pending-seek counter correctly.

use std::collections::VecDeque;
use std::path::Path;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::media::Type;
use ffmpeg_sys_next as ffs;

use crate::cmd::{DemuxCommand, DemuxPacket};
use crate::time::pts_to_us;

const CACHE_MAX_BYTES: usize = 150 * 1024 * 1024;

/// Refcounted clone via av_packet_ref (shared buffer, no memcpy).
/// Unlike Packet::clone() which calls av_packet_make_writable (deep copy),
/// this shares the underlying AVBufferRef.
fn clone_packet_ref(src: &ffmpeg::Packet) -> ffmpeg::Packet {
    // SAFETY: as_ptr()/as_mut_ptr() return valid AVPacket pointers.
    // av_packet_ref copies metadata and increments the data buffer's refcount
    // without memcpy. The destination packet was just created empty (zeroed).
    unsafe {
        use ffmpeg::codec::packet::traits::{Mut, Ref};
        let mut dst = ffmpeg::Packet::empty();
        ffs::av_packet_ref(dst.as_mut_ptr(), src.as_ptr());
        dst
    }
}

struct CachedPacket {
    packet: ffmpeg::Packet,
    stream_index: usize,
    pts_us: i64,
    is_video_keyframe: bool,
    data_size: usize,
}

struct PacketCache {
    packets: VecDeque<CachedPacket>,
    total_bytes: usize,
    max_bytes: usize,
    video_idx: Option<usize>,
    time_bases: Vec<(usize, ffmpeg::Rational)>,
}

impl PacketCache {
    fn new(
        max_bytes: usize,
        video_idx: Option<usize>,
        audio_idx: Option<usize>,
        subtitle_idx: Option<usize>,
        ictx: &Input,
    ) -> Self {
        let mut indices = Vec::new();
        if let Some(i) = video_idx {
            indices.push(i);
        }
        if let Some(i) = audio_idx {
            indices.push(i);
        }
        if let Some(i) = subtitle_idx {
            indices.push(i);
        }
        let time_bases: Vec<(usize, ffmpeg::Rational)> = indices
            .into_iter()
            .filter_map(|i| match ictx.stream(i) {
                Some(s) => Some((i, s.time_base())),
                None => {
                    log::warn!("Stream index {i} not found in input context");
                    None
                }
            })
            .collect();
        Self {
            packets: VecDeque::new(),
            total_bytes: 0,
            max_bytes,
            video_idx,
            time_bases,
        }
    }

    fn time_base_for(&self, stream_index: usize) -> Option<ffmpeg::Rational> {
        self.time_bases
            .iter()
            .find(|(i, _)| *i == stream_index)
            .map(|(_, tb)| *tb)
    }

    fn push(&mut self, packet: &ffmpeg::Packet, stream_index: usize) {
        let tb = match self.time_base_for(stream_index) {
            Some(tb) => tb,
            None => return,
        };
        let pts_raw = packet.pts().or(packet.dts()).unwrap_or(0);
        let pts_us = pts_to_us(pts_raw, tb);
        let is_video_keyframe = Some(stream_index) == self.video_idx && packet.is_key();
        let data_size = packet.size();
        let cloned = clone_packet_ref(packet);

        self.packets.push_back(CachedPacket {
            packet: cloned,
            stream_index,
            pts_us,
            is_video_keyframe,
            data_size,
        });
        self.total_bytes += data_size;

        while self.total_bytes > self.max_bytes {
            if let Some(evicted) = self.packets.pop_front() {
                self.total_bytes -= evicted.data_size;
            } else {
                break;
            }
        }
    }

    /// Find the cache index to start replay from.
    ///
    /// - Backward seek: last video keyframe with `pts_us <= target_us`
    ///   (matches `ictx.seek(target, ..target)`)
    /// - Forward seek: first video keyframe with `pts_us >= target_us`
    ///   (matches `ictx.seek(target, target..)`)
    ///
    /// Returns None if target is outside the cached range or no suitable
    /// keyframe exists.
    fn seek_position(&self, target_us: i64, forward: bool) -> Option<usize> {
        if self.packets.is_empty() {
            return None;
        }
        let first_pts = self.packets.front().unwrap().pts_us;
        let last_pts = self.packets.back().unwrap().pts_us;
        if target_us < first_pts || target_us > last_pts {
            return None;
        }

        if forward {
            // Binary search: first packet with pts_us >= target_us
            let start = self.packets.partition_point(|cp| cp.pts_us < target_us);
            // First video keyframe at or after target
            for i in start..self.packets.len() {
                if self.packets[i].is_video_keyframe && self.packets[i].pts_us >= target_us {
                    return Some(i);
                }
            }
            // Audio-only fallback: first packet at or after target
            if self.video_idx.is_none() && start < self.packets.len() {
                return Some(start);
            }
            None
        } else {
            // Binary search: first packet with pts_us > target_us
            let end = self.packets.partition_point(|cp| cp.pts_us <= target_us);
            // Scan backward from end for last video keyframe at or before target
            for i in (0..end).rev() {
                if self.packets[i].is_video_keyframe && self.packets[i].pts_us <= target_us {
                    return Some(i);
                }
            }
            // Audio-only fallback: nearest packet at or before target
            if self.video_idx.is_none() && end > 0 {
                return Some(end - 1);
            }
            None
        }
    }

    fn clear(&mut self) {
        self.packets.clear();
        self.total_bytes = 0;
    }

    fn len(&self) -> usize {
        self.packets.len()
    }
}

/// Metadata about streams in the file.
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub video_stream: Option<VideoStreamInfo>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
    pub duration_us: i64,
    /// Container-level metadata (title, artist, album, etc.)
    pub metadata: Vec<(String, String)>,
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
        .filter(|s| {
            s.parameters().medium() == Type::Video
                && !s
                    .disposition()
                    .contains(ffmpeg::format::stream::Disposition::ATTACHED_PIC)
        })
        .max_by_key(|s| {
            // Prefer default-flagged stream, then highest resolution
            let params = s.parameters();
            let is_default = s
                .disposition()
                .contains(ffmpeg::format::stream::Disposition::DEFAULT);
            let codec = ffmpeg::codec::context::Context::from_parameters(params).ok();
            let pixels = codec
                .and_then(|c| c.decoder().video().ok())
                .map(|v| v.width() as u64 * v.height() as u64)
                .unwrap_or(0);
            (is_default as u64, pixels)
        })
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

    let metadata: Vec<(String, String)> = ictx
        .metadata()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    Ok(StreamInfo {
        video_stream,
        audio_streams,
        subtitle_streams,
        duration_us,
        metadata,
    })
}

/// Mutable demuxer state threaded through the loop, eliminating global/duplicated handling.
struct DemuxState {
    ictx: Input,
    cache: PacketCache,
    replay_cursor: Option<usize>,
    video_idx: Option<usize>,
    audio_idx: Option<usize>,
    subtitle_idx: Option<usize>,
    cmd_rx: Receiver<DemuxCommand>,
    packet_tx: Sender<DemuxPacket>,
}

/// What the demuxer loop should do after processing a command.
enum Action {
    Continue,
    Stop,
}

impl DemuxState {
    fn new(
        path: &Path,
        video_idx: Option<usize>,
        audio_idx: Option<usize>,
        subtitle_idx: Option<usize>,
        cmd_rx: Receiver<DemuxCommand>,
        packet_tx: Sender<DemuxPacket>,
    ) -> Result<Self> {
        let ictx = ffmpeg::format::input(path)
            .with_context(|| format!("Failed to open: {}", path.display()))?;

        // Tell ffmpeg to discard packets from streams we don't need.
        let wanted: [Option<usize>; 3] = [video_idx, audio_idx, subtitle_idx];
        for stream in ictx.streams() {
            if !wanted.iter().any(|&w| w == Some(stream.index())) {
                // SAFETY: stream.as_ptr() returns a valid AVStream. We cast
                // to mutable to set the discard flag, which tells the demuxer
                // to skip packets for this stream. This is safe because we
                // haven't started reading packets yet.
                unsafe {
                    let s = stream.as_ptr() as *mut ffs::AVStream;
                    (*s).discard = ffs::AVDiscard::AVDISCARD_ALL;
                }
            }
        }

        let cache = PacketCache::new(CACHE_MAX_BYTES, video_idx, audio_idx, subtitle_idx, &ictx);

        Ok(Self {
            ictx,
            cache,
            replay_cursor: None,
            video_idx,
            audio_idx,
            subtitle_idx,
            cmd_rx,
            packet_tx,
        })
    }

    /// Classify a packet by stream index into the appropriate DemuxPacket variant.
    /// Returns None for streams we don't care about.
    fn classify_packet(&self, stream_idx: usize, packet: ffmpeg::Packet) -> Option<DemuxPacket> {
        if Some(stream_idx) == self.video_idx {
            Some(DemuxPacket::Video(packet))
        } else if Some(stream_idx) == self.audio_idx {
            Some(DemuxPacket::Audio(packet))
        } else if Some(stream_idx) == self.subtitle_idx {
            Some(DemuxPacket::Subtitle(packet))
        } else {
            None
        }
    }

    /// Handle a single demux command. Returns Stop if the loop should exit.
    fn handle_command(&mut self, cmd: DemuxCommand) -> Action {
        match cmd {
            DemuxCommand::Stop => Action::Stop,
            DemuxCommand::Seek {
                target_pts,
                forward,
            } => {
                // Coalesce any additional queued seeks, keeping only the last.
                let (target, fwd, count) = match coalesce_seeks(&self.cmd_rx, target_pts, forward) {
                    Some(v) => v,
                    None => return Action::Stop,
                };
                self.replay_cursor = self.try_cached_seek(target, fwd, count);
                Action::Continue
            }
            DemuxCommand::ChangeAudio(new_idx) => {
                self.change_audio(new_idx);
                Action::Continue
            }
        }
    }

    /// Drain all pending commands, coalescing seeks. Returns Stop if a Stop
    /// command was encountered.
    fn drain_commands(&mut self) -> Action {
        let mut last_seek: Option<(i64, bool)> = None;
        let mut seek_count: u32 = 0;

        loop {
            match self.cmd_rx.try_recv() {
                Ok(DemuxCommand::Stop) => return Action::Stop,
                Ok(DemuxCommand::Seek {
                    target_pts,
                    forward,
                }) => {
                    seek_count += 1;
                    last_seek = Some((target_pts, forward));
                }
                Ok(DemuxCommand::ChangeAudio(new_idx)) => {
                    self.change_audio(new_idx);
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => return Action::Stop,
            }
        }

        if let Some((target, forward)) = last_seek {
            self.replay_cursor = self.try_cached_seek(target, forward, seek_count);
        }
        Action::Continue
    }

    /// Send a packet, racing against incoming commands. If a command arrives
    /// while the send is blocked (channel full), handle it immediately.
    /// Returns Stop if the loop should exit.
    fn send_or_handle_command(&mut self, pkt: DemuxPacket) -> Action {
        crossbeam_channel::select! {
            send(self.packet_tx, pkt) -> res => {
                if res.is_err() { return Action::Stop; }
                Action::Continue
            }
            recv(self.cmd_rx) -> msg => {
                match msg {
                    Ok(cmd) => self.handle_command(cmd),
                    Err(_) => Action::Stop,
                }
            }
        }
    }

    fn change_audio(&mut self, new_idx: usize) {
        log::debug!("Demuxer: changing audio stream to {new_idx}");
        self.audio_idx = Some(new_idx);
        self.cache.clear();
        self.cache = PacketCache::new(
            CACHE_MAX_BYTES,
            self.video_idx,
            self.audio_idx,
            self.subtitle_idx,
            &self.ictx,
        );
        self.replay_cursor = None;
    }

    /// Try to satisfy a seek from the packet cache. Returns a replay cursor if
    /// the target is within the cache, or None after falling back to a file seek.
    fn try_cached_seek(&mut self, target: i64, forward: bool, count: u32) -> Option<usize> {
        if let Some(idx) = self.cache.seek_position(target, forward) {
            log::debug!("Demuxer: cache hit at index {idx}, coalesced {count}");
            for _ in 0..count {
                let _ = self.packet_tx.send(DemuxPacket::Flush);
            }
            Some(idx)
        } else {
            log::debug!("Demuxer: cache miss, file seek");
            self.cache.clear();
            self.do_seek(target, forward, count);
            None
        }
    }

    /// Execute a seek and send the corresponding Flush packets.
    fn do_seek(&mut self, target: i64, forward: bool, count: u32) {
        log::debug!("Demuxer: seek to {target}us, coalesced {count}");
        if forward {
            let _ = self.ictx.seek(target, target..);
        } else {
            let _ = self.ictx.seek(target, ..target);
        }
        for _ in 0..count {
            let _ = self.packet_tx.send(DemuxPacket::Flush);
        }
    }
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
    let mut state = DemuxState::new(path, video_idx, audio_idx, subtitle_idx, cmd_rx, packet_tx)?;

    loop {
        // Drain all pending commands — coalesce rapid seeks into one.
        if matches!(state.drain_commands(), Action::Stop) {
            return Ok(());
        }

        // Replay from cache
        if let Some(cursor) = state.replay_cursor {
            if cursor < state.cache.len() {
                let cp = &state.cache.packets[cursor];
                let stream_idx = cp.stream_index;
                let pkt = clone_packet_ref(&cp.packet);

                let Some(demux_pkt) = state.classify_packet(stream_idx, pkt) else {
                    state.replay_cursor = Some(cursor + 1);
                    continue;
                };

                if matches!(state.send_or_handle_command(demux_pkt), Action::Stop) {
                    return Ok(());
                }
                // Only advance cursor if we actually sent the packet (not preempted by seek)
                if state.replay_cursor == Some(cursor) {
                    state.replay_cursor = Some(cursor + 1);
                }
            } else {
                // Replay exhausted — resume reading from ictx (already positioned)
                state.replay_cursor = None;
            }
            continue;
        }

        // Normal read from file
        match read_next_packet(&mut state.ictx) {
            Some((stream_idx, packet)) => {
                state.cache.push(&packet, stream_idx);

                let Some(demux_pkt) = state.classify_packet(stream_idx, packet) else {
                    continue;
                };

                if matches!(state.send_or_handle_command(demux_pkt), Action::Stop) {
                    return Ok(());
                }
            }
            None => {
                let _ = state.packet_tx.send(DemuxPacket::Eof);
                log::debug!("Demuxer: EOF");

                // Block until a command arrives (seek back or stop).
                match state.cmd_rx.recv() {
                    Ok(cmd) => {
                        if matches!(state.handle_command(cmd), Action::Stop) {
                            return Ok(());
                        }
                    }
                    Err(_) => return Ok(()),
                }
            }
        }
    }
}

/// Drain additional seeks from the command channel, keeping only the last.
/// Returns None if a Stop was consumed (caller must exit).
fn coalesce_seeks(
    cmd_rx: &Receiver<DemuxCommand>,
    target_pts: i64,
    forward: bool,
) -> Option<(i64, bool, u32)> {
    let mut t = target_pts;
    let mut f = forward;
    let mut n: u32 = 1;
    while let Ok(cmd) = cmd_rx.try_recv() {
        match cmd {
            DemuxCommand::Stop => return None,
            DemuxCommand::Seek {
                target_pts: t2,
                forward: f2,
            } => {
                t = t2;
                f = f2;
                n += 1;
            }
            DemuxCommand::ChangeAudio(_) => {
                // ChangeAudio is handled at the top of the loop; ignore during coalesce
            }
        }
    }
    Some((t, f, n))
}

fn read_next_packet(ictx: &mut Input) -> Option<(usize, ffmpeg::Packet)> {
    ictx.packets()
        .next()
        .map(|(stream, packet)| (stream.index(), packet))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ffmpeg::codec::packet::Flags;

    /// Build a PacketCache without needing an Input context.
    fn test_cache(max_bytes: usize, video_idx: Option<usize>) -> PacketCache {
        // Use 1/1000000 time base so PTS values == microseconds directly
        let tb = ffmpeg::Rational::new(1, 1_000_000);
        let mut time_bases = Vec::new();
        if let Some(i) = video_idx {
            time_bases.push((i, tb));
        }
        // Also add audio stream index 1 if video is 0
        if video_idx == Some(0) {
            time_bases.push((1, tb));
        }
        PacketCache {
            packets: VecDeque::new(),
            total_bytes: 0,
            max_bytes,
            video_idx,
            time_bases,
        }
    }

    /// Build a test CachedPacket without FFI (empty packet, size tracked manually).
    fn test_cached_packet(
        stream_index: usize,
        pts_us: i64,
        is_video_keyframe: bool,
        data_size: usize,
    ) -> CachedPacket {
        let mut pkt = ffmpeg::Packet::empty();
        pkt.set_pts(Some(pts_us));
        if is_video_keyframe {
            pkt.set_flags(Flags::KEY);
        }
        pkt.set_stream(stream_index);
        CachedPacket {
            packet: pkt,
            stream_index,
            pts_us,
            is_video_keyframe,
            data_size,
        }
    }

    fn push_test_packet(
        cache: &mut PacketCache,
        stream_index: usize,
        pts_us: i64,
        is_video_keyframe: bool,
        data_size: usize,
    ) {
        let cp = test_cached_packet(stream_index, pts_us, is_video_keyframe, data_size);
        cache.total_bytes += cp.data_size;
        cache.packets.push_back(cp);
        while cache.total_bytes > cache.max_bytes {
            if let Some(evicted) = cache.packets.pop_front() {
                cache.total_bytes -= evicted.data_size;
            } else {
                break;
            }
        }
    }

    // --- PacketCache::seek_position ---

    #[test]
    fn seek_position_empty_cache() {
        let cache = test_cache(1024, Some(0));
        assert_eq!(cache.seek_position(1_000_000, false), None);
        assert_eq!(cache.seek_position(1_000_000, true), None);
    }

    #[test]
    fn seek_position_target_before_cache() {
        let mut cache = test_cache(1024, Some(0));
        push_test_packet(&mut cache, 0, 5_000_000, true, 100);
        push_test_packet(&mut cache, 0, 10_000_000, false, 100);
        assert_eq!(cache.seek_position(1_000_000, false), None);
        assert_eq!(cache.seek_position(1_000_000, true), None);
    }

    #[test]
    fn seek_position_target_after_cache() {
        let mut cache = test_cache(1024, Some(0));
        push_test_packet(&mut cache, 0, 5_000_000, true, 100);
        push_test_packet(&mut cache, 0, 10_000_000, false, 100);
        assert_eq!(cache.seek_position(20_000_000, false), None);
        assert_eq!(cache.seek_position(20_000_000, true), None);
    }

    #[test]
    fn seek_position_backward_finds_keyframe_before() {
        let mut cache = test_cache(4096, Some(0));
        push_test_packet(&mut cache, 0, 0, true, 100);
        push_test_packet(&mut cache, 0, 1_000_000, false, 100);
        push_test_packet(&mut cache, 0, 2_000_000, true, 100);
        push_test_packet(&mut cache, 0, 3_000_000, false, 100);
        push_test_packet(&mut cache, 0, 4_000_000, true, 100);

        // Backward seek to 2.5s → keyframe at 2s (index 2)
        assert_eq!(cache.seek_position(2_500_000, false), Some(2));
        // Backward seek to exactly 4s → keyframe at 4s (index 4)
        assert_eq!(cache.seek_position(4_000_000, false), Some(4));
        // Backward seek to 0 → keyframe at 0 (index 0)
        assert_eq!(cache.seek_position(0, false), Some(0));
    }

    #[test]
    fn seek_position_forward_finds_keyframe_after() {
        let mut cache = test_cache(4096, Some(0));
        push_test_packet(&mut cache, 0, 0, true, 100);
        push_test_packet(&mut cache, 0, 1_000_000, false, 100);
        push_test_packet(&mut cache, 0, 2_000_000, true, 100);
        push_test_packet(&mut cache, 0, 3_000_000, false, 100);
        push_test_packet(&mut cache, 0, 4_000_000, true, 100);

        // Forward seek to 0.5s → first keyframe >= 0.5s is at 2s (index 2)
        assert_eq!(cache.seek_position(500_000, true), Some(2));
        // Forward seek to exactly 2s → keyframe at 2s (index 2)
        assert_eq!(cache.seek_position(2_000_000, true), Some(2));
        // Forward seek to 3.5s → keyframe at 4s (index 4)
        assert_eq!(cache.seek_position(3_500_000, true), Some(4));
        // Forward seek to exactly 4s → keyframe at 4s (index 4)
        assert_eq!(cache.seek_position(4_000_000, true), Some(4));
    }

    #[test]
    fn seek_position_forward_miss_no_keyframe_after() {
        let mut cache = test_cache(4096, Some(0));
        push_test_packet(&mut cache, 0, 0, true, 100);
        push_test_packet(&mut cache, 0, 1_000_000, false, 100);
        push_test_packet(&mut cache, 0, 2_000_000, false, 100);

        // Forward seek to 0.5s — no keyframe >= 0.5s → cache miss
        assert_eq!(cache.seek_position(500_000, true), None);
    }

    #[test]
    fn seek_position_backward_miss_no_keyframe_before() {
        let mut cache = test_cache(4096, Some(0));
        push_test_packet(&mut cache, 0, 0, false, 100); // not a keyframe
        push_test_packet(&mut cache, 0, 1_000_000, false, 100);
        push_test_packet(&mut cache, 0, 2_000_000, true, 100);

        // Backward seek to 1.5s — no keyframe <= 1.5s → cache miss
        assert_eq!(cache.seek_position(1_500_000, false), None);
    }

    #[test]
    fn seek_position_audio_only_backward() {
        let mut cache = test_cache(4096, None);
        cache
            .time_bases
            .push((1, ffmpeg::Rational::new(1, 1_000_000)));
        push_test_packet(&mut cache, 1, 0, false, 100);
        push_test_packet(&mut cache, 1, 1_000_000, false, 100);
        push_test_packet(&mut cache, 1, 2_000_000, false, 100);

        // Backward: packet at or before 1.5s → index 1
        assert_eq!(cache.seek_position(1_500_000, false), Some(1));
    }

    #[test]
    fn seek_position_audio_only_forward() {
        let mut cache = test_cache(4096, None);
        cache
            .time_bases
            .push((1, ffmpeg::Rational::new(1, 1_000_000)));
        push_test_packet(&mut cache, 1, 0, false, 100);
        push_test_packet(&mut cache, 1, 1_000_000, false, 100);
        push_test_packet(&mut cache, 1, 2_000_000, false, 100);

        // Forward: packet at or after 0.5s → index 1
        assert_eq!(cache.seek_position(500_000, true), Some(1));
    }

    // --- PacketCache push + eviction ---

    #[test]
    fn push_evicts_when_over_budget() {
        let mut cache = test_cache(250, Some(0));
        push_test_packet(&mut cache, 0, 0, true, 100);
        push_test_packet(&mut cache, 0, 1_000_000, false, 100);
        assert_eq!(cache.packets.len(), 2);
        assert_eq!(cache.total_bytes, 200);

        // This push exceeds 250 bytes, should evict the oldest
        push_test_packet(&mut cache, 0, 2_000_000, true, 100);
        assert_eq!(cache.packets.len(), 2);
        assert_eq!(cache.total_bytes, 200);
        assert_eq!(cache.packets.front().unwrap().pts_us, 1_000_000);
    }

    #[test]
    fn push_tracks_total_bytes() {
        let mut cache = test_cache(10000, Some(0));
        push_test_packet(&mut cache, 0, 0, true, 500);
        push_test_packet(&mut cache, 0, 1_000_000, false, 300);
        push_test_packet(&mut cache, 0, 2_000_000, true, 200);
        assert_eq!(cache.total_bytes, 1000);
        assert_eq!(cache.packets.len(), 3);
    }

    // --- PacketCache::clear ---

    #[test]
    fn clear_resets_state() {
        let mut cache = test_cache(10000, Some(0));
        push_test_packet(&mut cache, 0, 0, true, 500);
        push_test_packet(&mut cache, 0, 1_000_000, false, 300);
        cache.clear();
        assert_eq!(cache.packets.len(), 0);
        assert_eq!(cache.total_bytes, 0);
    }

    // --- coalesce_seeks ---

    #[test]
    fn coalesce_single_seek() {
        let (_, rx) = crossbeam_channel::unbounded::<DemuxCommand>();
        let result = coalesce_seeks(&rx, 5_000_000, true);
        assert_eq!(result, Some((5_000_000, true, 1)));
    }

    #[test]
    fn coalesce_multiple_seeks_keeps_last() {
        let (tx, rx) = crossbeam_channel::unbounded::<DemuxCommand>();
        tx.send(DemuxCommand::Seek {
            target_pts: 10_000_000,
            forward: true,
        })
        .unwrap();
        tx.send(DemuxCommand::Seek {
            target_pts: 15_000_000,
            forward: false,
        })
        .unwrap();
        tx.send(DemuxCommand::Seek {
            target_pts: 20_000_000,
            forward: true,
        })
        .unwrap();

        let result = coalesce_seeks(&rx, 5_000_000, true);
        assert_eq!(result, Some((20_000_000, true, 4)));
    }

    #[test]
    fn coalesce_returns_none_on_stop() {
        let (tx, rx) = crossbeam_channel::unbounded::<DemuxCommand>();
        tx.send(DemuxCommand::Seek {
            target_pts: 10_000_000,
            forward: true,
        })
        .unwrap();
        tx.send(DemuxCommand::Stop).unwrap();
        tx.send(DemuxCommand::Seek {
            target_pts: 99_000_000,
            forward: true,
        })
        .unwrap();

        let result = coalesce_seeks(&rx, 5_000_000, true);
        assert_eq!(result, None);
    }

    #[test]
    fn coalesce_empty_channel() {
        let (tx, rx) = crossbeam_channel::unbounded::<DemuxCommand>();
        drop(tx);
        // Disconnected channel — no messages to drain
        let result = coalesce_seeks(&rx, 1_000_000, false);
        assert_eq!(result, Some((1_000_000, false, 1)));
    }
}
