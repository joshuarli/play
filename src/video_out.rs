use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr;

use objc2::encode::{Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};

use crate::cmd::VideoFrame;

// ── Opaque CoreMedia types ─────────────────────────────────────────

#[repr(C)]
struct OpaqueCMSampleBuffer {
    _priv: [u8; 0],
}

// SAFETY: Encoding matches CoreMedia's opaqueCMSampleBuffer struct pointer.
unsafe impl RefEncode for OpaqueCMSampleBuffer {
    const ENCODING_REF: Encoding =
        Encoding::Pointer(&Encoding::Struct("opaqueCMSampleBuffer", &[]));
}

#[repr(C)]
struct OpaqueCMTimebase {
    _priv: [u8; 0],
}

// SAFETY: Encoding matches CoreMedia's OpaqueCMTimebase struct pointer.
unsafe impl RefEncode for OpaqueCMTimebase {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("OpaqueCMTimebase", &[]));
}

type CVPixelBufferRef = *mut c_void;
type CMSampleBufferRef = *mut OpaqueCMSampleBuffer;
type CMVideoFormatDescriptionRef = *mut c_void;
type CMTimebaseRef = *mut OpaqueCMTimebase;
type CMClockRef = *mut c_void;
type CFAllocatorRef = *const c_void;
type OSStatus = i32;

// ── CMTime ─────────────────────────────────────────────────────────

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

// ── CoreMedia FFI ──────────────────────────────────────────────────

// SAFETY: CoreMedia/CoreVideo framework functions linked via the system SDK.
// All pointer parameters are validated at each call site before use.
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

// ── RAII wrappers ──────────────────────────────────────────────────

/// RAII wrapper for a CMTimebaseRef. Manages the timebase lifecycle and
/// provides safe accessors for time/rate operations.
struct Timebase(CMTimebaseRef);

impl Timebase {
    /// Create a new CMTimebase driven by the host time clock, starting paused.
    fn new() -> Option<Self> {
        let mut tb: CMTimebaseRef = ptr::null_mut();
        // SAFETY: CMTimebaseCreateWithSourceClock allocates a new timebase.
        // On success (status 0), tb is a valid CMTimebaseRef.
        let status = unsafe {
            CMTimebaseCreateWithSourceClock(ptr::null(), CMClockGetHostTimeClock(), &mut tb)
        };
        if status == 0 && !tb.is_null() {
            // SAFETY: tb is valid; set initial time and paused rate.
            unsafe {
                CMTimebaseSetTime(tb, CMTime::from_us(0));
                CMTimebaseSetRate(tb, 0.0);
            }
            Some(Self(tb))
        } else {
            log::warn!("Failed to create CMTimebase (status={status})");
            None
        }
    }

    fn set_time(&self, us: i64) {
        // SAFETY: self.0 is a valid CMTimebase (from new()).
        unsafe { CMTimebaseSetTime(self.0, CMTime::from_us(us)) };
    }

    fn set_rate(&self, rate: f64) {
        // SAFETY: self.0 is a valid CMTimebase.
        unsafe { CMTimebaseSetRate(self.0, rate) };
    }

    fn time_us(&self) -> i64 {
        // SAFETY: self.0 is a valid CMTimebase.
        unsafe { CMTimebaseGetTime(self.0) }.to_us()
    }

    fn raw(&self) -> CMTimebaseRef {
        self.0
    }
}

// Timebase is not dropped (lives for process lifetime via DisplayOutput).
// If we needed Drop: CMTimebase is a CFType, released via CFRelease.

/// RAII wrapper for a CMVideoFormatDescriptionRef.
struct FormatDescription(*mut c_void);

impl FormatDescription {
    fn from_pixel_buffer(pixel_buffer: CVPixelBufferRef) -> Option<Self> {
        let mut desc: CMVideoFormatDescriptionRef = ptr::null_mut();
        // SAFETY: pixel_buffer is a valid, retained CVPixelBuffer.
        let status = unsafe {
            CMVideoFormatDescriptionCreateForImageBuffer(ptr::null(), pixel_buffer, &mut desc)
        };
        if status != 0 {
            log::error!("CMVideoFormatDescriptionCreateForImageBuffer failed: {status}");
            return None;
        }
        Some(Self(desc))
    }

