use core::ffi::{c_char, c_int, c_uint, c_void};

pub const PLACEBO_VERSION: &str = "v7.387.0";
pub const PLACEBO_API_VERSION: c_int = 387;

pub const PL_LOG_NONE: c_int = 0;
pub const PL_LOG_FATAL: c_int = 1;
pub const PL_LOG_ERR: c_int = 2;
pub const PL_LOG_WARN: c_int = 3;
pub const PL_LOG_INFO: c_int = 4;

pub const PL_FMT_UNKNOWN: c_int = 0;
pub const PL_FMT_UNORM: c_int = 1;
pub const PL_FMT_FLOAT: c_int = 5;

pub const PL_FMT_CAP_SAMPLEABLE: c_int = 1;
pub const PL_FMT_CAP_HOST_READABLE: c_int = 1 << 9;

pub type PlLog = *const c_void;
pub type PlGpu = *mut c_void;
pub type PlTex = *mut c_void;
pub type PlFmt = *const c_void;
pub type PlRenderer = *mut c_void;

#[repr(C)]
pub struct PlVulkanHead {
    pub gpu: PlGpu,
}

pub type PlVulkan = *mut PlVulkanHead;

#[repr(C)]
pub struct PlHandle {
    pub ptr: *mut c_void,
}

#[repr(C)]
pub struct PlSharedMem {
    pub handle: PlHandle,
    pub size: usize,
    pub offset: usize,
    pub drm_format_mod: u64,
    pub stride_w: usize,
    pub stride_h: usize,
    pub plane: c_uint,
}

#[repr(C)]
pub struct PlTexParams {
    pub w: c_int,
    pub h: c_int,
    pub d: c_int,
    pub format: PlFmt,
    pub sampleable: bool,
    pub renderable: bool,
    pub storable: bool,
    pub blit_src: bool,
    pub blit_dst: bool,
    pub host_writable: bool,
    pub host_readable: bool,
    pub export_handle: c_int,
    pub import_handle: c_int,
    pub shared_mem: PlSharedMem,
    pub initial_data: *const c_void,
    pub user_data: *mut c_void,
    pub debug_tag: *const c_char,
}

#[repr(C)]
pub struct PlRect3d {
    pub x0: c_int,
    pub y0: c_int,
    pub z0: c_int,
    pub x1: c_int,
    pub y1: c_int,
    pub z1: c_int,
}

#[repr(C)]
pub struct PlTexTransferParams {
    pub tex: PlTex,
    pub rc: PlRect3d,
    pub row_pitch: usize,
    pub depth_pitch: usize,
    pub timer: *mut c_void,
    pub callback: Option<unsafe extern "C" fn(*mut c_void)>,
    pub priv_data: *mut c_void,
    pub buf: *mut c_void,
    pub buf_offset: usize,
    pub ptr: *mut c_void,
    pub no_import: bool,
}

unsafe extern "C" {
    pub fn pl_version() -> *const c_char;

    #[link_name = "pl_log_create_387"]
    pub fn pl_log_create(api_ver: c_int, params: *const c_void) -> PlLog;
    pub fn pl_log_destroy(log: *mut PlLog);

    pub fn pl_vulkan_create(log: PlLog, params: *const c_void) -> PlVulkan;
    pub fn pl_vulkan_destroy(vk: *mut PlVulkan);

    pub fn pl_renderer_create(log: PlLog, gpu: PlGpu) -> PlRenderer;
    pub fn pl_renderer_destroy(rr: *mut PlRenderer);

    pub fn pl_find_fmt(
        gpu: PlGpu,
        typ: c_int,
        num_components: c_int,
        min_depth: c_int,
        host_bits: c_int,
        caps: c_int,
    ) -> PlFmt;

    pub fn pl_tex_create(gpu: PlGpu, params: *const PlTexParams) -> PlTex;
    pub fn pl_tex_destroy(gpu: PlGpu, tex: *mut PlTex);
    pub fn pl_tex_upload(gpu: PlGpu, params: *const PlTexTransferParams) -> bool;
    pub fn pl_tex_download(gpu: PlGpu, params: *const PlTexTransferParams) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    #[test]
    fn transcribed_layouts_match_pinned_headers() {
        assert_eq!(size_of::<PlTexParams>(), 120);
        assert_eq!(offset_of!(PlTexParams, w), 0);
        assert_eq!(offset_of!(PlTexParams, h), 4);
        assert_eq!(offset_of!(PlTexParams, d), 8);
        assert_eq!(offset_of!(PlTexParams, format), 16);
        assert_eq!(offset_of!(PlTexParams, sampleable), 24);
        assert_eq!(offset_of!(PlTexParams, host_readable), 30);
        assert_eq!(offset_of!(PlTexParams, export_handle), 32);
        assert_eq!(offset_of!(PlTexParams, import_handle), 36);
        assert_eq!(offset_of!(PlTexParams, shared_mem), 40);
        assert_eq!(offset_of!(PlTexParams, initial_data), 96);
        assert_eq!(offset_of!(PlTexParams, user_data), 104);
        assert_eq!(offset_of!(PlTexParams, debug_tag), 112);

        assert_eq!(size_of::<PlSharedMem>(), 56);
        assert_eq!(offset_of!(PlSharedMem, size), 8);
        assert_eq!(offset_of!(PlSharedMem, drm_format_mod), 24);
        assert_eq!(offset_of!(PlSharedMem, plane), 48);

        assert_eq!(size_of::<PlTexTransferParams>(), 104);
        assert_eq!(offset_of!(PlTexTransferParams, tex), 0);
        assert_eq!(offset_of!(PlTexTransferParams, rc), 8);
        assert_eq!(offset_of!(PlTexTransferParams, ptr), 88);

        assert_eq!(size_of::<PlRect3d>(), 24);
        assert_eq!(align_of::<PlVulkanHead>(), align_of::<*mut c_void>());
        assert_eq!(offset_of!(PlVulkanHead, gpu), 0);
    }
}
