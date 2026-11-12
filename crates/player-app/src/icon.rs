use player_overlay::{Color, DisplayList, Path, Point, Rect, Shape};

/// The OSC's vector icons, drawn on a 24-unit grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Menu,
    PlaylistPrev,
    PlaylistNext,
    Play,
    Pause,
    ChapterPrev,
    ChapterNext,
    Volume(VolumeLevel),
    Mute,
    Audio,
    Subtitle,
    Fullscreen,
    ExitFullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VolumeLevel {
    Low,
    Medium,
    High,
    Boosted,
}

const GRID: f64 = 24.0;
const STROKE: f64 = 2.0;

/// Maps grid coordinates into a `size`-unit box centred on `center`.
struct Grid {
    left: f64,
    top: f64,
    unit: f64,
}

impl Grid {
    fn at(&self, x: f64, y: f64) -> (f64, f64) {
        (self.left + x * self.unit, self.top + y * self.unit)
    }

    fn polygon(&self, points: &[(f64, f64)]) -> Shape {
        let mapped: Vec<(f64, f64)> = points.iter().map(|&(x, y)| self.at(x, y)).collect();
        Shape::Path(Path::polygon(&mapped))
    }

    fn polyline(&self, points: &[(f64, f64)]) -> Shape {
        let mut path = Path::new();
        for (index, &(x, y)) in points.iter().enumerate() {
            let (x, y) = self.at(x, y);
            path = if index == 0 { path.move_to(x, y) } else { path.line_to(x, y) };
        }
        Shape::Path(path)
    }

    fn rect(&self, x: f64, y: f64, w: f64, h: f64) -> Shape {
        let (x, y) = self.at(x, y);
        Shape::Rect(Rect::new(x, y, w * self.unit, h * self.unit))
    }

    fn round_rect(&self, x: f64, y: f64, w: f64, h: f64, radius: f64) -> Shape {
        let (x, y) = self.at(x, y);
        Shape::RoundRect { rect: Rect::new(x, y, w * self.unit, h * self.unit), radius: radius * self.unit }
    }

    fn circle(&self, cx: f64, cy: f64, radius: f64) -> Shape {
        self.round_rect(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius, radius)
    }

