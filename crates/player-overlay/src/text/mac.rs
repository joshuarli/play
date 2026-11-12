use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;

use super::{GlyphRun, Line, Rendered, TextBlock, Weight};
use crate::compose::Canvas;

type CFTypeRef = *const c_void;
type CFIndex = isize;

#[repr(C)]
#[derive(Clone, Copy)]
struct CFRange {
    location: CFIndex,
    length: CFIndex,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGAffineTransform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

const UTF8: u32 = 0x0800_0100;
const UI_FONT_SYSTEM: u32 = 2;
const UI_FONT_EMPHASIZED_SYSTEM: u32 = 3;
const RUN_RIGHT_TO_LEFT: u32 = 1;
const PREMULTIPLIED_LAST_BIG_ENDIAN: u32 = 1 | (4 << 12);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFRelease(object: CFTypeRef);
    fn CFStringCreateWithBytes(
        allocator: CFTypeRef,
        bytes: *const u8,
        length: CFIndex,
        encoding: u32,
        external: bool,
    ) -> CFTypeRef;
    fn CFStringGetLength(string: CFTypeRef) -> CFIndex;
    fn CFStringGetCString(string: CFTypeRef, buffer: *mut u8, size: CFIndex, encoding: u32) -> bool;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: CFIndex,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
    fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFAttributedStringCreate(allocator: CFTypeRef, string: CFTypeRef, attributes: CFTypeRef) -> CFTypeRef;
    fn CFArrayGetCount(array: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: CFIndex) -> CFTypeRef;
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    static kCTFontAttributeName: CFTypeRef;
    static kCTForegroundColorFromContextAttributeName: CFTypeRef;
    fn CTFontCreateUIFontForLanguage(kind: u32, size: f64, language: CFTypeRef) -> CFTypeRef;
    fn CTFontGetAscent(font: CFTypeRef) -> f64;
    fn CTFontGetDescent(font: CFTypeRef) -> f64;
    fn CTFontGetLeading(font: CFTypeRef) -> f64;
    fn CTFontCopyPostScriptName(font: CFTypeRef) -> CFTypeRef;
    fn CTTypesetterCreateWithAttributedString(string: CFTypeRef) -> CFTypeRef;
    fn CTTypesetterSuggestLineBreak(typesetter: CFTypeRef, start: CFIndex, width: f64) -> CFIndex;
    fn CTTypesetterCreateLine(typesetter: CFTypeRef, range: CFRange) -> CFTypeRef;
    fn CTLineGetTypographicBounds(line: CFTypeRef, ascent: *mut f64, descent: *mut f64, leading: *mut f64) -> f64;
    fn CTLineGetTrailingWhitespaceWidth(line: CFTypeRef) -> f64;
    fn CTLineGetStringRange(line: CFTypeRef) -> CFRange;
    fn CTLineGetGlyphRuns(line: CFTypeRef) -> CFTypeRef;
    fn CTLineDraw(line: CFTypeRef, context: CFTypeRef);
    fn CTRunGetGlyphCount(run: CFTypeRef) -> CFIndex;
    fn CTRunGetGlyphs(run: CFTypeRef, range: CFRange, buffer: *mut u16);
    fn CTRunGetPositions(run: CFTypeRef, range: CFRange, buffer: *mut CGPoint);
    fn CTRunGetStatus(run: CFTypeRef) -> u32;
    fn CTRunGetAttributes(run: CFTypeRef) -> CFTypeRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGColorSpaceSRGB: CFTypeRef;
    fn CGColorSpaceCreateWithName(name: CFTypeRef) -> CFTypeRef;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: CFTypeRef,
        info: u32,
    ) -> CFTypeRef;
    fn CGContextSetRGBFillColor(context: CFTypeRef, r: f64, g: f64, b: f64, a: f64);
    fn CGContextSetTextMatrix(context: CFTypeRef, transform: CGAffineTransform);
    fn CGContextSetTextPosition(context: CFTypeRef, x: f64, y: f64);
    fn CGContextSetShouldSmoothFonts(context: CFTypeRef, smooth: bool);
    fn CGContextSetShouldAntialias(context: CFTypeRef, antialias: bool);
}

