use std::sync::Arc;

use crate::display::{Color, DisplayList, Item, Rect, Shape};
use crate::raster::Coverage;
use crate::text::TextEngine;

/// A straight-alpha RGBA bitmap in output pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Canvas { width, height, rgba: vec![0; width as usize * height as usize * 4] }
    }

    pub fn alpha_at(&self, x: u32, y: u32) -> u8 {
        self.rgba[(y as usize * self.width as usize + x as usize) * 4 + 3]
    }
}

/// The rendered overlay cropped to the pixels it covers, placed at `(x, y)`
/// within the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayImage {
    pub rgba: Vec<u8>,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Premultiplied linear-in-sRGB-values accumulation, as libplacebo blends
/// the overlay the same way.
struct Target {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    premultiplied: Vec<f32>,
}

impl Target {
    fn blend(&mut self, x: usize, y: usize, color: [f32; 3], alpha: f32) {
        if alpha <= 0.0 {
            return;
        }
        let pixel = &mut self.premultiplied[(y * self.width + x) * 4..(y * self.width + x) * 4 + 4];
        let keep = 1.0 - alpha;
        pixel[0] = color[0] * alpha + pixel[0] * keep;
        pixel[1] = color[1] * alpha + pixel[1] * keep;
        pixel[2] = color[2] * alpha + pixel[2] * keep;
        pixel[3] = alpha + pixel[3] * keep;
    }
}

