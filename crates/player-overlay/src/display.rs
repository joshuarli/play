#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    pub fn scaled(&self, scale: f64) -> Rect {
        Rect::new(self.x * scale, self.y * scale, self.w * scale, self.h * scale)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Color = Color::rgba(255, 255, 255, 255);
    pub const BLACK: Color = Color::rgba(0, 0, 0, 255);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    pub fn with_opacity(self, opacity: f64) -> Color {
        let a = (self.a as f64 * opacity.clamp(0.0, 1.0)).round() as u8;
        Color { a, ..self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

impl Align {
    pub fn offset(self, extent: f64) -> f64 {
        match self {
            Align::Start => 0.0,
            Align::Center => -extent / 2.0,
            Align::End => -extent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    CubicTo(Point, Point, Point),
    Close,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    pub commands: Vec<PathCommand>,
}

impl Path {
    pub fn new() -> Self {
        Path::default()
    }

    pub fn move_to(mut self, x: f64, y: f64) -> Self {
        self.commands.push(PathCommand::MoveTo(Point { x, y }));
        self
    }

    pub fn line_to(mut self, x: f64, y: f64) -> Self {
        self.commands.push(PathCommand::LineTo(Point { x, y }));
        self
    }

    pub fn cubic_to(mut self, c1: (f64, f64), c2: (f64, f64), end: (f64, f64)) -> Self {
        self.commands.push(PathCommand::CubicTo(
            Point { x: c1.0, y: c1.1 },
            Point { x: c2.0, y: c2.1 },
            Point { x: end.0, y: end.1 },
        ));
        self
    }

    pub fn close(mut self) -> Self {
        self.commands.push(PathCommand::Close);
        self
    }

    pub fn polygon(points: &[(f64, f64)]) -> Self {
        let mut path = Path::new();
        for (index, &(x, y)) in points.iter().enumerate() {
            path = if index == 0 { path.move_to(x, y) } else { path.line_to(x, y) };
        }
        path.close()
    }

    pub fn rect(rect: Rect) -> Self {
        Path::polygon(&[
            (rect.x, rect.y),
            (rect.right(), rect.y),
            (rect.right(), rect.bottom()),
            (rect.x, rect.bottom()),
        ])
    }

    pub fn round_rect(rect: Rect, radius: f64) -> Self {
        let r = radius.min(rect.w / 2.0).min(rect.h / 2.0).max(0.0);
        if r == 0.0 {
            return Path::rect(rect);
        }
        let k = 0.552_284_749_831 * r;
        let (x0, y0, x1, y1) = (rect.x, rect.y, rect.right(), rect.bottom());
        Path::new()
            .move_to(x0 + r, y0)
            .line_to(x1 - r, y0)
            .cubic_to((x1 - r + k, y0), (x1, y0 + r - k), (x1, y0 + r))
            .line_to(x1, y1 - r)
            .cubic_to((x1, y1 - r + k), (x1 - r + k, y1), (x1 - r, y1))
            .line_to(x0 + r, y1)
            .cubic_to((x0 + r - k, y1), (x0, y1 - r + k), (x0, y1 - r))
            .line_to(x0, y0 + r)
            .cubic_to((x0, y0 + r - k), (x0 + r - k, y0), (x0 + r, y0))
            .close()
    }

    pub fn extend(mut self, other: Path) -> Self {
        self.commands.extend(other.commands);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Rect(Rect),
    RoundRect { rect: Rect, radius: f64 },
    Path(Path),
}

impl Shape {
    pub fn to_path(&self) -> Path {
        match self {
            Shape::Rect(rect) => Path::rect(*rect),
            Shape::RoundRect { rect, radius } => Path::round_rect(*rect, *radius),
            Shape::Path(path) => path.clone(),
        }
    }
}

/// Which point of a text box sits at its origin: `x` picks left, centre,
/// or right, and `y` top, middle, or bottom of the line boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub x: Align,
    pub y: Align,
}

impl Anchor {
    pub const TOP_LEFT: Anchor = Anchor { x: Align::Start, y: Align::Start };
    pub const CENTER: Anchor = Anchor { x: Align::Center, y: Align::Center };
    pub const BOTTOM_CENTER: Anchor = Anchor { x: Align::Center, y: Align::End };
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Fill {
        shape: Shape,
        color: Color,
        clip: Option<Rect>,
    },
    Stroke {
        shape: Shape,
        width: f64,
        color: Color,
        clip: Option<Rect>,
    },
    /// Text whose size, outline, and wrap width are in logical units.
    Text {
        origin: Point,
        anchor: Anchor,
        block: crate::text::TextBlock,
        clip: Option<Rect>,
    },
    Image {
        origin: Point,
        image: std::sync::Arc<crate::compose::Canvas>,
        opacity: f64,
        clip: Option<Rect>,
    },
}

/// A frame of overlay content in logical units. `scale` maps logical units
/// to output pixels, and items paint in order.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayList {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub items: Vec<Item>,
}

impl DisplayList {
    pub fn new(width: u32, height: u32, scale: f64) -> Self {
        DisplayList { width, height, scale, items: Vec::new() }
    }

    pub fn fill(&mut self, shape: Shape, color: Color) {
        self.items.push(Item::Fill { shape, color, clip: None });
    }

    pub fn stroke(&mut self, shape: Shape, width: f64, color: Color) {
        self.items.push(Item::Stroke { shape, width, color, clip: None });
    }

    pub fn text(&mut self, origin: Point, anchor: Anchor, block: crate::text::TextBlock) {
        self.items.push(Item::Text { origin, anchor, block, clip: None });
    }

    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }
}
