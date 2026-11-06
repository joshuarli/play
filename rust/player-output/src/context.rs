use crate::sys;
use crate::{Instance, Surface};
use core::ffi::{c_int, c_void};
use std::ffi::CStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuApi {
    Vulkan,
    #[cfg(target_os = "macos")]
    Metal,
}

impl Default for GpuApi {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        {
            GpuApi::Metal
        }
        #[cfg(not(target_os = "macos"))]
        {
            GpuApi::Vulkan
        }
    }
}

impl GpuApi {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "vulkan" => Some(GpuApi::Vulkan),
            #[cfg(target_os = "macos")]
            "metal" => Some(GpuApi::Metal),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            GpuApi::Vulkan => "vulkan",
            #[cfg(target_os = "macos")]
            GpuApi::Metal => "metal",
        }
    }
}

pub struct Gpu {
    log: sys::PlLog,
    backend: Backend,
    gpu: sys::PlGpu,
}

enum Backend {
    Vulkan(sys::PlVulkan),
    #[cfg(target_os = "macos")]
    Metal(sys::PlMetal),
}

impl Gpu {
    pub fn headless(api: GpuApi) -> Result<Self, String> {
        match api {
            GpuApi::Vulkan => Self::vulkan(),
            #[cfg(target_os = "macos")]
            GpuApi::Metal => Self::metal(),
        }
    }

    pub fn vulkan() -> Result<Self, String> {
        Self::create_vulkan(core::ptr::null_mut(), 0)
    }

    pub fn vulkan_for_surface(instance: &Instance, surface: &Surface) -> Result<Self, String> {
        if instance.raw().is_null() || surface.raw() == 0 {
            return Err("vulkan surface needs a live instance and surface".to_string());
        }
        Self::create_vulkan(instance.raw(), surface.raw())
    }

    fn create_vulkan(instance: sys::VkInstance, surface: sys::VkSurfaceKHR) -> Result<Self, String> {
        let (log, version) = create_log()?;
        let params = sys::PlVulkanParams {
            instance,
            get_proc_addr: core::ptr::null_mut(),
            instance_params: core::ptr::null(),
            surface,
            device: core::ptr::null_mut(),
            device_name: core::ptr::null(),
            device_uuid: [0; 16],
            allow_software: true,
            async_transfer: true,
            async_compute: true,
            queue_count: 1,
            no_compute: false,
            extra_queues: 0,
            extensions: core::ptr::null(),
            num_extensions: 0,
            opt_extensions: core::ptr::null(),
            num_opt_extensions: 0,
            features: core::ptr::null(),
            max_glsl_version: 0,
            max_api_version: 0,
        };
        // SAFETY: params starts from the library defaults plus software GPUs.
        let vk = unsafe { sys::pl_vulkan_create(log, &params) };
        if vk.is_null() {
            // SAFETY: the log was created above and is still live.
            unsafe { sys::pl_log_destroy(&mut (log as sys::PlLog)) };
            return Err("pl_vulkan_create failed".to_string());
        }
        // SAFETY: vk is live; gpu is its first field.
        let gpu = unsafe { (*vk).gpu };
        eprintln!("libplacebo: GPU 0: Vulkan device (libplacebo {version})");
        Ok(Gpu {
            log,
            backend: Backend::Vulkan(vk),
            gpu,
        })
    }

    #[cfg(target_os = "macos")]
    pub fn metal() -> Result<Self, String> {
        let (log, version) = create_log()?;
        let params = sys::PlMetalParams {
            device: core::ptr::null_mut(),
            queue: core::ptr::null_mut(),
        };
        // SAFETY: the log is live; NULL device and queue select the system
        // default device and a new queue on it.
        let metal = unsafe { sys::pl_metal_create(log, &params) };
        if metal.is_null() {
            // SAFETY: the log was created above and is still live.
            unsafe { sys::pl_log_destroy(&mut (log as sys::PlLog)) };
            return Err("pl_metal_create failed".to_string());
        }
        // SAFETY: metal is live; gpu is its first field.
        let gpu = unsafe { (*metal).gpu };
        eprintln!("libplacebo: GPU 0: Metal device (libplacebo {version})");
        Ok(Gpu {
            log,
            backend: Backend::Metal(metal),
            gpu,
        })
    }

    pub fn gpu(&self) -> sys::PlGpu {
        self.gpu
    }