fn shape_bounds(shape: &Shape, pad: f64) -> Rect {
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for command in &shape.to_path().commands {
        let points: &[crate::display::Point] = match command {
            crate::display::PathCommand::MoveTo(point) | crate::display::PathCommand::LineTo(point) => {
                std::slice::from_ref(point)
            }
            crate::display::PathCommand::CubicTo(a, b, c) => &[*a, *b, *c],
            crate::display::PathCommand::Close => &[],
        };
        for point in points {
            x0 = x0.min(point.x);
            y0 = y0.min(point.y);
            x1 = x1.max(point.x);
            y1 = y1.max(point.y);
        }
    }
    if x0 > x1 {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    Rect::new(x0 - pad, y0 - pad, x1 - x0 + 2.0 * pad, y1 - y0 + 2.0 * pad)
}

fn pixel_box(rect: Rect, frame: (usize, usize)) -> Option<(usize, usize, usize, usize)> {
    let x0 = rect.x.floor().max(0.0) as usize;
    let y0 = rect.y.floor().max(0.0) as usize;
    let x1 = (rect.right().ceil().max(0.0) as usize).min(frame.0);
    let y1 = (rect.bottom().ceil().max(0.0) as usize).min(frame.1);
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

fn linear(color: Color) -> [f32; 3] {
    [color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0]
}

/// An item resolved to output pixels.
enum Paint<'a> {
    Fill { shape: &'a Shape, scale: f64, color: Color },
    Stroke { shape: &'a Shape, width: f64, scale: f64, color: Color },
    Bitmap { left: i64, top: i64, image: Arc<Canvas>, opacity: f64 },
}

fn resolve<'a>(item: &'a Item, scale: f64, text: &mut TextEngine) -> Result<(Paint<'a>, Rect, Option<Rect>), String> {
    let (paint, bounds, clip) = match item {
        Item::Fill { shape, color, clip } => {
            (Paint::Fill { shape, scale, color: *color }, shape_bounds(shape, 0.0).scaled(scale), *clip)
        }
        Item::Stroke { shape, width, color, clip } => (
            Paint::Stroke { shape, width: *width, scale, color: *color },
            shape_bounds(shape, *width).scaled(scale),
            *clip,
        ),
        Item::Image { origin, image, opacity, clip } => {
            let (left, top) = ((origin.x * scale).round() as i64, (origin.y * scale).round() as i64);
            let bounds = Rect::new(left as f64, top as f64, image.width as f64, image.height as f64);
            (Paint::Bitmap { left, top, image: image.clone(), opacity: *opacity }, bounds, *clip)
        }
        Item::Text { origin, anchor, block, clip } => {
            let layout = text.layout(&block.scaled(scale))?;
            let lines = layout.line_height * layout.lines.len() as f64;
            let left = origin.x * scale + anchor.x.offset(layout.text_width) - layout.inset;
            let top = origin.y * scale + anchor.y.offset(lines) - layout.inset;
            let (left, top) = (left.round() as i64, top.round() as i64);
            let bounds = Rect::new(left as f64, top as f64, layout.width(), layout.height());
            (Paint::Bitmap { left, top, image: layout.canvas.clone(), opacity: 1.0 }, bounds, *clip)
        }
    };
    Ok((paint, bounds, clip.map(|clip| clip.scaled(scale))))
}

/// Paints the layers bottom to top, each list's items in order, and returns
/// the covered region, or `None` when nothing is visible. Every layer must
/// describe the same output size.
pub fn render(layers: &[&DisplayList], text: &mut TextEngine) -> Result<Option<OverlayImage>, String> {
    let Some(first) = layers.first() else {
        return Ok(None);
    };
    let frame = (first.width as usize, first.height as usize);
    if let Some(layer) = layers.iter().find(|layer| (layer.width, layer.height) != (first.width, first.height)) {
        return Err(format!(
            "overlay layers disagree on size: {}x{} and {}x{}",
            first.width, first.height, layer.width, layer.height
        ));
    }
    let mut union: Option<(usize, usize, usize, usize)> = None;
    let mut paints = Vec::new();
    for layer in layers {
        for item in &layer.items {
            let (paint, bounds, clip) = resolve(item, layer.scale, text)?;
            let bounds = match clip {
                Some(clip) => bounds.intersect(&clip),
                None => Some(bounds),
            };
            let Some(pixels) = bounds.and_then(|bounds| pixel_box(bounds, frame)) else {
                continue;
            };
            let (x0, y0, x1, y1) = pixels;
            union = Some(match union {
                None => pixels,
                Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
            });
            paints.push((paint, pixels));
        }
    }
    let Some((ux0, uy0, ux1, uy1)) = union else {
        return Ok(None);
    };
    let mut target = Target {
        x: ux0,
        y: uy0,
        width: ux1 - ux0,
        height: uy1 - uy0,
        premultiplied: vec![0.0; (ux1 - ux0) * (uy1 - uy0) * 4],
    };
    for (paint, (x0, y0, x1, y1)) in paints {
        match paint {
            Paint::Fill { shape, scale, color } => {
                let mut raster = Coverage::new(x1 - x0, y1 - y0);
                raster.fill_path(&shape.to_path(), scale, -(x0 as f64), -(y0 as f64));
                paint_coverage(&mut target, &raster.finish(), (x0, y0, x1, y1), color);
            }
            Paint::Stroke { shape, width, scale, color } => {
                let mut raster = Coverage::new(x1 - x0, y1 - y0);
                raster.stroke_path(&shape.to_path(), width, scale, -(x0 as f64), -(y0 as f64));
                paint_coverage(&mut target, &raster.finish(), (x0, y0, x1, y1), color);
            }
            Paint::Bitmap { left, top, image, opacity } => {
                for y in y0..y1 {
                    let iy = y as i64 - top;
                    if iy < 0 || iy >= image.height as i64 {
                        continue;
                    }
                    for x in x0..x1 {
                        let ix = x as i64 - left;
                        if ix < 0 || ix >= image.width as i64 {
                            continue;
                        }
                        let offset = (iy as usize * image.width as usize + ix as usize) * 4;
                        let pixel = &image.rgba[offset..offset + 4];
                        let alpha = pixel[3] as f32 / 255.0 * opacity as f32;
                        let color = [pixel[0] as f32 / 255.0, pixel[1] as f32 / 255.0, pixel[2] as f32 / 255.0];
                        target.blend(x - ux0, y - uy0, color, alpha);
                    }
                }
            }
        }
    }
    let mut rgba = vec![0u8; target.width * target.height * 4];
    for (out, pixel) in rgba.chunks_exact_mut(4).zip(target.premultiplied.chunks_exact(4)) {
        let alpha = pixel[3];
        if alpha <= 0.0 {
            continue;
        }
        for channel in 0..3 {
            out[channel] = ((pixel[channel] / alpha).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        out[3] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    Ok(Some(OverlayImage {
        rgba,
        x: ux0 as u32,
        y: uy0 as u32,
        width: target.width as u32,
        height: target.height as u32,
    }))
}

impl OverlayImage {
    /// Blends the overlay over an opaque or straight-alpha RGBA frame.
    pub fn blend_onto(&self, frame: &mut [u8], width: u32, height: u32) -> Result<(), String> {
        let expected = width as usize * height as usize * 4;
        if frame.len() < expected {
            return Err(format!("overlay target holds {} bytes, needs {expected}", frame.len()));
        }
        for row in 0..self.height {
            let y = self.y + row;
            if y >= height {
                break;
            }
            for column in 0..self.width {
                let x = self.x + column;
                if x >= width {
                    break;
                }
                let source = &self.rgba[((row * self.width + column) * 4) as usize..][..4];
                let alpha = source[3] as u32;
                if alpha == 0 {
                    continue;
                }
                let target = &mut frame[((y * width + x) * 4) as usize..][..4];
                let keep = 255 - alpha;
                for channel in 0..3 {
                    target[channel] =
                        ((source[channel] as u32 * alpha + target[channel] as u32 * keep + 127) / 255) as u8;
                }
                target[3] = ((alpha * 255 + target[3] as u32 * keep + 127) / 255) as u8;
            }
        }
        Ok(())
    }
}

fn paint_coverage(target: &mut Target, coverage: &[f32], (x0, y0, x1, _): (usize, usize, usize, usize), color: Color) {
    let width = x1 - x0;
    let base_alpha = color.a as f32 / 255.0;
    let rgb = linear(color);
    let (ox, oy) = (target.x, target.y);
    for (index, value) in coverage.iter().enumerate() {
        if *value <= 0.0 {
            continue;
        }
        let (x, y) = (x0 + index % width, y0 + index / width);
        target.blend(x - ox, y - oy, rgb, value * base_alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{Anchor, Path, Point};
    use crate::text::{TextBlock, TextStyle, Weight};

    fn engine() -> TextEngine {
        TextEngine::new().expect("text engine starts")
    }

    fn pixel(image: &OverlayImage, x: u32, y: u32) -> [u8; 4] {
        let offset = (((y - image.y) * image.width + (x - image.x)) * 4) as usize;
        image.rgba[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn empty_list_renders_nothing() {
        assert_eq!(render(&[&DisplayList::new(100, 100, 1.0)], &mut engine()), Ok(None));
    }

    #[test]
    fn output_is_cropped_to_the_painted_area_and_scaled() {
        let mut list = DisplayList::new(200, 100, 2.0);
        list.fill(Shape::Rect(Rect::new(10.0, 5.0, 20.0, 10.0)), Color::rgba(255, 0, 0, 255));
        let image = render(&[&list], &mut engine()).unwrap().expect("rect paints");
        assert_eq!((image.x, image.y, image.width, image.height), (20, 10, 40, 20));
        assert_eq!(pixel(&image, 30, 15), [255, 0, 0, 255]);
    }

    #[test]
    fn later_items_blend_over_earlier_ones_in_straight_alpha() {
        let mut list = DisplayList::new(10, 10, 1.0);
        list.fill(Shape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), Color::rgba(0, 0, 0, 128));
        list.fill(Shape::Rect(Rect::new(0.0, 0.0, 5.0, 10.0)), Color::rgba(255, 255, 255, 255));
        let image = render(&[&list], &mut engine()).unwrap().unwrap();
        assert_eq!(pixel(&image, 2, 2), [255, 255, 255, 255]);
        assert_eq!(pixel(&image, 7, 2), [0, 0, 0, 128]);
    }

    #[test]
    fn clip_limits_painting() {
        let mut list = DisplayList::new(20, 20, 1.0);
        list.push(Item::Fill {
            shape: Shape::Rect(Rect::new(0.0, 0.0, 20.0, 20.0)),
            color: Color::WHITE,
            clip: Some(Rect::new(5.0, 5.0, 5.0, 5.0)),
        });
        let image = render(&[&list], &mut engine()).unwrap().unwrap();
        assert_eq!((image.x, image.y, image.width, image.height), (5, 5, 5, 5));
    }

    #[test]
    fn images_composite_at_their_pixel_origin_with_opacity() {
        let mut canvas = Canvas::new(2, 1);
        canvas.rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let mut list = DisplayList::new(20, 20, 2.0);
        list.push(Item::Image {
            origin: Point { x: 3.0, y: 4.0 },
            image: Arc::new(canvas),
            opacity: 0.5,
            clip: None,
        });
        let image = render(&[&list], &mut engine()).unwrap().unwrap();
        assert_eq!((image.x, image.y, image.width, image.height), (6, 8, 2, 1));
        assert_eq!(pixel(&image, 6, 8), [255, 0, 0, 128]);
        assert_eq!(pixel(&image, 7, 8), [0, 255, 0, 128]);
    }

    #[test]
    fn strokes_and_paths_paint_their_geometry() {
        let mut list = DisplayList::new(30, 30, 1.0);
        list.stroke(Shape::Rect(Rect::new(5.0, 5.0, 20.0, 20.0)), 2.0, Color::WHITE);
        list.fill(Shape::Path(Path::polygon(&[(10.0, 10.0), (20.0, 15.0), (10.0, 20.0)])), Color::WHITE);
        let image = render(&[&list], &mut engine()).unwrap().unwrap();
        assert_eq!(pixel(&image, 15, 5)[3], 255);
        assert_eq!(pixel(&image, 12, 15)[3], 255);
        assert_eq!(pixel(&image, 22, 15)[3], 0);
    }

    #[test]
    fn text_is_placed_by_its_anchor_in_scaled_units() {
        let mut list = DisplayList::new(400, 200, 2.0);
        let block = TextBlock {
            text: "Anchored".to_owned(),
            style: TextStyle { size: 12.0, weight: Weight::Regular, color: Color::WHITE, outline: None },
            max_width: None,
            align: crate::display::Align::Center,
        };
        list.text(Point { x: 100.0, y: 90.0 }, Anchor::BOTTOM_CENTER, block);
        let image = render(&[&list], &mut engine()).unwrap().expect("text paints");
        let centre = image.x as f64 + image.width as f64 / 2.0;
        assert!((centre - 200.0).abs() < 3.0, "{image:?}");
        assert!(image.y + image.height >= 172 && image.y + image.height <= 200 + 12);
        assert!(image.height < 60 && image.width > 60);
    }

    #[test]
    fn layers_share_one_output_and_must_agree_on_size() {
        let mut bottom = DisplayList::new(20, 20, 1.0);
        bottom.fill(Shape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), Color::BLACK);
        let mut top = DisplayList::new(20, 20, 2.0);
        top.fill(Shape::Rect(Rect::new(0.0, 0.0, 2.0, 2.0)), Color::WHITE);
        let image = render(&[&bottom, &top], &mut engine()).unwrap().unwrap();
        assert_eq!(pixel(&image, 3, 3), [255, 255, 255, 255]);
        assert_eq!(pixel(&image, 6, 6), [0, 0, 0, 255]);
        let other = DisplayList::new(30, 20, 1.0);
        assert!(render(&[&bottom, &other], &mut engine()).is_err());
    }

    #[test]
    fn blending_onto_a_frame_mixes_by_overlay_alpha() {
        let overlay = OverlayImage { rgba: vec![255, 255, 255, 128], x: 1, y: 0, width: 1, height: 1 };
        let mut frame = vec![0, 0, 0, 255, 0, 0, 0, 255];
        overlay.blend_onto(&mut frame, 2, 1).unwrap();
        assert_eq!(frame, vec![0, 0, 0, 255, 128, 128, 128, 255]);
    }
}
