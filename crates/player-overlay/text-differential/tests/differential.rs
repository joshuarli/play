#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_int, c_long, c_uint, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::OnceLock;

use harfrust::{Buffer, Direction, Script, ShapeOptions, ShaperFont, Tag};
use player_overlay::{Align, Color, TextBlock, TextEngine, TextStyle, Weight};
use skrifa::font::glyph::outline::PathElement;
use skrifa::{DrawMemory, Font, GlyphId, ScalerFont};
use unicode_bidi::BidiInfo;
use zeno::{Command, Mask, Point};

const SIZE: f64 = 32.0;
const HB_SCALE: f64 = 64.0;
const HEAVY: f32 = 700.0;

#[repr(C)]
struct BitmapSize {
    height: i16,
    width: i16,
    size: c_long,
    x_ppem: c_long,
    y_ppem: c_long,
}

#[repr(C)]
struct FaceRec {
    num_faces: c_long,
    face_index: c_long,
    face_flags: c_long,
    style_flags: c_long,
    num_glyphs: c_long,
    family_name: *const c_char,
    style_name: *const c_char,
    num_fixed_sizes: c_int,
    available_sizes: *const BitmapSize,
    num_charmaps: c_int,
    charmaps: *mut c_void,
    generic: [*mut c_void; 2],
    bbox: [c_long; 4],
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    height: i16,
    max_advance_width: i16,
    max_advance_height: i16,
    underline_position: i16,
    underline_thickness: i16,
    glyph: *mut GlyphSlotRec,
}

#[repr(C)]
struct FtBitmap {
    rows: c_uint,
    width: c_uint,
    pitch: c_int,
    buffer: *const u8,
    num_grays: u16,
    pixel_mode: u8,
    palette_mode: u8,
    palette: *mut c_void,
}

#[repr(C)]
struct GlyphSlotRec {
    library: *mut c_void,
    face: *mut FaceRec,
    next: *mut GlyphSlotRec,
    glyph_index: c_uint,
    generic: [*mut c_void; 2],
    metrics: [c_long; 8],
    linear_hori_advance: c_long,
    linear_vert_advance: c_long,
    advance: [c_long; 2],
    format: c_int,
    bitmap: FtBitmap,
    bitmap_left: c_int,
    bitmap_top: c_int,
}

#[repr(C)]
struct FtVector {
    x: c_long,
    y: c_long,
}

const FT_FACE_FLAG_SCALABLE: c_long = 1;
const FT_LOAD_NO_HINTING: c_int = 1 << 1;
const FT_LOAD_COLOR: c_int = 1 << 20;
const FT_RENDER_MODE_NORMAL: c_int = 0;
const FT_GLYPH_FORMAT_BITMAP: c_int = 0x6269_7473;
const FT_PIXEL_MODE_GRAY: u8 = 2;
const FT_PIXEL_MODE_BGRA: u8 = 7;

unsafe extern "C" {
    fn FT_Init_FreeType(library: *mut *mut c_void) -> c_int;
    fn FT_New_Face(library: *mut c_void, path: *const c_char, index: c_long, face: *mut *mut FaceRec) -> c_int;
    fn FT_Done_Face(face: *mut FaceRec) -> c_int;
    fn FT_Set_Char_Size(face: *mut FaceRec, width: c_long, height: c_long, h_res: c_uint, v_res: c_uint) -> c_int;
    fn FT_Select_Size(face: *mut FaceRec, strike: c_int) -> c_int;
    fn FT_Set_Transform(face: *mut FaceRec, matrix: *mut c_void, delta: *mut FtVector);
    fn FT_Load_Glyph(face: *mut FaceRec, glyph: c_uint, flags: c_int) -> c_int;
    fn FT_Render_Glyph(slot: *mut GlyphSlotRec, mode: c_int) -> c_int;
    fn FT_Get_Postscript_Name(face: *mut FaceRec) -> *const c_char;
    fn FT_Set_Var_Design_Coordinates(face: *mut FaceRec, count: c_uint, coords: *mut c_long) -> c_int;
}

#[repr(C)]
struct HbGlyphInfo {
    codepoint: u32,
    mask: u32,
    cluster: u32,
    var1: u32,
    var2: u32,
}

#[repr(C)]
struct HbGlyphPosition {
    x_advance: i32,
    y_advance: i32,
    x_offset: i32,
    y_offset: i32,
    var: u32,
}

#[repr(C)]
struct HbVariation {
    tag: u32,
    value: f32,
}

const HB_DIRECTION_LTR: c_uint = 4;
const HB_DIRECTION_RTL: c_uint = 5;
const SCRIPT_COMMON: u32 = u32::from_be_bytes(*b"Zyyy");
const SCRIPT_INHERITED: u32 = u32::from_be_bytes(*b"Zinh");
const SCRIPT_UNKNOWN: u32 = u32::from_be_bytes(*b"Zzzz");
const WGHT: u32 = u32::from_be_bytes(*b"wght");

