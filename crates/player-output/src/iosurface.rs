use core::ffi::c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRetain(object: *const c_void) -> *const c_void;
    fn CFRelease(object: *const c_void);
}

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceIncrementUseCount(surface: *mut c_void);
    fn IOSurfaceDecrementUseCount(surface: *mut c_void);
}

#[derive(Debug)]
pub struct NativeSurface {
    raw: *mut c_void,
    pub(crate) dimensions: [(i32, i32); 2],
}

// SAFETY: IOSurface is a reference-counted cross-thread shared object. This
// owner only samples the immutable decoded image while its use count is held.
unsafe impl Send for NativeSurface {}

impl NativeSurface {
    /// # Safety
    /// `raw` must be a live IOSurface belonging to the decoded frame and the
    /// frame must stay alive until this call returns. Dimensions must describe
    /// its two NV12 or P010 planes.
    pub unsafe fn retain(raw: *mut c_void, dimensions: [(i32, i32); 2]) -> Self {
        assert!(!raw.is_null());
        // SAFETY: the caller guarantees a live IOSurface for this call.
        unsafe { CFRetain(raw); IOSurfaceIncrementUseCount(raw); }
        Self { raw, dimensions }
    }

    pub(crate) fn raw(&self) -> *mut c_void { self.raw }
}

impl Clone for NativeSurface {
    fn clone(&self) -> Self {
        // SAFETY: self owns a live IOSurface with the same plane dimensions.
        unsafe { Self::retain(self.raw, self.dimensions) }
    }
}

impl Drop for NativeSurface {
    fn drop(&mut self) {
        // SAFETY: each owner holds one use count and one CF reference.
        unsafe { IOSurfaceDecrementUseCount(self.raw); CFRelease(self.raw); }
    }
}
