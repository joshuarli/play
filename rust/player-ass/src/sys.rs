use core::ffi::{c_char, c_double, c_int, c_uchar, c_uint, c_void};

pub const LIBASS_VERSION: c_int = 0x0170_5000;

pub const ASS_FONTPROVIDER_AUTODETECT: c_int = 1;

pub const ASS_SHAPING_COMPLEX: c_int = 1;

pub const ASS_FEATURE_WRAP_UNICODE: c_int = 2;

pub type AssLibraryRaw = *mut c_void;
pub type AssRendererRaw = *mut c_void;
pub type AssTrackRaw = *mut c_void;

#[repr(C)]
pub struct AssImage {
    pub w: c_int,
    pub h: c_int,
    pub stride: c_int,
    pub bitmap: *mut c_uchar,
    pub color: c_uint,
    pub dst_x: c_int,
    pub dst_y: c_int,
    pub next: *mut AssImage,
    pub image_type: c_int,
}

unsafe extern "C" {
    pub fn ass_library_version() -> c_int;
    pub fn ass_library_init() -> AssLibraryRaw;
    pub fn ass_library_done(priv_: AssLibraryRaw);
    pub fn ass_add_font(
        library: AssLibraryRaw,
        name: *const c_char,
        data: *const c_char,
        size: c_int,
    );
    pub fn ass_renderer_init(library: AssLibraryRaw) -> AssRendererRaw;
    pub fn ass_renderer_done(priv_: AssRendererRaw);
    pub fn ass_set_fonts(
        priv_: AssRendererRaw,
        default_font: *const c_char,
        default_family: *const c_char,
        dfp: c_int,
        config: *const c_char,
        update: c_int,
    );
    pub fn ass_set_shaper(priv_: AssRendererRaw, level: c_int);
    pub fn ass_set_frame_size(priv_: AssRendererRaw, w: c_int, h: c_int);
    pub fn ass_set_storage_size(priv_: AssRendererRaw, w: c_int, h: c_int);
    pub fn ass_set_pixel_aspect(priv_: AssRendererRaw, par: c_double);
    pub fn ass_new_track(library: AssLibraryRaw) -> AssTrackRaw;
    pub fn ass_free_track(track: AssTrackRaw);
    pub fn ass_track_set_feature(track: AssTrackRaw, feature: c_int, enable: c_int) -> c_int;
    pub fn ass_process_data(track: AssTrackRaw, data: *const c_char, size: c_int);
    pub fn ass_process_codec_private(track: AssTrackRaw, data: *const c_char, size: c_int);
    pub fn ass_process_chunk(
        track: AssTrackRaw,
        data: *const c_char,
        size: c_int,
        start: i64,
        duration: i64,
    );
    pub fn ass_flush_events(track: AssTrackRaw);
    pub fn ass_render_frame(
        priv_: AssRendererRaw,
        track: AssTrackRaw,
        now_ms: i64,
        detect_change: *mut c_int,
    ) -> *mut AssImage;
}

#[cfg(test)]
mod tests {
    use core::mem::{offset_of, size_of};

    use super::AssImage;

    #[test]
    fn image_layout_matches_libass() {
        assert_eq!(size_of::<AssImage>(), 56);
        assert_eq!(offset_of!(AssImage, w), 0);
        assert_eq!(offset_of!(AssImage, h), 4);
        assert_eq!(offset_of!(AssImage, stride), 8);
        assert_eq!(offset_of!(AssImage, bitmap), 16);
        assert_eq!(offset_of!(AssImage, color), 24);
        assert_eq!(offset_of!(AssImage, dst_x), 28);
        assert_eq!(offset_of!(AssImage, dst_y), 32);
        assert_eq!(offset_of!(AssImage, next), 40);
        assert_eq!(offset_of!(AssImage, image_type), 48);
    }
}
