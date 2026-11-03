use crate::session::{MediaSession, StreamKind, StreamInfo};
use crate::sys::*;
use std::ffi::{c_int, c_void, CString};
use std::ptr;

pub const AVCOL_SPC_UNSPECIFIED: c_int = 2;
pub const AVCOL_SPC_BT709: c_int = 1;
pub const AVCOL_RANGE_UNSPECIFIED: c_int = 0;
pub const AVCOL_RANGE_MPEG: c_int = 1;
pub const AVCOL_RANGE_JPEG: c_int = 2;

const AV_CODEC_ID_MPEG4: c_int = 12;
const AV_CODEC_ID_H264: c_int = 27;
const AV_CODEC_ID_HEVC: c_int = 172;
const AV_CODEC_ID_VP9: c_int = 166;
const AV_CODEC_ID_AV1: c_int = 222;

const AV_NOPTS_VALUE: i64 = i64::MIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwRequest {
    None,
    VideoToolbox,
    VaApi,
}

impl HwRequest {
    fn device_type(self) -> c_int {
        match self {
            HwRequest::None => AV_HWDEVICE_TYPE_NONE,
            HwRequest::VideoToolbox => AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
            HwRequest::VaApi => AV_HWDEVICE_TYPE_VAAPI,
        }
    }

    fn pixel_format(self) -> c_int {
        match self {
            HwRequest::None => AV_PIX_FMT_NONE,
            HwRequest::VideoToolbox => AV_PIX_FMT_VIDEOTOOLBOX,
            HwRequest::VaApi => AV_PIX_FMT_VAAPI,
        }
    }

    fn type_name(self) -> &'static str {
        match self {
            HwRequest::None => "none",
            HwRequest::VideoToolbox => "videotoolbox",
            HwRequest::VaApi => "vaapi",
        }
    }
}

unsafe extern "C" fn pick_format(ctx: *mut AvCodecContext, formats: *const c_int) -> c_int {
    if ctx.is_null() || formats.is_null() {
        return AV_PIX_FMT_NONE;
    }
    // SAFETY: opaque holds the boxed wanted format for the session lifetime;
    // formats is a NONE-terminated list.
    let wanted = unsafe {
        let slot = (*ctx).opaque as *const c_int;
        if slot.is_null() {
            AV_PIX_FMT_NONE
        } else {
            *slot
        }
    };
    let mut index = 0;
    let mut first = AV_PIX_FMT_NONE;
    loop {
        // SAFETY: formats is NONE-terminated by contract.
        let format = unsafe { *formats.add(index) };
        if format == AV_PIX_FMT_NONE {
            break;
        }
        if first == AV_PIX_FMT_NONE {
            first = format;
        }
        if format == wanted {
            return format;
        }
        index += 1;
    }
    first
}

fn thread_count() -> i64 {
    let parallelism = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    (parallelism + 1).min(16).max(1) as i64
}

fn set_option(context: *mut AvCodecContext, name: &str, value: i64) -> Result<(), String> {
    let name = CString::new(name).expect("option name");
    // SAFETY: the context is live and the name is valid.
    let status = unsafe { av_opt_set_int(context as *mut c_void, name.as_ptr(), value, 0) };
    if status < 0 {
        return Err(format!("av_opt_set_int({name:?}) failed: {status}"));
    }
    Ok(())
}

fn codec_id_for_name(name: &str) -> Result<c_int, String> {
    match name {
        "mpeg4" => Ok(AV_CODEC_ID_MPEG4),
        "h264" => Ok(AV_CODEC_ID_H264),
        "hevc" => Ok(AV_CODEC_ID_HEVC),
        "vp9" => Ok(AV_CODEC_ID_VP9),
        "av1" => Ok(AV_CODEC_ID_AV1),
        _ => Err(format!("video codec '{name}' is not in the FFmpeg closure")),
    }
}

pub struct VideoFrame {
    frame: *mut AvFrame,
}