unsafe extern "C" {
    fn hb_version_string() -> *const c_char;
    fn hb_blob_create_from_file(path: *const c_char) -> *mut c_void;
    fn hb_blob_destroy(blob: *mut c_void);
    fn hb_face_create(blob: *mut c_void, index: c_uint) -> *mut c_void;
    fn hb_face_destroy(face: *mut c_void);
    fn hb_face_get_glyph_count(face: *mut c_void) -> c_uint;
    fn hb_font_create(face: *mut c_void) -> *mut c_void;
    fn hb_font_destroy(font: *mut c_void);
    fn hb_font_set_scale(font: *mut c_void, x: c_int, y: c_int);
    fn hb_font_set_variations(font: *mut c_void, variations: *const HbVariation, count: c_uint);
    fn hb_buffer_create() -> *mut c_void;
    fn hb_buffer_destroy(buffer: *mut c_void);
    fn hb_buffer_add_utf8(buffer: *mut c_void, text: *const c_char, length: c_int, offset: c_uint, count: c_int);
    fn hb_buffer_set_direction(buffer: *mut c_void, direction: c_uint);
    fn hb_buffer_set_script(buffer: *mut c_void, script: u32);
    fn hb_buffer_guess_segment_properties(buffer: *mut c_void);
    fn hb_buffer_get_length(buffer: *mut c_void) -> c_uint;
    fn hb_buffer_get_glyph_infos(buffer: *mut c_void, length: *mut c_uint) -> *mut HbGlyphInfo;
    fn hb_buffer_get_glyph_positions(buffer: *mut c_void, length: *mut c_uint) -> *mut HbGlyphPosition;
    fn hb_shape(font: *mut c_void, buffer: *mut c_void, features: *const c_void, count: c_uint);
    fn hb_unicode_funcs_get_default() -> *mut c_void;
    fn hb_unicode_script(funcs: *mut c_void, codepoint: u32) -> u32;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shaped {
    gid: u32,
    cluster: u32,
    x_advance: i32,
    y_advance: i32,
    x_offset: i32,
    y_offset: i32,
}

#[derive(Clone)]
struct FaceRef {
    path: PathBuf,
    index: u32,
}

struct Instance {
    label: &'static str,
    weight: Option<f32>,
}

struct Entry {
    id: &'static str,
    text: &'static str,
    variable: bool,
}

const CORPUS: &[Entry] = &[
    Entry { id: "latin-combining-marks", text: "Ame\u{301}lie na\u{308}ve zo\u{302}\u{323}n\u{303} Ha\u{30A}kon fi ffl", variable: false },
    Entry { id: "arabic", text: "مرحبا بالعالم ببب لا", variable: false },
    Entry { id: "hebrew-bidi", text: "שלום world עולם 123 !", variable: false },
    Entry { id: "devanagari", text: "नमस्ते दुनिया क्षत्रिय हिन्दी", variable: false },
    Entry { id: "thai", text: "สวัสดีชาวโลก ภาษาไทย", variable: false },
    Entry { id: "cjk", text: "你好，世界 こんにちは世界 안녕하세요", variable: false },
    Entry {
        id: "zwj-emoji",
        text: "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466} \u{1F3F3}\u{FE0F}\u{200D}\u{1F308} \u{1F44D}\u{1F3FD} 1\u{FE0F}\u{20E3} \u{1F1EF}\u{1F1F5}",
        variable: false,
    },
    Entry { id: "variable-weight", text: "Hamburgefonstiv 0123 fi ffl", variable: true },
];

#[derive(Default)]
struct Stats {
    counts: BTreeMap<String, u64>,
    maxima: BTreeMap<String, i64>,
}

impl Stats {
    fn add(&mut self, key: &str, by: u64) {
        *self.counts.entry(key.to_owned()).or_default() += by;
    }

    fn inc(&mut self, key: &str) {
        self.add(key, 1);
    }

    fn max(&mut self, key: &str, value: i64) {
        let slot = self.maxima.entry(key.to_owned()).or_insert(0);
        *slot = (*slot).max(value);
    }
}

fn ft_library() -> *mut c_void {
    static LIBRARY: OnceLock<usize> = OnceLock::new();
    *LIBRARY.get_or_init(|| {
        let mut library = ptr::null_mut();
        // SAFETY: `library` is a valid out-pointer; the library lives for the test process.
        assert_eq!(unsafe { FT_Init_FreeType(&mut library) }, 0, "FreeType starts");
        library as usize
    }) as *mut c_void
}

fn neutral(script: u32) -> bool {
    matches!(script, SCRIPT_COMMON | SCRIPT_INHERITED | SCRIPT_UNKNOWN)
}

fn resolve_scripts(text: &str) -> Vec<u32> {
    // SAFETY: returns HarfBuzz's static default Unicode functions.
    let funcs = unsafe { hb_unicode_funcs_get_default() };
    // SAFETY: `funcs` is the live default table; any code point is accepted.
    let mut scripts: Vec<u32> = text.chars().map(|c| unsafe { hb_unicode_script(funcs, c as u32) }).collect();
    let mut previous = None;
    for script in &mut scripts {
        if neutral(*script) {
            if let Some(known) = previous {
                *script = known;
            }
        } else {
            previous = Some(*script);
        }
    }
    let mut next = None;
    for script in scripts.iter_mut().rev() {
        if neutral(*script) {
            if let Some(known) = next {
                *script = known;
            }
        } else {
            next = Some(*script);
        }
    }
    scripts
}

struct Run {
    text: String,
    rtl: bool,
    script: u32,
}

fn segment(text: &str) -> Vec<Run> {
    let info = BidiInfo::new(text, None);
    let scripts = resolve_scripts(text);
    let mut runs: Vec<Run> = Vec::new();
    for (at, (offset, c)) in text.char_indices().enumerate() {
        let rtl = info.levels[offset].number() % 2 == 1;
        let script = scripts[at];
        match runs.last_mut() {
            Some(run) if run.rtl == rtl && (run.script == script || (neutral(run.script) && neutral(script))) => run.text.push(c),
            _ => runs.push(Run { text: c.to_string(), rtl, script }),
        }
    }
    runs.retain_mut(|run| {
        run.text.truncate(run.text.trim_end().len());
        !run.text.is_empty()
    });
    runs
}

fn tag_text(tag: u32) -> String {
    String::from_utf8_lossy(&tag.to_be_bytes()).into_owned()
}

struct HbShaper {
    blob: *mut c_void,
    face: *mut c_void,
    font: *mut c_void,
    buffer: *mut c_void,
}

impl HbShaper {
    fn open(path: &CStr, index: u32, weight: Option<f32>) -> Result<Self, String> {
        // SAFETY: `path` is NUL-terminated; HarfBuzz returns empty objects, not null, on failure,
        // which the glyph count check below detects. Each handle is released in `Drop`.
        unsafe {
            let blob = hb_blob_create_from_file(path.as_ptr());
            let face = hb_face_create(blob, index & 0xFFFF);
            let font = hb_font_create(face);
            let scale = (SIZE * HB_SCALE).round() as c_int;
            hb_font_set_scale(font, scale, scale);
            if let Some(weight) = weight {
                let variation = HbVariation { tag: WGHT, value: weight };
                hb_font_set_variations(font, &variation, 1);
            }
            let shaper = HbShaper { blob, face, font, buffer: hb_buffer_create() };
            if hb_face_get_glyph_count(face) == 0 {
                return Err(format!("HarfBuzz could not load {}", path.to_string_lossy()));
            }
            Ok(shaper)
        }
    }

    fn shape(&self, text: &str, rtl: bool, script: u32) -> Vec<Shaped> {
        // SAFETY: the buffer is re-created per call so no earlier contents remain; `text` is valid
        // UTF-8 and the whole range is added; the info and position arrays hold `length` entries
        // and are copied before the buffer is destroyed.
        unsafe {
            let buffer = hb_buffer_create();
            hb_buffer_add_utf8(buffer, text.as_ptr().cast(), text.len() as c_int, 0, text.len() as c_int);
            hb_buffer_set_direction(buffer, if rtl { HB_DIRECTION_RTL } else { HB_DIRECTION_LTR });
            if !neutral(script) {
                hb_buffer_set_script(buffer, script);
            }
            hb_buffer_guess_segment_properties(buffer);
            hb_shape(self.font, buffer, ptr::null(), 0);
            let mut length = hb_buffer_get_length(buffer);
            let infos = hb_buffer_get_glyph_infos(buffer, &mut length);
            let positions = hb_buffer_get_glyph_positions(buffer, &mut length);
            let shaped = if infos.is_null() || positions.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(infos, length as usize)
                    .iter()
                    .zip(std::slice::from_raw_parts(positions, length as usize))
                    .map(|(info, position)| Shaped {
                        gid: info.codepoint,
                        cluster: info.cluster,
                        x_advance: position.x_advance,
                        y_advance: position.y_advance,
                        x_offset: position.x_offset,
                        y_offset: position.y_offset,
                    })
                    .collect()
            };
            hb_buffer_destroy(buffer);
            shaped
        }
    }
}

