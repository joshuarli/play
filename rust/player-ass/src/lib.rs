mod sys;

use std::ffi::{CString, c_char, c_int};
use std::sync::Arc;

pub struct RenderedImage {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub bitmap: Vec<u8>,
    pub color: u32,
    pub dst_x: i32,
    pub dst_y: i32,
}

struct Shared {
    raw: sys::AssLibraryRaw,
}

// SAFETY: a library handle is moved between threads but only used from one
// thread at a time.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

#[derive(Clone)]
pub struct Library {
    shared: Arc<Shared>,
}

impl Library {
    pub fn new() -> Result<Self, String> {
        // SAFETY: ass_library_init returns an owned library handle or NULL.
        let raw = unsafe { sys::ass_library_init() };
        if raw.is_null() {
            return Err("ass_library_init failed".to_owned());
        }
        // SAFETY: ass_library_version reads no state.
        let version = unsafe { sys::ass_library_version() };
        if version != sys::LIBASS_VERSION {
            // SAFETY: the handle came from ass_library_init and is not yet shared.
            unsafe { sys::ass_library_done(raw) };
            return Err(format!("libass version mismatch: got {version:#x}"));
        }
        Ok(Library {
            shared: Arc::new(Shared { raw }),
        })
    }

    pub fn add_font(&self, name: &str, data: &[u8]) -> Result<(), String> {
        if name.is_empty() {
            return Err("font name is empty".to_owned());
        }
        if data.is_empty() {
            return Err("embedded font is empty".to_owned());
        }
        let name = CString::new(name).map_err(|_| "font name contains NUL".to_owned())?;
        let size = i32::try_from(data.len())
            .map_err(|_| "embedded font is too large".to_owned())?;
        // SAFETY: the library is live, the name is a valid C string, and the
        // data slice is valid for the call; libass copies the font bytes.
        unsafe {
            sys::ass_add_font(
                self.shared.raw,
                name.as_ptr(),
                data.as_ptr() as *const c_char,
                size,
            )
        };
        Ok(())
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        // SAFETY: the handle came from ass_library_init; every renderer and
        // track was dropped before the last Arc released it.
        unsafe { sys::ass_library_done(self.raw) };
    }
}

pub struct Renderer {
    raw: sys::AssRendererRaw,
    _library: Arc<Shared>,
}

impl Renderer {
    pub fn new(library: &Library) -> Result<Self, String> {
        // SAFETY: library is live; the renderer holds a shared reference to it.
        let raw = unsafe { sys::ass_renderer_init(library.shared.raw) };
        if raw.is_null() {
            return Err("ass_renderer_init failed".to_owned());
        }
        let mut renderer = Renderer {
            raw,
            _library: Arc::clone(&library.shared),
        };
        renderer.configure()?;
        Ok(renderer)
    }

    fn configure(&mut self) -> Result<(), String> {
        let family =
            CString::new("sans-serif").map_err(|_| "font family contains NUL".to_owned())?;
        // SAFETY: the renderer is live; NULL pointers select libass defaults
        // and the family string is valid for the call.
        unsafe {
            sys::ass_set_fonts(
                self.raw,
                std::ptr::null(),
                family.as_ptr(),
                sys::ASS_FONTPROVIDER_AUTODETECT,
                std::ptr::null(),
                1,
            );
            sys::ass_set_shaper(self.raw, sys::ASS_SHAPING_COMPLEX);
        }
        Ok(())
    }

    pub fn clear_cache(&mut self) {
        // SAFETY: the renderer is live; a 1x1 frame forces libass to drop
        // glyphs cached for the previous PlayRes.
        unsafe { sys::ass_set_frame_size(self.raw, 1, 1) };
    }

    pub fn render(
        &mut self,
        track: &Track,
        width: i32,
        height: i32,
        storage: Option<(i32, i32)>,
        par: f64,
        now_ms: i64,
    ) -> Result<Vec<RenderedImage>, String> {
        // SAFETY: renderer and track are live; sizes are render parameters.
        unsafe {
            sys::ass_set_frame_size(self.raw, width, height);
            if let Some((w, h)) = storage {
                sys::ass_set_storage_size(self.raw, w, h);
            }
            sys::ass_set_pixel_aspect(self.raw, par);
        }
        let mut changed = 0;
        // SAFETY: renderer and track are live; libass owns the image list.
        let mut image = unsafe { sys::ass_render_frame(self.raw, track.raw, now_ms, &mut changed) };
        let mut images = Vec::new();
        while !image.is_null() {
            // SAFETY: libass returned a valid image; the list is only read.
            let (width, height, stride, color, dst_x, dst_y, bitmap, next) = unsafe {
                let image = &*image;
                let size = (image.stride as usize) * (image.h.max(0) as usize);
                let bitmap = std::slice::from_raw_parts(image.bitmap, size).to_vec();
                (
                    image.w,
                    image.h,
                    image.stride,
                    image.color,
                    image.dst_x,
                    image.dst_y,
                    bitmap,
                    image.next,
                )
            };
            if width > 0 && height > 0 {
                images.push(RenderedImage {
                    width: width as u32,
                    height: height as u32,
                    stride: stride as usize,
                    bitmap,
                    color,
                    dst_x,
                    dst_y,
                });
            }
            image = next;
        }
        Ok(images)
    }
}

// SAFETY: a renderer is moved between threads but only used from one thread
// at a time; it shares library ownership.
unsafe impl Send for Renderer {}

