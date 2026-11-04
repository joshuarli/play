use crate::display::{Path, PathCommand, Point};

/// Anti-aliased coverage of filled paths, computed from signed edge areas
/// accumulated per row (the font-rs technique). Paths combine by summing
/// signed coverage, so a counter-clockwise subpath cuts a hole in a
/// clockwise one, and the result is clamped to `0..=1`.
pub struct Coverage {
    width: usize,
    height: usize,
    stride: usize,
    accumulation: Vec<f32>,
}

impl Coverage {
    pub fn new(width: usize, height: usize) -> Self {
        let stride = width + 2;
        Coverage { width, height, stride, accumulation: vec![0.0; stride * height] }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Adds a path with its origin moved by `(dx, dy)` and scaled by `scale`.
    pub fn fill_path(&mut self, path: &Path, scale: f64, dx: f64, dy: f64) {
        let map = |point: Point| Point { x: point.x * scale + dx, y: point.y * scale + dy };
        let mut start: Option<Point> = None;
        let mut current: Option<Point> = None;
        for command in &path.commands {
            match *command {
                PathCommand::MoveTo(point) => {
                    if let (Some(from), Some(to)) = (current, start) {
                        self.line(from, to);
                    }
                    let point = map(point);
                    start = Some(point);
                    current = Some(point);
                }
                PathCommand::LineTo(point) => {
                    let point = map(point);
                    if let Some(from) = current {
                        self.line(from, point);
                    }
                    current = Some(point);
                }
                PathCommand::CubicTo(c1, c2, end) => {
                    let (c1, c2, end) = (map(c1), map(c2), map(end));
                    if let Some(from) = current {
                        self.cubic(from, c1, c2, end);
                    }
                    current = Some(end);
                }
                PathCommand::Close => {
                    if let (Some(from), Some(to)) = (current, start) {
                        self.line(from, to);
                    }
                    current = start;
                }
            }
        }
        if let (Some(from), Some(to)) = (current, start) {
            self.line(from, to);
        }
    }

    /// Adds the area swept by a pen of `width` along every segment of the
    /// path, as oriented quads that union under clamping.
    pub fn stroke_path(&mut self, path: &Path, width: f64, scale: f64, dx: f64, dy: f64) {
        let half = width * scale / 2.0;
        for (a, b) in segments(path, scale, dx, dy) {
            let (ex, ey) = (b.x - a.x, b.y - a.y);
            let length = (ex * ex + ey * ey).sqrt();
            if length == 0.0 {
                continue;
            }
            let (ux, uy) = (ex / length * half, ey / length * half);
            let (nx, ny) = (-uy, ux);
            let corners = [
                Point { x: a.x - ux + nx, y: a.y - uy + ny },
                Point { x: b.x + ux + nx, y: b.y + uy + ny },
                Point { x: b.x + ux - nx, y: b.y + uy - ny },
                Point { x: a.x - ux - nx, y: a.y - uy - ny },
            ];
            let signed = shoelace(&corners);
            let ordered: Vec<Point> =
                if signed >= 0.0 { corners.to_vec() } else { corners.iter().rev().copied().collect() };
            for index in 0..4 {
                self.line(ordered[index], ordered[(index + 1) % 4]);
            }
        }
    }

    fn cubic(&mut self, p0: Point, p1: Point, p2: Point, p3: Point) {
        let hull = distance(p0, p1) + distance(p1, p2) + distance(p2, p3);
        let steps = ((hull / 2.0).ceil() as usize).clamp(2, 64);
        let mut previous = p0;
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let point = cubic_point(p0, p1, p2, p3, t);
            self.line(previous, point);
            previous = point;
        }
    }

    fn line(&mut self, p0: Point, p1: Point) {
        let right = self.width as f64;
        let mut cuts = [0.0, 1.0, 2.0, 2.0];
        let mut count = 2;
        if p0.x != p1.x {
            for edge in [0.0, right] {
                let t = (edge - p0.x) / (p1.x - p0.x);
                if t > 0.0 && t < 1.0 {
                    cuts[count] = t;
                    count += 1;
                }
            }
        }
        let cuts = &mut cuts[..count];
        cuts.sort_by(f64::total_cmp);
        let at = |t: f64| Point {
            x: (p0.x + (p1.x - p0.x) * t).clamp(0.0, right),
            y: p0.y + (p1.y - p0.y) * t,
        };
        for pair in cuts.windows(2) {
            self.clipped_line(at(pair[0]), at(pair[1]));
        }
    }