impl Drop for HbShaper {
    fn drop(&mut self) {
        // SAFETY: each handle was created in `open`, is owned solely by this value, and is
        // released once in dependency order.
        unsafe {
            hb_buffer_destroy(self.buffer);
            hb_font_destroy(self.font);
            hb_face_destroy(self.face);
            hb_blob_destroy(self.blob);
        }
    }
}

fn harfrust_shape(font: &Font, text: &str, rtl: bool, script: u32) -> Vec<Shaped> {
    let scale = (SIZE * HB_SCALE).round() as i32;
    let shaper = ShaperFont::new(font).with_scale(scale);
    let mut buffer = Buffer::new();
    buffer.push_str(text);
    buffer.set_direction(if rtl { Direction::RightToLeft } else { Direction::LeftToRight });
    if !neutral(script) {
        buffer.set_script(Script::from_iso15924_tag(Tag::from_be_bytes(script.to_be_bytes())));
    }
    buffer.guess_segment_properties();
    harfrust::shape(&shaper, &mut buffer, ShapeOptions::new()).expect("harfrust shapes");
    buffer
        .glyph_infos()
        .iter()
        .zip(buffer.glyph_positions())
        .map(|(info, position)| Shaped {
            gid: info.glyph_id,
            cluster: info.cluster,
            x_advance: position.x_advance,
            y_advance: position.y_advance,
            x_offset: position.x_offset,
            y_offset: position.y_offset,
        })
        .collect()
}