impl VideoFrame {
    pub fn width(&self) -> i32 {
        // SAFETY: the frame is live.
        unsafe { (*self.frame).width }
    }

    pub fn height(&self) -> i32 {
        // SAFETY: the frame is live.
        unsafe { (*self.frame).height }
    }

    pub fn format(&self) -> c_int {
        // SAFETY: the frame is live.
        unsafe { (*self.frame).format }
    }

    pub fn is_hardware(&self) -> bool {
        let format = self.format();
        format == AV_PIX_FMT_VAAPI || format == AV_PIX_FMT_VIDEOTOOLBOX
    }

    pub fn best_effort_ticks(&self) -> i64 {
        // SAFETY: the frame is live.
        unsafe { (*self.frame).best_effort_timestamp }
    }

    pub fn pts_ticks(&self) -> i64 {
        // SAFETY: the frame is live.
        unsafe { (*self.frame).pts }
    }

    pub fn color_space(&self) -> c_int {
        // SAFETY: the frame is live.
        unsafe { (*self.frame).colorspace }
    }

    pub fn color_range(&self) -> c_int {
        // SAFETY: the frame is live.
        unsafe { (*self.frame).color_range }
    }

    pub fn plane(&self, index: usize) -> Option<(&[u8], usize)> {
        if index >= AV_NUM_DATA_POINTERS {
            return None;
        }
        // SAFETY: the frame is live; lavc guarantees data and linesize for
        // returned planes.
        unsafe {
            let data = (*self.frame).data[index];
            if data.is_null() {
                return None;
            }
            let linesize = (*self.frame).linesize[index].max(0) as usize;
            let height = self.plane_height(index);
            Some((
                std::slice::from_raw_parts(data, linesize * height),
                linesize,
            ))
        }
    }

    fn plane_height(&self, index: usize) -> usize {
        let height = self.height().max(0) as usize;
        match self.format() {
            AV_PIX_FMT_YUV420P | AV_PIX_FMT_NV12 | AV_PIX_FMT_NV21 => {
                if index == 0 {
                    height
                } else {
                    height.div_ceil(2)
                }
            }
            _ => height,
        }
    }

    pub fn native_handle(&self) -> Option<*mut c_void> {
        if !self.is_hardware() {
            return None;
        }
        // SAFETY: the frame is live; data[3] carries the native surface for
        // both VideoToolbox and VA-API.
        unsafe {
            let handle = (*self.frame).data[3];
            if handle.is_null() {
                None
            } else {
                Some(handle as *mut c_void)
            }
        }
    }

    pub fn transfer_to_software(&self) -> Result<VideoFrame, String> {
        if !self.is_hardware() {
            return Err("frame is already software".to_string());
        }
        // SAFETY: a fresh frame receives the transfer.
        let dst = unsafe { av_frame_alloc() };
        if dst.is_null() {
            return Err("av_frame_alloc failed".to_string());
        }
        // SAFETY: dst is live; the source is the live hardware frame.
        let status = unsafe {
            av_hwframe_transfer_data(dst, self.frame, AV_HWFRAME_TRANSFER_DIRECTION_FROM)
        };
        if status < 0 {
            // SAFETY: dst was allocated above.
            unsafe {
                let mut frame = dst;
                av_frame_free(&mut frame);
            }
            return Err(format!("av_hwframe_transfer_data failed: {status}"));
        }
        Ok(VideoFrame { frame: dst })
    }
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFrame")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("format", &self.format())
            .field("ticks", &self.best_effort_ticks())
            .finish()
    }
}

impl Drop for VideoFrame {
    fn drop(&mut self) {
        // SAFETY: the frame is owned here.
        unsafe {
            av_frame_unref(self.frame);
            let mut frame = self.frame;
            av_frame_free(&mut frame);
        }
    }
}

