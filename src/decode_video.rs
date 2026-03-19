//! VideoToolbox-accelerated video decoder.
//!
//! Wraps an ffmpeg video decoder configured for hardware-accelerated decoding
//! via Apple's VideoToolbox framework. Decoded frames arrive as
//! `CVPixelBufferRef` (GPU-resident surfaces) which are retained and wrapped
//! in [`PixelBuffer`] RAII handles for zero-copy handoff to the display layer.
//!
//! The decoder is **not** `Send` by default (ffmpeg contexts are thread-local);
//! we manually impl `Send` because the player thread is the sole owner.

use std::ffi::c_void;
use std::ptr;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling;
use ffmpeg_next::util::frame::video::Video;
use ffmpeg_sys_next as ffs;

use crate::cmd::{PixelBuffer, VideoFrame};
use crate::time::pts_to_us;

/// RAII wrapper for an ffmpeg AVBufferRef holding a HW device context.
struct HwDeviceCtx(*mut ffs::AVBufferRef);

impl HwDeviceCtx {
    /// Create a new VideoToolbox HW device context. Returns None if unavailable.
    fn new_videotoolbox() -> Option<Self> {
        let mut ctx: *mut ffs::AVBufferRef = ptr::null_mut();
        // SAFETY: av_hwdevice_ctx_create allocates a new HW device context.
        // On success (ret >= 0), ctx is a valid AVBufferRef.
        let ret = unsafe {
            ffs::av_hwdevice_ctx_create(
                &mut ctx,
                ffs::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
                ptr::null(),
                ptr::null_mut(),
                0,
            )
        };
        if ret >= 0 { Some(Self(ctx)) } else { None }
    }

    fn as_ptr(&self) -> *mut ffs::AVBufferRef {
        self.0
    }
}

impl Drop for HwDeviceCtx {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: self.0 was allocated by av_hwdevice_ctx_create.
            // av_buffer_unref decrements the refcount and NULLs the pointer.
            unsafe { ffs::av_buffer_unref(&mut self.0) };
        }
    }
}

/// VideoToolbox-accelerated video decoder.
pub struct VideoDecoder {
    decoder: ffmpeg::decoder::Video,
    _hw_device_ctx: Option<HwDeviceCtx>,
    stream_time_base: ffmpeg::Rational,
    frame: Video,
    width: u32,
    height: u32,
    /// Lazy-init pixel-format scaler for software decode fallback.
    sws_ctx: Option<scaling::Context>,
    /// Reusable NV12 frame buffer for software decode output.
    nv12_frame: Video,
}

// SAFETY: VideoDecoder is only accessed from the player thread. The hw_device_ctx
// is an ffmpeg-managed reference that outlives all decoded frames.
unsafe impl Send for VideoDecoder {}

impl VideoDecoder {
    /// Create a new video decoder with VideoToolbox hardware acceleration.
    pub fn new(stream: &ffmpeg::Stream) -> Result<Self> {
        let mut codec_ctx = CodecContext::from_parameters(stream.parameters())
            .context("Failed to create video codec context")?;

        // SAFETY: as_mut_ptr() returns the underlying AVCodecContext. We set
        // hw_device_ctx and get_format before opening the decoder, which is
        // the required ordering per ffmpeg docs.
        let avctx = unsafe { codec_ctx.as_mut_ptr() };

        // Try to set up VideoToolbox hardware acceleration
        let hw_device_ctx = HwDeviceCtx::new_videotoolbox();
        if let Some(ref ctx) = hw_device_ctx {
            // SAFETY: avctx is valid; av_buffer_ref increments the refcount
            // so the codec context shares ownership. We set get_format to
            // prefer the VideoToolbox pixel format.
            unsafe {
                (*avctx).hw_device_ctx = ffs::av_buffer_ref(ctx.as_ptr());
                (*avctx).get_format = Some(get_hw_format);
            }
            log::info!("VideoToolbox hardware acceleration enabled");
        } else {
            log::warn!("VideoToolbox not available, using software decode");
        }

        let decoder = codec_ctx
            .decoder()
            .video()
            .context("Failed to open video decoder")?;
        let width = decoder.width();
        let height = decoder.height();

        Ok(Self {
            decoder,
            _hw_device_ctx: hw_device_ctx,
            stream_time_base: stream.time_base(),
            frame: Video::empty(),
            width,
            height,
            sws_ctx: None,
            nv12_frame: Video::new(Pixel::NV12, width, height),
        })
    }

