use std::ffi::c_void;
use std::ptr;

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::context::Context as CodecContext;
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

                if unsafe { (*raw).format } != ffs::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX as i32 {
                    log::error!("Software decoded frame has no CVPixelBuffer — skipping");
                    return None;
                }

                // SAFETY: For VideoToolbox frames, data[3] is the
                // CVPixelBufferRef per ffmpeg convention. We retain it so it
                // outlives the AVFrame (which may reuse its internal buffer).
                let cvbuf = unsafe { (*raw).data[3] as *mut c_void };
                if cvbuf.is_null() {
                    return None;
                }
                // SAFETY: CVPixelBufferRetain increments the refcount of a
                // valid CVPixelBuffer. PixelBuffer::new takes ownership of
                // the retained reference and releases it on drop.
                unsafe { CVPixelBufferRetain(cvbuf) };

                Some(VideoFrame {
                    pixel_buffer: Some(PixelBuffer::new(cvbuf)),
                    pts_us,
                    duration_us,
                    seek_flush: false,
                })
            }
            Err(_) => None,
        }
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

/// Release a CVPixelBuffer that was retained by the decoder.
pub unsafe fn release_pixel_buffer(buf: *mut c_void) {
    if !buf.is_null() {
        // SAFETY: Caller guarantees `buf` is a valid, retained CVPixelBuffer.
        unsafe { CVPixelBufferRelease(buf) };
    }
}

// CoreVideo FFI
unsafe extern "C" {
    fn CVPixelBufferRetain(pixelBuffer: *mut c_void) -> *mut c_void;
    fn CVPixelBufferRelease(pixelBuffer: *mut c_void);
}