struct FtFont {
    face: *mut FaceRec,
}

impl FtFont {
    fn open(face: &FaceRef, design: Option<&[c_long]>) -> Result<Self, String> {
        let path = CString::new(face.path.to_string_lossy().as_bytes()).map_err(|_| "path holds NUL".to_owned())?;
        let mut handle = ptr::null_mut();
        // SAFETY: `path` is NUL-terminated, the library is live, and `handle` is a valid out-pointer.
        if unsafe { FT_New_Face(ft_library(), path.as_ptr(), face.index as c_long, &mut handle) } != 0 {
            return Err(format!("FreeType could not open {}", face.path.display()));
        }
        // SAFETY: `handle` is a live face; `available_sizes` holds `num_fixed_sizes` entries when
        // non-null; the design array has one entry per axis.
        unsafe {
            if let Some(design) = design {
                let mut coords = design.to_vec();
                FT_Set_Var_Design_Coordinates(handle, coords.len() as c_uint, coords.as_mut_ptr());
            }
            let record = &*handle;
            let fixed_only = record.face_flags & FT_FACE_FLAG_SCALABLE == 0;
            if fixed_only && record.num_fixed_sizes > 0 {
                let sizes = std::slice::from_raw_parts(record.available_sizes, record.num_fixed_sizes as usize);
                let best = sizes
                    .iter()
                    .enumerate()
                    .min_by(|a, b| {
                        let distance = |size: &BitmapSize| (size.y_ppem as f64 / 64.0 - SIZE).abs();
                        distance(a.1).total_cmp(&distance(b.1))
                    })
                    .map(|(at, _)| at)
                    .expect("at least one strike");
                FT_Select_Size(handle, best as c_int);
                Ok(FtFont { face: handle })
            } else {
                FT_Set_Char_Size(handle, 0, (SIZE * 64.0).round() as c_long, 72, 72);
                Ok(FtFont { face: handle })
            }
        }
    }

    fn postscript_name(&self) -> String {
        // SAFETY: the face is live; the returned strings are NUL-terminated and owned by the face.
        unsafe {
            let mut name = FT_Get_Postscript_Name(self.face);
            if name.is_null() {
                name = (*self.face).family_name;
            }
            if name.is_null() { String::new() } else { CStr::from_ptr(name).to_string_lossy().into_owned() }
        }
    }

    fn render(&self, gid: u32) -> FtGlyph {
        let mut delta = FtVector { x: 0, y: 0 };
        // SAFETY: the face is live; after a successful load the slot and its bitmap stay valid
        // until the next call on this face, and rows are read within `rows`, `width`, and `pitch`.
        unsafe {
            FT_Set_Transform(self.face, ptr::null_mut(), &mut delta);
            if FT_Load_Glyph(self.face, gid, FT_LOAD_NO_HINTING | FT_LOAD_COLOR) != 0 {
                return FtGlyph::Failed;
            }
            let slot = (*self.face).glyph;
            if (*slot).format != FT_GLYPH_FORMAT_BITMAP && FT_Render_Glyph(slot, FT_RENDER_MODE_NORMAL) != 0 {
                return FtGlyph::Failed;
            }
            let bitmap = &(*slot).bitmap;
            if bitmap.rows == 0 || bitmap.width == 0 || bitmap.buffer.is_null() {
                return FtGlyph::Mask(Mask8::empty());
            }
            if bitmap.pixel_mode == FT_PIXEL_MODE_BGRA {
                return FtGlyph::Color;
            }
            if bitmap.pixel_mode != FT_PIXEL_MODE_GRAY || bitmap.pitch < 0 {
                return FtGlyph::Unsupported;
            }
            let (rows, width, pitch) = (bitmap.rows as usize, bitmap.width as usize, bitmap.pitch as usize);
            let mut pixels = Vec::with_capacity(rows * width);
            for row in 0..rows {
                pixels.extend_from_slice(std::slice::from_raw_parts(bitmap.buffer.add(row * pitch), width));
            }
            FtGlyph::Mask(Mask8 { left: (*slot).bitmap_left, top: (*slot).bitmap_top, width: width as u32, height: rows as u32, pixels })
        }
    }
}

impl Drop for FtFont {
    fn drop(&mut self) {
        // SAFETY: the face was created in `open` and is released once.
        unsafe {
            FT_Done_Face(self.face);
        }
    }
}

struct Mask8 {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Mask8 {
    fn empty() -> Self {
        Mask8 { left: 0, top: 0, width: 0, height: 0, pixels: Vec::new() }
    }

    fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.pixels.iter().all(|value| *value == 0)
    }

    fn at(&self, x: i32, y_up: i32) -> u8 {
        let column = x - self.left;
        let row = self.top - y_up;
        if column < 0 || row < 0 || column >= self.width as i32 || row >= self.height as i32 {
            return 0;
        }
        self.pixels[row as usize * self.width as usize + column as usize]
    }

    fn edges(&self) -> [i32; 4] {
        [self.left, self.top - self.height as i32, self.left + self.width as i32, self.top]
    }
}

enum FtGlyph {
    Failed,
    Unsupported,
    Color,
    Mask(Mask8),
}