/// An owned Core Foundation reference, released on drop.
struct Owned(CFTypeRef);

impl Owned {
    fn new(object: CFTypeRef, what: &str) -> Result<Self, String> {
        if object.is_null() {
            Err(format!("Core Text could not create {what}"))
        } else {
            Ok(Owned(object))
        }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: `Owned` is only built from a non-null reference returned by
        // a Create or Copy function, so this releases the single +1 it holds.
        unsafe { CFRelease(self.0) }
    }
}

fn cf_string(text: &str) -> Result<Owned, String> {
    // SAFETY: the pointer and length describe `text`, which is valid UTF-8
    // and outlives the call; Core Foundation copies the bytes.
    let string = unsafe { CFStringCreateWithBytes(ptr::null(), text.as_ptr(), text.len() as CFIndex, UTF8, false) };
    Owned::new(string, "a string")
}

fn rust_string(string: CFTypeRef) -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: `string` is a live CFString and the buffer size matches the
    // length passed, so Core Foundation writes at most that many bytes.
    let ok = unsafe { CFStringGetCString(string, buffer.as_mut_ptr(), buffer.len() as CFIndex, UTF8) };
    if !ok {
        return String::new();
    }
    let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

struct Font {
    font: Owned,
    ascent: f64,
    descent: f64,
    leading: f64,
}

pub(crate) struct Backend {
    fonts: HashMap<(Weight, u64), Font>,
    color_space: Owned,
}

// SAFETY: CTFont and CGColorSpace are immutable and documented thread-safe,
// and the backend holds no other Core Foundation state between calls.
unsafe impl Send for Backend {}

/// UTF-16 offset to byte offset for every UTF-16 position in `text`.
fn utf16_to_bytes(text: &str) -> Vec<usize> {
    let mut map = Vec::with_capacity(text.len() + 1);
    for (byte, character) in text.char_indices() {
        for _ in 0..character.len_utf16() {
            map.push(byte);
        }
    }
    map.push(text.len());
    map
}