    pub(crate) fn vulkan_handle(&self) -> Option<sys::PlVulkan> {
        match self.backend {
            Backend::Vulkan(vk) => Some(vk),
            #[cfg(target_os = "macos")]
            Backend::Metal(_) => None,
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn metal_handle(&self) -> Option<sys::PlMetal> {
        match self.backend {
            Backend::Metal(metal) => Some(metal),
            Backend::Vulkan(_) => None,
        }
    }

    pub fn find_rgba8(&self, caps: c_int) -> Result<sys::PlFmt, String> {
        self.find_format(sys::PL_FMT_UNORM, 4, 8, caps)
            .ok_or_else(|| "no RGBA8 texture format".to_string())
    }

    pub fn find_format(
        &self,
        format_type: c_int,
        components: c_int,
        depth: c_int,
        caps: c_int,
    ) -> Option<sys::PlFmt> {
        // SAFETY: the gpu is live.
        let fmt = unsafe { sys::pl_find_fmt(self.gpu, format_type, components, depth, 8, caps) };
        if fmt.is_null() {
            None
        } else {
            Some(fmt)
        }
    }

    pub fn create_renderer(&self) -> Result<crate::Renderer, String> {
        crate::Renderer::new(self.log, self.gpu)
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

    pub fn create_render_target(
        &self,
        width: c_int,
        height: c_int,
        fmt: sys::PlFmt,
    ) -> Result<sys::PlTex, String> {
        let params = sys::PlTexParams {
            w: width,
            h: height,
            d: 0,
            format: fmt,
            sampleable: false,
            renderable: true,
            storable: false,
            blit_src: false,
            blit_dst: false,
            host_writable: false,
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
            return Err("pl_tex_create render target failed".to_string());
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
        self.upload_strided(tex, width, height, 0, data)
    }

    pub fn upload_strided(
        &self,
        tex: sys::PlTex,
        width: c_int,
        height: c_int,
        row_pitch: usize,
        data: &[u8],
    ) -> Result<(), String> {
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
            row_pitch,
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

pub fn with_autorelease_pool<T>(body: impl FnOnce() -> T) -> T {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: push has no preconditions; the token is popped exactly once
        // below, on this thread, after every object autoreleased inside.
        let pool = unsafe { sys::objc_autoreleasePoolPush() };
        let result = body();
        // SAFETY: pool came from the matching push on this thread.
        unsafe { sys::objc_autoreleasePoolPop(pool) };
        result
    }
    #[cfg(not(target_os = "macos"))]
    body()
}

fn create_log() -> Result<(sys::PlLog, String), String> {
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
    Ok((log, version))
}

impl Drop for Gpu {
    fn drop(&mut self) {
        match &mut self.backend {
            // SAFETY: all textures must be destroyed before this runs.
            Backend::Vulkan(vk) => unsafe { sys::pl_vulkan_destroy(vk) },
            #[cfg(target_os = "macos")]
            // SAFETY: all textures, renderers, and swapchains on this device
            // are destroyed before this runs.
            Backend::Metal(metal) => unsafe { sys::pl_metal_destroy(metal) },
        }
        // SAFETY: the log was created with the context and is still live.
        unsafe { sys::pl_log_destroy(&mut (self.log as sys::PlLog)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys;
    use crate::OpenGpu;

    #[test]
    fn linked_placebo_matches_pin() {
        // SAFETY: pl_version returns a static string.
        let version = unsafe { CStr::from_ptr(sys::pl_version()) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(version, sys::PLACEBO_VERSION);
        assert_eq!(sys::PLACEBO_API_VERSION, 387);
    }

    fn init_and_tex_roundtrip(open: OpenGpu) {
        let vk = open().expect("gpu initializes headlessly");
        let fmt = vk
            .find_rgba8(sys::PL_FMT_CAP_HOST_READABLE)
            .expect("rgba8 format exists");
        let tex = vk.create_tex(4, 4, fmt).expect("texture creates");
        let pattern: Vec<u8> = (0..64).map(|i| (i * 7 + 3) as u8).collect();
        vk.upload(tex, 4, 4, &pattern).expect("upload works");
        let back = vk.download(tex, 4, 4).expect("download works");
        assert_eq!(back, pattern);
        vk.destroy_tex(tex);
    }

    gpu_tests!(init_and_tex_roundtrip);
}
