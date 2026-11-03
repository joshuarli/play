//! Minimal libavformat bindings transcribed from the pinned FFmpeg n9.0.2
//! headers (`libavformat/avformat.h`, `libavcodec/packet.h`,
//! `libavcodec/codec_par.h`, `libavcodec/codec_desc.h`,
//! `libavutil/channel_layout.h`, `libavutil/dict.h`).
//!
//! Only the demux path the player uses is covered. Field order and types must
//! match the headers exactly; bumping the FFmpeg pin requires
//! re-transcribing them. `open()` refuses to start when
//! `avformat_version()` is not libavformat 63.

use std::ffi::{c_char, c_int, c_uint, c_void};

pub const AVFORMAT_VERSION_MAJOR: u32 = 63;

pub const AVMEDIA_TYPE_VIDEO: c_int = 0;
pub const AVMEDIA_TYPE_AUDIO: c_int = 1;
pub const AVMEDIA_TYPE_DATA: c_int = 2;
pub const AVMEDIA_TYPE_SUBTITLE: c_int = 3;
pub const AVMEDIA_TYPE_ATTACHMENT: c_int = 4;

pub const AV_DISPOSITION_ATTACHED_PIC: c_int = 1 << 10;

pub const AVFMT_FLAG_CUSTOM_IO: c_int = 0x0080;
pub const AV_DICT_IGNORE_SUFFIX: c_int = 2;

pub const SEEK_SET: c_int = 0;
pub const SEEK_CUR: c_int = 1;
pub const SEEK_END: c_int = 2;
pub const AVSEEK_SIZE: c_int = 0x10000;

/// `AVERROR_EOF`, proven by the packet-drain tests on every platform.
pub const AVERROR_EOF: c_int = -541478725;

pub const MAX_STREAMS: usize = 256;
pub const MAX_CHAPTERS: usize = 256;
pub const MAX_METADATA_ENTRIES: usize = 1024;
pub const MAX_PACKET_BYTES: usize = 64 * 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AvRational {
    pub num: c_int,
    pub den: c_int,
}

pub enum AvDictionary {}

/// Prefix of `AVIOContext` through `buffer`. lavf may reallocate the buffer
/// during probing, so the session always frees the current pointer with
/// `av_free`, never the original allocation.
#[repr(C)]
pub struct AvIoContext {
    pub av_class: *const c_void,
    pub buffer: *mut u8,
}

#[repr(C)]
pub struct AvDictionaryEntry {
    pub key: *mut c_char,
    pub value: *mut c_char,
}

#[repr(C)]
pub struct AvChapter {
    pub id: i64,
    pub time_base: AvRational,
    pub start: i64,
    pub end: i64,
    pub metadata: *mut AvDictionary,
}

#[repr(C)]
pub union AvChannelUnion {
    pub mask: u64,
    pub map: *mut c_void,
}

#[repr(C)]
pub struct AvChannelLayout {
    pub order: c_int,
    pub nb_channels: c_int,
    pub u: AvChannelUnion,
    pub opaque: *mut c_void,
}

#[repr(C)]
pub struct AvCodecParameters {
    pub codec_type: c_int,
    pub codec_id: c_int,
    pub codec_tag: u32,
    pub extradata: *mut u8,
    pub extradata_size: c_int,
    pub coded_side_data: *mut c_void,
    pub nb_coded_side_data: c_int,
    pub format: c_int,
    pub bit_rate: i64,
    pub bits_per_coded_sample: c_int,
    pub bits_per_raw_sample: c_int,
    pub profile: c_int,
    pub level: c_int,
    pub width: c_int,
    pub height: c_int,
    pub sample_aspect_ratio: AvRational,
    pub framerate: AvRational,
    pub field_order: c_int,
    pub color_range: c_int,
    pub color_primaries: c_int,
    pub color_trc: c_int,
    pub color_space: c_int,
    pub chroma_location: c_int,
    pub video_delay: c_int,
    pub ch_layout: AvChannelLayout,
    pub sample_rate: c_int,
    pub block_align: c_int,
    pub frame_size: c_int,
    pub initial_padding: c_int,
    pub trailing_padding: c_int,
    pub seek_preroll: c_int,
    pub alpha_mode: c_int,
}

#[repr(C)]
pub struct AvPacket {
    pub buf: *mut c_void,
    pub pts: i64,
    pub dts: i64,
    pub data: *mut u8,
    pub size: c_int,
    pub stream_index: c_int,
    pub flags: c_int,
    pub side_data: *mut c_void,
    pub side_data_elems: c_int,
    pub duration: i64,
    pub pos: i64,
    pub opaque: *mut c_void,
    pub opaque_ref: *mut c_void,
    pub time_base: AvRational,
}

