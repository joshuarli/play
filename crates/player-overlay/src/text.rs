use std::collections::HashMap;
use std::sync::Arc;

use crate::compose::Canvas;
use crate::display::{Align, Color};

#[cfg(target_os = "linux")]
#[path = "text/linux.rs"]
mod backend;
#[cfg(target_os = "macos")]
#[path = "text/mac.rs"]
mod backend;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub width: f64,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Font size in output pixels.
    pub size: f64,
    pub weight: Weight,
    pub color: Color,
    pub outline: Option<Outline>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    pub text: String,
    pub style: TextStyle,
    /// Width in output pixels to wrap lines at.
    pub max_width: Option<f64>,
    /// Horizontal alignment of each line within the block.
    pub align: Align,
}

impl TextBlock {
    /// The block with sizes, outline, and wrap width multiplied by `scale`.
    pub fn scaled(&self, scale: f64) -> TextBlock {
        let mut block = self.clone();
        block.style.size *= scale;
        if let Some(outline) = &mut block.style.outline {
            outline.width *= scale;
        }
        block.max_width = block.max_width.map(|width| width * scale);
        block
    }
}

/// One shaped run in visual order: a single font and direction.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    pub font: String,
    pub glyphs: Vec<u32>,
    pub right_to_left: bool,
    /// Pen position of each glyph relative to its line start, in pixels.
    pub x: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Byte range of the source text on this line.
    pub range: std::ops::Range<usize>,
    pub width: f64,
    pub runs: Vec<GlyphRun>,
}

/// Shaped and rasterized text. The canvas includes the outline and a
/// margin of `inset` pixels on every side for outlines and glyph overhang;
/// `ascent` is the first baseline's distance from the canvas top.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    pub canvas: Arc<Canvas>,
    pub inset: f64,
    pub ascent: f64,
    /// Widest line's advance, without the inset.
    pub text_width: f64,
    pub line_height: f64,
    pub lines: Vec<Line>,
    /// Glyphs no installed font could supply.
    pub missing_glyphs: usize,
}

impl TextLayout {
    pub fn width(&self) -> f64 {
        self.canvas.width as f64
    }

    pub fn height(&self) -> f64 {
        self.canvas.height as f64
    }
}

/// What a backend hands back before outlining.
pub(crate) struct Rendered {
    pub canvas: Canvas,
    pub inset: f64,
    pub ascent: f64,
    pub text_width: f64,
    pub line_height: f64,
    pub lines: Vec<Line>,
    pub missing_glyphs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    text: String,
    size: u64,
    weight: Weight,
    color: Color,
    outline: Option<(u64, Color)>,
    max_width: Option<u64>,
    align: u8,
}

impl Key {
    fn new(block: &TextBlock) -> Self {
        Key {
            text: block.text.clone(),
            size: block.style.size.to_bits(),
            weight: block.style.weight,
            color: block.style.color,
            outline: block.style.outline.map(|outline| (outline.width.to_bits(), outline.color)),
            max_width: block.max_width.map(f64::to_bits),
            align: block.align as u8,
        }
    }
}

const CACHE_LIMIT: usize = 512;

/// Native text shaping and rasterization with a layout cache.
pub struct TextEngine {
    backend: backend::Backend,
    cache: HashMap<Key, Arc<TextLayout>>,
    rendered: u64,
}

impl TextEngine {
    pub fn new() -> Result<Self, String> {
        Ok(TextEngine { backend: backend::Backend::new()?, cache: HashMap::new(), rendered: 0 })
    }

    /// Layouts shaped and rasterized so far; cache hits do not count.
    pub fn rendered(&self) -> u64 {
        self.rendered
    }

