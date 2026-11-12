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
pub const AVCODEC_VERSION_MAJOR: u32 = 63;

pub const AVMEDIA_TYPE_VIDEO: c_int = 0;
pub const AVMEDIA_TYPE_AUDIO: c_int = 1;
pub const AVMEDIA_TYPE_DATA: c_int = 2;
pub const AVMEDIA_TYPE_SUBTITLE: c_int = 3;
pub const AVMEDIA_TYPE_ATTACHMENT: c_int = 4;

pub const AV_DISPOSITION_DEFAULT: c_int = 1 << 0;
pub const AV_DISPOSITION_FORCED: c_int = 1 << 6;
pub const AV_DISPOSITION_ATTACHED_PIC: c_int = 1 << 10;

pub const AV_LOG_ERROR: c_int = 16;

pub const AVFMT_FLAG_CUSTOM_IO: c_int = 0x0080;
pub const AV_DICT_IGNORE_SUFFIX: c_int = 2;

pub const SEEK_SET: c_int = 0;
pub const SEEK_CUR: c_int = 1;
pub const SEEK_END: c_int = 2;
pub const AVSEEK_SIZE: c_int = 0x10000;

/// `AVERROR_EOF`, proven by the packet-drain tests on every platform.
pub const AVERROR_EOF: c_int = -541478725;

/// `AVERROR(EAGAIN)`: the decoder's try-again signal is the negated
/// platform errno, 35 on macOS and 11 on Linux.
#[cfg(target_os = "macos")]
pub const AVERROR_EAGAIN: c_int = -35;
/// `AVERROR(EAGAIN)`: the decoder's try-again signal is the negated
/// platform errno, 35 on macOS and 11 on Linux.
#[cfg(not(target_os = "macos"))]
pub const AVERROR_EAGAIN: c_int = -11;

pub const AV_NUM_DATA_POINTERS: usize = 8;

pub const AV_PIX_FMT_NONE: c_int = -1;
pub const AV_PIX_FMT_YUV420P: c_int = 0;
pub const AV_PIX_FMT_RGB24: c_int = 2;
pub const AV_PIX_FMT_YUV422P: c_int = 4;
pub const AV_PIX_FMT_YUV444P: c_int = 5;
pub const AV_PIX_FMT_GRAY8: c_int = 8;
pub const AV_PIX_FMT_NV12: c_int = 23;
pub const AV_PIX_FMT_NV21: c_int = 24;
pub const AV_PIX_FMT_RGBA: c_int = 26;
pub const AV_PIX_FMT_YUV420P10LE: c_int = 62;
pub const AV_PIX_FMT_YUV422P10LE: c_int = 64;
pub const AV_PIX_FMT_YUV444P10LE: c_int = 68;
pub const AV_PIX_FMT_VAAPI: c_int = 44;
pub const AV_PIX_FMT_VIDEOTOOLBOX: c_int = 157;

pub const AV_HWDEVICE_TYPE_NONE: c_int = 0;
pub const AV_HWDEVICE_TYPE_VAAPI: c_int = 3;
pub const AV_HWDEVICE_TYPE_VIDEOTOOLBOX: c_int = 6;

pub const AV_HWFRAME_TRANSFER_DIRECTION_FROM: c_int = 0;

pub enum AvCodec {}

pub enum AvBufferRef {}

pub enum SwsContext {}

pub const SWS_BILINEAR: c_int = 1 << 1;

pub enum AvFrameSideData {}

/// Prefix of `AVFrame` through `hw_frames_ctx`, transcribed from
/// `libavutil/frame.h` of the pinned FFmpeg. Fields after
/// `hw_frames_ctx` are not mapped; never access them.
#[repr(C)]
pub struct AvFrame {
    pub data: [*mut u8; AV_NUM_DATA_POINTERS],
    pub linesize: [c_int; AV_NUM_DATA_POINTERS],
    pub extended_data: *mut *mut u8,
    pub width: c_int,
    pub height: c_int,
    pub nb_samples: c_int,
    pub format: c_int,
    pub pict_type: c_int,
    pub sample_aspect_ratio: AvRational,
    pub pts: i64,
    pub pkt_dts: i64,
    pub time_base: AvRational,
    pub quality: c_int,
    pub opaque: *mut c_void,
    pub repeat_pict: c_int,
    pub sample_rate: c_int,
    pub buf: [*mut AvBufferRef; AV_NUM_DATA_POINTERS],
    pub extended_buf: *mut *mut AvBufferRef,
    pub nb_extended_buf: c_int,
    pub side_data: *mut *mut AvFrameSideData,
    pub nb_side_data: c_int,
    pub flags: c_int,
    pub color_range: c_int,
    pub color_primaries: c_int,
    pub color_trc: c_int,
    pub colorspace: c_int,
    pub chroma_location: c_int,
    pub best_effort_timestamp: i64,
    pub metadata: *mut AvDictionary,
    pub decode_error_flags: c_int,
    pub hw_frames_ctx: *mut AvBufferRef,
}