fn skrifa_mask(scaler: &ScalerFont, gid: u32) -> Option<Mask8> {
    let outline = scaler.outline(GlyphId::new(gid))?;
    let mut path: Vec<PathElement> = Vec::new();
    outline.draw(DrawMemory::Scratch, &mut path).ok()?;
    let point = |x: f32, y: f32| Point::new(x, -y);
    let commands: Vec<Command> = path
        .iter()
        .map(|element| match *element {
            PathElement::MoveTo { x, y } => Command::MoveTo(point(x, y)),
            PathElement::LineTo { x, y } => Command::LineTo(point(x, y)),
            PathElement::QuadTo { cx0, cy0, x, y } => Command::QuadTo(point(cx0, cy0), point(x, y)),
            PathElement::CurveTo { cx0, cy0, cx1, cy1, x, y } => {
                Command::CurveTo(point(cx0, cy0), point(cx1, cy1), point(x, y))
            }
            PathElement::Close => Command::Close,
        })
        .collect();
    if commands.is_empty() {
        return Some(Mask8::empty());
    }
    let (pixels, placement) = Mask::new(&commands).render();
    Some(Mask8 { left: placement.left, top: -placement.top, width: placement.width, height: placement.height, pixels })
}

fn compare_shaping(stats: &mut Stats, prefix: &str, hb: &[Shaped], hr: &[Shaped]) -> Option<String> {
    stats.inc(&format!("{prefix}.shape.runs"));
    if hb == hr {
        stats.inc(&format!("{prefix}.shape.runs_identical"));
        stats.add(&format!("{prefix}.shape.glyphs"), hb.len() as u64);
        return None;
    }
    if hb.len() != hr.len() {
        stats.inc(&format!("{prefix}.shape.glyph_count_differs"));
        return Some(format!("glyph count hb={} harfrust={}", hb.len(), hr.len()));
    }
    stats.add(&format!("{prefix}.shape.glyphs"), hb.len() as u64);
    let mut first = None;
    for (at, (a, b)) in hb.iter().zip(hr).enumerate() {
        let mut differs = false;
        let mut check = |key: &str, equal: bool, delta: i64| {
            if !equal {
                differs = true;
                stats.inc(&format!("{prefix}.shape.{key}_differs"));
                stats.max(&format!("{prefix}.shape.{key}_max_abs_delta"), delta.abs());
            }
        };
        check("glyph_id", a.gid == b.gid, a.gid as i64 - b.gid as i64);
        check("cluster", a.cluster == b.cluster, a.cluster as i64 - b.cluster as i64);
        check("x_advance", a.x_advance == b.x_advance, (a.x_advance - b.x_advance) as i64);
        check("y_advance", a.y_advance == b.y_advance, (a.y_advance - b.y_advance) as i64);
        check("x_offset", a.x_offset == b.x_offset, (a.x_offset - b.x_offset) as i64);
        check("y_offset", a.y_offset == b.y_offset, (a.y_offset - b.y_offset) as i64);
        if differs {
            stats.inc(&format!("{prefix}.shape.glyphs_with_any_difference"));
            first.get_or_insert_with(|| format!("glyph {at}: hb={a:?} harfrust={b:?}"));
        }
    }
    stats.inc(&format!("{prefix}.shape.runs_differing"));
    first
}

fn compare_raster(stats: &mut Stats, ft: &FtFont, scaler: &ScalerFont, font: &Font, gid: u32) {
    stats.inc("raster.glyphs");
    let ft_mask = match ft.render(gid) {
        FtGlyph::Failed => {
            stats.inc("raster.freetype_load_failed");
            return;
        }
        FtGlyph::Unsupported => {
            stats.inc("raster.freetype_unsupported_bitmap_mode");
            return;
        }
        FtGlyph::Color => {
            stats.inc("raster.freetype_color_bitmap_not_comparable");
            let glyph = GlyphId::new(gid);
            if font.color_glyphs().get(glyph).is_some() {
                stats.inc("raster.freetype_color_bitmap.skrifa_has_color_paint");
            } else if scaler.outline(glyph).is_some() {
                stats.inc("raster.freetype_color_bitmap.skrifa_has_monochrome_outline_only");
            } else {
                stats.inc("raster.freetype_color_bitmap.skrifa_has_no_outline");
            }
            return;
        }
        FtGlyph::Mask(mask) => mask,
    };
    let Some(sk_mask) = skrifa_mask(scaler, gid) else {
        stats.inc(if ft_mask.is_empty() { "raster.skrifa_no_outline.freetype_empty" } else { "raster.skrifa_no_outline.freetype_has_ink" });
        return;
    };
    match (ft_mask.is_empty(), sk_mask.is_empty()) {
        (true, true) => {
            stats.inc("raster.both_empty");
            return;
        }
        (true, false) | (false, true) => {
            stats.inc("raster.one_side_empty");
            return;
        }
        (false, false) => {}
    }
    stats.inc("raster.compared");
    let (a, b) = (ft_mask.edges(), sk_mask.edges());
    let widest = a.iter().zip(b).map(|(x, y)| (x - y).abs()).max().unwrap_or(0);
    stats.inc(match widest {
        0 => "raster.bbox_identical",
        1 => "raster.bbox_edges_within_1px",
        _ => "raster.bbox_edges_differ_more_than_1px",
    });
    stats.max("raster.bbox_max_edge_delta_px", widest as i64);
    let left = a[0].min(b[0]);
    let bottom = a[1].min(b[1]);
    let right = a[2].max(b[2]);
    let top = a[3].max(b[3]);
    let (mut max_diff, mut sum_diff, mut ink, mut ft_sum, mut sk_sum) = (0i64, 0u64, 0u64, 0u64, 0u64);
    for y in bottom..top {
        for x in left..right {
            let (p, q) = (ft_mask.at(x, y), sk_mask.at(x, y));
            ft_sum += p as u64;
            sk_sum += q as u64;
            if p != 0 || q != 0 {
                ink += 1;
                let diff = (p as i64 - q as i64).abs();
                max_diff = max_diff.max(diff);
                sum_diff += diff as u64;
            }
        }
    }
    stats.inc(match max_diff {
        0 => "raster.coverage_identical",
        1..=8 => "raster.coverage_max_diff_1_to_8_of_255",
        9..=32 => "raster.coverage_max_diff_9_to_32_of_255",
        33..=96 => "raster.coverage_max_diff_33_to_96_of_255",
        _ => "raster.coverage_max_diff_over_96_of_255",
    });
    stats.max("raster.coverage_max_diff_of_255", max_diff);
    stats.add("raster.union_ink_pixels", ink);
    stats.add("raster.sum_abs_coverage_diff", sum_diff);
    stats.add("raster.freetype_coverage_sum", ft_sum);
    stats.add("raster.skrifa_zeno_coverage_sum", sk_sum);
}

