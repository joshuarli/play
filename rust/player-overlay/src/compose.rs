use crate::display::{Color, DisplayList, Item, Rect, Shape};
use crate::raster::Coverage;

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
    x: i64,
    y: i64,
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

/// The output-pixel area an item may paint, before clipping to the frame.
fn item_bounds(item: &Item, scale: f64) -> (Rect, Option<Rect>) {
    match item {
        Item::Fill { shape, clip, .. } => (shape_bounds(shape, 0.0).scaled(scale), clip.map(|c| c.scaled(scale))),
        Item::Stroke { shape, width, clip, .. } => {
            (shape_bounds(shape, *width).scaled(scale), clip.map(|c| c.scaled(scale)))
        }
        Item::Image { origin, image, clip, .. } => (
            Rect::new(origin.x * scale, origin.y * scale, image.width as f64, image.height as f64),
            clip.map(|c| c.scaled(scale)),
        ),
    }
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

/// Paints the list in order and returns the covered region, or `None` when
/// nothing is visible.
pub fn render(list: &DisplayList) -> Option<OverlayImage> {
    let frame = (list.width as usize, list.height as usize);
    let mut union: Option<(usize, usize, usize, usize)> = None;
    let mut boxes = Vec::with_capacity(list.items.len());
    for item in &list.items {
        let (bounds, clip) = item_bounds(item, list.scale);
        let bounds = match clip {
            Some(clip) => bounds.intersect(&clip),
            None => Some(bounds),
        };
        let pixels = bounds.and_then(|bounds| pixel_box(bounds, frame));
        if let Some((x0, y0, x1, y1)) = pixels {
            union = Some(match union {
                None => (x0, y0, x1, y1),
                Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
            });
        }
        boxes.push(pixels);
    }
    let (ux0, uy0, ux1, uy1) = union?;
    let mut target = Target {
        x: ux0 as i64,
        y: uy0 as i64,
        width: ux1 - ux0,
        height: uy1 - uy0,
        premultiplied: vec![0.0; (ux1 - ux0) * (uy1 - uy0) * 4],
    };
    for (item, pixels) in list.items.iter().zip(boxes) {
        let Some((x0, y0, x1, y1)) = pixels else {
            continue;
        };
        match item {
            Item::Fill { shape, color, .. } => {
                let mut raster = Coverage::new(x1 - x0, y1 - y0);
                raster.fill_path(&shape.to_path(), list.scale, -(x0 as f64), -(y0 as f64));
                paint_coverage(&mut target, &raster.finish(), (x0, y0, x1, y1), *color);
            }
            Item::Stroke { shape, width, color, .. } => {
                let mut raster = Coverage::new(x1 - x0, y1 - y0);
                raster.stroke_path(&shape.to_path(), *width, list.scale, -(x0 as f64), -(y0 as f64));
                paint_coverage(&mut target, &raster.finish(), (x0, y0, x1, y1), *color);
            }
            Item::Image { origin, image, opacity, .. } => {
                let left = (origin.x * list.scale).round() as i64;
                let top = (origin.y * list.scale).round() as i64;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let (ix, iy) = (x as i64 - left, y as i64 - top);
                        if ix < 0 || iy < 0 || ix >= image.width as i64 || iy >= image.height as i64 {
                            continue;
                        }
                        let offset = (iy as usize * image.width as usize + ix as usize) * 4;
                        let pixel = &image.rgba[offset..offset + 4];
                        let alpha = pixel[3] as f32 / 255.0 * *opacity as f32;
                        let color = [pixel[0] as f32 / 255.0, pixel[1] as f32 / 255.0, pixel[2] as f32 / 255.0];
                        target.blend(x - ux0, y - uy0, color, alpha);
                    }
                }
            }
        }
    }
    let _ = (target.x, target.y);
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
    Some(OverlayImage {
        rgba,
        x: ux0 as u32,
        y: uy0 as u32,
        width: target.width as u32,
        height: target.height as u32,
    })
}

fn paint_coverage(target: &mut Target, coverage: &[f32], (x0, y0, x1, _): (usize, usize, usize, usize), color: Color) {
    let width = x1 - x0;
    let base_alpha = color.a as f32 / 255.0;
    let rgb = linear(color);
    let (ox, oy) = (target.x as usize, target.y as usize);
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
    use crate::display::{Path, Point};
    use std::sync::Arc;

    fn pixel(image: &OverlayImage, x: u32, y: u32) -> [u8; 4] {
        let offset = (((y - image.y) * image.width + (x - image.x)) * 4) as usize;
        image.rgba[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn empty_list_renders_nothing() {
        assert_eq!(render(&DisplayList::new(100, 100, 1.0)), None);
    }

    #[test]
    fn output_is_cropped_to_the_painted_area_and_scaled() {
        let mut list = DisplayList::new(200, 100, 2.0);
        list.fill(Shape::Rect(Rect::new(10.0, 5.0, 20.0, 10.0)), Color::rgba(255, 0, 0, 255));
        let image = render(&list).expect("rect paints");
        assert_eq!((image.x, image.y, image.width, image.height), (20, 10, 40, 20));
        assert_eq!(pixel(&image, 30, 15), [255, 0, 0, 255]);
    }

    #[test]
    fn later_items_blend_over_earlier_ones_in_straight_alpha() {
        let mut list = DisplayList::new(10, 10, 1.0);
        list.fill(Shape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), Color::rgba(0, 0, 0, 128));
        list.fill(Shape::Rect(Rect::new(0.0, 0.0, 5.0, 10.0)), Color::rgba(255, 255, 255, 255));
        let image = render(&list).unwrap();
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
        let image = render(&list).unwrap();
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
        let image = render(&list).unwrap();
        assert_eq!((image.x, image.y, image.width, image.height), (6, 8, 2, 1));
        assert_eq!(pixel(&image, 6, 8), [255, 0, 0, 128]);
        assert_eq!(pixel(&image, 7, 8), [0, 255, 0, 128]);
    }

    #[test]
    fn strokes_and_paths_paint_their_geometry() {
        let mut list = DisplayList::new(30, 30, 1.0);
        list.stroke(Shape::Rect(Rect::new(5.0, 5.0, 20.0, 20.0)), 2.0, Color::WHITE);
        list.fill(Shape::Path(Path::polygon(&[(10.0, 10.0), (20.0, 15.0), (10.0, 20.0)])), Color::WHITE);
        let image = render(&list).unwrap();
        assert_eq!(pixel(&image, 15, 5)[3], 255);
        assert_eq!(pixel(&image, 12, 15)[3], 255);
        assert_eq!(pixel(&image, 22, 15)[3], 0);
    }
}