pub struct VideoSession {
    session: Option<MediaSession>,
    stream_index: usize,
    context: *mut AvCodecContext,
    wanted: *mut c_int,
    time_base: (i32, i32),
}

impl VideoSession {
    pub fn open(path: &str, stream_index: usize, hw: HwRequest) -> Result<Self, String> {
        let version = unsafe { avcodec_version() };
        if (version >> 16) & 0xff != AVCODEC_VERSION_MAJOR {
            return Err(format!(
                "libavcodec major mismatch: got {}, want {}",
                (version >> 16) & 0xff,
                AVCODEC_VERSION_MAJOR
            ));
        }
        let session = MediaSession::open(path).map_err(|err| err.to_string())?;
        Self::open_impl(Some(session), stream_index, hw)
    }

    pub fn open_shared(
        session: &MediaSession,
        stream_index: usize,
        hw: HwRequest,
    ) -> Result<Self, String> {
        let info = session
            .streams()
            .into_iter()
            .find(|stream| stream.index == stream_index && stream.kind == StreamKind::Video)
            .ok_or_else(|| format!("no video stream {stream_index}"))?;
        check_dimensions(&info)?;
        // SAFETY: the codec parameters outlive the session.
        let parameters = session
            .codecpar_ptr(stream_index)
            .ok_or_else(|| "stream parameters vanished".to_string())?;
        Self::open_decoder(None, stream_index, &info, parameters, hw)
    }

    fn open_impl(
        session: Option<MediaSession>,
        stream_index: usize,
        hw: HwRequest,
    ) -> Result<Self, String> {
        let info = session
            .as_ref()
            .ok_or_else(|| "no owned session".to_string())?
            .streams()
            .into_iter()
            .find(|stream| stream.index == stream_index && stream.kind == StreamKind::Video)
            .ok_or_else(|| format!("no video stream {stream_index}"))?;
        check_dimensions(&info)?;
        // SAFETY: the codec parameters outlive the session.
        let parameters = session
            .as_ref()
            .expect("checked")
            .codecpar_ptr(stream_index)
            .ok_or_else(|| "stream parameters vanished".to_string())?;
        Self::open_decoder(session, stream_index, &info, parameters, hw)
    }

    fn open_decoder(
        session: Option<MediaSession>,
        stream_index: usize,
        info: &StreamInfo,
        parameters: *const AvCodecParameters,
        hw: HwRequest,
    ) -> Result<Self, String> {
        let codec_id = codec_id_for_name(&info.codec_name)?;
        // SAFETY: codec_id names a decoder in the pinned closure.
        let codec = unsafe { avcodec_find_decoder(codec_id) };
        if codec.is_null() {
            return Err(format!("no decoder for '{}'", info.codec_name));
        }
        // SAFETY: the codec pointer is valid.
        let context = unsafe { avcodec_alloc_context3(codec) };
        if context.is_null() {
            return Err("avcodec_alloc_context3 failed".to_string());
        }
        let mut wanted: *mut c_int = ptr::null_mut();
        let open = Self::configure(context, parameters, info, hw, &mut wanted);
        if let Err(message) = open {
            // SAFETY: the context was allocated here and never opened.
            unsafe {
                if !wanted.is_null() {
                    drop(Box::from_raw(wanted));
                }
                let mut context = context;
                avcodec_free_context(&mut context);
            }
            return Err(message);
        }
        Ok(VideoSession {
            session,
            stream_index,
            context,
            wanted,
            time_base: info.time_base,
        })
    }