impl Drop for Renderer {
    fn drop(&mut self) {
        // SAFETY: the handle came from ass_renderer_init.
        unsafe { sys::ass_renderer_done(self.raw) };
    }
}

pub struct Track {
    raw: sys::AssTrackRaw,
    _library: Arc<Shared>,
}

impl Track {
    pub fn new(library: &Library) -> Result<Self, String> {
        // SAFETY: library is live; the track holds a shared reference to it.
        let raw = unsafe { sys::ass_new_track(library.shared.raw) };
        if raw.is_null() {
            return Err("ass_new_track failed".to_owned());
        }
        // SAFETY: the track is live and the feature enum comes from the
        // pinned libass headers; unavailable features return an error.
        unsafe {
            sys::ass_track_set_feature(raw, sys::ASS_FEATURE_WRAP_UNICODE, 1);
        }
        Ok(Track {
            raw,
            _library: Arc::clone(&library.shared),
        })
    }

    pub fn process_data(&mut self, data: &str) -> Result<(), String> {
        // SAFETY: the track is live, the &str is valid UTF-8, and the data
        // slice is valid for the call.
        let (data, len) = (data.as_ptr() as *const c_char, data.len() as c_int);
        unsafe { sys::ass_process_data(self.raw, data, len) };
        Ok(())
    }

    pub fn process_codec_private(&mut self, data: &[u8]) -> Result<(), String> {
        // SAFETY: the track is live and the data slice is valid for the call.
        unsafe {
            sys::ass_process_codec_private(
                self.raw,
                data.as_ptr() as *const c_char,
                data.len() as c_int,
            )
        };
        Ok(())
    }

    pub fn process_chunk(&mut self, data: &[u8], start_ms: i64, duration_ms: i64) {
        // SAFETY: the track is live and the data slice is valid for the call.
        unsafe {
            sys::ass_process_chunk(
                self.raw,
                data.as_ptr() as *const c_char,
                data.len() as c_int,
                start_ms,
                duration_ms,
            )
        };
    }

    pub fn flush_events(&mut self) {
        // SAFETY: the track is live.
        unsafe { sys::ass_flush_events(self.raw) };
    }
}

// SAFETY: a track is moved between threads but only used from one thread at
// a time; it shares library ownership.
unsafe impl Send for Track {}

impl Drop for Track {
    fn drop(&mut self) {
        // SAFETY: the handle came from ass_new_track.
        unsafe { sys::ass_free_track(self.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::{Library, Renderer, Track};

    const HEADER: &str = "[Script Info]\n\
        ScriptType: v4.00+\n\
        PlayResX: 1280\n\
        PlayResY: 720\n\
        ScaledBorderAndShadow: yes\n\
        \n\
        [V4+ Styles]\n\
        Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
        Style: Default,Arial,48,&H00FFFFFF,&H000019FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
        \n\
        [Events]\n\
        Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
        Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hello\n";

    fn inked_pixels(images: &[super::RenderedImage]) -> usize {
        images
            .iter()
            .map(|image| image.bitmap.iter().filter(|pixel| **pixel > 0).count())
            .sum()
    }

    #[test]
    fn dialogue_renders_only_while_visible() {
        let library = Library::new().expect("library starts");
        let mut renderer = Renderer::new(&library).expect("renderer starts");
        let mut track = Track::new(&library).expect("track starts");
        track.process_data(HEADER).expect("header parses");
        let during = renderer
            .render(&track, 1280, 720, None, 1.0, 1500)
            .expect("frame renders");
        assert!(!during.is_empty(), "no bitmaps during the event");
        assert!(inked_pixels(&during) > 100, "no ink during the event");
        for image in &during {
            assert!(image.dst_x >= 0 && image.dst_y >= 0);
            assert!(image.dst_x + image.width as i32 <= 1280);
            assert!(image.dst_y + image.height as i32 <= 720);
        }
        let before = renderer
            .render(&track, 1280, 720, None, 1.0, 500)
            .expect("frame renders");
        assert!(before.is_empty(), "bitmaps before the event");
        let after = renderer
            .render(&track, 1280, 720, None, 1.0, 2500)
            .expect("frame renders");
        assert!(after.is_empty(), "bitmaps after the event");
    }

    #[test]
    fn flushed_events_stop_rendering() {
        let library = Library::new().expect("library starts");
        let mut renderer = Renderer::new(&library).expect("renderer starts");
        let mut track = Track::new(&library).expect("track starts");
        track.process_data(HEADER).expect("header parses");
        track.flush_events();
        let during = renderer
            .render(&track, 1280, 720, None, 1.0, 1500)
            .expect("frame renders");
        assert!(during.is_empty(), "bitmaps survive a flush");
    }

    #[test]
    fn chunk_feed_renders_at_packet_time() {
        let library = Library::new().expect("library starts");
        let mut renderer = Renderer::new(&library).expect("renderer starts");
        let mut track = Track::new(&library).expect("track starts");
        let header = "[Script Info]\nPlayResX: 1280\nPlayResY: 720\n";
        track
            .process_codec_private(header.as_bytes())
            .expect("header parses");
        let event = "0,0,Default,,0,0,0,,Hello";
        track.process_chunk(event.as_bytes(), 1000, 1000);
        let during = renderer
            .render(&track, 1280, 720, None, 1.0, 1500)
            .expect("frame renders");
        assert!(!during.is_empty(), "no bitmaps for the chunk");
        assert!(inked_pixels(&during) > 100, "no ink for the chunk");
    }
}