impl Backend {
    pub fn new() -> Result<Self, String> {
        // SAFETY: `kCGColorSpaceSRGB` is a constant CFString provided by
        // CoreGraphics; the call returns a +1 reference or null.
        let color_space = unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) };
        Ok(Backend { fonts: HashMap::new(), color_space: Owned::new(color_space, "an sRGB color space")? })
    }

    fn font(&mut self, weight: Weight, size: f64) -> Result<&Font, String> {
        let key = (weight, size.to_bits());
        if let std::collections::hash_map::Entry::Vacant(e) = self.fonts.entry(key) {
            let kind = match weight {
                Weight::Regular => UI_FONT_SYSTEM,
                Weight::Bold => UI_FONT_EMPHASIZED_SYSTEM,
            };
            // SAFETY: plain value arguments; the result is a +1 CTFont or null.
            let font = Owned::new(unsafe { CTFontCreateUIFontForLanguage(kind, size, ptr::null()) }, "the system font")?;
            // SAFETY: `font.0` is a live CTFont.
            let (ascent, descent, leading) =
                unsafe { (CTFontGetAscent(font.0), CTFontGetDescent(font.0), CTFontGetLeading(font.0)) };
            e.insert(Font { font, ascent, descent, leading });
        }
        Ok(&self.fonts[&key])
    }

    pub fn render(&mut self, block: &TextBlock, margin: u32) -> Result<Rendered, String> {
        let size = block.style.size;
        let font = self.font(block.style.weight, size)?;
        let (font_ref, font_ascent, font_descent, font_leading) = (font.font.0, font.ascent, font.descent, font.leading);
        // SAFETY: the keys are Core Text's constant attribute names and the
        // values a live CTFont and the constant kCFBooleanTrue; the
        // dictionary retains both through the CFType callbacks.
        let attributes = Owned::new(
            unsafe {
                let keys = [kCTFontAttributeName, kCTForegroundColorFromContextAttributeName];
                let values = [font_ref, kCFBooleanTrue];
                CFDictionaryCreate(
                    ptr::null(),
                    keys.as_ptr(),
                    values.as_ptr(),
                    2,
                    &kCFTypeDictionaryKeyCallBacks,
                    &kCFTypeDictionaryValueCallBacks,
                )
            },
            "text attributes",
        )?;
        let wrap = block.max_width.unwrap_or(f64::MAX / 4.0).max(size);
        let mut lines = Vec::new();
        let mut ct_lines = Vec::new();
        let mut missing = 0;
        let mut paragraph_start = 0;
        let mut max_ascent = font_ascent;
        let mut max_descent = font_descent;
        for paragraph in block.text.split('\n') {
            let base = paragraph_start;
            paragraph_start += paragraph.len() + 1;
            if paragraph.is_empty() {
                lines.push(Line { range: base..base, width: 0.0, runs: Vec::new() });
                ct_lines.push(None);
                continue;
            }
            let string = cf_string(paragraph)?;
            let offsets = utf16_to_bytes(paragraph);
            // SAFETY: `string` and `attributes` are live; the attributed
            // string and typesetter returned are +1 references or null.
            let attributed = Owned::new(
                unsafe { CFAttributedStringCreate(ptr::null(), string.0, attributes.0) },
                "an attributed string",
            )?;
            let typesetter =
                Owned::new(unsafe { CTTypesetterCreateWithAttributedString(attributed.0) }, "a typesetter")?;
            // SAFETY: `string` is a live CFString.
            let length = unsafe { CFStringGetLength(string.0) };
            let mut start: CFIndex = 0;
            while start < length {
                // SAFETY: `typesetter` is live and `start` lies within the
                // string it was built from.
                let count = unsafe { CTTypesetterSuggestLineBreak(typesetter.0, start, wrap) }.max(1);
                let line = Owned::new(
                    unsafe { CTTypesetterCreateLine(typesetter.0, CFRange { location: start, length: count }) },
                    "a line",
                )?;
                let (mut ascent, mut descent, mut leading) = (0.0, 0.0, 0.0);
                // SAFETY: `line` is a live CTLine and the out-pointers are
                // valid locals.
                let width = unsafe {
                    CTLineGetTypographicBounds(line.0, &mut ascent, &mut descent, &mut leading)
                        - CTLineGetTrailingWhitespaceWidth(line.0)
                };
                max_ascent = max_ascent.max(ascent);
                max_descent = max_descent.max(descent);
                // SAFETY: `line` is live.
                let range = unsafe { CTLineGetStringRange(line.0) };
                let from = offsets[range.location as usize];
                let to = offsets[(range.location + range.length) as usize];
                let (runs, line_missing) = glyph_runs(line.0);
                missing += line_missing;
                lines.push(Line { range: base + from..base + to, width: width.max(0.0), runs });
                ct_lines.push(Some(line));
                start += count;
            }
        }
        let line_height = (max_ascent + max_descent + font_leading).ceil().max((font_ascent + font_descent).ceil());
        let ascent = max_ascent.ceil();
        let overhang = (size * 0.25).ceil();
        let inset = margin as f64 + overhang;
        let text_width = lines.iter().map(|line| line.width).fold(0.0, f64::max);
        let width = (text_width.ceil() + 2.0 * inset) as usize;
        let height = (line_height * lines.len() as f64 + 2.0 * inset) as usize;
        let mut pixels = vec![0u8; width * height * 4];
        // SAFETY: `pixels` holds exactly `height` rows of `width * 4` bytes
        // and outlives the context, which is released before it is read.
        let context = Owned::new(
            unsafe {
                CGBitmapContextCreate(
                    pixels.as_mut_ptr().cast(),
                    width,
                    height,
                    8,
                    width * 4,
                    self.color_space.0,
                    PREMULTIPLIED_LAST_BIG_ENDIAN,
                )
            },
            "a bitmap context",
        )?;
        let color = block.style.color;
        // SAFETY: `context` is a live bitmap context.
        unsafe {
            CGContextSetShouldAntialias(context.0, true);
            CGContextSetShouldSmoothFonts(context.0, false);
            CGContextSetTextMatrix(context.0, CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 });
            CGContextSetRGBFillColor(
                context.0,
                color.r as f64 / 255.0,
                color.g as f64 / 255.0,
                color.b as f64 / 255.0,
                color.a as f64 / 255.0,
            );
        }
        for (index, (line, ct_line)) in lines.iter().zip(&ct_lines).enumerate() {
            let Some(ct_line) = ct_line else {
                continue;
            };
            let x = inset - block.align.offset(text_width - line.width);
            let baseline_from_top = inset + ascent + line_height * index as f64;
            // SAFETY: both references are live; drawing writes into `pixels`
            // through the context.
            unsafe {
                CGContextSetTextPosition(context.0, x, height as f64 - baseline_from_top);
                CTLineDraw(ct_line.0, context.0);
            }
        }
        drop(context);
        for pixel in pixels.as_chunks_mut::<4>().0.iter_mut() {
            let alpha = pixel[3] as u32;
            if alpha == 0 || alpha == 255 {
                continue;
            }
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
        for line in &mut lines {
            let shift = -block.align.offset(text_width - line.width);
            for run in &mut line.runs {
                for x in &mut run.x {
                    *x += shift;
                }
            }
        }
        Ok(Rendered {
            canvas: Canvas { width: width as u32, height: height as u32, rgba: pixels },
            inset,
            ascent,
            text_width,
            line_height,
            lines,
            missing_glyphs: missing,
        })
    }
}

