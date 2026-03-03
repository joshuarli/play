use std::ffi::c_void;
use std::ptr;

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::util::frame::video::Video;
use ffmpeg_sys_next as ffs;

use crate::cmd::VideoFrame;
use crate::time::pts_to_us;

/// VideoToolbox-accelerated video decoder.
pub struct VideoDecoder {
    decoder: ffmpeg::decoder::Video,
    hw_device_ctx: *mut ffs::AVBufferRef,
    stream_time_base: ffmpeg::Rational,
    frame: Video,
    width: u32,
    height: u32,
}

unsafe impl Send for VideoDecoder {}

impl VideoDecoder {
    /// Create a new video decoder with VideoToolbox hardware acceleration.
    pub fn new(stream: &ffmpeg::Stream) -> Result<Self> {
        let mut codec_ctx = CodecContext::from_parameters(stream.parameters())
            .context("Failed to create video codec context")?;

        let avctx = unsafe { codec_ctx.as_mut_ptr() };

        // Try to set up VideoToolbox hardware acceleration
        let mut hw_device_ctx: *mut ffs::AVBufferRef = ptr::null_mut();
        let ret = unsafe {
            ffs::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                ffs::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
                ptr::null(),
                ptr::null_mut(),
                0,
            )
        };

        if ret >= 0 {
            unsafe {
                (*avctx).hw_device_ctx = ffs::av_buffer_ref(hw_device_ctx);
                (*avctx).get_format = Some(get_hw_format);
            }
            log::info!("VideoToolbox hardware acceleration enabled");
        } else {
            log::warn!("VideoToolbox not available (err={ret}), using software decode");
        }

        let decoder = codec_ctx.decoder().video().context("Failed to open video decoder")?;
        let width = decoder.width();
        let height = decoder.height();

        Ok(Self {
            decoder,
            hw_device_ctx,
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
                let raw = unsafe { self.frame.as_mut_ptr() };
                let pts = unsafe { (*raw).pts };
                let pts_us = pts_to_us(pts, self.stream_time_base);
                let duration = unsafe { (*raw).duration };
                let duration_us = pts_to_us(duration, self.stream_time_base);

                let pixel_buffer = if unsafe { (*raw).format }
                    == ffs::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX as i32
                {
                    // data[3] is the CVPixelBufferRef
                    let cvbuf = unsafe { (*raw).data[3] as *mut c_void };
                    // Retain the CVPixelBuffer so it outlives the frame
                    if !cvbuf.is_null() {
                        unsafe {
                            CVPixelBufferRetain(cvbuf);
                        }
                    }
                    cvbuf
                } else {
                    // Software decode: transfer to CVPixelBuffer
                    // For now, return null (we'll handle software path later)
                    log::warn!("Software decoded frame (no CVPixelBuffer)");
                    ptr::null_mut()
                };

                Some(VideoFrame {
                    pixel_buffer,
                    pts_us,
                    duration_us,
                    width: self.width,
                    height: self.height,
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

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        if !self.hw_device_ctx.is_null() {
            unsafe {
                ffs::av_buffer_unref(&mut self.hw_device_ctx);
            }
        }
    }
}

/// get_format callback: prefer VideoToolbox pixel format.
unsafe extern "C" fn get_hw_format(
    _ctx: *mut ffs::AVCodecContext,
    pix_fmts: *const ffs::AVPixelFormat,
) -> ffs::AVPixelFormat {
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
        unsafe { CVPixelBufferRelease(buf) };
    }
}

// CoreVideo FFI
unsafe extern "C" {
    fn CVPixelBufferRetain(pixelBuffer: *mut c_void) -> *mut c_void;
    fn CVPixelBufferRelease(pixelBuffer: *mut c_void);
    #[allow(dead_code)]
    fn CVPixelBufferGetWidth(pixelBuffer: *mut c_void) -> usize;
    #[allow(dead_code)]
    fn CVPixelBufferGetHeight(pixelBuffer: *mut c_void) -> usize;
}
