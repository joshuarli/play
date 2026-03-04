use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::encode::{Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};

use crate::cmd::VideoFrame;

/// Opaque CoreMedia/CoreVideo types with proper ObjC encoding.
#[repr(C)]
struct OpaqueCMSampleBuffer {
    _priv: [u8; 0],
}

unsafe impl RefEncode for OpaqueCMSampleBuffer {
    const ENCODING_REF: Encoding =
        Encoding::Pointer(&Encoding::Struct("opaqueCMSampleBuffer", &[]));
}

type CVPixelBufferRef = *mut c_void;
type CMSampleBufferRef = *mut OpaqueCMSampleBuffer;
type CMVideoFormatDescriptionRef = *mut c_void;
/// Opaque CMTimebase with correct ObjC encoding for msg_send! validation.
#[repr(C)]
struct OpaqueCMTimebase {
    _priv: [u8; 0],
}

unsafe impl RefEncode for OpaqueCMTimebase {
    const ENCODING_REF: Encoding =
        Encoding::Pointer(&Encoding::Struct("OpaqueCMTimebase", &[]));
}

type CMTimebaseRef = *mut OpaqueCMTimebase;
type CMClockRef = *mut c_void;
type CFAllocatorRef = *const c_void;
type OSStatus = i32;

/// CMTime
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

const K_CM_TIME_FLAGS_VALID: u32 = 1;

impl CMTime {
    fn make(value: i64, timescale: i32) -> Self {
        Self {
            value,
            timescale,
            flags: K_CM_TIME_FLAGS_VALID,
            epoch: 0,
        }
    }

    fn from_us(us: i64) -> Self {
        Self::make(us, 1_000_000)
    }

    fn to_us(self) -> i64 {
        if self.timescale == 0 || self.flags & K_CM_TIME_FLAGS_VALID == 0 {
            return 0;
        }
        self.value * 1_000_000 / self.timescale as i64
    }
}

/// CMSampleTimingInfo
#[repr(C)]
struct CMSampleTimingInfo {
    duration: CMTime,
    presentation_time_stamp: CMTime,
    decode_time_stamp: CMTime,
}

const K_CM_TIME_INVALID: CMTime = CMTime {
    value: 0,
    timescale: 0,
    flags: 0,
    epoch: 0,
};

// CoreMedia C FFI
unsafe extern "C" {
    fn CMVideoFormatDescriptionCreateForImageBuffer(
        allocator: CFAllocatorRef,
        image_buffer: CVPixelBufferRef,
        format_description_out: *mut CMVideoFormatDescriptionRef,
    ) -> OSStatus;

    fn CMSampleBufferCreateReadyWithImageBuffer(
        allocator: CFAllocatorRef,
        image_buffer: CVPixelBufferRef,
        format_description: CMVideoFormatDescriptionRef,
        sample_timing: *const CMSampleTimingInfo,
        sample_buffer_out: *mut CMSampleBufferRef,
    ) -> OSStatus;

    fn CFRelease(cf: *const c_void);
    fn CVPixelBufferRelease(pixelBuffer: *mut c_void);

    fn CMClockGetHostTimeClock() -> CMClockRef;
    fn CMTimebaseCreateWithSourceClock(
        allocator: CFAllocatorRef,
        source_clock: CMClockRef,
        timebase_out: *mut CMTimebaseRef,
    ) -> OSStatus;
    fn CMTimebaseSetTime(timebase: CMTimebaseRef, time: CMTime) -> OSStatus;
    fn CMTimebaseSetRate(timebase: CMTimebaseRef, rate: f64) -> OSStatus;
    fn CMTimebaseGetTime(timebase: CMTimebaseRef) -> CMTime;
}

/// Wrapper to make *mut c_void Send+Sync for OnceLock.
struct SendPtr(*mut c_void);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

/// Global display layer (created on main thread, used from main thread timer).
static DISPLAY_LAYER: OnceLock<SendPtr> = OnceLock::new();

/// Cached CMVideoFormatDescription — identical for all frames of the same file.
/// Mutex instead of OnceLock so it can be reset between files (different resolutions).
static CACHED_FORMAT_DESC: Mutex<Option<SendPtr>> = Mutex::new(None);

