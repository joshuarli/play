use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, CString, c_char, c_int, c_long, c_uint, c_void};
use std::ops::Range;
use std::ptr;

use unicode_bidi::BidiInfo;
use unicode_linebreak::linebreaks;

use super::{GlyphRun, Line, Rendered, TextBlock, Weight};
use crate::compose::Canvas;
use crate::display::Color;

#[repr(C)]
struct FcPattern {
    _private: [u8; 0],
}
#[repr(C)]
struct FcConfig {
    _private: [u8; 0],
}
#[repr(C)]
struct FcCharSet {
    _private: [u8; 0],
}
#[repr(C)]
struct FcFontSet {
    nfont: c_int,
    sfont: c_int,
    fonts: *mut *mut FcPattern,
}

const FC_MATCH_PATTERN: c_int = 0;
const FC_RESULT_MATCH: c_int = 0;
const FC_WEIGHT_REGULAR: c_int = 80;
const FC_WEIGHT_BOLD: c_int = 200;

#[link(name = "fontconfig")]
unsafe extern "C" {
    fn FcInitLoadConfigAndFonts() -> *mut FcConfig;
    fn FcConfigDestroy(config: *mut FcConfig);
    fn FcPatternCreate() -> *mut FcPattern;
    fn FcPatternDestroy(pattern: *mut FcPattern);
    fn FcPatternAddString(pattern: *mut FcPattern, object: *const c_char, value: *const u8) -> c_int;
    fn FcPatternAddInteger(pattern: *mut FcPattern, object: *const c_char, value: c_int) -> c_int;
    fn FcConfigSubstitute(config: *mut FcConfig, pattern: *mut FcPattern, kind: c_int) -> c_int;
    fn FcDefaultSubstitute(pattern: *mut FcPattern);
    fn FcFontSort(
        config: *mut FcConfig,
        pattern: *mut FcPattern,
        trim: c_int,
        charset: *mut *mut FcCharSet,
        result: *mut c_int,
    ) -> *mut FcFontSet;
    fn FcFontSetDestroy(set: *mut FcFontSet);
    fn FcCharSetDestroy(charset: *mut FcCharSet);
    fn FcPatternGetString(pattern: *mut FcPattern, object: *const c_char, id: c_int, value: *mut *const c_char) -> c_int;
    fn FcPatternGetInteger(pattern: *mut FcPattern, object: *const c_char, id: c_int, value: *mut c_int) -> c_int;
    fn FcPatternGetCharSet(
        pattern: *mut FcPattern,
        object: *const c_char,
        id: c_int,
        value: *mut *mut FcCharSet,
    ) -> c_int;
    fn FcPatternGetBool(pattern: *mut FcPattern, object: *const c_char, id: c_int, value: *mut c_int) -> c_int;
    fn FcCharSetHasChar(charset: *const FcCharSet, ucs4: u32) -> c_int;
}

type FtLibrary = *mut c_void;
type FtFace = *mut FaceRec;

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
    library: FtLibrary,
    face: FtFace,
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
const FT_PIXEL_MODE_MONO: u8 = 1;
const FT_PIXEL_MODE_GRAY: u8 = 2;
const FT_PIXEL_MODE_BGRA: u8 = 7;

#[link(name = "freetype")]
unsafe extern "C" {
    fn FT_Init_FreeType(library: *mut FtLibrary) -> c_int;
    fn FT_Done_FreeType(library: FtLibrary) -> c_int;
    fn FT_New_Face(library: FtLibrary, path: *const c_char, index: c_long, face: *mut FtFace) -> c_int;
    fn FT_Done_Face(face: FtFace) -> c_int;
    fn FT_Set_Char_Size(face: FtFace, width: c_long, height: c_long, h_res: c_uint, v_res: c_uint) -> c_int;
    fn FT_Select_Size(face: FtFace, strike: c_int) -> c_int;
    fn FT_Set_Transform(face: FtFace, matrix: *mut c_void, delta: *mut FtVector);
    fn FT_Load_Glyph(face: FtFace, glyph: c_uint, flags: c_int) -> c_int;
    fn FT_Render_Glyph(slot: *mut GlyphSlotRec, mode: c_int) -> c_int;
    fn FT_Get_Postscript_Name(face: FtFace) -> *const c_char;
}