    /// Send a packet to the decoder.
    pub fn send_packet(&mut self, packet: &ffmpeg::Packet) -> Result<()> {
        self.decoder.send_packet(packet)?;
        Ok(())
    }

    /// Send EOF to the decoder.
    pub fn send_eof(&mut self) -> Result<()> {
        self.decoder.send_eof()?;
        Ok(())
    }

    /// Receive decoded frames. Returns None when the decoder needs more input.
    pub fn receive_frame(&mut self) -> Option<VideoFrame> {
        match self.decoder.receive_frame(&mut self.frame) {
            Ok(()) => {
                // SAFETY: as_mut_ptr() returns the underlying AVFrame. We read
                // pts, duration, format, and data[3] which are valid after a
                // successful receive_frame().
                let raw = unsafe { self.frame.as_mut_ptr() };
                let pts = unsafe { (*raw).pts };
                let pts_us = pts_to_us(pts, self.stream_time_base);
                let duration = unsafe { (*raw).duration };
                let duration_us = pts_to_us(duration, self.stream_time_base);

                let pixel_buffer = if unsafe { (*raw).format }
                    == ffs::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX as i32
                {
                    // SAFETY: For VideoToolbox frames, data[3] is the
                    // CVPixelBufferRef per ffmpeg convention. We retain it so
                    // it outlives the AVFrame (which may reuse its buffer).
                    let cvbuf = unsafe { (*raw).data[3] as *mut c_void };
                    if cvbuf.is_null() {
                        return None;
                    }
                    // SAFETY: CVPixelBufferRetain increments the refcount.
                    // PixelBuffer::new takes ownership of the retained ref.
                    unsafe { CVPixelBufferRetain(cvbuf) };
                    cvbuf
                } else {
                    // Software decoded: convert to NV12 and wrap in CVPixelBuffer.
                    self.sw_frame_to_pixelbuffer()?
                };

                Some(VideoFrame {
                    pixel_buffer: Some(PixelBuffer::new(pixel_buffer)),
                    pts_us,
                    duration_us,
                    seek_flush: false,
                })
            }
            // receive_frame returns EAGAIN (need more input) or EOF (drain
            // complete) during normal operation.  Real decode errors are rare
            // but worth logging when they occur.
            Err(ref e) if matches!(e, ffmpeg::Error::Eof) => None,
            Err(ref e) if matches!(e, ffmpeg::Error::Other { .. }) => None, // EAGAIN
            Err(e) => {
                log::warn!("Video receive_frame error: {e}");
                None
            }
        }
    }

    /// Convert the current software-decoded frame to an NV12 CVPixelBuffer.
    fn sw_frame_to_pixelbuffer(&mut self) -> Option<*mut c_void> {
        // Lazy-init the pixel format scaler
        if self.sws_ctx.is_none() {
            let src_fmt = self.frame.format();
            self.sws_ctx = Some(
                scaling::Context::get(
                    src_fmt,
                    self.width,
                    self.height,
                    Pixel::NV12,
                    self.width,
                    self.height,
                    scaling::Flags::BILINEAR,
                )
                .ok()?,
            );
            eprintln!(
                "VideoToolbox unavailable for this codec, using software decode ({src_fmt:?} → NV12)"
            );
        }

        self.sws_ctx
            .as_mut()
            .unwrap()
            .run(&self.frame, &mut self.nv12_frame)
            .ok()?;

        // Create IOSurface-backed CVPixelBuffer
        let mut cvbuf: *mut c_void = ptr::null_mut();
        // SAFETY: CVPixelBufferCreate with valid dimensions and NV12 format.
        // io_surface_properties() returns a cached CFDictionary.
        let status = unsafe {
            CVPixelBufferCreate(
                ptr::null(),
                self.width as usize,
                self.height as usize,
                K_CV_PIXEL_FORMAT_NV12,
                io_surface_properties(),
                &mut cvbuf,
            )
        };
        if status != 0 || cvbuf.is_null() {
            log::error!("CVPixelBufferCreate failed: {status}");
            return None;
        }

        // SAFETY: cvbuf is a valid CVPixelBuffer from Create above.
        // Lock, copy NV12 planes, unlock.
        unsafe { CVPixelBufferLockBaseAddress(cvbuf, 0) };

        let nv12_raw = unsafe { self.nv12_frame.as_mut_ptr() };

        // Y plane (plane 0)
        copy_plane(
            unsafe { (*nv12_raw).data[0] },
            unsafe { (*nv12_raw).linesize[0] as usize },
            unsafe { CVPixelBufferGetBaseAddressOfPlane(cvbuf, 0) },
            unsafe { CVPixelBufferGetBytesPerRowOfPlane(cvbuf, 0) },
            self.width as usize,
            self.height as usize,
        );

        // UV interleaved plane (plane 1, half height)
        copy_plane(
            unsafe { (*nv12_raw).data[1] },
            unsafe { (*nv12_raw).linesize[1] as usize },
            unsafe { CVPixelBufferGetBaseAddressOfPlane(cvbuf, 1) },
            unsafe { CVPixelBufferGetBytesPerRowOfPlane(cvbuf, 1) },
            self.width as usize,
            self.height as usize / 2,
        );

        unsafe { CVPixelBufferUnlockBaseAddress(cvbuf, 0) };

        Some(cvbuf)
    }