fn fontconfig_dirs() -> Vec<PathBuf> {
    let file = std::env::var_os("FONTCONFIG_FILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/etc/fonts/fonts.conf"));
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut dirs = Vec::new();
    for part in text.split("<dir").skip(1) {
        let Some((head, rest)) = part.split_once('>') else { continue };
        if head.contains("prefix=") {
            continue;
        }
        if let Some((path, _)) = rest.split_once("</dir>") {
            let path = path.trim();
            if path.starts_with('/') {
                dirs.push(PathBuf::from(path));
            }
        }
    }
    for extra in std::env::var("PLAY_TEXT_DIFF_FONTS").unwrap_or_default().split(':').filter(|path| !path.is_empty()) {
        dirs.push(PathBuf::from(extra));
    }
    dirs
}

fn font_files(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            font_files(&path, into);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc"))
        {
            into.push(path);
        }
    }
}

fn index_fonts() -> (Vec<(String, FaceRef)>, usize) {
    let mut files = Vec::new();
    for dir in fontconfig_dirs() {
        font_files(&dir, &mut files);
    }
    let mut faces = Vec::new();
    for path in files {
        let mut index = 0u32;
        loop {
            let face = FaceRef { path: path.clone(), index };
            let Ok(font) = FtFont::open(&face, None) else { break };
            // SAFETY: the face is live.
            let count = unsafe { (*font.face).num_faces };
            faces.push((font.postscript_name(), face));
            index += 1;
            if index as c_long >= count {
                break;
            }
        }
    }
    let total = faces.len();
    (faces, total)
}

fn covers(font: &Font, text: &str) -> bool {
    let map = font.charmap();
    text.chars().filter(|c| !c.is_control() && !matches!(*c, '\u{200D}' | '\u{FE0F}' | '\u{200B}')).all(|c| map.map_unicode(c).is_some())
}

fn variable_face(faces: &[(String, FaceRef)], text: &str) -> Option<(FaceRef, Vec<(Tag, f32, f32, f32)>)> {
    let wght = Tag::new(b"wght");
    for (_, face) in faces {
        let Ok(bytes) = std::fs::read(&face.path) else { continue };
        let Some(font) = Font::new(bytes, face.index) else { continue };
        let axes: Vec<_> = font.axes().iter().map(|axis| (axis.tag(), axis.min_value(), axis.default_value(), axis.max_value())).collect();
        if axes.iter().any(|axis| axis.0 == wght) && covers(&font, text) {
            return Some((face.clone(), axes));
        }
    }
    None
}

fn design_coordinates(axes: &[(Tag, f32, f32, f32)], weight: f32) -> Vec<c_long> {
    let wght = Tag::new(b"wght");
    axes.iter()
        .map(|(tag, min, default, max)| {
            let value = if *tag == wght { weight.clamp(*min, *max) } else { *default };
            (value as f64 * 65536.0).round() as c_long
        })
        .collect()
}

struct PlayRun {
    font: String,
    ids: Vec<u32>,
    single: bool,
    rtl: bool,
    missing: usize,
    glyph_runs: usize,
}

const SHAPE_KEYS: [&str; 14] = [
    "runs",
    "runs_identical",
    "runs_differing",
    "glyph_count_differs",
    "glyphs",
    "glyphs_with_any_difference",
    "glyph_id_differs",
    "cluster_differs",
    "x_advance_differs",
    "y_advance_differs",
    "x_offset_differs",
    "y_offset_differs",
    "x_advance_max_abs_delta",
    "x_offset_max_abs_delta",
];

