use std::ffi::{c_int, c_void, CStr, CString};
use std::marker::PhantomData;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;

use player_core::input::classify_input;
use player_core::local_file::{LocalFile, SeekWhence};

use crate::error::MediaError;
use crate::sys::*;

const AVIO_BUFFER_BYTES: usize = 32 * 1024;

struct ReaderState {
    file: LocalFile,
}

unsafe extern "C" fn io_read(
    opaque: *mut c_void,
    buf: *mut u8,
    buf_size: c_int,
) -> c_int {
    // SAFETY: opaque is the ReaderState pointer the session installed, and
    // buf has buf_size writable bytes for the call.
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let state = &mut *(opaque as *mut ReaderState);
        let out = slice::from_raw_parts_mut(buf, buf_size as usize);
        match state.file.read(out) {
            Ok(0) => AVERROR_EOF,
            Ok(read) => read as c_int,
            Err(_) => -5,
        }
    }));
    result.unwrap_or(-5)
}

unsafe extern "C" fn io_seek(
    opaque: *mut c_void,
    offset: i64,
    whence: c_int,
) -> i64 {
    // SAFETY: opaque is the ReaderState pointer the session installed.
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let state = &mut *(opaque as *mut ReaderState);
        if whence == AVSEEK_SIZE {
            return state.file.length().map(|length| length as i64).unwrap_or(-1);
        }
        let seek_whence = match whence {
            SEEK_SET => SeekWhence::Start,
            SEEK_CUR => SeekWhence::Current,
            SEEK_END => SeekWhence::End,
            _ => return -1,
        };
        state
            .file
            .seek(offset, seek_whence)
            .map(|position| position as i64)
            .unwrap_or(-1)
    }));
    result.unwrap_or(-1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Attachment,
    Data,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub index: usize,
    pub kind: StreamKind,
    pub codec_name: String,
    pub width: i32,
    pub height: i32,
    pub sample_rate: i32,
    pub channels: i32,
    pub time_base: (i32, i32),
    pub extradata: Vec<u8>,
    pub channel_mask: u64,
    pub attached: bool,
    pub language: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChapterInfo {
    pub id: i64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub stream_index: usize,
    pub codec_name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub stream_index: usize,
    pub pts_seconds: Option<f64>,
    pub dts_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

pub struct MediaSession {
    context: *mut AvFormatContext,
    io_context: *mut AvIoContext,
    reader: *mut ReaderState,
    time_bases: Vec<(i32, i32)>,
    _no_send: PhantomData<*mut ()>,
}

impl MediaSession {
    pub fn open(path: &str) -> Result<Self, MediaError> {
        let version = unsafe { avformat_version() };
        if (version >> 16) & 0xff != AVFORMAT_VERSION_MAJOR {
            return Err(MediaError::Unsupported(format!(
                "libavformat major {} is not {}",
                (version >> 16) & 0xff,
                AVFORMAT_VERSION_MAJOR
            )));
        }
        let local = match classify_input(path, false) {
            Ok(player_core::input::InputPath::Local(resolved)) => resolved,
            Err(error) => return Err(MediaError::Rejected(error.to_string())),
        };
        let file = LocalFile::open_read(&local).map_err(|error| MediaError::Open(error.to_string()))?;
        // SAFETY: avformat_alloc_context returns a live context or null.
        let context = unsafe { avformat_alloc_context() };
        if context.is_null() {
            return Err(MediaError::Open("avformat_alloc_context failed".to_owned()));
        }
        let reader = Box::into_raw(Box::new(ReaderState { file }));
        // The buffer is a lavf allocation: probing may reallocate it, so the
        // session frees whatever pointer the IO context holds at teardown.
        // SAFETY: av_malloc returns a live allocation or null.
        let io_buffer = unsafe { av_malloc(AVIO_BUFFER_BYTES) } as *mut u8;
        if io_buffer.is_null() {
            // SAFETY: reader came from Box::into_raw above and is owned here.
            drop(unsafe { Box::from_raw(reader) });
            // SAFETY: context is a live allocated context.
            unsafe { avformat_free_context(context) };
            return Err(MediaError::Open("av_malloc failed".to_owned()));
        }
        // SAFETY: the buffer and reader outlive the IO context; the session
        // frees all three in Drop.
        let mut io_context = unsafe {
            avio_alloc_context(
                io_buffer,
                AVIO_BUFFER_BYTES as c_int,
                0,
                reader as *mut c_void,
                Some(io_read),
                None,
                Some(io_seek),
            )
        };
        if io_context.is_null() {
            // SAFETY: reader came from Box::into_raw above and is owned here.
            drop(unsafe { Box::from_raw(reader) });
            // SAFETY: io_buffer is a live av_malloc allocation.
            unsafe { av_free(io_buffer as *mut c_void) };
            // SAFETY: context is a live allocated context.
            unsafe { avformat_free_context(context) };
            return Err(MediaError::Open("avio_alloc_context failed".to_owned()));
        }
        // SAFETY: context is live and owned; pb and flags are the mapped fields.
        unsafe {
            (*context).pb = io_context;
            (*context).flags |= AVFMT_FLAG_CUSTOM_IO;
        }
        let mut context_ptr = context;
        // SAFETY: null URL with a custom IO context probes the stream; the
        // path never reaches FFmpeg, so no protocol can resolve it.
        // The filename is a probing hint only: the preset custom IO context
        // means libavformat never opens it, so no protocol can resolve it.
        let url = CString::new(path).ok();
        let status = unsafe {
            avformat_open_input(
                &mut context_ptr,
                url.as_ref().map(|url| url.as_ptr()).unwrap_or(ptr::null()),
                ptr::null(),
                ptr::null_mut(),
            )
        };
        if status < 0 {
            // open_input consumes the context on failure and nulls the
            // pointer (proven against the pinned build in .work/tmp/s2probe):
            // never touch it here. The custom IO context was never adopted,
            // so it stays ours; free the current buffer pointer, which
            // probing may have reallocated.
            Self::free_io(io_context);
            // SAFETY: reader came from Box::into_raw above and is owned here.
            drop(unsafe { Box::from_raw(reader) });
            return Err(MediaError::Format(format!("avformat_open_input: {status}")));
        }
        // SAFETY: open_input replaced the pointer with the live session context.
        let context = context_ptr;
        let mut time_bases = Vec::new();
        // SAFETY: nb_streams/streams are valid after a successful open.
        let stream_count = unsafe { (*context).nb_streams as usize };
        if stream_count == 0 || stream_count > MAX_STREAMS {
            Self::close(context, io_context, reader);
            return Err(MediaError::Format("no usable streams".to_owned()));
        }
        for index in 0..stream_count {
            // SAFETY: index is below nb_streams.
            let time_base = unsafe { (*(*((*context).streams.add(index)))).time_base };
            time_bases.push((time_base.num, time_base.den));
        }
        // Stream info is best-effort: a failing probe still leaves a usable
        // context for containers that need none, but treat failure as an
        // error so truncated headers surface here instead of mid-playback.
        // SAFETY: context is live and exclusively owned.
        if unsafe { avformat_find_stream_info(context, ptr::null_mut()) } < 0 {
            Self::close(context, io_context, reader);
            return Err(MediaError::Format("avformat_find_stream_info failed".to_owned()));
        }
        Ok(Self {
            context,
            io_context,
            reader,
            time_bases,
            _no_send: PhantomData,
        })
    }

    /// Frees a custom IO context and its current buffer. The buffer pointer
    /// is always read back from the live context because probing may have
    /// reallocated it; freeing the original allocation instead would corrupt
    /// the heap.
    fn free_io(io_context: *mut AvIoContext) {
        // SAFETY: the buffer pointer is read from the live IO context.
        unsafe { av_free((*io_context).buffer as *mut c_void) };
        // SAFETY: the IO context outlived the buffer free above.
        unsafe { av_free(io_context as *mut c_void) };
    }

    fn close(
        context: *mut AvFormatContext,
        io_context: *mut AvIoContext,
        reader: *mut ReaderState,
    ) {
        let mut context_ptr = context;
        // SAFETY: all three parts are owned by the closing session.
        unsafe { avformat_close_input(&mut context_ptr) };
        // Mirror demux/demux_lavf.c: free the buffer, then the context.
        Self::free_io(io_context);
        // SAFETY: reader came from Box::into_raw in open and is owned here.
        drop(unsafe { Box::from_raw(reader) });
    }

    pub fn streams(&self) -> Vec<StreamInfo> {
        let mut out = Vec::new();
        // SAFETY: streams/nb_streams are valid for the open session.
        let count = unsafe { (*self.context).nb_streams as usize }.min(MAX_STREAMS);
        for index in 0..count {
            // SAFETY: index is below nb_streams; codecpar is set by the probe.
            let stream = unsafe { *(*self.context).streams.add(index) };
            let codecpar = unsafe { (*stream).codecpar };
            if codecpar.is_null() {
                continue;
            }
            // SAFETY: codecpar fields are plain data set by the probe.
            let params = unsafe { &*codecpar };
            let kind = match params.codec_type {
                AVMEDIA_TYPE_VIDEO => StreamKind::Video,
                AVMEDIA_TYPE_AUDIO => StreamKind::Audio,
                AVMEDIA_TYPE_SUBTITLE => StreamKind::Subtitle,
                AVMEDIA_TYPE_ATTACHMENT => StreamKind::Attachment,
                AVMEDIA_TYPE_DATA => StreamKind::Data,
                _ => StreamKind::Unknown,
            };
            // SAFETY: descriptor lookup only reads static tables.
            let codec_name = unsafe {
                let descriptor = avcodec_descriptor_get(params.codec_id);
                if descriptor.is_null() || (*descriptor).name.is_null() {
                    format!("unknown-{}", params.codec_id)
                } else {
                    CStr::from_ptr((*descriptor).name).to_string_lossy().into_owned()
                }
            };
            let time_base = self.time_bases.get(index).copied().unwrap_or((0, 0));
            // SAFETY: metadata reads only the stream dictionary.
            let language = unsafe { dict_value((*stream).metadata, "language") };
            let title = unsafe { dict_value((*stream).metadata, "title") };
            out.push(StreamInfo {
                index,
                kind,
                codec_name,
                width: params.width,
                height: params.height,
                sample_rate: params.sample_rate,
                // SAFETY: ch_layout is plain data set by the probe.
                channels: unsafe { (*codecpar).ch_layout.nb_channels },
                // SAFETY: the mask is valid when the layout order is native.
                channel_mask: unsafe {
                    if (*codecpar).ch_layout.order == 1 {
                        (*codecpar).ch_layout.u.mask
                    } else {
                        0
                    }
                },
                time_base,
                // SAFETY: extradata is plain data set by the probe.
                extradata: unsafe {
                    if (*codecpar).extradata.is_null() || (*codecpar).extradata_size <= 0 {
                        Vec::new()
                    } else {
                        std::slice::from_raw_parts(
                            (*codecpar).extradata as *const u8,
                            (*codecpar).extradata_size as usize,
                        )
                        .to_vec()
                    }
                },
                // SAFETY: disposition is plain data set by the probe.
                attached: unsafe { (*stream).disposition } & AV_DISPOSITION_ATTACHED_PIC != 0,
                language,
                title,
            });
        }
        out
    }

    pub fn metadata(&self) -> Vec<(String, String)> {
        // SAFETY: the format dictionary is valid for the open session.
        unsafe { dict_entries((*self.context).metadata) }
    }

    pub fn duration_seconds(&self) -> Option<f64> {
        // SAFETY: duration is plain data set by the probe.
        let duration = unsafe { (*self.context).duration };
        if duration < 0 {
            None
        } else {
            Some(duration as f64 / 1_000_000.0)
        }
    }

    pub fn chapters(&self) -> Vec<ChapterInfo> {
        let mut out = Vec::new();
        // SAFETY: chapters/nb_chapters are valid for the open session.
        let count = unsafe { (*self.context).nb_chapters as usize }.min(MAX_CHAPTERS);
        for index in 0..count {
            // SAFETY: index is below nb_chapters.
            let chapter = unsafe { *(*self.context).chapters.add(index) };
            // SAFETY: chapter fields are plain data.
            let (id, time_base, start, end, metadata) = unsafe {
                (
                    (*chapter).id,
                    (*chapter).time_base,
                    (*chapter).start,
                    (*chapter).end,
                    (*chapter).metadata,
                )
            };
            let seconds = |value: i64| {
                if time_base.den == 0 {
                    None
                } else {
                    Some(value as f64 * time_base.num as f64 / time_base.den as f64)
                }
            };
            let (Some(start_seconds), Some(end_seconds)) = (seconds(start), seconds(end)) else {
                continue;
            };
            // SAFETY: chapter metadata reads only the chapter dictionary.
            let title = unsafe { dict_value(metadata, "title") };
            out.push(ChapterInfo {
                id,
                start_seconds,
                end_seconds,
                title,
            });
        }
        out
    }

    pub fn attachments(&self) -> Result<Vec<Attachment>, MediaError> {
        let mut out = Vec::new();
        for stream in self.streams() {
            if !stream.attached {
                continue;
            }
            // SAFETY: index is a valid stream number from streams().
            let stream_ptr = unsafe { *(*self.context).streams.add(stream.index) };
            let packet = unsafe { &(*stream_ptr).attached_pic };
            // SAFETY: attached_pic data is owned by the open session.
            if packet.data.is_null() || packet.size <= 0 {
                continue;
            }
            let size = packet.size as usize;
            if size > MAX_PACKET_BYTES {
                return Err(MediaError::TooLarge("attachment"));
            }
            // SAFETY: data has size readable bytes for the open session.
            let data = unsafe { slice::from_raw_parts(packet.data, size) }.to_vec();
            out.push(Attachment {
                stream_index: stream.index,
                codec_name: stream.codec_name.clone(),
                data,
            });
        }
        Ok(out)
    }

    pub fn read_packet(&mut self) -> Result<Packet, MediaError> {
        // SAFETY: av_packet_alloc returns a live packet or null.
        let packet = unsafe { av_packet_alloc() };
        if packet.is_null() {
            return Err(MediaError::Io("av_packet_alloc failed".to_owned()));
        }
        // SAFETY: packet and context are live; read_packet fills the packet.
        let status = unsafe { av_read_frame(self.context, packet) };
        if status == AVERROR_EOF {
            // SAFETY: packet is live and owned here.
            unsafe { av_packet_free(&mut (packet as *mut AvPacket)) };
            return Err(MediaError::Eof);
        }
        if status < 0 {
            // SAFETY: packet is live and owned here.
            unsafe { av_packet_free(&mut (packet as *mut AvPacket)) };
            return Err(MediaError::Format(format!("av_read_frame: {status}")));
        }
        // SAFETY: the packet was filled by av_read_frame.
        let (stream_index, pts, dts, duration, flags, data_ptr, size) = unsafe {
            (
                (*packet).stream_index,
                (*packet).pts,
                (*packet).dts,
                (*packet).duration,
                (*packet).flags,
                (*packet).data,
                (*packet).size,
            )
        };
        if stream_index < 0
            || stream_index as usize >= self.time_bases.len()
            || size < 0
            || size as usize > MAX_PACKET_BYTES
        {
            // SAFETY: packet is live and owned here.
            unsafe { av_packet_free(&mut (packet as *mut AvPacket)) };
            return Err(MediaError::Format("packet out of bounds".to_owned()));
        }
        let time_base = self.time_bases[stream_index as usize];
        let seconds = |value: i64| {
            if value < 0 || time_base.1 == 0 {
                None
            } else {
                Some(value as f64 * time_base.0 as f64 / time_base.1 as f64)
            }
        };
        let result = if data_ptr.is_null() {
            Packet {
                stream_index: stream_index as usize,
                pts_seconds: seconds(pts),
                dts_seconds: seconds(dts),
                duration_seconds: seconds(duration),
                keyframe: flags & 0x0001 != 0,
                data: Vec::new(),
            }
        } else {
            // SAFETY: data has size readable bytes until unref below.
            let data = unsafe { slice::from_raw_parts(data_ptr, size as usize) }.to_vec();
            Packet {
                stream_index: stream_index as usize,
                pts_seconds: seconds(pts),
                dts_seconds: seconds(dts),
                duration_seconds: seconds(duration),
                keyframe: flags & 0x0001 != 0,
                data,
            }
        };
        // SAFETY: packet is live and owned here.
        unsafe { av_packet_free(&mut (packet as *mut AvPacket)) };
        Ok(result)
    }

    pub fn seek_seconds(&mut self, seconds: f64) -> Result<(), MediaError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(MediaError::Seek("invalid seek target".to_owned()));
        }
        let target = (seconds * 1_000_000.0) as i64;
        // SAFETY: the context is live; stream -1 seeks by presentation time.
        let status = unsafe {
            avformat_seek_file(self.context, -1, i64::MIN, target, i64::MAX, 0)
        };
        if status < 0 {
            return Err(MediaError::Seek(format!("avformat_seek_file: {status}")));
        }
        Ok(())
    }

    /// Raw codec parameters for a stream index. The pointer stays valid
    /// while the session lives; the caller must not free it.
    pub fn codecpar_ptr(&self, stream_index: usize) -> Option<*const AvCodecParameters> {
        // SAFETY: streams/nb_streams are valid for the open session.
        let count = unsafe { (*self.context).nb_streams as usize }.min(MAX_STREAMS);
        if stream_index >= count {
            return None;
        }
        // SAFETY: index is below nb_streams; codecpar is set by the probe.
        let stream = unsafe { *(*self.context).streams.add(stream_index) };
        let codecpar = unsafe { (*stream).codecpar };
        if codecpar.is_null() {
            None
        } else {
            Some(codecpar)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");
        let dir = root.join(".work/fixtures");
        assert!(
            dir.join("h264.mp4").is_file(),
            "run tests/fixtures/gen.sh first"
        );
        dir
    }

    fn fixture(name: &str) -> String {
        fixtures().join(name).to_string_lossy().into_owned()
    }

    fn open(name: &str) -> MediaSession {
        MediaSession::open(&fixture(name)).expect("fixture opens")
    }

    #[test]
    fn opens_and_lists_av_streams() {
        let session = open("h264.mp4");
        let streams = session.streams();
        let kinds: HashSet<_> = streams.iter().map(|stream| stream.kind).collect();
        assert!(kinds.contains(&StreamKind::Video));
        assert!(kinds.contains(&StreamKind::Audio));
        let video = streams.iter().find(|stream| stream.kind == StreamKind::Video).unwrap();
        assert_eq!(video.codec_name, "h264");
        assert_eq!((video.width, video.height), (320, 240));
        assert!(video.time_base.1 > 0, "video time base {:?}", video.time_base);
        let audio = streams.iter().find(|stream| stream.kind == StreamKind::Audio).unwrap();
        assert_eq!(audio.codec_name, "aac");
        assert_eq!(audio.sample_rate, 48000);
        assert_eq!(audio.channels, 2);
        let duration = session.duration_seconds().expect("duration");
        assert!((duration - 3.0).abs() < 0.2, "duration {duration}");
    }

    #[test]
    fn lists_chapters_with_titles() {
        let session = open("chapters.mkv");
        let chapters = session.chapters();
        assert_eq!(chapters.len(), 3);
        let titles: Vec<_> = chapters.iter().map(|chapter| chapter.title.clone()).collect();
        assert_eq!(
            titles,
            [Some("Intro".to_owned()), Some("Middle".to_owned()), Some("End".to_owned())]
        );
        assert!((chapters[0].start_seconds - 0.0).abs() < 0.05);
        assert!((chapters[1].start_seconds - 1.0).abs() < 0.05);
        assert!((chapters[2].end_seconds - 3.0).abs() < 0.05);
    }

    #[test]
    fn lists_multiple_audio_tracks() {
        let session = open("multiaudio.mkv");
        let audio: Vec<_> = session
            .streams()
            .into_iter()
            .filter(|stream| stream.kind == StreamKind::Audio)
            .collect();
        assert_eq!(audio.len(), 2);
        assert_eq!(audio[0].language.as_deref(), Some("eng"));
        assert_eq!(audio[1].language.as_deref(), Some("jpn"));
    }

    #[test]
    fn reads_packets_and_hits_eof() {
        let mut session = open("h264.mp4");
        let mut video_packets = 0;
        let mut audio_packets = 0;
        let mut keyframes = 0;
        loop {
            match session.read_packet() {
                Ok(packet) => {
                    let streams = session.streams();
                    let kind = streams[packet.stream_index].kind;
                    match kind {
                        StreamKind::Video => video_packets += 1,
                        StreamKind::Audio => audio_packets += 1,
                        _ => {}
                    }
                    if packet.keyframe {
                        keyframes += 1;
                    }
                    assert!(!packet.data.is_empty());
                }
                Err(MediaError::Eof) => break,
                Err(error) => panic!("packet read failed: {error}"),
            }
        }
        assert!(video_packets > 0, "video packets flow");
        assert!(audio_packets > 0, "audio packets flow");
        assert!(keyframes > 0, "keyframes flagged");
    }

    #[test]
    fn seeks_and_reads_forward() {
        let mut session = open("h264.mp4");
        session.seek_seconds(1.5).expect("seek works");
        let mut video_after_seek = 0;
        for _ in 0..200 {
            match session.read_packet() {
                Ok(packet) => {
                    if session.streams()[packet.stream_index].kind == StreamKind::Video
                        && packet.pts_seconds.unwrap_or(0.0) >= 1.0
                    {
                        video_after_seek += 1;
                    }
                }
                Err(MediaError::Eof) => break,
                Err(error) => panic!("packet read failed: {error}"),
            }
        }
        assert!(video_after_seek > 0, "video resumes after seek");
        assert!(session.seek_seconds(f64::NAN).is_err());
        assert!(session.seek_seconds(-1.0).is_err());
    }

    #[test]
    fn extracts_cover_art() {
        for name in ["audio-cover.flac", "audio-cover-jpeg.mp3"] {
            let session = open(name);
            let attachments = session.attachments().expect("attachments read");
            assert_eq!(attachments.len(), 1, "{name} has cover art");
            let art = &attachments[0].data;
            let png = art.starts_with(b"\x89PNG\r\n\x1a\n");
            let jpeg = art.starts_with(b"\xff\xd8\xff");
            assert!(png || jpeg, "{name} has image magic");
        }
    }

    #[test]
    fn rejects_nonlocal_input() {
        for input in [
            "https://example.com/a.mp4",
            "file:///etc/passwd",
            "smb://host/share/a.mkv",
            "",
        ] {
            assert!(
                matches!(MediaSession::open(input), Err(MediaError::Rejected(_))),
                "{input} is rejected"
            );
        }
        assert!(matches!(
            MediaSession::open(&fixture("missing-nope.mp4")),
            Err(MediaError::Open(_))
        ));
    }

    #[test]
    fn handles_malformed_input_without_panicking() {
        let mut truncated = open("truncated.mp4");
        loop {
            match truncated.read_packet() {
                Ok(_) => {}
                Err(MediaError::Eof) | Err(MediaError::Format(_)) => break,
                Err(error) => panic!("truncated ends cleanly, got {error}"),
            }
        }
        match MediaSession::open(&fixture("corrupt.mkv")) {
            Ok(mut session) => loop {
                match session.read_packet() {
                    Ok(_) => {}
                    Err(MediaError::Eof) | Err(MediaError::Format(_)) => break,
                    Err(error) => panic!("corrupt ends cleanly, got {error}"),
                }
            },
            Err(MediaError::Format(_)) => {}
            Err(error) => panic!("corrupt opens or fails cleanly, got {error}"),
        }
    }

    #[test]
    fn exposes_track_metadata() {
        let session = open("audio-cover.flac");
        let audio: Vec<_> = session
            .streams()
            .into_iter()
            .filter(|stream| stream.kind == StreamKind::Audio)
            .collect();
        assert_eq!(audio.len(), 1);
        assert_eq!(audio[0].codec_name, "flac");
        let titles: Vec<_> = session
            .metadata()
            .into_iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("title"))
            .map(|(_, value)| value)
            .collect();
        assert!(
            titles.iter().any(|title| title.contains("Paper Satellites")),
            "cover title tagged: {titles:?}"
        );
    }
}