    fn configure(
        context: *mut AvCodecContext,
        parameters: *const AvCodecParameters,
        info: &StreamInfo,
        hw: HwRequest,
        wanted: &mut *mut c_int,
    ) -> Result<(), String> {
        // SAFETY: the context and parameters are live.
        if unsafe { avcodec_parameters_to_context(context, parameters) } < 0 {
            return Err("avcodec_parameters_to_context failed".to_string());
        }
        // SAFETY: the context is live; the time base comes from the probe.
        unsafe {
            (*context).pkt_timebase = AvRational {
                num: info.time_base.0,
                den: info.time_base.1,
            };
        }
        set_option(context, "threads", thread_count())?;
        #[cfg(target_os = "linux")]
        if hw == HwRequest::VaApi && !render_node_present() {
            return Err("vaapi: no DRM render node".to_string());
        }
        if hw != HwRequest::None {
            // SAFETY: the device reference is created here.
            let mut device: *mut AvBufferRef = ptr::null_mut();
            let status = unsafe {
                av_hwdevice_ctx_create(
                    &mut device,
                    hw.device_type(),
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                )
            };
            if status < 0 || device.is_null() {
                return Err(format!(
                    "av_hwdevice_ctx_create({}) failed: {status}",
                    hw.type_name()
                ));
            }
            let slot = Box::into_raw(Box::new(hw.pixel_format()));
            // SAFETY: the context is live; free_context owns the device ref.
            unsafe {
                (*context).hw_device_ctx = device;
                (*context).opaque = slot as *mut c_void;
                (*context).get_format = Some(pick_format);
            }
            *wanted = slot;
        }
        // SAFETY: the context is fully configured.
        if unsafe { avcodec_open2(context, (*context).codec, ptr::null_mut()) } < 0 {
            return Err("avcodec_open2 failed".to_string());
        }
        Ok(())
    }

    pub fn negotiated_format(&self) -> c_int {
        // SAFETY: the context is live.
        unsafe { (*self.context).pix_fmt }
    }

    pub fn send(&mut self, data: &[u8], pts_seconds: Option<f64>) -> Result<bool, String> {
        // SAFETY: a packet carries one compressed frame.
        let packet = unsafe { av_packet_alloc() };
        if packet.is_null() {
            return Err("av_packet_alloc failed".to_string());
        }
        // SAFETY: the packet buffer holds the copied data.
        let status = unsafe { av_new_packet(packet, data.len() as c_int) };
        if status < 0 {
            // SAFETY: the packet was allocated above.
            unsafe {
                let mut packet = packet;
                av_packet_free(&mut packet);
            }
            return Err(format!("av_new_packet failed: {status}"));
        }
        let (num, den) = self.time_base;
        let den = if den == 0 { 1 } else { den };
        let num = if num == 0 { 1 } else { num };
        let pts = pts_seconds
            .map(|seconds| (seconds * den as f64 / num as f64).round() as i64)
            .unwrap_or(AV_NOPTS_VALUE);
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), (*packet).data, data.len());
            (*packet).pts = pts;
            (*packet).dts = pts;
            (*packet).stream_index = self.stream_index as c_int;
            // SAFETY: the context and packet are live; lavc references the packet.
            let status = avcodec_send_packet(self.context, packet);
            let mut packet = packet;
            av_packet_free(&mut packet);
            if status == AVERROR_EAGAIN {
                return Ok(false);
            }
            if status < 0 {
                return Err(format!("avcodec_send_packet failed: {status}"));
            }
            Ok(true)
        }
    }

    pub fn receive(&mut self) -> Result<Option<VideoFrame>, String> {
        // SAFETY: a fresh frame receives the decoded output.
        let frame = unsafe { av_frame_alloc() };
        if frame.is_null() {
            return Err("av_frame_alloc failed".to_string());
        }
        // SAFETY: the context and frame are live.
        let status = unsafe { avcodec_receive_frame(self.context, frame) };
        if status == AVERROR_EAGAIN || status == AVERROR_EOF {
            // SAFETY: nothing was written into the frame.
            unsafe {
                let mut frame = frame;
                av_frame_free(&mut frame);
            }
            return Ok(None);
        }
        if status < 0 {
            // SAFETY: the frame was allocated above.
            unsafe {
                let mut frame = frame;
                av_frame_free(&mut frame);
            }
            return Err(format!("avcodec_receive_frame failed: {status}"));
        }
        Ok(Some(VideoFrame { frame }))
    }

    pub fn drain(&mut self) -> Result<(), String> {
        // SAFETY: a NULL packet drains the decoder.
        let status = unsafe { avcodec_send_packet(self.context, ptr::null()) };
        if status < 0 && status != AVERROR_EOF {
            return Err(format!("drain send failed: {status}"));
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        // SAFETY: the context is live.
        unsafe { avcodec_flush_buffers(self.context) };
    }

    pub fn seek_seconds(&mut self, seconds: f64) -> Result<(), String> {
        self.session
            .as_mut()
            .ok_or_else(|| "no owned session".to_string())?
            .seek_seconds(seconds)
            .map_err(|err| err.to_string())?;
        self.reset();
        Ok(())
    }

    pub fn pump(&mut self) -> Result<PumpResult, String> {
        if self.session.is_none() {
            return Err("no owned session".to_string());
        }
        loop {
            let packet = self
                .session
                .as_mut()
                .expect("checked")
                .read_packet();
            match packet {
                Ok(packet) => {
                    if packet.stream_index != self.stream_index {
                        continue;
                    }
                    if !self.send(&packet.data, packet.pts_seconds)? {
                        continue;
                    }
                    if let Some(frame) = self.receive()? {
                        return Ok(PumpResult::Frame(frame));
                    }
                    return Ok(PumpResult::NeedMore);
                }
                Err(crate::MediaError::Eof) => {
                    self.drain()?;
                    if let Some(frame) = self.receive()? {
                        return Ok(PumpResult::Frame(frame));
                    }
                    return Ok(PumpResult::Ended);
                }
                Err(err) => return Err(err.to_string()),
            }
        }
    }

    pub fn drain_all(&mut self) -> Result<Vec<VideoFrame>, String> {
        let mut frames = Vec::new();
        loop {
            match self.pump()? {
                PumpResult::Frame(frame) => frames.push(frame),
                PumpResult::NeedMore => {}
                PumpResult::Ended => break,
            }
        }
        Ok(frames)
    }
}