const RASTER_KEYS: [&str; 17] = [
    "glyphs",
    "compared",
    "freetype_load_failed",
    "freetype_unsupported_bitmap_mode",
    "freetype_color_bitmap_not_comparable",
    "skrifa_no_outline.freetype_empty",
    "skrifa_no_outline.freetype_has_ink",
    "both_empty",
    "one_side_empty",
    "bbox_identical",
    "bbox_edges_within_1px",
    "bbox_edges_differ_more_than_1px",
    "coverage_identical",
    "coverage_max_diff_1_to_8_of_255",
    "coverage_max_diff_9_to_32_of_255",
    "coverage_max_diff_33_to_96_of_255",
    "coverage_max_diff_over_96_of_255",
];

struct Context {
    faces: Vec<(String, FaceRef)>,
    engine: TextEngine,
}

impl Context {
    fn play_font(&mut self, text: &str) -> Result<PlayRun, String> {
        let block = TextBlock {
            text: text.to_owned(),
            style: TextStyle { size: SIZE, weight: Weight::Regular, color: Color::WHITE, outline: None },
            max_width: None,
            align: Align::Start,
        };
        let layout = self.engine.layout(&block)?;
        let first = layout.lines.first().and_then(|line| line.runs.first()).ok_or("Play laid out no glyph run")?;
        let single = layout.lines.len() == 1 && layout.lines[0].runs.len() == 1;
        let glyph_runs = layout.lines.iter().map(|line| line.runs.len()).sum();
        Ok(PlayRun {
            font: first.font.clone(),
            ids: first.glyphs.clone(),
            single,
            rtl: first.right_to_left,
            missing: layout.missing_glyphs,
            glyph_runs,
        })
    }

    fn find(&self, name: &str) -> Option<FaceRef> {
        self.faces.iter().find(|(known, _)| known == name).map(|(_, face)| face.clone())
    }
}

fn describe(shaped: &[Shaped]) -> String {
    let shown: Vec<String> = shaped
        .iter()
        .take(12)
        .map(|g| format!("{}@{}+{}({},{})", g.gid, g.cluster, g.x_advance, g.x_offset, g.y_offset))
        .collect();
    format!("[{}{}]", shown.join(" "), if shaped.len() > 12 { " ..." } else { "" })
}

fn run_pair(
    stats: &mut Stats,
    face: &FaceRef,
    run: &Run,
    instance: &Instance,
    axes: Option<&[(Tag, f32, f32, f32)]>,
    covered: bool,
    play: Option<&PlayRun>,
) -> Result<(), String> {
    let path = CString::new(face.path.to_string_lossy().as_bytes()).map_err(|_| "path holds NUL".to_owned())?;
    let hb = HbShaper::open(&path, face.index, instance.weight)?;
    let bytes = std::fs::read(&face.path).map_err(|error| error.to_string())?;
    let base = Font::new(bytes, face.index).ok_or_else(|| format!("skrifa could not load {}", face.path.display()))?;
    let font = match instance.weight {
        Some(weight) => base.instance_builder().variations([("wght", weight)]).build(),
        None => base,
    };
    let hb_glyphs = hb.shape(&run.text, run.rtl, run.script);
    let hr_glyphs = harfrust_shape(&font, &run.text, run.rtl, run.script);
    let prefix = format!("{}.{}", if covered { "covered" } else { "uncovered" }, instance.label);
    if let Some(detail) = compare_shaping(stats, &prefix, &hb_glyphs, &hr_glyphs) {
        println!("    [{prefix}] differs: {detail}\n      hb      {}\n      harfrust {}", describe(&hb_glyphs), describe(&hr_glyphs));
    }
    if let Some(played) = play {
        if instance.weight.is_none() {
            if !played.single {
                stats.inc("play_engine.not_comparable_fallback_or_wrapped_runs");
            } else if played.rtl != run.rtl {
                stats.inc("play_engine.not_comparable_isolated_run_direction_differs");
            } else {
                let direct: Vec<u32> = hb_glyphs.iter().map(|g| g.gid).collect();
                let ids = &played.ids;
                if &direct == ids {
                    stats.inc("play_engine.glyph_ids_match_direct_harfbuzz");
                } else {
                    stats.inc("play_engine.glyph_ids_differ_from_direct_harfbuzz");
                    println!("    play engine ids {ids:?} vs direct harfbuzz {direct:?}");
                }
            }
        }
    }
    if !covered {
        return Ok(());
    }
    let design = axes.zip(instance.weight).map(|(axes, weight)| design_coordinates(axes, weight));
    let ft = FtFont::open(face, design.as_deref())?;
    let scaler = ScalerFont::unhinted(font.clone(), Some(SIZE as f32));
    let mut gids: Vec<u32> = hb_glyphs.iter().map(|g| g.gid).filter(|gid| *gid != 0).collect();
    gids.sort_unstable();
    gids.dedup();
    let mut local = Stats::default();
    for gid in gids {
        compare_raster(&mut local, &ft, &scaler, &font, gid);
    }
    for (key, value) in local.counts {
        stats.add(&format!("{}.{key}", instance.label), value);
    }
    for (key, value) in local.maxima {
        stats.max(&format!("{}.{key}", instance.label), value);
    }
    Ok(())
}