/// Runs sorted left to right, and how many glyphs fell back to nothing.
fn glyph_runs(line: CFTypeRef) -> (Vec<GlyphRun>, usize) {
    let mut runs = Vec::new();
    let mut missing = 0;
    // SAFETY: `line` is a live CTLine; the array and runs it returns are
    // borrowed from it and only used while it lives. Buffers are sized to
    // each run's glyph count.
    unsafe {
        let array = CTLineGetGlyphRuns(line);
        for index in 0..CFArrayGetCount(array) {
            let run = CFArrayGetValueAtIndex(array, index);
            let count = CTRunGetGlyphCount(run);
            let all = CFRange { location: 0, length: count };
            let mut glyphs = vec![0u16; count as usize];
            let mut positions = vec![CGPoint::default(); count as usize];
            CTRunGetGlyphs(run, all, glyphs.as_mut_ptr());
            CTRunGetPositions(run, all, positions.as_mut_ptr());
            let font = CFDictionaryGetValue(CTRunGetAttributes(run), kCTFontAttributeName);
            let name = if font.is_null() {
                String::new()
            } else {
                match Owned::new(CTFontCopyPostScriptName(font), "a font name") {
                    Ok(name) => rust_string(name.0),
                    Err(_) => String::new(),
                }
            };
            if name == "LastResort" {
                missing += glyphs.len();
            } else {
                missing += glyphs.iter().filter(|glyph| **glyph == 0).count();
            }
            runs.push(GlyphRun {
                font: name,
                glyphs: glyphs.into_iter().map(u32::from).collect(),
                right_to_left: CTRunGetStatus(run) & RUN_RIGHT_TO_LEFT != 0,
                x: positions.iter().map(|point| point.x).collect(),
            });
        }
    }
    runs.sort_by(|a, b| {
        let left = |run: &GlyphRun| run.x.iter().copied().fold(f64::INFINITY, f64::min);
        left(a).total_cmp(&left(b))
    });
    (runs, missing)
}