pub type AvGetFormat =
    Option<unsafe extern "C" fn(ctx: *mut AvCodecContext, formats: *const c_int) -> c_int>;

pub type AvGetBuffer2 = Option<
    unsafe extern "C" fn(ctx: *mut AvCodecContext, frame: *mut AvFrame, flags: c_int) -> c_int,
>;

pub type AvDrawHorizBand = Option<
    unsafe extern "C" fn(
        ctx: *mut AvCodecContext,
        src: *const AvFrame,
        offset: *const c_int,
        y: c_int,
        band_type: c_int,
        height: c_int,
    ),
>;

/// Prefix of `AVCodecContext` through `extra_hw_frames`, transcribed
/// from `libavcodec/avcodec.h` of the pinned FFmpeg. The conditional
/// `intra_dc_precision` field is present because the pin keeps the
/// pre-64 compatibility defines. Fields after `extra_hw_frames` are
/// not mapped; never access them.
#[repr(C)]
pub struct AvCodecContext {
    pub av_class: *const c_void,
    pub log_level_offset: c_int,
    pub codec_type: c_int,
    pub codec: *const AvCodec,
    pub codec_id: c_int,
    pub codec_tag: c_uint,
    pub priv_data: *mut c_void,
    pub internal: *mut c_void,
    pub opaque: *mut c_void,
    pub bit_rate: i64,
    pub flags: c_int,
    pub flags2: c_int,
    pub extradata: *mut u8,
    pub extradata_size: c_int,
    pub time_base: AvRational,
    pub pkt_timebase: AvRational,
    pub framerate: AvRational,
    pub delay: c_int,
    pub width: c_int,
    pub height: c_int,
    pub coded_width: c_int,
    pub coded_height: c_int,
    pub sample_aspect_ratio: AvRational,
    pub pix_fmt: c_int,
    pub sw_pix_fmt: c_int,
    pub color_primaries: c_int,
    pub color_trc: c_int,
    pub colorspace: c_int,
    pub color_range: c_int,
    pub chroma_sample_location: c_int,
    pub field_order: c_int,
    pub refs: c_int,
    pub has_b_frames: c_int,
    pub slice_flags: c_int,
    pub draw_horiz_band: AvDrawHorizBand,
    pub get_format: AvGetFormat,
    pub max_b_frames: c_int,
    pub b_quant_factor: f32,
    pub b_quant_offset: f32,
    pub i_quant_factor: f32,
    pub i_quant_offset: f32,
    pub lumi_masking: f32,
    pub temporal_cplx_masking: f32,
    pub spatial_cplx_masking: f32,
    pub p_masking: f32,
    pub dark_masking: f32,
    pub nsse_weight: c_int,
    pub me_cmp: c_int,
    pub me_sub_cmp: c_int,
    pub mb_cmp: c_int,
    pub ildct_cmp: c_int,
    pub dia_size: c_int,
    pub last_predictor_count: c_int,
    pub me_pre_cmp: c_int,
    pub pre_dia_size: c_int,
    pub me_subpel_quality: c_int,
    pub me_range: c_int,
    pub mb_decision: c_int,
    pub intra_matrix: *mut u16,
    pub inter_matrix: *mut u16,
    pub chroma_intra_matrix: *mut u16,
    pub intra_dc_precision: c_int,
    pub mb_lmin: c_int,
    pub mb_lmax: c_int,
    pub bidir_refine: c_int,
    pub keyint_min: c_int,
    pub gop_size: c_int,
    pub mv0_threshold: c_int,
    pub slices: c_int,
    pub sample_rate: c_int,
    pub sample_fmt: c_int,
    pub ch_layout: AvChannelLayout,
    pub frame_size: c_int,
    pub block_align: c_int,
    pub cutoff: c_int,
    pub audio_service_type: c_int,
    pub request_sample_fmt: c_int,
    pub initial_padding: c_int,
    pub trailing_padding: c_int,
    pub seek_preroll: c_int,
    pub get_buffer2: AvGetBuffer2,
    pub bit_rate_tolerance: c_int,
    pub global_quality: c_int,
    pub compression_level: c_int,
    pub qcompress: f32,
    pub qblur: f32,
    pub qmin: c_int,
    pub qmax: c_int,
    pub max_qdiff: c_int,
    pub rc_buffer_size: c_int,
    pub rc_override_count: c_int,
    pub rc_override: *mut c_void,
    pub rc_max_rate: i64,
    pub rc_min_rate: i64,
    pub rc_max_available_vbv_use: f32,
    pub rc_min_vbv_overflow_use: f32,
    pub rc_initial_buffer_occupancy: c_int,
    pub trellis: c_int,
    pub stats_out: *mut c_char,
    pub stats_in: *mut c_char,
    pub workaround_bugs: c_int,
    pub strict_std_compliance: c_int,
    pub error_concealment: c_int,
    pub debug: c_int,
    pub err_recognition: c_int,
    pub hwaccel: *const c_void,
    pub hwaccel_context: *mut c_void,
    pub hw_frames_ctx: *mut AvBufferRef,
    pub hw_device_ctx: *mut AvBufferRef,
    pub hwaccel_flags: c_int,
    pub extra_hw_frames: c_int,
}

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
    pub fn av_log_set_level(level: c_int);
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
    pub fn av_new_packet(pkt: *mut AvPacket, size: c_int) -> c_int;
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
    pub fn avcodec_version() -> c_uint;
    pub fn avcodec_find_decoder(id: c_int) -> *const AvCodec;
    pub fn avcodec_alloc_context3(codec: *const AvCodec) -> *mut AvCodecContext;
    pub fn avcodec_parameters_to_context(
        ctx: *mut AvCodecContext,
        par: *const AvCodecParameters,
    ) -> c_int;
    pub fn avcodec_open2(
        ctx: *mut AvCodecContext,
        codec: *const AvCodec,
        options: *mut *mut AvDictionary,
    ) -> c_int;
    pub fn avcodec_send_packet(ctx: *mut AvCodecContext, pkt: *const AvPacket) -> c_int;
    pub fn avcodec_receive_frame(ctx: *mut AvCodecContext, frame: *mut AvFrame) -> c_int;
    pub fn avcodec_flush_buffers(ctx: *mut AvCodecContext);
    pub fn avcodec_free_context(ctx: *mut *mut AvCodecContext);
    pub fn av_frame_alloc() -> *mut AvFrame;
    pub fn av_frame_free(frame: *mut *mut AvFrame);
    pub fn av_frame_unref(frame: *mut AvFrame);
    pub fn av_hwdevice_ctx_create(
        device_ctx: *mut *mut AvBufferRef,
        device_type: c_int,
        device: *const c_char,
        opts: *mut AvDictionary,
        flags: c_int,
    ) -> c_int;
    pub fn av_hwdevice_find_type_by_name(name: *const c_char) -> c_int;
    pub fn av_hwframe_transfer_data(dst: *mut AvFrame, src: *const AvFrame, flags: c_int)
    -> c_int;
    pub fn av_get_pix_fmt_name(pix_fmt: c_int) -> *const c_char;
    pub fn av_color_space_name(space: c_int) -> *const c_char;
    pub fn av_color_range_name(range: c_int) -> *const c_char;
    pub fn av_opt_set_int(
        obj: *mut c_void,
        name: *const c_char,
        value: i64,
        search_flags: c_int,
    ) -> c_int;
    pub fn av_opt_set(
        obj: *mut c_void,
        name: *const c_char,
        value: *const c_char,
        search_flags: c_int,
    ) -> c_int;
    pub fn sws_getContext(
        src_w: c_int,
        src_h: c_int,
        src_format: c_int,
        dst_w: c_int,
        dst_h: c_int,
        dst_format: c_int,
        flags: c_int,
        src_filter: *const c_void,
        dst_filter: *const c_void,
        param: *const f64,
    ) -> *mut SwsContext;
    pub fn sws_scale(
        context: *mut SwsContext,
        src_slice: *const *const u8,
        src_stride: *const c_int,
        src_slice_y: c_int,
        src_slice_h: c_int,
        dst: *const *mut u8,
        dst_stride: *const c_int,
    ) -> c_int;
    pub fn sws_freeContext(context: *mut SwsContext);
}