    /// Flush the decoder (after seek).
    pub fn flush(&mut self) {
        self.decoder.flush();
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

/// get_format callback: prefer VideoToolbox pixel format.
unsafe extern "C" fn get_hw_format(
    _ctx: *mut ffs::AVCodecContext,
    pix_fmts: *const ffs::AVPixelFormat,
) -> ffs::AVPixelFormat {
    // SAFETY: ffmpeg passes a NULL-terminated array of pixel formats the
    // codec supports. We iterate until we find VideoToolbox or reach the
    // sentinel NONE value.
    let mut p = pix_fmts;
    while unsafe { *p } != ffs::AVPixelFormat::AV_PIX_FMT_NONE {
        if unsafe { *p } == ffs::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX {
            return ffs::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX;
        }
        p = unsafe { p.add(1) };
    }
    unsafe { *pix_fmts }
}

/// Copy `height` rows from `src` to `dst`, respecting independent strides.
fn copy_plane(
    src: *const u8,
    src_stride: usize,
    dst: *mut u8,
    dst_stride: usize,
    width: usize,
    height: usize,
) {
    for row in 0..height {
        // SAFETY: src/dst are valid plane pointers from ffmpeg and CoreVideo
        // with at least `height` rows of the given strides.
        unsafe {
            ptr::copy_nonoverlapping(src.add(row * src_stride), dst.add(row * dst_stride), width);
        }
    }
}

/// kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange ('420v')
pub(crate) const K_CV_PIXEL_FORMAT_NV12: u32 = 0x3432_3076;

/// Lazily create a CFDictionary with kCVPixelBufferIOSurfacePropertiesKey
/// so that CVPixelBufferCreate returns IOSurface-backed buffers (required by
/// AVSampleBufferDisplayLayer).
pub(crate) fn io_surface_properties() -> *const c_void {
    static PROPS: OnceLock<usize> = OnceLock::new();
    *PROPS.get_or_init(|| unsafe {
        let empty = CFDictionaryCreate(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );
        let keys: [*const c_void; 1] = [kCVPixelBufferIOSurfacePropertiesKey];
        let vals: [*const c_void; 1] = [empty];
        let attrs = CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );
        CFRelease(empty);
        attrs as usize
    }) as *const c_void
}

/// Release a CVPixelBuffer that was retained by the decoder.
pub unsafe fn release_pixel_buffer(buf: *mut c_void) {
    if !buf.is_null() {
        // SAFETY: Caller guarantees `buf` is a valid, retained CVPixelBuffer.
        unsafe { CVPixelBufferRelease(buf) };
    }
}

// SAFETY: CoreVideo/CoreFoundation framework functions linked via the system SDK.
unsafe extern "C" {
    fn CVPixelBufferRetain(pixelBuffer: *mut c_void) -> *mut c_void;
    fn CVPixelBufferRelease(pixelBuffer: *mut c_void);

    fn CVPixelBufferCreate(
        allocator: *const c_void,
        width: usize,
        height: usize,
        pixel_format_type: u32,
        pixel_buffer_attributes: *const c_void,
        pixel_buffer_out: *mut *mut c_void,
    ) -> i32;

    fn CVPixelBufferLockBaseAddress(pixel_buffer: *mut c_void, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pixel_buffer: *mut c_void, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddressOfPlane(pixel_buffer: *mut c_void, plane: usize) -> *mut u8;
    fn CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer: *mut c_void, plane: usize) -> usize;

    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;

    fn CFRelease(cf: *const c_void);

    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    static kCVPixelBufferIOSurfacePropertiesKey: *const c_void;
}