impl Drop for MediaSession {
    fn drop(&mut self) {
        Self::close(self.context, self.io_context, self.reader);
    }
}

/// Reads one dictionary value. The caller guarantees `dict` is valid; a null
/// dictionary reads as absent.
unsafe fn dict_value(dict: *mut AvDictionary, key: &str) -> Option<String> {
    let key = CString::new(key).ok()?;
    // SAFETY: dict is a valid dictionary; key is NUL-terminated.
    let entry = unsafe { av_dict_get(dict, key.as_ptr(), ptr::null(), 0) };
    if entry.is_null() {
        return None;
    }
    // SAFETY: key/value are NUL-terminated strings owned by the dictionary,
    // which outlives the returned owned copy.
    unsafe { CStr::from_ptr((*entry).value) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Drains a dictionary in order, capped at MAX_METADATA_ENTRIES. The caller
/// guarantees `dict` is valid; a null dictionary reads as empty.
unsafe fn dict_entries(dict: *mut AvDictionary) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut prev: *const AvDictionaryEntry = ptr::null();
    for _ in 0..MAX_METADATA_ENTRIES {
        // SAFETY: dict is valid; prev chains the iteration. The ignore-suffix
        // flag makes the empty key match every entry in order.
        let entry = unsafe { av_dict_get(dict, c"".as_ptr(), prev, AV_DICT_IGNORE_SUFFIX) };
        if entry.is_null() {
            break;
        }
        // SAFETY: key/value are NUL-terminated strings owned by the dictionary.
        let pair = unsafe {
            (
                CStr::from_ptr((*entry).key).to_string_lossy().into_owned(),
                CStr::from_ptr((*entry).value).to_string_lossy().into_owned(),
            )
        };
        out.push(pair);
        prev = entry;
    }
    out
}
