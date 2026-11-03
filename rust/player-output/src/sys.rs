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
pub const PL_FMT_CAP_RENDERABLE: c_int = 1 << 3;
pub const PL_FMT_CAP_HOST_READABLE: c_int = 1 << 9;

pub type PlLog = *const c_void;
pub type PlGpu = *mut c_void;
pub type PlTex = *mut c_void;
pub type PlFmt = *const c_void;
pub type PlRenderer = *mut c_void;
pub type PlTimer = *mut c_void;
pub type PlBuf = *mut c_void;

pub const PL_COLOR_SYSTEM_UNKNOWN: c_int = 0;
pub const PL_COLOR_SYSTEM_BT_601: c_int = 1;
pub const PL_COLOR_SYSTEM_BT_709: c_int = 2;
pub const PL_COLOR_SYSTEM_BT_2020_NC: c_int = 4;
pub const PL_COLOR_SYSTEM_RGB: c_int = 12;
pub const PL_COLOR_LEVELS_UNKNOWN: c_int = 0;
pub const PL_COLOR_LEVELS_LIMITED: c_int = 1;
pub const PL_COLOR_LEVELS_FULL: c_int = 2;
pub const PL_COLOR_PRIM_BT_709: c_int = 3;
pub const PL_COLOR_TRC_SRGB: c_int = 2;
pub const PL_ALPHA_INDEPENDENT: c_int = 1;

pub const PL_TEX_ADDRESS_CLAMP: c_int = 0;
pub const PL_FIELD_NONE: c_int = 0;
pub const PL_ROTATION_0: c_int = 0;

#[repr(C)]
pub struct PlPlane {
    pub texture: PlTex,
    pub address_mode: c_int,
    pub flipped: bool,
    pub components: c_int,
    pub component_mapping: [c_int; 4],
    pub shift_x: f32,
    pub shift_y: f32,
}

#[repr(C)]
pub struct PlBitEncoding {
    pub sample_depth: c_int,
    pub color_depth: c_int,
    pub bit_shift: c_int,
}

#[repr(C)]
pub struct PlColorRepr {
    pub sys: c_int,
    pub levels: c_int,
    pub alpha: c_int,
    pub bits: PlBitEncoding,
    pub dovi: *const c_void,
}

#[repr(C)]
pub struct PlHdrMetadata {
    pub opaque: [u8; 148],
}

#[repr(C)]
pub struct PlColorSpace {
    pub primaries: c_int,
    pub transfer: c_int,
    pub hdr: PlHdrMetadata,
}

#[repr(C)]
pub struct PlIccProfile {
    pub data: *const c_void,
    pub len: usize,
    pub signature: u64,
}

#[derive(Clone, Copy)]
#[repr(C, align(8))]
pub struct PlFilmGrain {
    pub opaque: [u8; 208],
}

#[repr(C)]
pub struct PlRect2df {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

#[repr(C)]
pub struct PlFrame {
    pub num_planes: c_int,
    pub planes: [PlPlane; 4],
    pub enhancement_layer: *const PlFrame,
    pub field: c_int,
    pub first_field: c_int,
    pub prev: *const PlFrame,
    pub next: *const PlFrame,
    pub acquire: Option<unsafe extern "C" fn(PlGpu, *mut PlFrame) -> bool>,
    pub release: Option<unsafe extern "C" fn(PlGpu, *mut PlFrame)>,
    pub repr: PlColorRepr,
    pub color: PlColorSpace,
    pub icc: *mut c_void,
    pub profile: PlIccProfile,
    pub lut: *const c_void,
    pub lut_type: c_int,
    pub crop: PlRect2df,
    pub rotation: c_int,
    pub pixel_aspect_ratio: f32,
    pub overlays: *const c_void,
    pub num_overlays: c_int,
    pub film_grain: PlFilmGrain,
    pub user_data: *mut c_void,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct PlRenderParams {
    pub opaque: [u8; 288],
}

#[repr(C)]
pub struct PlVulkanParams {
    pub instance: *mut c_void,
    pub get_proc_addr: *mut c_void,
    pub instance_params: *const c_void,
    pub surface: *mut c_void,
    pub device: *mut c_void,
    pub device_name: *const c_char,
    pub device_uuid: [u8; 16],
    pub allow_software: bool,
    pub async_transfer: bool,
    pub async_compute: bool,
    pub queue_count: c_int,
    pub no_compute: bool,
    pub extra_queues: u32,
    pub extensions: *const *const c_char,
    pub num_extensions: c_int,
    pub opt_extensions: *const *const c_char,
    pub num_opt_extensions: c_int,
    pub features: *const c_void,
    pub max_glsl_version: c_int,
    pub max_api_version: u32,
}

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