fn print_stats(title: &str, stats: &Stats) {
    println!("--- {title}");
    for (key, value) in &stats.counts {
        println!("{key} = {value}");
    }
    for (key, value) in &stats.maxima {
        println!("{key} = {value}");
    }
}

#[test]
#[ignore = "differential evidence harness; run explicitly, see docs/rust-text-differential.md"]
fn current_text_path_versus_harfrust_and_skrifa() {
    let (faces, total) = index_fonts();
    // SAFETY: returns a static NUL-terminated version string.
    let hb_version = unsafe { CStr::from_ptr(hb_version_string()) }.to_string_lossy().into_owned();
    println!("harness: size {SIZE}px, FreeType NO_HINTING|COLOR vs skrifa unhinted + zeno; HarfBuzz {hb_version} vs harfrust");
    println!("host font faces indexed: {total}");
    assert!(total > 0, "no host fonts found under fontconfig <dir> entries or PLAY_TEXT_DIFF_FONTS");
    let engine = TextEngine::new().expect("Play text engine starts");
    let mut context = Context { faces, engine };
    let mut totals = Stats::default();
    let mut compared_runs = 0u64;
    for entry in CORPUS {
        println!("== {} ({:?})", entry.id, entry.text);
        let mut stats = Stats::default();
        let runs = segment(entry.text);
        if entry.variable {
            let Some((face, axes)) = variable_face(&context.faces, entry.text) else {
                println!("  no host font has a wght axis covering the text; set PLAY_TEXT_DIFF_FONTS to a directory with one");
                stats.inc("corpus.entry_skipped_no_variable_font");
                print_stats(entry.id, &stats);
                for (key, value) in stats.counts {
                    totals.add(&key, value);
                }
                continue;
            };
            let instances = [Instance { label: "default-instance", weight: None }, Instance { label: "wght-700-instance", weight: Some(HEAVY) }];
            for run in &runs {
                println!("  run {:?} script={} rtl={} font={}", run.text, tag_text(run.script), run.rtl, face.path.display());
                for instance in &instances {
                    match run_pair(&mut stats, &face, run, instance, Some(&axes), true, None) {
                        Ok(()) => compared_runs += 1,
                        Err(error) => {
                            println!("    skipped: {error}");
                            stats.inc("corpus.run_errors");
                        }
                    }
                }
            }
        } else {
            let instances = [Instance { label: "default-instance", weight: None }];
            for run in &runs {
                let played = context.play_font(&run.text);
                let played = match played {
                    Ok(played) => played,
                    Err(error) => {
                        println!("  run {:?}: Play could not lay out: {error}", run.text);
                        stats.inc("corpus.run_errors");
                        continue;
                    }
                };
                let (name, missing, play_runs) = (played.font.clone(), played.missing, played.glyph_runs);
                let covered = missing == 0;
                println!(
                    "  run {:?} script={} rtl={} play_font={name} play_glyph_runs={play_runs} play_missing_glyphs={missing}",
                    run.text,
                    tag_text(run.script),
                    run.rtl
                );
                if !covered {
                    stats.inc("corpus.runs_with_no_covering_host_font");
                }
                let Some(face) = context.find(&name) else {
                    println!("    skipped: font {name} is not under the indexed font directories");
                    stats.inc("corpus.run_errors");
                    continue;
                };
                for instance in &instances {
                    match run_pair(&mut stats, &face, run, instance, None, covered, Some(&played)) {
                        Ok(()) => compared_runs += 1,
                        Err(error) => {
                            println!("    skipped: {error}");
                            stats.inc("corpus.run_errors");
                        }
                    }
                }
            }
        }
        if entry.variable {
            let count = |key: &str| stats.counts.get(key).copied().unwrap_or(0);
            if count("wght-700-instance.raster.compared") > 0 {
                for source in ["freetype_coverage_sum", "skrifa_zeno_coverage_sum"] {
                    assert!(
                        count(&format!("wght-700-instance.raster.{source}")) > count(&format!("default-instance.raster.{source}")),
                        "{source}: wght=700 must add ink over the default instance, or the variation was not applied"
                    );
                }
            }
        }
        print_stats(entry.id, &stats);
        for (key, value) in &stats.counts {
            totals.add(key, *value);
        }
        for (key, value) in &stats.maxima {
            totals.max(key, *value);
        }
    }
    for prefix in ["covered.default-instance", "uncovered.default-instance", "covered.wght-700-instance"] {
        for key in SHAPE_KEYS {
            totals.add(&format!("{prefix}.shape.{key}"), 0);
        }
    }
    for label in ["default-instance", "wght-700-instance"] {
        for key in RASTER_KEYS {
            totals.add(&format!("{label}.raster.{key}"), 0);
        }
    }
    for key in ["play_engine.glyph_ids_match_direct_harfbuzz", "play_engine.glyph_ids_differ_from_direct_harfbuzz"] {
        totals.add(key, 0);
    }
    print_stats("TOTAL", &totals);
    println!("runs compared: {compared_runs}");
    assert!(compared_runs > 0, "no corpus run could be compared");
}