#[repr(C)]
pub struct AvStream {
    pub av_class: *const c_void,
    pub index: c_int,
    pub id: c_int,
    pub codecpar: *mut AvCodecParameters,
    pub priv_data: *mut c_void,
    pub time_base: AvRational,
    pub start_time: i64,
    pub duration: i64,
    pub nb_frames: i64,
    pub disposition: c_int,
    pub discard: c_int,
    pub sample_aspect_ratio: AvRational,
    pub metadata: *mut AvDictionary,
    pub avg_frame_rate: AvRational,
    pub attached_pic: AvPacket,
    pub event_flags: c_int,
    pub r_frame_rate: AvRational,
    pub pts_wrap_bits: c_int,
}

/// Prefix of `AVFormatContext` through `metadata`. Fields after `metadata`
/// are not mapped; never access them.
#[repr(C)]
pub struct AvFormatContext {
    pub av_class: *const c_void,
    pub iformat: *const c_void,
    pub oformat: *const c_void,
    pub priv_data: *mut c_void,
    pub pb: *mut AvIoContext,
    pub ctx_flags: c_int,
    pub nb_streams: c_uint,
    pub streams: *mut *mut AvStream,
    pub nb_stream_groups: c_uint,
    pub stream_groups: *mut c_void,
    pub nb_chapters: c_uint,
    pub chapters: *mut *mut AvChapter,
    pub url: *mut c_char,
    pub start_time: i64,
    pub duration: i64,
    pub bit_rate: i64,
    pub packet_size: c_uint,
    pub max_delay: c_int,
    pub flags: c_int,
    pub probesize: i64,
    pub max_analyze_duration: i64,
    pub key: *const u8,
    pub keylen: c_int,
    pub nb_programs: c_uint,
    pub programs: *mut c_void,
    pub video_codec_id: c_int,
    pub audio_codec_id: c_int,
    pub subtitle_codec_id: c_int,
    pub data_codec_id: c_int,
    pub metadata: *mut AvDictionary,
}

#[repr(C)]
pub struct AvCodecDescriptor {
    pub id: c_int,
    pub media_type: c_int,
    pub name: *const c_char,
}

pub type AvioReadPacket = unsafe extern "C" fn(
    opaque: *mut c_void,
    buf: *mut u8,
    buf_size: c_int,
) -> c_int;

pub type AvioSeek =
    unsafe extern "C" fn(opaque: *mut c_void, offset: i64, whence: c_int) -> i64;

unsafe extern "C" {
    pub fn av_malloc(size: usize) -> *mut c_void;
    pub fn av_free(ptr: *mut c_void);
    pub fn avformat_alloc_context() -> *mut AvFormatContext;
    pub fn avformat_open_input(
        ps: *mut *mut AvFormatContext,
        url: *const c_char,
        fmt: *const c_void,
        options: *mut *mut AvDictionary,
    ) -> c_int;
    pub fn avformat_find_stream_info(
        ic: *mut AvFormatContext,
        options: *mut *mut AvDictionary,
    ) -> c_int;
    pub fn avformat_close_input(s: *mut *mut AvFormatContext);
    pub fn avformat_free_context(s: *mut AvFormatContext);
    pub fn av_read_frame(s: *mut AvFormatContext, pkt: *mut AvPacket) -> c_int;
    pub fn avformat_seek_file(
        s: *mut AvFormatContext,
        stream_index: c_int,
        min_ts: i64,
        ts: i64,
        max_ts: i64,
        flags: c_int,
    ) -> c_int;
    pub fn av_packet_alloc() -> *mut AvPacket;
    pub fn av_packet_free(pkt: *mut *mut AvPacket);
    pub fn av_dict_get(
        m: *const AvDictionary,
        key: *const c_char,
        prev: *const AvDictionaryEntry,
        flags: c_int,
    ) -> *mut AvDictionaryEntry;
    pub fn avio_alloc_context(
        buffer: *mut u8,
        buffer_size: c_int,
        write_flag: c_int,
        opaque: *mut c_void,
        read_packet: Option<AvioReadPacket>,
        write_packet: Option<
            unsafe extern "C" fn(
                opaque: *mut c_void,
                buf: *const u8,
                buf_size: c_int,
            ) -> c_int,
        >,
        seek: Option<AvioSeek>,
    ) -> *mut AvIoContext;
    pub fn avcodec_descriptor_get(id: c_int) -> *const AvCodecDescriptor;
    pub fn avformat_version() -> c_uint;
}