    pub fn layout(&mut self, block: &TextBlock) -> Result<Arc<TextLayout>, String> {
        let key = Key::new(block);
        if let Some(layout) = self.cache.get(&key) {
            return Ok(layout.clone());
        }
        let outline = block.style.outline.map(|outline| outline.width.max(0.0)).unwrap_or(0.0);
        let mut rendered = self.backend.render(block, outline.ceil() as u32)?;
        if let Some(outline) = block.style.outline {
            rendered.canvas = outlined(&rendered.canvas, outline.width, outline.color);
        }
        let layout = Arc::new(TextLayout {
            canvas: Arc::new(rendered.canvas),
            inset: rendered.inset,
            ascent: rendered.ascent,
            text_width: rendered.text_width,
            line_height: rendered.line_height,
            lines: rendered.lines,
            missing_glyphs: rendered.missing_glyphs,
        });
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(key, layout.clone());
        self.rendered += 1;
        Ok(layout)
    }
}

/// Paints `color` under the text wherever a disc of radius `width` around a
/// pixel touches glyph coverage, then the text over it. The backend leaves
/// a margin of `ceil(width)` pixels on every side for this.
fn outlined(text: &Canvas, width: f64, color: Color) -> Canvas {
    let (w, h) = (text.width as usize, text.height as usize);
    let radius = width.max(0.0);
    let reach = radius.ceil() as isize;
    let mut kernel = Vec::new();
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let distance = ((dx * dx + dy * dy) as f64).sqrt();
            let weight = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if weight > 0.0 {
                kernel.push((dx, dy, weight as f32));
            }
        }
    }
    let alpha: Vec<f32> = text.rgba.as_chunks::<4>().0.iter().map(|pixel| pixel[3] as f32 / 255.0).collect();
    let mut out = Canvas::new(text.width, text.height);
    let outline_alpha = color.a as f32 / 255.0;
    for y in 0..h {
        for x in 0..w {
            let mut edge = 0.0f32;
            for &(dx, dy, weight) in &kernel {
                let (sx, sy) = (x as isize + dx, y as isize + dy);
                if sx < 0 || sy < 0 || sx >= w as isize || sy >= h as isize {
                    continue;
                }
                edge = edge.max(alpha[sy as usize * w + sx as usize] * weight);
                if edge >= 1.0 {
                    break;
                }
            }
            let index = (y * w + x) * 4;
            let top = alpha[y * w + x];
            let under = edge * outline_alpha;
            let total = top + under * (1.0 - top);
            if total <= 0.0 {
                continue;
            }
            let mix = |text_channel: u8, outline_channel: u8| -> u8 {
                let value = (text_channel as f32 * top + outline_channel as f32 * under * (1.0 - top)) / total;
                value.round().clamp(0.0, 255.0) as u8
            };
            out.rgba[index] = mix(text.rgba[index], color.r);
            out.rgba[index + 1] = mix(text.rgba[index + 1], color.g);
            out.rgba[index + 2] = mix(text.rgba[index + 2], color.b);
            out.rgba[index + 3] = (total * 255.0).round() as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(size: f64) -> TextStyle {
        TextStyle { size, weight: Weight::Regular, color: Color::WHITE, outline: None }
    }

    fn block(text: &str, size: f64) -> TextBlock {
        TextBlock { text: text.to_owned(), style: style(size), max_width: None, align: Align::Start }
    }

    #[test]
    fn outline_surrounds_glyph_coverage_without_covering_it() {
        let mut text = Canvas::new(9, 9);
        let center = (4 * 9 + 4) * 4;
        text.rgba[center..center + 4].copy_from_slice(&[255, 255, 255, 255]);
        let out = outlined(&text, 2.0, Color::BLACK);
        assert_eq!(&out.rgba[center..center + 4], &[255, 255, 255, 255]);
        let near = (4 * 9 + 5) * 4;
        assert_eq!(&out.rgba[near..near + 4], &[0, 0, 0, 255]);
        let rim = (4 * 9 + 6) * 4;
        assert_eq!(&out.rgba[rim..rim + 4], &[0, 0, 0, 128]);
        let far = 0;
        assert_eq!(out.rgba[far + 3], 0);
    }

    #[test]
    fn layouts_are_cached_by_content_and_style() {
        let mut engine = TextEngine::new().expect("text engine starts");
        let first = engine.layout(&block("Cached", 24.0)).unwrap();
        let second = engine.layout(&block("Cached", 24.0)).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(engine.rendered(), 1);
        engine.layout(&block("Cached", 25.0)).unwrap();
        assert_eq!(engine.rendered(), 2);
    }
}