    fn raw(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for FormatDescription {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: self.0 is a valid CMVideoFormatDescriptionRef (CFType).
            unsafe { CFRelease(self.0) };
        }
    }
}

// ── DisplayOutput ──────────────────────────────────────────────────

/// Owns the AVSampleBufferDisplayLayer and its associated state.
/// Created once on the main thread; state is reset between files.
pub struct DisplayOutput {
    /// Raw pointer to the AVSampleBufferDisplayLayer (Retained::into_raw).
    layer: *mut AnyObject,
    timebase: Option<Timebase>,
    timebase_started: bool,
    cached_format_desc: Option<FormatDescription>,
}

impl DisplayOutput {
    /// Create a new DisplayOutput with an AVSampleBufferDisplayLayer.
    pub fn new(width: u32, height: u32) -> Self {
        let cls = AnyClass::get(c"AVSampleBufferDisplayLayer")
            .expect("AVSampleBufferDisplayLayer class not found");
        // SAFETY: [AVSampleBufferDisplayLayer new] creates a new layer.
        let layer: Retained<AnyObject> = unsafe { msg_send![cls, new] };

        // SAFETY: setVideoGravity: is a valid method.
        let gravity = objc2_foundation::NSString::from_str("AVLayerVideoGravityResizeAspect");
        let _: () = unsafe { msg_send![&*layer, setVideoGravity: &*gravity] };

        let timebase = Timebase::new();
        if let Some(ref tb) = timebase {
            // SAFETY: layer and tb.raw() are valid; setControlTimebase:
            // transfers timing control to our timebase.
            let _: () = unsafe { msg_send![&*layer, setControlTimebase: tb.raw()] };
            log::debug!("Display layer timebase created (paused until first frame)");
        }

        let layer_ptr = Retained::into_raw(layer);
        log::debug!("Display layer initialized for {width}x{height}");

        Self {
            layer: layer_ptr,
            timebase,
            timebase_started: false,
            cached_format_desc: None,
        }
    }

    /// Get the raw layer pointer (for adding as sublayer).
    pub fn layer_ptr(&self) -> *mut c_void {
        self.layer as *mut c_void
    }

    /// Enqueue a video frame for display.
    pub fn enqueue_frame(&mut self, mut frame: VideoFrame) {
        let Some(pb) = frame.pixel_buffer.take() else {
            return;
        };

        // Flush + reset timebase on seek
        if frame.seek_flush {
            // SAFETY: self.layer is a valid AVSampleBufferDisplayLayer.
            let _: () = unsafe { msg_send![self.layer, flush] };
            if let Some(ref tb) = self.timebase {
                tb.set_time(frame.pts_us);
            }
            self.timebase_started = true;
        } else if !self.timebase_started {
            // First frame: align timebase and start playback
            if let Some(ref tb) = self.timebase {
                tb.set_time(frame.pts_us);
                tb.set_rate(1.0);
            }
            self.timebase_started = true;
            log::debug!("Timebase started at PTS {}us", frame.pts_us);
        }

        // Take ownership of the pixel buffer
        let pixel_buffer = pb.take();

        // Reuse cached format description (same resolution/format per file)
        let format_desc_raw = if let Some(ref fd) = self.cached_format_desc {
            fd.raw()
        } else {
            match FormatDescription::from_pixel_buffer(pixel_buffer) {
                Some(fd) => {
                    let raw = fd.raw();
                    self.cached_format_desc = Some(fd);
                    raw
                }
                None => {
                    // SAFETY: pixel_buffer is valid.
                    unsafe { CVPixelBufferRelease(pixel_buffer) };
                    return;
                }
            }
        };

        // Create CMSampleBuffer
        let timing = CMSampleTimingInfo {
            duration: CMTime::from_us(frame.duration_us),
            presentation_time_stamp: CMTime::from_us(frame.pts_us),
            decode_time_stamp: K_CM_TIME_INVALID,
        };

        let mut sample_buffer: CMSampleBufferRef = ptr::null_mut();
        // SAFETY: pixel_buffer and format_desc_raw are valid.
        let status = unsafe {
            CMSampleBufferCreateReadyWithImageBuffer(
                ptr::null(),
                pixel_buffer,
                format_desc_raw,
                &timing,
                &mut sample_buffer,
            )
        };

        if status != 0 {
            log::error!("CMSampleBufferCreateReadyWithImageBuffer failed: {status}");
            // SAFETY: pixel_buffer is still valid.
            unsafe { CVPixelBufferRelease(pixel_buffer) };
            return;
        }

        // SAFETY: layer is valid; enqueueSampleBuffer: retains the sample
        // buffer internally.
        let _: () = unsafe { msg_send![self.layer, enqueueSampleBuffer: sample_buffer] };

        // SAFETY: Release our references. CMSampleBuffer retains the pixel
        // buffer internally.
        unsafe {
            CFRelease(sample_buffer as *const c_void);
            CVPixelBufferRelease(pixel_buffer);
        }
    }