#[repr(C)]
struct HbBlob {
    _private: [u8; 0],
}
#[repr(C)]
struct HbFace {
    _private: [u8; 0],
}
#[repr(C)]
struct HbFont {
    _private: [u8; 0],
}
#[repr(C)]
struct HbBuffer {
    _private: [u8; 0],
}
#[repr(C)]
struct HbUnicodeFuncs {
    _private: [u8; 0],
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
#[derive(Default)]
struct HbFontExtents {
    ascender: i32,
    descender: i32,
    line_gap: i32,
    reserved: [i32; 9],
}

const HB_DIRECTION_LTR: c_uint = 4;
const HB_DIRECTION_RTL: c_uint = 5;
const HB_SCRIPT_COMMON: u32 = u32::from_be_bytes(*b"Zyyy");
const HB_SCRIPT_INHERITED: u32 = u32::from_be_bytes(*b"Zinh");
const HB_SCRIPT_UNKNOWN: u32 = u32::from_be_bytes(*b"Zzzz");
const HB_SCALE: f64 = 64.0;
const PICTOGRAPH_START: u32 = 0x1F000;

#[link(name = "harfbuzz")]
unsafe extern "C" {
    fn hb_blob_create_from_file(path: *const c_char) -> *mut HbBlob;
    fn hb_blob_destroy(blob: *mut HbBlob);
    fn hb_face_create(blob: *mut HbBlob, index: c_uint) -> *mut HbFace;
    fn hb_face_destroy(face: *mut HbFace);
    fn hb_face_get_glyph_count(face: *mut HbFace) -> c_uint;
    fn hb_font_create(face: *mut HbFace) -> *mut HbFont;
    fn hb_font_destroy(font: *mut HbFont);
    fn hb_font_set_scale(font: *mut HbFont, x: c_int, y: c_int);
    fn hb_font_get_h_extents(font: *mut HbFont, extents: *mut HbFontExtents) -> c_int;
    fn hb_buffer_create() -> *mut HbBuffer;
    fn hb_buffer_destroy(buffer: *mut HbBuffer);
    fn hb_buffer_clear_contents(buffer: *mut HbBuffer);
    fn hb_buffer_add_utf8(buffer: *mut HbBuffer, text: *const c_char, length: c_int, offset: c_uint, count: c_int);
    fn hb_buffer_set_direction(buffer: *mut HbBuffer, direction: c_uint);
    fn hb_buffer_set_script(buffer: *mut HbBuffer, script: u32);
    fn hb_buffer_guess_segment_properties(buffer: *mut HbBuffer);
    fn hb_buffer_get_length(buffer: *mut HbBuffer) -> c_uint;
    fn hb_buffer_get_glyph_infos(buffer: *mut HbBuffer, length: *mut c_uint) -> *mut HbGlyphInfo;
    fn hb_buffer_get_glyph_positions(buffer: *mut HbBuffer, length: *mut c_uint) -> *mut HbGlyphPosition;
    fn hb_shape(font: *mut HbFont, buffer: *mut HbBuffer, features: *const c_void, count: c_uint);
    fn hb_unicode_funcs_get_default() -> *mut HbUnicodeFuncs;
    fn hb_unicode_script(funcs: *mut HbUnicodeFuncs, codepoint: u32) -> u32;
    fn hb_ot_tags_from_script_and_language(
        script: u32,
        language: *const c_void,
        script_count: *mut c_uint,
        script_tags: *mut u32,
        language_count: *mut c_uint,
        language_tags: *mut u32,
    );
    fn hb_ot_layout_table_select_script(
        face: *mut HbFace,
        table_tag: u32,
        script_count: c_uint,
        script_tags: *const u32,
        script_index: *mut c_uint,
        chosen_script: *mut u32,
    ) -> c_int;
}

struct Candidate {
    path: CString,
    index: c_int,
    charset: *mut FcCharSet,
    color: bool,
}

struct Ranked {
    set: *mut FcFontSet,
    candidates: Vec<Candidate>,
}

struct Face {
    ft: FtFace,
    blob: *mut HbBlob,
    hb_face: *mut HbFace,
    hb_font: *mut HbFont,
    name: String,
    ascent: f64,
    descent: f64,
    line_gap: f64,
    strike_ppem: Option<f64>,
}

impl Drop for Face {
    fn drop(&mut self) {
        // SAFETY: each handle was created by its matching constructor in
        // `Backend::face`, is owned solely by this `Face`, and is released
        // once, in dependency order.
        unsafe {
            hb_font_destroy(self.hb_font);
            hb_face_destroy(self.hb_face);
            hb_blob_destroy(self.blob);
            FT_Done_Face(self.ft);
        }
    }
}

pub(crate) struct Backend {
    config: *mut FcConfig,
    library: FtLibrary,
    buffer: *mut HbBuffer,
    ranked: [Option<Ranked>; 2],
    faces: Vec<Face>,
    face_ids: HashMap<(usize, usize, u64), usize>,
    script_support: HashMap<(usize, usize, u32), bool>,
}

// SAFETY: the backend exclusively owns every Fontconfig, FreeType, and
// HarfBuzz handle it holds and never shares them; each library is only used
// through `&mut self`, so moving the whole backend to another thread is sound.
unsafe impl Send for Backend {}

impl Drop for Backend {
    fn drop(&mut self) {
        self.faces.clear();
        for ranked in self.ranked.iter_mut().filter_map(Option::take) {
            // SAFETY: the set came from FcFontSort and owns the patterns and
            // charsets the candidates borrow; none outlive this loop body.
            unsafe { FcFontSetDestroy(ranked.set) }
        }
        // SAFETY: each handle is owned by the backend, valid, and released
        // once after every face that depends on the library is gone.
        unsafe {
            hb_buffer_destroy(self.buffer);
            FT_Done_FreeType(self.library);
            FcConfigDestroy(self.config);
        }
    }
}

#[derive(Clone)]
struct Glyph {
    id: u32,
    cluster: usize,
    advance: f64,
    dx: f64,
    dy: f64,
    face: usize,
}

struct Unit {
    start: usize,
    end: usize,
    width: f64,
    glyphs: Vec<Glyph>,
    segment: usize,
    level: u8,
}

struct Placed {
    face: usize,
    id: u32,
    x: f64,
    dy: f64,
}

struct Piece {
    level: u8,
    glyphs: Vec<Glyph>,
}

fn weight_slot(weight: Weight) -> usize {
    match weight {
        Weight::Regular => 0,
        Weight::Bold => 1,
    }
}

fn resolve_scripts(text: &str) -> Vec<(usize, u32)> {
    // SAFETY: returns HarfBuzz's static default Unicode functions.
    let funcs = unsafe { hb_unicode_funcs_get_default() };
    let mut scripts: Vec<(usize, u32)> = text
        .char_indices()
        .map(|(at, c)| {
            // SAFETY: `funcs` is the live default table; any code point is accepted.
            (at, unsafe { hb_unicode_script(funcs, c as u32) })
        })
        .collect();
    let neutral = |script: u32| matches!(script, HB_SCRIPT_COMMON | HB_SCRIPT_INHERITED | HB_SCRIPT_UNKNOWN);
    let mut previous = None;
    for (_, script) in &mut scripts {
        if neutral(*script) {
            if let Some(known) = previous {
                *script = known;
            }
        } else {
            previous = Some(*script);
        }
    }
    let mut next = None;
    for (_, script) in scripts.iter_mut().rev() {
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

impl Backend {
    pub fn new() -> Result<Self, String> {
        // SAFETY: no preconditions; returns an owned config or null.
        let config = unsafe { FcInitLoadConfigAndFonts() };
        if config.is_null() {
            return Err("Fontconfig could not load its configuration".to_owned());
        }
        let mut library: FtLibrary = ptr::null_mut();
        // SAFETY: `library` is a valid out-pointer.
        if unsafe { FT_Init_FreeType(&mut library) } != 0 {
            // SAFETY: `config` is the owned config created above.
            unsafe { FcConfigDestroy(config) };
            return Err("FreeType could not initialize".to_owned());
        }
        // SAFETY: no preconditions; returns an owned buffer or the inert empty buffer.
        let buffer = unsafe { hb_buffer_create() };
        Ok(Backend {
            config,
            library,
            buffer,
            ranked: [None, None],
            faces: Vec::new(),
            face_ids: HashMap::new(),
            script_support: HashMap::new(),
        })
    }

    fn ranked(&mut self, weight: Weight) -> Result<&Ranked, String> {
        let slot = weight_slot(weight);
        if self.ranked[slot].is_none() {
            let value = match weight {
                Weight::Regular => FC_WEIGHT_REGULAR,
                Weight::Bold => FC_WEIGHT_BOLD,
            };
            // SAFETY: the pattern is created and destroyed here; the
            // C-string literals are NUL-terminated and outlive each call;
            // `config` is live; FcFontSort returns an owned set whose
            // patterns and charsets stay valid until it is destroyed, which
            // only `Drop` does. Out-pointers are valid locals.
            let ranked = unsafe {
                let pattern = FcPatternCreate();
                if pattern.is_null() {
                    return Err("Fontconfig could not create a pattern".to_owned());
                }
                FcPatternAddString(pattern, c"family".as_ptr(), c"sans-serif".as_ptr().cast());
                FcPatternAddInteger(pattern, c"weight".as_ptr(), value);
                FcConfigSubstitute(self.config, pattern, FC_MATCH_PATTERN);
                FcDefaultSubstitute(pattern);
                let mut coverage: *mut FcCharSet = ptr::null_mut();
                let mut result = 0;
                let set = FcFontSort(self.config, pattern, 1, &mut coverage, &mut result);
                FcPatternDestroy(pattern);
                if !coverage.is_null() {
                    FcCharSetDestroy(coverage);
                }
                if set.is_null() || (*set).nfont <= 0 {
                    return Err("Fontconfig found no fonts".to_owned());
                }
                let mut candidates = Vec::new();
                for at in 0..(*set).nfont as usize {
                    let font = *(*set).fonts.add(at);
                    let mut file: *const c_char = ptr::null();
                    let mut index = 0;
                    let mut charset: *mut FcCharSet = ptr::null_mut();
                    if FcPatternGetString(font, c"file".as_ptr(), 0, &mut file) != FC_RESULT_MATCH
                        || FcPatternGetInteger(font, c"index".as_ptr(), 0, &mut index) != FC_RESULT_MATCH
                        || FcPatternGetCharSet(font, c"charset".as_ptr(), 0, &mut charset) != FC_RESULT_MATCH
                    {
                        continue;
                    }
                    let mut color = 0;
                    if FcPatternGetBool(font, c"color".as_ptr(), 0, &mut color) != FC_RESULT_MATCH {
                        color = 0;
                    }
                    candidates.push(Candidate {
                        path: CStr::from_ptr(file).to_owned(),
                        index,
                        charset,
                        color: color != 0,
                    });
                }
                Ranked { set, candidates }
            };
            if ranked.candidates.is_empty() {
                // SAFETY: the set was just returned by FcFontSort and nothing borrows it.
                unsafe { FcFontSetDestroy(ranked.set) };
                return Err("Fontconfig fonts have no file, index, or coverage".to_owned());
            }
            self.ranked[slot] = Some(ranked);
        }
        Ok(self.ranked[slot].as_ref().expect("ranked fonts were just built"))
    }

    fn covers(&self, weight: Weight, candidate: usize, c: char) -> bool {
        let ranked = self.ranked[weight_slot(weight)].as_ref().expect("ranked fonts are built before use");
        // SAFETY: the charset is owned by the live font set and `c` is a valid code point.
        unsafe { FcCharSetHasChar(ranked.candidates[candidate].charset, c as u32) != 0 }
    }

    fn supports(&mut self, weight: Weight, candidate: usize, script: u32, size: f64) -> Result<bool, String> {
        if matches!(script, HB_SCRIPT_COMMON | HB_SCRIPT_INHERITED | HB_SCRIPT_UNKNOWN) {
            return Ok(true);
        }
        let key = (weight_slot(weight), candidate, script);
        if let Some(known) = self.script_support.get(&key) {
            return Ok(*known);
        }
        let id = self.face(weight, candidate, size)?;
        let face = self.faces[id].hb_face;
        let mut tags = [0u32; 4];
        let mut count = tags.len() as c_uint;
        let mut index = 0;
        let mut chosen = 0;
        // SAFETY: `face` is a live HarfBuzz face; `tags` has room for the
        // `count` entries HarfBuzz may write, and the other out-pointers are
        // valid locals or null where optional.
        let supported = unsafe {
            hb_ot_tags_from_script_and_language(script, ptr::null(), &mut count, tags.as_mut_ptr(), ptr::null_mut(), ptr::null_mut());
            hb_ot_layout_table_select_script(face, u32::from_be_bytes(*b"GSUB"), count, tags.as_ptr(), &mut index, &mut chosen) != 0
        };
        self.script_support.insert(key, supported);
        Ok(supported)
    }

    fn pick(
        &mut self,
        weight: Weight,
        chars: &[char],
        script: u32,
        size: f64,
        exclude: Option<usize>,
    ) -> Result<Option<usize>, String> {
        let ranked = self.ranked(weight)?;
        let mut order: Vec<usize> = (0..ranked.candidates.len()).filter(|candidate| Some(*candidate) != exclude).collect();
        if chars.iter().any(|c| *c as u32 >= PICTOGRAPH_START) {
            let colored = |candidate: &usize| !ranked.candidates[*candidate].color;
            order.sort_by_key(colored);
        }
        let mut covering = None;
        for candidate in order {
            if chars.is_empty() || !chars.iter().all(|c| self.covers(weight, candidate, *c)) {
                continue;
            }
            if self.supports(weight, candidate, script, size)? {
                return Ok(Some(candidate));
            }
            covering.get_or_insert(candidate);
        }
        Ok(covering)
    }

    fn face(&mut self, weight: Weight, candidate: usize, size: f64) -> Result<usize, String> {
        let key = (weight_slot(weight), candidate, size.to_bits());
        if let Some(id) = self.face_ids.get(&key) {
            return Ok(*id);
        }
        let (path, index) = {
            let entry = &self.ranked(weight)?.candidates[candidate];
            (entry.path.clone(), entry.index)
        };
        let shown = path.to_string_lossy().into_owned();
        let mut ft: FtFace = ptr::null_mut();
        // SAFETY: `path` is NUL-terminated, `library` is live, and `ft` is a valid out-pointer.
        if unsafe { FT_New_Face(self.library, path.as_ptr(), index as c_long, &mut ft) } != 0 {
            return Err(format!("FreeType could not open {shown}"));
        }
        // SAFETY: `ft` is a live face; `available_sizes` holds `num_fixed_sizes`
        // entries when non-null; `FT_Get_Postscript_Name` and `family_name`
        // are NUL-terminated strings owned by the face or null.
        let (strike_ppem, name) = unsafe {
            let record = &*ft;
            let fixed_only = record.face_flags & FT_FACE_FLAG_SCALABLE == 0;
            let strike_ppem = if fixed_only && record.num_fixed_sizes > 0 {
                let sizes = std::slice::from_raw_parts(record.available_sizes, record.num_fixed_sizes as usize);
                let (best, ppem) = sizes
                    .iter()
                    .enumerate()
                    .map(|(at, strike)| (at, strike.y_ppem as f64 / 64.0))
                    .min_by(|a, b| (a.1 - size).abs().total_cmp(&(b.1 - size).abs()))
                    .expect("at least one strike");
                if FT_Select_Size(ft, best as c_int) != 0 {
                    FT_Done_Face(ft);
                    return Err(format!("FreeType could not select a strike in {shown}"));
                }
                Some(ppem)
            } else {
                if FT_Set_Char_Size(ft, 0, (size * 64.0).round() as c_long, 72, 72) != 0 {
                    FT_Done_Face(ft);
                    return Err(format!("FreeType could not size {shown}"));
                }
                None
            };
            let mut name = FT_Get_Postscript_Name(ft);
            if name.is_null() {
                name = record.family_name;
            }
            let name = if name.is_null() { String::new() } else { CStr::from_ptr(name).to_string_lossy().into_owned() };
            (strike_ppem, name)
        };
        // SAFETY: `path` is NUL-terminated; HarfBuzz returns an empty (not
        // null) blob on failure, detected through the glyph count below. The
        // face, font, and extents calls receive handles created just above.
        let (blob, hb_face, hb_font, extents) = unsafe {
            let blob = hb_blob_create_from_file(path.as_ptr());
            let hb_face = hb_face_create(blob, (index & 0xFFFF) as c_uint);
            let hb_font = hb_font_create(hb_face);
            let scale = (size * HB_SCALE).round() as c_int;
            hb_font_set_scale(hb_font, scale, scale);
            let mut extents = HbFontExtents::default();
            let have = hb_font_get_h_extents(hb_font, &mut extents) != 0;
            if !have || hb_face_get_glyph_count(hb_face) == 0 {
                hb_font_destroy(hb_font);
                hb_face_destroy(hb_face);
                hb_blob_destroy(blob);
                FT_Done_Face(ft);
                return Err(format!("HarfBuzz could not load {shown}"));
            }
            (blob, hb_face, hb_font, extents)
        };
        let id = self.faces.len();
        self.faces.push(Face {
            ft,
            blob,
            hb_face,
            hb_font,
            name,
            ascent: extents.ascender as f64 / HB_SCALE,
            descent: -(extents.descender as f64) / HB_SCALE,
            line_gap: extents.line_gap as f64 / HB_SCALE,
            strike_ppem,
        });
        self.face_ids.insert(key, id);
        Ok(id)
    }

    fn shape(&mut self, text: &str, range: Range<usize>, level: u8, script: u32, face_id: usize) -> Vec<Glyph> {
        let rtl = level % 2 == 1;
        let font = self.faces[face_id].hb_font;
        // SAFETY: `text` is valid UTF-8 and the offset/count lie within it;
        // `buffer` and `font` are live; the info and position arrays have
        // `length` entries and are read before the buffer is reused.
        unsafe {
            hb_buffer_clear_contents(self.buffer);
            hb_buffer_add_utf8(
                self.buffer,
                text.as_ptr().cast(),
                text.len() as c_int,
                range.start as c_uint,
                (range.end - range.start) as c_int,
            );
            hb_buffer_set_direction(self.buffer, if rtl { HB_DIRECTION_RTL } else { HB_DIRECTION_LTR });
            if !matches!(script, HB_SCRIPT_COMMON | HB_SCRIPT_INHERITED | HB_SCRIPT_UNKNOWN) {
                hb_buffer_set_script(self.buffer, script);
            }
            hb_buffer_guess_segment_properties(self.buffer);
            hb_shape(font, self.buffer, ptr::null(), 0);
            let mut length = hb_buffer_get_length(self.buffer);
            let infos = hb_buffer_get_glyph_infos(self.buffer, &mut length);
            let positions = hb_buffer_get_glyph_positions(self.buffer, &mut length);
            if infos.is_null() || positions.is_null() {
                return Vec::new();
            }
            let infos = std::slice::from_raw_parts(infos, length as usize);
            let positions = std::slice::from_raw_parts(positions, length as usize);
            infos
                .iter()
                .zip(positions)
                .map(|(info, position)| Glyph {
                    id: info.codepoint,
                    cluster: info.cluster as usize,
                    advance: position.x_advance as f64 / HB_SCALE,
                    dx: position.x_offset as f64 / HB_SCALE,
                    dy: position.y_offset as f64 / HB_SCALE,
                    face: face_id,
                })
                .collect()
        }
    }

    fn shape_with_fallback(
        &mut self,
        text: &str,
        range: Range<usize>,
        level: u8,
        script: u32,
        weight: Weight,
        size: f64,
        candidate: usize,
    ) -> Result<Vec<Glyph>, String> {
        let face_id = self.face(weight, candidate, size)?;
        let mut glyphs = self.shape(text, range.clone(), level, script, face_id);
        let mut at = 0;
        while at < glyphs.len() {
            if glyphs[at].id != 0 {
                at += 1;
                continue;
            }
            let mut stop = at;
            while stop < glyphs.len() && glyphs[stop].id == 0 {
                stop += 1;
            }
            let first = glyphs[at..stop].iter().map(|glyph| glyph.cluster).min().expect("non-empty run");
            let last = glyphs[at..stop].iter().map(|glyph| glyph.cluster).max().expect("non-empty run");
            let end = glyphs
                .iter()
                .map(|glyph| glyph.cluster)
                .filter(|cluster| *cluster > last)
                .min()
                .unwrap_or(range.end);
            let chars: Vec<char> = text[first..end].chars().filter(|c| !c.is_control()).collect();
            let alternative = self.pick(weight, &chars, script, size, Some(candidate))?;
            let run_start = at;
            at = stop;
            if let Some(other) = alternative {
                let other_face = self.face(weight, other, size)?;
                let reshaped = self.shape(text, first..end, level, script, other_face);
                if !reshaped.is_empty() && reshaped.iter().all(|glyph| glyph.id != 0) {
                    let count = reshaped.len();
                    glyphs.splice(run_start..stop, reshaped);
                    at = run_start + count;
                }
            }
        }
        Ok(glyphs)
    }

    fn paragraph_units(&mut self, text: &str, weight: Weight, size: f64) -> Result<Vec<Unit>, String> {
        let info = BidiInfo::new(text, None);
        let scripts = resolve_scripts(text);
        self.ranked(weight)?;
        let mut units = Vec::new();
        let mut segment = 0;
        let characters: Vec<(usize, char)> = text.char_indices().collect();
        let mut item_start = 0;
        while item_start < characters.len() {
            let level = info.levels[characters[item_start].0];
            let script = scripts[item_start].1;
            let mut at = item_start + 1;
            while at < characters.len() && info.levels[characters[at].0] == level && scripts[at].1 == script {
                at += 1;
            }
            let level = level.number();
            let item_begin = characters[item_start].0;
            let item_end = characters.get(at).map_or(text.len(), |(byte, _)| *byte);
            let mut current: Option<usize> = None;
            let mut sub_start = item_begin;
            let mut subs: Vec<(Range<usize>, usize)> = Vec::new();
            for &(byte, c) in &characters[item_start..at] {
                let keep = current.is_some_and(|candidate| self.covers(weight, candidate, c));
                if keep {
                    continue;
                }
                let Some(next) = self.pick(weight, &[c], script, size, None)? else {
                    if current.is_none() {
                        current = Some(0);
                    }
                    continue;
                };
                if let Some(previous) = current {
                    if byte > sub_start {
                        subs.push((sub_start..byte, previous));
                        sub_start = byte;
                    }
                }
                current = Some(next);
            }
            subs.push((sub_start..item_end, current.unwrap_or(0)));
            for (range, candidate) in subs {
                let glyphs = self.shape_with_fallback(text, range.clone(), level, script, weight, size, candidate)?;
                units.extend(group_units(glyphs, range, level, segment));
                segment += 1;
            }
            item_start = at;
        }
        Ok(units)
    }
}

fn group_units(glyphs: Vec<Glyph>, range: Range<usize>, level: u8, segment: usize) -> Vec<Unit> {
    let mut clusters: BTreeMap<usize, Vec<Glyph>> = BTreeMap::new();
    for glyph in glyphs {
        clusters.entry(glyph.cluster).or_default().push(glyph);
    }
    let starts: Vec<usize> = clusters.keys().copied().collect();
    clusters
        .into_iter()
        .enumerate()
        .map(|(at, (start, glyphs))| Unit {
            start,
            end: starts.get(at + 1).copied().unwrap_or(range.end),
            width: glyphs.iter().map(|glyph| glyph.advance).sum(),
            glyphs,
            segment,
            level,
        })
        .collect()
}

fn blank(text: &str, unit: &Unit) -> bool {
    text[unit.start..unit.end].chars().all(char::is_whitespace)
}

fn break_lines(text: &str, units: &[Unit], wrap: f64) -> Vec<Range<usize>> {
    let mut allowed = vec![false; text.len() + 1];
    for (at, _) in linebreaks(text) {
        allowed[at] = true;
    }
    let mut lines = Vec::new();
    let mut start = 0;
    while start < units.len() {
        let mut all = 0.0;
        let mut trimmed = 0.0;
        let mut last_break = None;
        let mut at = start;
        let mut end = units.len();
        while at < units.len() {
            let unit = &units[at];
            all += unit.width;
            if !blank(text, unit) {
                trimmed = all;
            }
            if trimmed > wrap && at > start {
                end = last_break.unwrap_or(at);
                break;
            }
            if allowed[unit.end] {
                last_break = Some(at + 1);
            }
            at += 1;
        }
        lines.push(start..end);
        start = end;
    }
    lines
}

fn visual_pieces(units: &[Unit]) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut at = 0;
    while at < units.len() {
        let mut stop = at;
        while stop < units.len() && units[stop].segment == units[at].segment {
            stop += 1;
        }
        let level = units[at].level;
        let group = &units[at..stop];
        let glyphs: Vec<Glyph> = if level % 2 == 1 {
            group.iter().rev().flat_map(|unit| unit.glyphs.iter().cloned()).collect()
        } else {
            group.iter().flat_map(|unit| unit.glyphs.iter().cloned()).collect()
        };
        pieces.push(Piece { level, glyphs });
        at = stop;
    }
    let highest = pieces.iter().map(|piece| piece.level).max().unwrap_or(0);
    let lowest_odd = pieces.iter().map(|piece| piece.level).filter(|level| level % 2 == 1).min();
    if let Some(lowest_odd) = lowest_odd {
        for level in (lowest_odd..=highest).rev() {
            let mut at = 0;
            while at < pieces.len() {
                if pieces[at].level < level {
                    at += 1;
                    continue;
                }
                let mut stop = at;
                while stop < pieces.len() && pieces[stop].level >= level {
                    stop += 1;
                }
                pieces[at..stop].reverse();
                at = stop;
            }
        }
    }
    pieces
}

impl Backend {
    pub fn render(&mut self, block: &TextBlock, margin: u32) -> Result<Rendered, String> {
        let size = block.style.size;
        let weight = block.style.weight;
        self.ranked(weight)?;
        let primary = self.face(weight, 0, size)?;
        let (primary_ascent, primary_descent, primary_leading) = {
            let face = &self.faces[primary];
            (face.ascent, face.descent, face.line_gap)
        };
        let wrap = block.max_width.unwrap_or(f64::MAX / 4.0).max(size);
        let mut lines = Vec::new();
        let mut placed: Vec<Vec<Placed>> = Vec::new();
        let mut missing = 0;
        let mut max_ascent = primary_ascent;
        let mut max_descent = primary_descent;
        let mut base = 0;
        for paragraph in block.text.split('\n') {
            let origin = base;
            base += paragraph.len() + 1;
            if paragraph.is_empty() {
                lines.push(Line { range: origin..origin, width: 0.0, runs: Vec::new() });
                placed.push(Vec::new());
                continue;
            }
            let units = self.paragraph_units(paragraph, weight, size)?;
            for span in break_lines(paragraph, &units, wrap) {
                let from = units[span.start].start;
                let to = units[span.end - 1].end;
                let mut kept = span.end;
                while kept > span.start && blank(paragraph, &units[kept - 1]) {
                    kept -= 1;
                }
                let mut runs: Vec<(usize, GlyphRun)> = Vec::new();
                let mut draws = Vec::new();
                let mut pen = 0.0;
                for piece in visual_pieces(&units[span.start..kept]) {
                    let rtl = piece.level % 2 == 1;
                    for glyph in piece.glyphs {
                        if glyph.id == 0 {
                            missing += 1;
                        }
                        let face = &self.faces[glyph.face];
                        max_ascent = max_ascent.max(face.ascent);
                        max_descent = max_descent.max(face.descent);
                        let continues = runs.last().is_some_and(|(id, run)| *id == glyph.face && run.right_to_left == rtl);
                        if !continues {
                            runs.push((
                                glyph.face,
                                GlyphRun { font: face.name.clone(), glyphs: Vec::new(), right_to_left: rtl, x: Vec::new() },
                            ));
                        }
                        let (_, run) = runs.last_mut().expect("a run was just pushed");
                        run.glyphs.push(glyph.id);
                        run.x.push(pen);
                        draws.push(Placed { face: glyph.face, id: glyph.id, x: pen + glyph.dx, dy: glyph.dy });
                        pen += glyph.advance;
                    }
                }
                lines.push(Line {
                    range: origin + from..origin + to,
                    width: pen,
                    runs: runs.into_iter().map(|(_, run)| run).collect(),
                });
                placed.push(draws);
            }
        }
        let line_height = (max_ascent + max_descent + primary_leading).ceil().max((primary_ascent + primary_descent).ceil());
        let ascent = max_ascent.ceil();
        let inset = margin as f64 + (size * 0.25).ceil();
        let text_width = lines.iter().map(|line| line.width).fold(0.0, f64::max);
        let width = (text_width.ceil() + 2.0 * inset) as usize;
        let height = (line_height * lines.len() as f64 + 2.0 * inset) as usize;
        let mut surface = Surface { width, height, premultiplied: vec![0.0; width * height * 4] };
        let color = block.style.color;
        for (index, (line, draws)) in lines.iter_mut().zip(&placed).enumerate() {
            let shift = -block.align.offset(text_width - line.width);
            let baseline = inset + ascent + line_height * index as f64;
            for run in &mut line.runs {
                for x in &mut run.x {
                    *x += shift;
                }
            }
            for draw in draws {
                self.draw(&mut surface, draw, inset + shift, baseline, size, color)?;
            }
        }
        Ok(Rendered {
            canvas: Canvas { width: width as u32, height: height as u32, rgba: surface.straight() },
            inset,
            ascent,
            text_width,
            line_height,
            lines,
            missing_glyphs: missing,
        })
    }

    fn draw(
        &self,
        surface: &mut Surface,
        glyph: &Placed,
        left: f64,
        baseline: f64,
        size: f64,
        color: Color,
    ) -> Result<(), String> {
        let face = &self.faces[glyph.face];
        let x = left + glyph.x;
        let y = baseline - glyph.dy;
        let (column, row) = (x.floor(), y.floor());
        let mut delta = FtVector { x: ((x - column) * 64.0).round() as c_long, y: (-(y - row) * 64.0).round() as c_long };
        // SAFETY: `face.ft` is a live face; `delta` is a valid vector for
        // the duration of the calls. After a successful load the glyph slot
        // pointer and its bitmap buffer stay valid until the next call on
        // this face, and the bitmap is only read inside this block with
        // `rows`/`pitch`/`width` bounds that FreeType reports for it.
        unsafe {
            FT_Set_Transform(face.ft, ptr::null_mut(), &mut delta);
            if FT_Load_Glyph(face.ft, glyph.id, FT_LOAD_NO_HINTING | FT_LOAD_COLOR) != 0 {
                return Err(format!("FreeType could not load glyph {} of {}", glyph.id, face.name));
            }
            let slot = (*face.ft).glyph;
            if (*slot).format != FT_GLYPH_FORMAT_BITMAP && FT_Render_Glyph(slot, FT_RENDER_MODE_NORMAL) != 0 {
                return Err(format!("FreeType could not render glyph {} of {}", glyph.id, face.name));
            }
            let bitmap = &(*slot).bitmap;
            if bitmap.rows == 0 || bitmap.width == 0 || bitmap.buffer.is_null() {
                return Ok(());
            }
            let scale = face.strike_ppem.map_or(1.0, |ppem| size / ppem);
            let source = Bitmap {
                rows: bitmap.rows as usize,
                width: bitmap.width as usize,
                pitch: bitmap.pitch as isize,
                buffer: bitmap.buffer,
                mode: bitmap.pixel_mode,
            };
            let at_x = column as i64 + ((*slot).bitmap_left as f64 * scale).round() as i64;
            let at_y = row as i64 - ((*slot).bitmap_top as f64 * scale).round() as i64;
            source.blit(surface, at_x, at_y, scale, color);
        }
        Ok(())
    }
}

struct Surface {
    width: usize,
    height: usize,
    premultiplied: Vec<f32>,
}

impl Surface {
    fn over(&mut self, x: i64, y: i64, premultiplied: [f32; 4]) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 || premultiplied[3] <= 0.0 {
            return;
        }
        let at = (y as usize * self.width + x as usize) * 4;
        let keep = 1.0 - premultiplied[3];
        for channel in 0..4 {
            self.premultiplied[at + channel] = premultiplied[channel] + self.premultiplied[at + channel] * keep;
        }
    }

    fn straight(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.premultiplied.len()];
        for (pixel, target) in self.premultiplied.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
            let alpha = pixel[3];
            if alpha <= 0.0 {
                continue;
            }
            for channel in 0..3 {
                target[channel] = (pixel[channel] / alpha * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            target[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        out
    }
}

struct Bitmap {
    rows: usize,
    width: usize,
    pitch: isize,
    buffer: *const u8,
    mode: u8,
}

impl Bitmap {
    /// # Safety
    /// `buffer` must point to `rows` rows of `pitch` bytes as FreeType laid them out.
    unsafe fn row(&self, row: usize) -> &[u8] {
        let stride = self.pitch.unsigned_abs();
        let index = if self.pitch >= 0 { row } else { self.rows - 1 - row };
        let start = if self.pitch >= 0 { self.buffer } else { unsafe { self.buffer.sub(stride * (self.rows - 1)) } };
        // SAFETY: the caller guarantees the layout; `index < rows`.
        unsafe { std::slice::from_raw_parts(start.add(stride * index), stride) }
    }

    /// # Safety
    /// Same as [`Bitmap::row`].
    unsafe fn blit(&self, surface: &mut Surface, left: i64, top: i64, scale: f64, color: Color) {
        let tint = [color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0];
        let opacity = color.a as f32 / 255.0;
        match self.mode {
            FT_PIXEL_MODE_GRAY | FT_PIXEL_MODE_MONO => {
                for row in 0..self.rows {
                    // SAFETY: forwarded from this function's contract.
                    let data = unsafe { self.row(row) };
                    for column in 0..self.width {
                        let coverage = if self.mode == FT_PIXEL_MODE_GRAY {
                            data[column] as f32 / 255.0
                        } else if data[column / 8] & (0x80 >> (column % 8)) != 0 {
                            1.0
                        } else {
                            0.0
                        };
                        let alpha = coverage * opacity;
                        surface.over(
                            left + column as i64,
                            top + row as i64,
                            [tint[0] * alpha, tint[1] * alpha, tint[2] * alpha, alpha],
                        );
                    }
                }
            }
            FT_PIXEL_MODE_BGRA => {
                let out_width = ((self.width as f64 * scale).ceil() as usize).max(1);
                let out_height = ((self.rows as f64 * scale).ceil() as usize).max(1);
                let step = 1.0 / scale;
                for out_row in 0..out_height {
                    let (y0, y1) = (out_row as f64 * step, ((out_row + 1) as f64 * step).min(self.rows as f64));
                    for out_column in 0..out_width {
                        let (x0, x1) = (out_column as f64 * step, ((out_column + 1) as f64 * step).min(self.width as f64));
                        let mut sum = [0.0f64; 4];
                        let mut weight = 0.0;
                        let mut sy = y0.floor() as usize;
                        while (sy as f64) < y1 && sy < self.rows {
                            let wy = (y1.min(sy as f64 + 1.0) - y0.max(sy as f64)).max(0.0);
                            // SAFETY: forwarded from this function's contract.
                            let data = unsafe { self.row(sy) };
                            let mut sx = x0.floor() as usize;
                            while (sx as f64) < x1 && sx < self.width {
                                let wx = (x1.min(sx as f64 + 1.0) - x0.max(sx as f64)).max(0.0);
                                let w = wx * wy;
                                let pixel = &data[sx * 4..sx * 4 + 4];
                                sum[0] += pixel[2] as f64 * w;
                                sum[1] += pixel[1] as f64 * w;
                                sum[2] += pixel[0] as f64 * w;
                                sum[3] += pixel[3] as f64 * w;
                                weight += w;
                                sx += 1;
                            }
                            sy += 1;
                        }
                        if weight <= 0.0 {
                            continue;
                        }
                        let average = |value: f64| (value / weight / 255.0) as f32 * opacity;
                        surface.over(
                            left + out_column as i64,
                            top + out_row as i64,
                            [average(sum[0]), average(sum[1]), average(sum[2]), average(sum[3])],
                        );
                    }
                }
            }
            _ => {}
        }
    }
}