    fn clipped_line(&mut self, p0: Point, p1: Point) {
        if (p0.y - p1.y).abs() <= f64::EPSILON {
            return;
        }
        let (direction, p0, p1) = if p0.y < p1.y { (1.0, p0, p1) } else { (-1.0, p1, p0) };
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
        let mut x = p0.x;
        if p0.y < 0.0 {
            x -= p0.y * dxdy;
        }
        let first = p0.y.max(0.0) as usize;
        let last = (p1.y.ceil().max(0.0) as usize).min(self.height);
        for y in first..last {
            let row = y * self.stride;
            let dy = ((y + 1) as f64).min(p1.y) - (y as f64).max(p0.y);
            let x_next = x + dxdy * dy;
            let d = (dy * direction) as f32;
            let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
            let x0_floor = x0.floor();
            let x0i = x0_floor as usize;
            let x1_ceil = x1.ceil();
            let x1i = x1_ceil as usize;
            if x1i <= x0i + 1 {
                let mid = (0.5 * (x + x_next) - x0_floor) as f32;
                self.accumulation[row + x0i] += d - d * mid;
                self.accumulation[row + x0i + 1] += d * mid;
            } else {
                let s = (1.0 / (x1 - x0)) as f32;
                let x0f = (x0 - x0_floor) as f32;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = (x1 - x1_ceil + 1.0) as f32;
                let am = 0.5 * s * x1f * x1f;
                self.accumulation[row + x0i] += d * a0;
                if x1i == x0i + 2 {
                    self.accumulation[row + x0i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.accumulation[row + x0i + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.accumulation[row + xi] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.accumulation[row + x1i - 1] += d * (1.0 - a2 - am);
                }
                self.accumulation[row + x1i] += d * am;
            }
            x = x_next;
        }
    }

    /// Coverage per pixel in `0..=1`, row-major, `width * height` values.
    pub fn finish(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.width * self.height);
        for y in 0..self.height {
            let row = &self.accumulation[y * self.stride..y * self.stride + self.width];
            let mut sum = 0.0f32;
            for value in row {
                sum += value;
                out.push(sum.abs().min(1.0));
            }
        }
        out
    }
}

fn distance(a: Point, b: Point) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

fn cubic_point(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point {
        x: a * p0.x + b * p1.x + c * p2.x + d * p3.x,
        y: a * p0.y + b * p1.y + c * p2.y + d * p3.y,
    }
}

fn shoelace(points: &[Point]) -> f64 {
    let mut area = 0.0;
    for index in 0..points.len() {
        let (a, b) = (points[index], points[(index + 1) % points.len()]);
        area += a.x * b.y - b.x * a.y;
    }
    area / 2.0
}

/// The straight segments of a path after flattening curves, mapped to
/// pixels; closed subpaths include their closing segment.
fn segments(path: &Path, scale: f64, dx: f64, dy: f64) -> Vec<(Point, Point)> {
    let map = |point: Point| Point { x: point.x * scale + dx, y: point.y * scale + dy };
    let mut out = Vec::new();
    let mut start = None;
    let mut current: Option<Point> = None;
    for command in &path.commands {
        match *command {
            PathCommand::MoveTo(point) => {
                start = Some(map(point));
                current = start;
            }
            PathCommand::LineTo(point) => {
                let point = map(point);
                if let Some(from) = current {
                    out.push((from, point));
                }
                current = Some(point);
            }
            PathCommand::CubicTo(c1, c2, end) => {
                let (c1, c2, end) = (map(c1), map(c2), map(end));
                if let Some(from) = current {
                    let hull = distance(from, c1) + distance(c1, c2) + distance(c2, end);
                    let steps = ((hull / 2.0).ceil() as usize).clamp(2, 64);
                    let mut previous = from;
                    for step in 1..=steps {
                        let point = cubic_point(from, c1, c2, end, step as f64 / steps as f64);
                        out.push((previous, point));
                        previous = point;
                    }
                }
                current = Some(end);
            }
            PathCommand::Close => {
                if let (Some(from), Some(to)) = (current, start) {
                    out.push((from, to));
                }
                current = start;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::Rect;

    fn sum(coverage: &[f32]) -> f32 {
        coverage.iter().sum()
    }

    #[test]
    fn axis_aligned_rect_covers_exactly_its_area() {
        let mut raster = Coverage::new(10, 10);
        raster.fill_path(&Path::rect(Rect::new(2.0, 3.0, 4.0, 5.0)), 1.0, 0.0, 0.0);
        let coverage = raster.finish();
        assert!((sum(&coverage) - 20.0).abs() < 1e-3);
        assert_eq!(coverage[3 * 10 + 2], 1.0);
        assert_eq!(coverage[2 * 10 + 2], 0.0);
        assert_eq!(coverage[3 * 10 + 6], 0.0);
    }

    #[test]
    fn fractional_edges_give_partial_coverage() {
        let mut raster = Coverage::new(4, 1);
        raster.fill_path(&Path::rect(Rect::new(0.5, 0.0, 2.0, 1.0)), 1.0, 0.0, 0.0);
        let coverage = raster.finish();
        assert!((coverage[0] - 0.5).abs() < 1e-4 && coverage[1] == 1.0 && (coverage[2] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn opposite_winding_cuts_a_hole() {
        let mut raster = Coverage::new(10, 10);
        let outer = Path::rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        let inner = Path::polygon(&[(3.0, 3.0), (3.0, 7.0), (7.0, 7.0), (7.0, 3.0)]);
        raster.fill_path(&outer.extend(inner), 1.0, 0.0, 0.0);
        let coverage = raster.finish();
        assert!((sum(&coverage) - 84.0).abs() < 1e-3);
        assert_eq!(coverage[5 * 10 + 5], 0.0);
    }

    #[test]
    fn circle_area_matches_pi_r_squared() {
        let mut raster = Coverage::new(40, 40);
        raster.fill_path(&Path::round_rect(Rect::new(5.0, 5.0, 30.0, 30.0), 15.0), 1.0, 0.0, 0.0);
        let area = sum(&raster.finish());
        let expected = std::f32::consts::PI * 225.0;
        assert!((area - expected).abs() / expected < 0.01, "{area} vs {expected}");
    }

    #[test]
    fn shapes_partly_outside_are_clipped() {
        let mut raster = Coverage::new(4, 4);
        raster.fill_path(&Path::rect(Rect::new(-2.0, -2.0, 4.0, 4.0)), 1.0, 0.0, 0.0);
        let coverage = raster.finish();
        assert!((sum(&coverage) - 4.0).abs() < 1e-3);
        raster = Coverage::new(4, 4);
        raster.fill_path(&Path::rect(Rect::new(2.0, 2.0, 10.0, 10.0)), 1.0, 0.0, 0.0);
        assert!((sum(&raster.finish()) - 4.0).abs() < 1e-3);
    }

    #[test]
    fn diagonal_edges_crossing_the_canvas_side_keep_their_area() {
        let mut raster = Coverage::new(10, 10);
        raster.fill_path(&Path::polygon(&[(-10.0, 0.0), (10.0, 0.0), (10.0, 10.0)]), 1.0, 0.0, 0.0);
        let area = sum(&raster.finish());
        assert!((area - 75.0).abs() < 1e-2, "{area}");
    }

    #[test]
    fn stroke_of_a_square_covers_its_border_band() {
        let mut raster = Coverage::new(20, 20);
        raster.stroke_path(&Path::rect(Rect::new(5.0, 5.0, 10.0, 10.0)), 2.0, 1.0, 0.0, 0.0);
        let coverage = raster.finish();
        assert_eq!(coverage[10 * 20 + 10], 0.0, "inside stays empty");
        assert_eq!(coverage[5 * 20 + 10], 1.0, "edge is covered");
        let area = sum(&coverage);
        assert!((area - 80.0).abs() < 1.0, "band area {area}");
    }
}