/// Wrapper to make CMTimebaseRef Send+Sync for OnceLock.
struct SendTimebase(CMTimebaseRef);
unsafe impl Send for SendTimebase {}
unsafe impl Sync for SendTimebase {}

/// CMTimebase driving the display layer's presentation timing.
static TIMEBASE: OnceLock<SendTimebase> = OnceLock::new();
/// Whether the timebase has been started (aligned to first frame).
static TIMEBASE_STARTED: AtomicBool = AtomicBool::new(false);

/// Initialize the AVSampleBufferDisplayLayer. Must be called on main thread.
pub fn init_display_layer(width: u32, height: u32) {
    let cls = AnyClass::get(c"AVSampleBufferDisplayLayer")
        .expect("AVSampleBufferDisplayLayer class not found");
    let layer: Retained<AnyObject> = unsafe { msg_send![cls, new] };

    // Set video gravity to resize aspect
    let gravity = objc2_foundation::NSString::from_str("AVLayerVideoGravityResizeAspect");
    let _: () = unsafe { msg_send![&*layer, setVideoGravity: &*gravity] };

    // Create a CMTimebase — start paused (rate 0.0) until the first frame arrives
    let mut timebase: CMTimebaseRef = ptr::null_mut();
    let status = unsafe {
        CMTimebaseCreateWithSourceClock(ptr::null(), CMClockGetHostTimeClock(), &mut timebase)
    };
    if status == 0 && !timebase.is_null() {
        unsafe {
            CMTimebaseSetTime(timebase, CMTime::from_us(0));
            CMTimebaseSetRate(timebase, 0.0);
        }
        let _: () = unsafe { msg_send![&*layer, setControlTimebase: timebase] };
        TIMEBASE.set(SendTimebase(timebase)).ok();
        log::debug!("Display layer timebase created (paused until first frame)");
    } else {
        log::warn!("Failed to create CMTimebase (status={status}), playback timing may be wrong");
    }

    // Store the raw pointer
    let ptr: *mut c_void = Retained::into_raw(layer) as *mut c_void;
    DISPLAY_LAYER.set(SendPtr(ptr)).ok();

    log::debug!("Display layer initialized for {width}x{height}");
}

/// Get the display layer as a CALayer to add as sublayer.
pub fn display_layer_ptr() -> Option<*mut c_void> {
    DISPLAY_LAYER.get().map(|p| p.0)
}