    /// An open arc around `(cx, cy)` from `from` to `to` radians, clockwise
    /// on screen, in cubic segments of at most a quarter turn.
    fn arc(&self, cx: f64, cy: f64, radius: f64, from: f64, to: f64) -> Shape {
        let segments = ((to - from).abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
        let step = (to - from) / segments as f64;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let point = |angle: f64| (cx + radius * angle.cos(), cy + radius * angle.sin());
        let (x, y) = self.at(point(from).0, point(from).1);
        let mut path = Path::new().move_to(x, y);
        for index in 0..segments {
            let a0 = from + step * index as f64;
            let a1 = a0 + step;
            let (p0, p1) = (point(a0), point(a1));
            let c1 = (p0.0 - k * radius * a0.sin(), p0.1 + k * radius * a0.cos());
            let c2 = (p1.0 + k * radius * a1.sin(), p1.1 - k * radius * a1.cos());
            path = path.cubic_to(self.at(c1.0, c1.1), self.at(c2.0, c2.1), self.at(p1.0, p1.1));
        }
        Shape::Path(path)
    }
}

enum Mark {
    Fill(Shape),
    Stroke(Shape),
}

fn speaker(grid: &Grid) -> Mark {
    Mark::Fill(grid.polygon(&[(3.0, 9.0), (7.0, 9.0), (12.0, 4.0), (12.0, 20.0), (7.0, 15.0), (3.0, 15.0)]))
}

fn marks(icon: Icon, grid: &Grid) -> Vec<Mark> {
    use Mark::{Fill, Stroke};
    let swing = 0.9;
    match icon {
        Icon::Menu => vec![
            Fill(grid.rect(4.0, 5.0, 16.0, 2.5)),
            Fill(grid.rect(4.0, 10.75, 16.0, 2.5)),
            Fill(grid.rect(4.0, 16.5, 16.0, 2.5)),
        ],
        Icon::Play => vec![Fill(grid.polygon(&[(7.0, 4.0), (20.0, 12.0), (7.0, 20.0)]))],
        Icon::Pause => vec![Fill(grid.rect(6.0, 5.0, 4.0, 14.0)), Fill(grid.rect(14.0, 5.0, 4.0, 14.0))],
        Icon::PlaylistPrev => vec![
            Fill(grid.rect(5.0, 6.0, 2.5, 12.0)),
            Fill(grid.polygon(&[(19.0, 6.0), (8.5, 12.0), (19.0, 18.0)])),
        ],
        Icon::PlaylistNext => vec![
            Fill(grid.polygon(&[(5.0, 6.0), (15.5, 12.0), (5.0, 18.0)])),
            Fill(grid.rect(16.5, 6.0, 2.5, 12.0)),
        ],
        Icon::ChapterPrev => vec![
            Fill(grid.polygon(&[(12.0, 6.0), (2.0, 12.0), (12.0, 18.0)])),
            Fill(grid.polygon(&[(22.0, 6.0), (12.0, 12.0), (22.0, 18.0)])),
        ],
        Icon::ChapterNext => vec![
            Fill(grid.polygon(&[(2.0, 6.0), (12.0, 12.0), (2.0, 18.0)])),
            Fill(grid.polygon(&[(12.0, 6.0), (22.0, 12.0), (12.0, 18.0)])),
        ],
        Icon::Volume(level) => {
            let mut marks = vec![speaker(grid)];
            let waves: &[f64] = match level {
                VolumeLevel::Low => &[],
                VolumeLevel::Medium => &[4.0],
                VolumeLevel::High => &[4.0, 7.5],
                VolumeLevel::Boosted => &[4.0, 7.5, 11.0],
            };
            for radius in waves {
                marks.push(Stroke(grid.arc(12.0, 12.0, *radius, -swing, swing)));
            }
            marks
        }
        Icon::Mute => vec![
            speaker(grid),
            Stroke(grid.polyline(&[(15.0, 9.0), (21.0, 15.0)])),
            Stroke(grid.polyline(&[(21.0, 9.0), (15.0, 15.0)])),
        ],
        Icon::Audio => vec![
            Fill(grid.circle(8.0, 17.0, 3.5)),
            Fill(grid.rect(10.0, 4.0, 1.5, 13.0)),
            Fill(grid.polygon(&[(11.5, 4.0), (18.0, 6.5), (18.0, 9.5), (11.5, 7.0)])),
        ],
        Icon::Subtitle => vec![
            Stroke(grid.round_rect(3.0, 5.0, 18.0, 14.0, 2.5)),
            Fill(grid.rect(6.0, 13.0, 5.0, 2.0)),
            Fill(grid.rect(13.0, 13.0, 5.0, 2.0)),
            Fill(grid.rect(6.0, 9.0, 12.0, 2.0)),
        ],
        Icon::Fullscreen => vec![
            Stroke(grid.polyline(&[(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)])),
            Stroke(grid.polyline(&[(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)])),
            Stroke(grid.polyline(&[(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)])),
            Stroke(grid.polyline(&[(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)])),
        ],
        Icon::ExitFullscreen => vec![
            Stroke(grid.polyline(&[(4.0, 9.0), (9.0, 9.0), (9.0, 4.0)])),
            Stroke(grid.polyline(&[(15.0, 4.0), (15.0, 9.0), (20.0, 9.0)])),
            Stroke(grid.polyline(&[(20.0, 15.0), (15.0, 15.0), (15.0, 20.0)])),
            Stroke(grid.polyline(&[(9.0, 20.0), (9.0, 15.0), (4.0, 15.0)])),
        ],
    }
}

/// Draws `icon` in a `size`-unit square centred on `center`.
pub fn draw(list: &mut DisplayList, icon: Icon, center: Point, size: f64, color: Color) {
    let unit = size / GRID;
    let grid = Grid { left: center.x - size / 2.0, top: center.y - size / 2.0, unit };
    for mark in marks(icon, &grid) {
        match mark {
            Mark::Fill(shape) => list.fill(shape, color),
            Mark::Stroke(shape) => list.stroke(shape, STROKE * unit, color),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 16] = [
        Icon::Menu,
        Icon::PlaylistPrev,
        Icon::PlaylistNext,
        Icon::Play,
        Icon::Pause,
        Icon::ChapterPrev,
        Icon::ChapterNext,
        Icon::Volume(VolumeLevel::Low),
        Icon::Volume(VolumeLevel::Medium),
        Icon::Volume(VolumeLevel::High),
        Icon::Volume(VolumeLevel::Boosted),
        Icon::Mute,
        Icon::Audio,
        Icon::Subtitle,
        Icon::Fullscreen,
        Icon::ExitFullscreen,
    ];

    fn rendered(icon: Icon) -> player_overlay::OverlayImage {
        let mut list = DisplayList::new(48, 48, 2.0);
        draw(&mut list, icon, Point { x: 12.0, y: 12.0 }, 24.0, Color::WHITE);
        let mut text = player_overlay::TextEngine::new().unwrap();
        player_overlay::render(&[&list], &mut text).unwrap().expect("icon paints")
    }

    #[test]
    fn every_icon_paints_inside_its_box_and_icons_are_distinct() {
        let images: Vec<_> = ALL.iter().map(|icon| rendered(*icon)).collect();
        for (icon, image) in ALL.iter().zip(&images) {
            assert!(image.x + image.width <= 48 && image.y + image.height <= 48, "{icon:?} {image:?}");
            let inked = image.rgba.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] > 128).count();
            assert!(inked > 40, "{icon:?} paints {inked} pixels");
        }
        for (a, first) in images.iter().enumerate() {
            for (b, second) in images.iter().enumerate().skip(a + 1) {
                assert_ne!(first, second, "{:?} and {:?} look the same", ALL[a], ALL[b]);
            }
        }
    }

    #[test]
    fn louder_levels_draw_more_waves() {
        let ink = |level| {
            rendered(Icon::Volume(level)).rgba.as_chunks::<4>().0.iter().map(|pixel| pixel[3] as u32).sum::<u32>()
        };
        assert!(ink(VolumeLevel::Low) < ink(VolumeLevel::Medium));
        assert!(ink(VolumeLevel::Medium) < ink(VolumeLevel::High));
        assert!(ink(VolumeLevel::High) < ink(VolumeLevel::Boosted));
    }
}
