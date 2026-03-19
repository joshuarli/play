//! Bitmap subtitle compositing onto NV12 CVPixelBuffers.
//!
//! Creates a new NV12 CVPixelBuffer matching the source frame's actual
//! dimensions, copies both planes, and alpha-blends the subtitle bitmap.

use std::ffi::c_void;
use std::ptr;

use crate::decode_video::{K_CV_PIXEL_FORMAT_NV12, io_surface_properties};
use crate::subtitle::BitmapSubtitle;

// SAFETY: CoreVideo framework functions linked via the system SDK.
unsafe extern "C" {
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
    fn CVPixelBufferGetHeightOfPlane(pixel_buffer: *mut c_void, plane: usize) -> usize;
    fn CVPixelBufferGetWidth(pixel_buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetHeight(pixel_buffer: *mut c_void) -> usize;
}

const K_CV_LOCK_READ_ONLY: u64 = 1;

/// Composite bitmap subtitles onto an NV12 CVPixelBuffer.
/// Returns a NEW CVPixelBuffer with subtitles burned in, or None on failure.
pub fn composite_subtitles(src: *mut c_void, subs: &[&BitmapSubtitle]) -> Option<*mut c_void> {
    if src.is_null() || subs.is_empty() {
        return None;
    }

    // Read actual dimensions from the source pixel buffer
    let width = unsafe { CVPixelBufferGetWidth(src) };
    let height = unsafe { CVPixelBufferGetHeight(src) };

    // Create destination with matching dimensions
    let mut dst_pb: *mut c_void = ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreate(
            ptr::null(),
            width,
            height,
            K_CV_PIXEL_FORMAT_NV12,
            io_surface_properties(),
            &mut dst_pb,
        )
    };
    if status != 0 || dst_pb.is_null() {
        return None;
    }

    // SAFETY: Both pixel buffers are valid.
    unsafe { CVPixelBufferLockBaseAddress(src, K_CV_LOCK_READ_ONLY) };
    unsafe { CVPixelBufferLockBaseAddress(dst_pb, 0) };

    // Copy Y plane
    let src_y = unsafe { CVPixelBufferGetBaseAddressOfPlane(src, 0) };
    let dst_y = unsafe { CVPixelBufferGetBaseAddressOfPlane(dst_pb, 0) };
    let src_y_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(src, 0) };
    let dst_y_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(dst_pb, 0) };
    let y_height = unsafe { CVPixelBufferGetHeightOfPlane(src, 0) };

    for row in 0..y_height {
        unsafe {
            ptr::copy_nonoverlapping(
                src_y.add(row * src_y_stride),
                dst_y.add(row * dst_y_stride),
                src_y_stride.min(dst_y_stride),
            );
        }
    }

    // Copy UV plane
    let src_uv = unsafe { CVPixelBufferGetBaseAddressOfPlane(src, 1) };
    let dst_uv = unsafe { CVPixelBufferGetBaseAddressOfPlane(dst_pb, 1) };
    let src_uv_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(src, 1) };
    let dst_uv_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(dst_pb, 1) };
    let uv_height = unsafe { CVPixelBufferGetHeightOfPlane(src, 1) };

    for row in 0..uv_height {
        unsafe {
            ptr::copy_nonoverlapping(
                src_uv.add(row * src_uv_stride),
                dst_uv.add(row * dst_uv_stride),
                src_uv_stride.min(dst_uv_stride),
            );
        }
    }

    // Blend subtitles
    let w = width as u32;
    let h = height as u32;
    for sub in subs {
        blend_y_plane(dst_y, dst_y_stride, w, h, sub);
        blend_uv_plane(dst_uv, dst_uv_stride, w, h, sub);
    }

    unsafe { CVPixelBufferUnlockBaseAddress(dst_pb, 0) };
    unsafe { CVPixelBufferUnlockBaseAddress(src, K_CV_LOCK_READ_ONLY) };

    Some(dst_pb)
}

/// Alpha-blend subtitle RGBA onto the NV12 Y plane.
fn blend_y_plane(
    y_plane: *mut u8,
    y_stride: usize,
    video_w: u32,
    video_h: u32,
    sub: &BitmapSubtitle,
) {
    for row in 0..sub.h {
        let dst_row = sub.y + row;
        if dst_row >= video_h {
            break;
        }
        for col in 0..sub.w {
            let dst_col = sub.x + col;
            if dst_col >= video_w {
                continue;
            }
            let src_idx = (row * sub.w + col) as usize * 4;
            let alpha = sub.rgba[src_idx + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let (sy, _, _) = rgba_to_ycbcr(
                sub.rgba[src_idx],
                sub.rgba[src_idx + 1],
                sub.rgba[src_idx + 2],
            );
            let dst_off = dst_row as usize * y_stride + dst_col as usize;
            unsafe {
                if alpha == 255 {
                    *y_plane.add(dst_off) = sy;
                } else {
                    let bg = *y_plane.add(dst_off) as u32;
                    *y_plane.add(dst_off) =
                        ((sy as u32 * alpha + bg * (255 - alpha) + 128) / 255) as u8;
                }
            }
        }
    }
}

/// Alpha-blend subtitle RGBA onto the NV12 interleaved UV plane (half resolution).
fn blend_uv_plane(
    uv_plane: *mut u8,
    uv_stride: usize,
    video_w: u32,
    video_h: u32,
    sub: &BitmapSubtitle,
) {
    for row in (0..sub.h).step_by(2) {
        let dst_row = sub.y + row;
        if dst_row >= video_h {
            break;
        }
        let uv_row = (dst_row / 2) as usize;
        for col in (0..sub.w).step_by(2) {
            let dst_col = sub.x + col;
            if dst_col >= video_w {
                continue;
            }
            let src_idx = (row * sub.w + col) as usize * 4;
            let alpha = sub.rgba[src_idx + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let (_, cb, cr) = rgba_to_ycbcr(
                sub.rgba[src_idx],
                sub.rgba[src_idx + 1],
                sub.rgba[src_idx + 2],
            );
            let uv_off = uv_row * uv_stride + (dst_col as usize & !1);
            unsafe {
                if alpha == 255 {
                    *uv_plane.add(uv_off) = cb;
                    *uv_plane.add(uv_off + 1) = cr;
                } else {
                    let bg_cb = *uv_plane.add(uv_off) as u32;
                    let bg_cr = *uv_plane.add(uv_off + 1) as u32;
                    *uv_plane.add(uv_off) =
                        ((cb as u32 * alpha + bg_cb * (255 - alpha) + 128) / 255) as u8;
                    *uv_plane.add(uv_off + 1) =
                        ((cr as u32 * alpha + bg_cr * (255 - alpha) + 128) / 255) as u8;
                }
            }
        }
    }
}

/// BT.601 RGBA → Y, Cb, Cr (video range 16-235/16-240).
fn rgba_to_ycbcr(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let rf = r as f32;
    let gf = g as f32;
    let bf = b as f32;
    let y = (16.0 + 65.481 * rf / 255.0 + 128.553 * gf / 255.0 + 24.966 * bf / 255.0) as u8;
    let cb = (128.0 - 37.797 * rf / 255.0 - 74.203 * gf / 255.0 + 112.0 * bf / 255.0) as u8;
    let cr = (128.0 + 112.0 * rf / 255.0 - 93.786 * gf / 255.0 - 18.214 * bf / 255.0) as u8;
    (y, cb, cr)
}
