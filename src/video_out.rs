use std::ffi::c_void;
use std::ptr;
use std::sync::OnceLock;

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
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("opaqueCMSampleBuffer", &[]));
}

type CVPixelBufferRef = *mut c_void;
type CMSampleBufferRef = *mut OpaqueCMSampleBuffer;
type CMVideoFormatDescriptionRef = *mut c_void;
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
}

/// Wrapper to make *mut c_void Send+Sync for OnceLock.
/// SAFETY: the display layer pointer is only accessed from the main thread.
struct SendPtr(*mut c_void);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

/// Global display layer (created on main thread, used from main thread timer).
static DISPLAY_LAYER: OnceLock<SendPtr> = OnceLock::new();

/// Initialize the AVSampleBufferDisplayLayer. Must be called on main thread.
pub fn init_display_layer(width: u32, height: u32) {
    let cls = AnyClass::get(c"AVSampleBufferDisplayLayer").expect("AVSampleBufferDisplayLayer class not found");
    let layer: Retained<AnyObject> = unsafe { msg_send![cls, new] };

    // Set video gravity to resize aspect
    let gravity = objc2_foundation::NSString::from_str("AVLayerVideoGravityResizeAspect");
    let _: () = unsafe { msg_send![&*layer, setVideoGravity: &*gravity] };

    // Store the raw pointer (retain is held by our layer_retained below)
    let ptr: *mut c_void = Retained::into_raw(layer) as *mut c_void;
    DISPLAY_LAYER.set(SendPtr(ptr)).ok();

    log::debug!("Display layer initialized for {width}x{height}");
}

/// Get the display layer as a CALayer to add as sublayer.
pub fn display_layer_ptr() -> Option<*mut c_void> {
    DISPLAY_LAYER.get().map(|p| p.0)
}

/// Enqueue a video frame for display. Must be called on main thread.
pub fn enqueue_frame(frame: VideoFrame) {
    let Some(layer_wrap) = DISPLAY_LAYER.get() else {
        // No display layer — release the pixel buffer and return
        if !frame.pixel_buffer.is_null() {
            unsafe { CVPixelBufferRelease(frame.pixel_buffer) };
        }
        return;
    };

    if frame.pixel_buffer.is_null() {
        return;
    }

    // Create CMVideoFormatDescription
    let mut format_desc: CMVideoFormatDescriptionRef = ptr::null_mut();
    let status = unsafe {
        CMVideoFormatDescriptionCreateForImageBuffer(
            ptr::null(),
            frame.pixel_buffer,
            &mut format_desc,
        )
    };
    if status != 0 {
        log::error!("CMVideoFormatDescriptionCreateForImageBuffer failed: {status}");
        unsafe { CVPixelBufferRelease(frame.pixel_buffer) };
        return;
    }

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
            frame.pixel_buffer,
            format_desc,
            &timing,
            &mut sample_buffer,
        )
    };

    unsafe { CFRelease(format_desc) };

    if status != 0 {
        log::error!("CMSampleBufferCreateReadyWithImageBuffer failed: {status}");
        unsafe { CVPixelBufferRelease(frame.pixel_buffer) };
        return;
    }

    // Enqueue to display layer
    let layer_ptr = layer_wrap.0;
    let layer = layer_ptr as *mut AnyObject;
    let _: () = unsafe { msg_send![layer, enqueueSampleBuffer: sample_buffer] };

    // Release
    unsafe {
        CFRelease(sample_buffer as *const c_void);
        CVPixelBufferRelease(frame.pixel_buffer);
    }
}