    pub fn pl_vulkan_create(log: PlLog, params: *const PlVulkanParams) -> PlVulkan;
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

    pub static pl_render_default_params: PlRenderParams;

    pub fn pl_frames_infer(rr: PlRenderer, image: *mut PlFrame, target: *mut PlFrame);
    pub fn pl_render_image(
        rr: PlRenderer,
        image: *const PlFrame,
        target: *const PlFrame,
        params: *const PlRenderParams,
    ) -> bool;
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

        assert_eq!(PL_COLOR_SYSTEM_RGB, 12);
        assert_eq!(PL_COLOR_LEVELS_FULL, 2);
        assert_eq!(PL_COLOR_SYSTEM_BT_601, 1);
        assert_eq!(PL_COLOR_SYSTEM_BT_709, 2);
        assert_eq!(PL_COLOR_SYSTEM_BT_2020_NC, 4);
        assert_eq!(PL_COLOR_LEVELS_LIMITED, 1);
        assert_eq!(PL_COLOR_PRIM_BT_709, 3);
        assert_eq!(PL_COLOR_TRC_SRGB, 2);

        assert_eq!(size_of::<PlPlane>(), 48);
        assert_eq!(offset_of!(PlPlane, texture), 0);
        assert_eq!(offset_of!(PlPlane, components), 16);
        assert_eq!(offset_of!(PlPlane, component_mapping), 20);
        assert_eq!(offset_of!(PlPlane, shift_x), 36);

        assert_eq!(size_of::<PlColorRepr>(), 32);
        assert_eq!(size_of::<PlColorSpace>(), 156);
        assert_eq!(offset_of!(PlColorSpace, primaries), 0);
        assert_eq!(offset_of!(PlColorSpace, hdr), 8);
        assert_eq!(size_of::<PlHdrMetadata>(), 148);
        assert_eq!(size_of::<PlIccProfile>(), 24);

        assert_eq!(size_of::<PlFrame>(), 744);
        assert_eq!(offset_of!(PlFrame, num_planes), 0);
        assert_eq!(offset_of!(PlFrame, planes), 8);
        assert_eq!(offset_of!(PlFrame, repr), 248);
        assert_eq!(offset_of!(PlFrame, color), 280);
        assert_eq!(offset_of!(PlFrame, icc), 440);
        assert_eq!(offset_of!(PlFrame, profile), 448);
        assert_eq!(offset_of!(PlFrame, lut), 472);
        assert_eq!(offset_of!(PlFrame, lut_type), 480);
        assert_eq!(offset_of!(PlFrame, crop), 484);
        assert_eq!(offset_of!(PlFrame, rotation), 500);
        assert_eq!(offset_of!(PlFrame, pixel_aspect_ratio), 504);
        assert_eq!(offset_of!(PlFrame, overlays), 512);
        assert_eq!(offset_of!(PlFrame, num_overlays), 520);
        assert_eq!(offset_of!(PlFrame, film_grain), 528);
        assert_eq!(offset_of!(PlFrame, user_data), 736);

        assert_eq!(size_of::<PlRenderParams>(), 288);

        assert_eq!(size_of::<PlVulkanParams>(), 128);
        assert_eq!(offset_of!(PlVulkanParams, instance), 0);
        assert_eq!(offset_of!(PlVulkanParams, device_uuid), 48);
        assert_eq!(offset_of!(PlVulkanParams, allow_software), 64);
        assert_eq!(offset_of!(PlVulkanParams, queue_count), 68);
        assert_eq!(offset_of!(PlVulkanParams, extra_queues), 76);
        assert_eq!(offset_of!(PlVulkanParams, features), 112);
        assert_eq!(offset_of!(PlVulkanParams, max_api_version), 124);
    }
}
