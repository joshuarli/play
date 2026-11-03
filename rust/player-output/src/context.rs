use crate::sys;
use core::ffi::{c_int, c_void};
use std::ffi::CStr;

pub struct Vulkan {
    log: sys::PlLog,
    vk: sys::PlVulkan,
    gpu: sys::PlGpu,
}

impl Vulkan {
    pub fn create() -> Result<Self, String> {
        // SAFETY: pl_version returns a static string.
        let version = unsafe { CStr::from_ptr(sys::pl_version()) }
            .to_string_lossy()
            .into_owned();
        if version != sys::PLACEBO_VERSION {
            return Err(format!("libplacebo version mismatch: got {version}"));
        }
        // SAFETY: NULL params select the library defaults.
        let log = unsafe { sys::pl_log_create(sys::PLACEBO_API_VERSION, core::ptr::null()) };
        if log.is_null() {
            return Err("pl_log_create failed".to_string());
        }
        // SAFETY: NULL params select the default instance and best GPU.
        let vk = unsafe { sys::pl_vulkan_create(log, core::ptr::null()) };
        if vk.is_null() {
            // SAFETY: the log was created above and is still live.
            unsafe { sys::pl_log_destroy(&mut (log as sys::PlLog)) };
            return Err("pl_vulkan_create failed".to_string());
        }
        // SAFETY: vk is live; gpu is its first field.
        let gpu = unsafe { (*vk).gpu };
        Ok(Vulkan { log, vk, gpu })
    }

    pub fn gpu(&self) -> sys::PlGpu {
        self.gpu
    }

    pub fn find_rgba8(&self) -> Result<sys::PlFmt, String> {
        // SAFETY: the gpu is live.
        let fmt = unsafe {
            sys::pl_find_fmt(
                self.gpu,
                sys::PL_FMT_UNORM,
                4,
                8,
                8,
                sys::PL_FMT_CAP_HOST_READABLE,
            )
        };
        if fmt.is_null() {
            return Err("no RGBA8 texture format".to_string());
        }
        Ok(fmt)
    }

    pub fn create_tex(&self, width: c_int, height: c_int, fmt: sys::PlFmt) -> Result<sys::PlTex, String> {
        let params = sys::PlTexParams {
            w: width,
            h: height,
            d: 0,
            format: fmt,
            sampleable: true,
            renderable: false,
            storable: false,
            blit_src: false,
            blit_dst: false,
            host_writable: true,
            host_readable: true,
            export_handle: 0,
            import_handle: 0,
            shared_mem: sys::PlSharedMem {
                handle: sys::PlHandle {
                    ptr: core::ptr::null_mut(),
                },
                size: 0,
                offset: 0,
                drm_format_mod: 0,
                stride_w: 0,
                stride_h: 0,
                plane: 0,
            },
            initial_data: core::ptr::null(),
            user_data: core::ptr::null_mut(),
            debug_tag: core::ptr::null(),
        };
        // SAFETY: the gpu is live and params outlives the call.
        let tex = unsafe { sys::pl_tex_create(self.gpu, &params) };
        if tex.is_null() {
            return Err("pl_tex_create failed".to_string());
        }
        Ok(tex)
    }

    pub fn destroy_tex(&self, tex: sys::PlTex) {
        if tex.is_null() {
            return;
        }
        // SAFETY: the gpu and texture are live.
        unsafe { sys::pl_tex_destroy(self.gpu, &mut (tex as sys::PlTex)) };
    }

    pub fn upload(&self, tex: sys::PlTex, width: c_int, height: c_int, data: &[u8]) -> Result<(), String> {
        let params = sys::PlTexTransferParams {
            tex,
            rc: sys::PlRect3d {
                x0: 0,
                y0: 0,
                z0: 0,
                x1: width,
                y1: height,
                z1: 1,
            },
            row_pitch: 0,
            depth_pitch: 0,
            timer: core::ptr::null_mut(),
            callback: None,
            priv_data: core::ptr::null_mut(),
            buf: core::ptr::null_mut(),
            buf_offset: 0,
            ptr: data.as_ptr() as *mut c_void,
            no_import: false,
        };
        // SAFETY: the gpu and texture are live; data holds the region.
        if unsafe { sys::pl_tex_upload(self.gpu, &params) } {
            Ok(())
        } else {
            Err("pl_tex_upload failed".to_string())
        }
    }

    pub fn download(&self, tex: sys::PlTex, width: c_int, height: c_int) -> Result<Vec<u8>, String> {
        let mut data = vec![0u8; width.max(0) as usize * height.max(0) as usize * 4];
        let params = sys::PlTexTransferParams {
            tex,
            rc: sys::PlRect3d {
                x0: 0,
                y0: 0,
                z0: 0,
                x1: width,
                y1: height,
                z1: 1,
            },
            row_pitch: 0,
            depth_pitch: 0,
            timer: core::ptr::null_mut(),
            callback: None,
            priv_data: core::ptr::null_mut(),
            buf: core::ptr::null_mut(),
            buf_offset: 0,
            ptr: data.as_mut_ptr() as *mut c_void,
            no_import: false,
        };
        // SAFETY: the gpu and texture are live; data holds the region.
        if unsafe { sys::pl_tex_download(self.gpu, &params) } {
            Ok(data)
        } else {
            Err("pl_tex_download failed".to_string())
        }
    }
}

impl Drop for Vulkan {
    fn drop(&mut self) {
        // SAFETY: all textures must be destroyed before this runs.
        unsafe { sys::pl_vulkan_destroy(&mut self.vk) };
        // SAFETY: the log was created with the context and is still live.
        unsafe { sys::pl_log_destroy(&mut (self.log as sys::PlLog)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys;

    #[test]
    fn linked_placebo_matches_pin() {
        // SAFETY: pl_version returns a static string.
        let version = unsafe { CStr::from_ptr(sys::pl_version()) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(version, sys::PLACEBO_VERSION);
        assert_eq!(sys::PLACEBO_API_VERSION, 387);
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn init_and_tex_roundtrip() {
        let vk = Vulkan::create().expect("vulkan initializes headlessly");
        let fmt = vk.find_rgba8().expect("rgba8 format exists");
        let tex = vk.create_tex(4, 4, fmt).expect("texture creates");
        let pattern: Vec<u8> = (0..64).map(|i| (i * 7 + 3) as u8).collect();
        vk.upload(tex, 4, 4, &pattern).expect("upload works");
        let back = vk.download(tex, 4, 4).expect("download works");
        assert_eq!(back, pattern);
        vk.destroy_tex(tex);
    }
}