    /// Sync timebase to audio clock. Only adjusts if drift exceeds 5ms.
    pub fn sync_timebase(&self, audio_pts_us: i64) {
        if !self.timebase_started {
            return;
        }
        if let Some(ref tb) = self.timebase {
            let drift = audio_pts_us - tb.time_us();
            if drift.abs() > 5_000 {
                tb.set_time(audio_pts_us);
                log::debug!("Timebase drift corrected: {drift}us");
            }
        }
    }

    /// Set the timebase rate (1.0 = playing, 0.0 = paused).
    pub fn set_playback_rate(&self, rate: f64) {
        if let Some(ref tb) = self.timebase {
            tb.set_rate(rate);
        }
    }

    /// Flush the display layer and reset timebase for a seek.
    pub fn flush_and_seek(&mut self, pts_us: i64) {
        // SAFETY: self.layer is a valid AVSampleBufferDisplayLayer.
        let _: () = unsafe { msg_send![self.layer, flush] };
        if let Some(ref tb) = self.timebase {
            tb.set_time(pts_us);
        }
        self.timebase_started = true;
    }

    /// Reset state between files (new resolution, new format description).
    pub fn reset_for_new_file(&mut self) {
        self.cached_format_desc = None;
        self.timebase_started = false;
        // SAFETY: self.layer is a valid AVSampleBufferDisplayLayer.
        let _: () = unsafe { msg_send![self.layer, flush] };
    }
}

// ── Global access (main-thread-only) ───────────────────────────────
//
// DisplayOutput lives in a thread-local RefCell because AppKit requires
// main-thread access patterns and define_class! structs can't hold ivars.

std::thread_local! {
    static DISPLAY: RefCell<Option<DisplayOutput>> = const { RefCell::new(None) };
}

/// Initialize the global DisplayOutput. Must be called on main thread.
pub fn init_display(width: u32, height: u32) {
    DISPLAY.with(|d| *d.borrow_mut() = Some(DisplayOutput::new(width, height)));
}

/// Get the raw display layer pointer (for adding as sublayer).
pub fn display_layer_ptr() -> Option<*mut c_void> {
    DISPLAY.with(|d| d.borrow().as_ref().map(|d| d.layer_ptr()))
}

/// Enqueue a video frame. Must be called on main thread.
pub fn enqueue_frame(frame: VideoFrame) {
    DISPLAY.with(|d| {
        if let Some(ref mut display) = *d.borrow_mut() {
            display.enqueue_frame(frame);
        }
    });
}

/// Sync timebase to audio clock. Must be called on main thread.
pub fn sync_timebase(audio_pts_us: i64) {
    DISPLAY.with(|d| {
        if let Some(ref display) = *d.borrow() {
            display.sync_timebase(audio_pts_us);
        }
    });
}

/// Set playback rate. Must be called on main thread.
pub fn set_playback_rate(rate: f64) {
    DISPLAY.with(|d| {
        if let Some(ref display) = *d.borrow() {
            display.set_playback_rate(rate);
        }
    });
}

/// Flush and seek. Must be called on main thread.
pub fn flush_and_seek(pts_us: i64) {
    DISPLAY.with(|d| {
        if let Some(ref mut display) = *d.borrow_mut() {
            display.flush_and_seek(pts_us);
        }
    });
}

/// Reset for new file. Must be called on main thread.
pub fn reset_for_new_file() {
    DISPLAY.with(|d| {
        if let Some(ref mut display) = *d.borrow_mut() {
            display.reset_for_new_file();
        }
    });
}
