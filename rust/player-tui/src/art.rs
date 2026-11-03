pub type Rgb = (u8, u8, u8);

pub const COVER_SIZE: usize = 320;

#[derive(Clone)]
pub struct Cover {
    pub pixels: Vec<u8>,
}

impl Cover {
    pub fn new(pixels: Vec<u8>) -> Option<Self> {
        if pixels.len() == COVER_SIZE * COVER_SIZE * 3 {
            Some(Self { pixels })
        } else {
            None
        }
    }

    fn pixel(&self, x: usize, y: usize) -> Rgb {
        let index = (y * COVER_SIZE + x) * 3;
        (self.pixels[index], self.pixels[index + 1], self.pixels[index + 2])
    }

    pub fn accent(&self) -> Rgb {
        let mut scored = Vec::with_capacity(COVER_SIZE * COVER_SIZE);
        for y in 0..COVER_SIZE {
            for x in 0..COVER_SIZE {
                let rgb = self.pixel(x, y);
                let (_, saturation, value) = to_hsv(rgb);
                if value < 0.18 || (saturation < 0.12 && value > 0.86) {
                    continue;
                }
                let score = saturation
                    * (0.35 + 0.65 * value)
                    * (1.0 - (value - 0.92).max(0.0) * 4.0);
                scored.push((score, rgb));
            }
        }
        if scored.is_empty() {
            return (255, 150, 106);
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let count = (scored.len() / 12).max(1);
        let mut sum = (0.0f32, 0.0f32, 0.0f32);
        for (_, rgb) in &scored[..count] {
            sum.0 += rgb.0 as f32;
            sum.1 += rgb.1 as f32;
            sum.2 += rgb.2 as f32;
        }
        let average = (
            (sum.0 / count as f32) as u8,
            (sum.1 / count as f32) as u8,
            (sum.2 / count as f32) as u8,
        );
        let (hue, saturation, value) = to_hsv(average);
        from_hsv(
            hue,
            (saturation * 0.95).clamp(0.42, 0.68),
            (value + 0.14).clamp(0.88, 0.95),
        )
    }

    pub fn half_blocks(&self, width: usize, height: usize) -> Vec<(Rgb, Rgb)> {
        let mut out = Vec::with_capacity(width * height);
        if width == 0 || height == 0 {
            return out;
        }
        let mut xs = Vec::with_capacity(width);
        for x in 0..width {
            xs.push((
                x * COVER_SIZE / width,
                ((x + 1) * COVER_SIZE / width).max(x * COVER_SIZE / width + 1),
            ));
        }
        let mut ys = Vec::with_capacity(height * 2);
        for y in 0..height * 2 {
            ys.push((
                y * COVER_SIZE / (height * 2),
                ((y + 1) * COVER_SIZE / (height * 2))
                    .max(y * COVER_SIZE / (height * 2) + 1),
            ));
        }
        for row in 0..height {
            for &(x0, x1) in &xs {
                let top = self.box_average(x0, x1, ys[row * 2].0, ys[row * 2].1);
                let bottom = self.box_average(x0, x1, ys[row * 2 + 1].0, ys[row * 2 + 1].1);
                out.push((top, bottom));
            }
        }
        out
    }

    fn box_average(&self, x0: usize, x1: usize, y0: usize, y1: usize) -> Rgb {
        let mut sum = (0u64, 0u64, 0u64);
        let mut count = 0u64;
        for y in y0..y1 {
            for x in x0..x1 {
                let rgb = self.pixel(x, y);
                sum.0 += rgb.0 as u64;
                sum.1 += rgb.1 as u64;
                sum.2 += rgb.2 as u64;
                count += 1;
            }
        }
        if count == 0 {
            return (0, 0, 0);
        }
        (
            (sum.0 / count) as u8,
            (sum.1 / count) as u8,
            (sum.2 / count) as u8,
        )
    }
}

fn to_hsv(rgb: Rgb) -> (f32, f32, f32) {
    let r = rgb.0 as f32 / 255.0;
    let g = rgb.1 as f32 / 255.0;
    let b = rgb.2 as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    (hue, saturation, max)
}

fn from_hsv(hue: f32, saturation: f32, value: f32) -> Rgb {
    let h = (hue.fract() + 1.0).fract() * 6.0;
    let chroma = value * saturation;
    let x = chroma * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    (
        ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover(fill: Rgb) -> Cover {
        let mut pixels = Vec::with_capacity(COVER_SIZE * COVER_SIZE * 3);
        for _ in 0..COVER_SIZE * COVER_SIZE {
            pixels.extend_from_slice(&[fill.0, fill.1, fill.2]);
        }
        Cover { pixels }
    }

    #[test]
    fn accent_prefers_the_colour_of_the_cover() {
        let accent = cover((220, 90, 70)).accent();
        assert!(accent.0 > accent.1 && accent.1 > accent.2, "{accent:?}");
        assert!(accent.0 > 180, "{accent:?}");
    }

    #[test]
    fn half_blocks_match_the_requested_cell_grid() {
        let blocks = cover((10, 20, 30)).half_blocks(13, 7);
        assert_eq!(blocks.len(), 13 * 7);
        assert!(blocks.iter().all(|(top, bottom)| *top == (10, 20, 30) && *bottom == *top));
    }
}