/// Enqueue a video frame for display. Must be called on main thread.
pub fn enqueue_frame(mut frame: VideoFrame) {
    let Some(layer_wrap) = DISPLAY_LAYER.get() else {
        return; // Drop releases the pixel buffer
    };

    if frame.pixel_buffer.is_null() {
        return;
    }

    // Timebase handling: call get() once, then branch on seek_flush vs first-frame.
    let timebase = TIMEBASE.get();

    // Flush display layer and reset timebase right before enqueuing, so the
    // old frame is replaced atomically with no VSync gap.
    if frame.seek_flush {
        let layer = layer_wrap.0 as *mut AnyObject;
        let _: () = unsafe { msg_send![layer, flush] };
        if let Some(tb) = timebase {
            unsafe {
                CMTimebaseSetTime(tb.0, CMTime::from_us(frame.pts_us));
            }
            TIMEBASE_STARTED.store(true, Ordering::Relaxed);
        }
    } else if !TIMEBASE_STARTED.load(Ordering::Relaxed) {
        // On the first valid frame, align the timebase to this frame's PTS and start it
        if let Some(tb) = timebase {
            unsafe {
                CMTimebaseSetTime(tb.0, CMTime::from_us(frame.pts_us));
                CMTimebaseSetRate(tb.0, 1.0);
            }
            TIMEBASE_STARTED.store(true, Ordering::Relaxed);
            log::debug!("Timebase started at PTS {}us", frame.pts_us);
        }
    }

    // Take ownership — we'll release after handing to CoreMedia
    let pixel_buffer = frame.take_pixel_buffer();

    // Reuse cached CMVideoFormatDescription (same resolution/pixel format for entire file)
    let format_desc = {
        let mut guard = CACHED_FORMAT_DESC.lock().unwrap();
        if let Some(ref cached) = *guard {
            cached.0
        } else {
            let mut desc: CMVideoFormatDescriptionRef = ptr::null_mut();
            let status = unsafe {
                CMVideoFormatDescriptionCreateForImageBuffer(ptr::null(), pixel_buffer, &mut desc)
            };
            if status != 0 {
                log::error!("CMVideoFormatDescriptionCreateForImageBuffer failed: {status}");
                unsafe { CVPixelBufferRelease(pixel_buffer) };
                return;
            }
            *guard = Some(SendPtr(desc));
            desc
        }
    };

    // Create CMSampleBuffer
    let timing = CMSampleTimingInfo {
        duration: CMTime::from_us(frame.duration_us),
        presentation_time_stamp: CMTime::from_us(frame.pts_us),
        decode_time_stamp: K_CM_TIME_INVALID,
    };

    let mut sample_buffer: CMSampleBufferRef = ptr::null_mut();
    let status = unsafe {
        CMSampleBufferCreateReadyWithImageBuffer(
            ptr::null(),
            pixel_buffer,
            format_desc,
            &timing,
            &mut sample_buffer,
        )
    };

    if status != 0 {
        log::error!("CMSampleBufferCreateReadyWithImageBuffer failed: {status}");
        unsafe { CVPixelBufferRelease(pixel_buffer) };
        return;
    }

    // Enqueue to display layer
    let layer_ptr = layer_wrap.0;
    let layer = layer_ptr as *mut AnyObject;
    let _: () = unsafe { msg_send![layer, enqueueSampleBuffer: sample_buffer] };

    // Release — CMSampleBuffer retains the pixel buffer, so we release our reference
    unsafe {
        CFRelease(sample_buffer as *const c_void);
        CVPixelBufferRelease(pixel_buffer);
    }
}

/// Sync the display layer timebase to the audio clock position.
/// Only adjusts if drift exceeds 5ms to avoid fighting the timebase.
pub fn sync_timebase(audio_pts_us: i64) {
    if !TIMEBASE_STARTED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(tb) = TIMEBASE.get() {
        let tb_us = unsafe { CMTimebaseGetTime(tb.0) }.to_us();
        let drift = audio_pts_us - tb_us;
        if drift.abs() > 5_000 {
            unsafe {
                CMTimebaseSetTime(tb.0, CMTime::from_us(audio_pts_us));
            }
            log::debug!("Timebase drift corrected: {drift}us");
        }
    }
}

/// Set the timebase rate (1.0 = playing, 0.0 = paused).
pub fn set_playback_rate(rate: f64) {
    if let Some(tb) = TIMEBASE.get() {
        unsafe {
            CMTimebaseSetRate(tb.0, rate);
        }
    }
}

/// Flush the display layer and reset timebase for a seek.
/// Must be called on the main thread.
pub fn flush_and_seek(pts_us: i64) {
    if let Some(layer_wrap) = DISPLAY_LAYER.get() {
        let layer = layer_wrap.0 as *mut AnyObject;
        let _: () = unsafe { msg_send![layer, flush] };
    }
    if let Some(tb) = TIMEBASE.get() {
        unsafe {
            CMTimebaseSetTime(tb.0, CMTime::from_us(pts_us));
        }
        // Ensure it's running after seek
        TIMEBASE_STARTED.store(true, Ordering::Relaxed);
    }
}

/// Reset state between files. Must be called on the main thread.
pub fn reset_for_new_file() {
    // Release and clear cached format description (resolution may differ)
    {
        let mut guard = CACHED_FORMAT_DESC.lock().unwrap();
        if let Some(desc) = guard.take() {
            if !desc.0.is_null() {
                unsafe { CFRelease(desc.0) };
            }
        }
    }
    // Reset timebase state so next file starts fresh
    TIMEBASE_STARTED.store(false, Ordering::Relaxed);
    // Flush display layer
    if let Some(layer_wrap) = DISPLAY_LAYER.get() {
        let layer = layer_wrap.0 as *mut AnyObject;
        let _: () = unsafe { msg_send![layer, flush] };
    }
}