#[derive(Debug)]
pub enum PumpResult {
    Frame(VideoFrame),
    NeedMore,
    Ended,
}

impl Drop for VideoSession {
    fn drop(&mut self) {
        // SAFETY: the context was opened here; free_context releases the
        // hardware device reference lavc owns.
        unsafe {
            let mut context = self.context;
            avcodec_free_context(&mut context);
        }
        if !self.wanted.is_null() {
            // SAFETY: the box was created at open and outlives decoding.
            unsafe {
                drop(Box::from_raw(self.wanted));
            }
        }
    }
}

fn check_dimensions(info: &StreamInfo) -> Result<(), String> {
    if info.width <= 0 || info.height <= 0 {
        return Err("video stream has no dimensions".to_string());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn render_node_present() -> bool {
    std::fs::read_dir("/dev/dri")
        .map(|entries| {
            entries.filter_map(|entry| entry.ok()).any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("renderD")
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn video_index(path: &str) -> usize {
        MediaSession::open(path)
            .expect("fixture opens")
            .streams()
            .into_iter()
            .find(|stream| stream.kind == StreamKind::Video)
            .expect("video stream")
            .index
    }

    fn open_sw(name: &str) -> VideoSession {
        let path = fixture(name);
        let index = video_index(&path);
        VideoSession::open(&path, index, HwRequest::None).expect("decoder opens")
    }

    fn pix_name(format: c_int) -> String {
        // SAFETY: the name table is static for the linked libavutil.
        let pointer = unsafe { av_get_pix_fmt_name(format) };
        if pointer.is_null() {
            return String::new();
        }
        unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }

    fn hw_type_id(name: &str) -> c_int {
        let name = CString::new(name).unwrap();
        // SAFETY: the type table is static for the linked libavutil.
        unsafe { av_hwdevice_find_type_by_name(name.as_ptr()) }
    }

    #[test]
    fn pixfmt_constants_match_linked_library() {
        assert_eq!(pix_name(AV_PIX_FMT_YUV420P), "yuv420p");
        assert_eq!(pix_name(AV_PIX_FMT_YUV422P), "yuv422p");
        assert_eq!(pix_name(AV_PIX_FMT_YUV444P), "yuv444p");
        assert_eq!(pix_name(AV_PIX_FMT_NV12), "nv12");
        assert_eq!(pix_name(AV_PIX_FMT_NV21), "nv21");
        assert_eq!(pix_name(AV_PIX_FMT_RGB24), "rgb24");
        assert_eq!(pix_name(AV_PIX_FMT_RGBA), "rgba");
        assert_eq!(pix_name(AV_PIX_FMT_GRAY8), "gray");
        assert_eq!(pix_name(AV_PIX_FMT_VAAPI), "vaapi");
        assert_eq!(pix_name(AV_PIX_FMT_VIDEOTOOLBOX), "videotoolbox_vld");
    }

    #[test]
    fn hwdevice_constants_match_linked_library() {
        assert_eq!(hw_type_id("vaapi"), AV_HWDEVICE_TYPE_VAAPI);
        assert_eq!(hw_type_id("videotoolbox"), AV_HWDEVICE_TYPE_VIDEOTOOLBOX);
    }

    #[test]
    fn codec_id_constants_match_descriptors() {
        let cases = [
            ("mpeg4", AV_CODEC_ID_MPEG4),
            ("h264", AV_CODEC_ID_H264),
            ("hevc", AV_CODEC_ID_HEVC),
            ("vp9", AV_CODEC_ID_VP9),
            ("av1", AV_CODEC_ID_AV1),
        ];
        for (name, id) in cases {
            // SAFETY: the descriptor table is static for the linked libavcodec.
            let descriptor = unsafe { avcodec_descriptor_get(id) };
            assert!(!descriptor.is_null(), "{name}: no descriptor for {id}");
            // SAFETY: the descriptor is valid.
            let found = unsafe { std::ffi::CStr::from_ptr((*descriptor).name) }
                .to_string_lossy();
            assert_eq!(found, name, "codec id {id}");
        }
    }

    #[test]
    fn avcodec_major_matches_pin() {
        let version = unsafe { avcodec_version() };
        assert_eq!((version >> 16) & 0xff, AVCODEC_VERSION_MAJOR);
    }

    #[test]
    fn decodes_h264_to_software_frames() {
        let mut session = open_sw("h264.mp4");
        let frames = session.drain_all().expect("drain works");
        assert!((60..=90).contains(&frames.len()), "got {}", frames.len());
        let mut last_ticks = i64::MIN;
        for frame in &frames {
            assert_eq!((frame.width(), frame.height()), (320, 240));
            assert_eq!(frame.format(), AV_PIX_FMT_YUV420P);
            assert!(!frame.is_hardware());
            assert_eq!(frame.color_space(), AVCOL_SPC_UNSPECIFIED);
            assert_eq!(frame.color_range(), AVCOL_RANGE_UNSPECIFIED);
            assert!(frame.best_effort_ticks() >= last_ticks);
            last_ticks = frame.best_effort_ticks();
            let (y, y_stride) = frame.plane(0).expect("luma plane");
            assert!(y_stride >= 320, "stride {y_stride}");
            assert_eq!(y.len(), y_stride * 240);
            let (u, u_stride) = frame.plane(1).expect("chroma plane");
            assert!(u_stride >= 160, "stride {u_stride}");
            assert_eq!(u.len(), u_stride * 120);
            assert!(frame.native_handle().is_none());
        }
        assert!(last_ticks > 0);
    }

    #[test]
    fn decodes_each_video_codec() {
        let cases = ["h264.mp4", "hevc.mkv", "mpeg4.avi", "vp9.webm", "av1.webm"];
        for name in cases {
            let mut session = open_sw(name);
            let frames = session.drain_all().expect("drain works");
            assert!(!frames.is_empty(), "{name}");
            assert!(!frames[0].is_hardware(), "{name}");
            assert!(frames[0].width() > 0 && frames[0].height() > 0, "{name}");
        }
    }

    #[test]
    fn decode_is_deterministic() {
        let first = open_sw("h264.mp4").drain_all().expect("drain");
        let second = open_sw("h264.mp4").drain_all().expect("drain");
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.best_effort_ticks(), b.best_effort_ticks());
            let (pa, _) = a.plane(0).expect("plane");
            let (pb, _) = b.plane(0).expect("plane");
            assert_eq!(pa, pb);
        }
    }

    #[test]
    fn seek_resets_decode_position() {
        let path = fixture("h264.mp4");
        let info = MediaSession::open(&path)
            .expect("fixture opens")
            .streams()
            .into_iter()
            .find(|stream| stream.kind == StreamKind::Video)
            .expect("video stream");
        let index = info.index;
        let ticks_per_second = info.time_base.1 as f64 / info.time_base.0.max(1) as f64;
        assert!(ticks_per_second > 0.0);
        let mut session = VideoSession::open(&path, index, HwRequest::None).expect("opens");
        let mut before = None;
        for _ in 0..60 {
            match session.pump().expect("pump") {
                PumpResult::Frame(frame) => {
                    before = Some(frame.best_effort_ticks());
                    break;
                }
                PumpResult::Ended => panic!("ended before any frame"),
                PumpResult::NeedMore => {}
            }
        }
        session.seek_seconds(2.0).expect("seek works");
        let mut after = None;
        for _ in 0..200 {
            match session.pump().expect("pump after seek") {
                PumpResult::Frame(frame) => {
                    after = Some(frame.best_effort_ticks());
                    break;
                }
                PumpResult::Ended => panic!("ended right after seek"),
                PumpResult::NeedMore => {}
            }
        }
        let before_seconds = before.expect("frame") as f64 / ticks_per_second;
        let after_seconds = after.expect("frame") as f64 / ticks_per_second;
        assert!(before_seconds < 1.0, "started at {before_seconds}");
        assert!(
            (1.5..2.5).contains(&after_seconds),
            "seek landed at {after_seconds}"
        );
    }

    #[test]
    fn hardware_or_fallback_decodes() {
        let path = fixture("h264.mp4");
        let index = video_index(&path);
        let hw = if cfg!(target_os = "macos") {
            HwRequest::VideoToolbox
        } else {
            HwRequest::VaApi
        };
        match VideoSession::open(&path, index, hw) {
            Ok(mut session) => {
                let frames = session.drain_all().expect("drain works");
                assert!(!frames.is_empty());
                let first = &frames[0];
                assert!(first.is_hardware(), "expected hardware frames");
                assert!(first.native_handle().is_some());
                let sw = first.transfer_to_software().expect("transfer works");
                assert_eq!((sw.width(), sw.height()), (320, 240));
                assert!(!sw.is_hardware());
                assert!(sw.plane(0).is_some());
            }
            Err(message) => {
                let mut fallback =
                    VideoSession::open(&path, index, HwRequest::None).expect("fallback works");
                let frames = fallback.drain_all().expect("drain works");
                assert!(!frames.is_empty(), "fallback decoded nothing: {message}");
            }
        }
    }

    #[test]
    fn rejects_image_codecs_and_bad_streams() {
        let path = fixture("audio-cover.flac");
        let index = video_index(&path);
        let err = match VideoSession::open(&path, index, HwRequest::None) {
            Ok(_) => panic!("png must be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("not in the FFmpeg closure"), "unexpected: {err}");
        let h264 = fixture("h264.mp4");
        assert!(VideoSession::open(&h264, 99, HwRequest::None).is_err());
    }
}
