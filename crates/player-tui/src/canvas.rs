use std::fmt::Write as _;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::art::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub half: Option<(Rgb, Rgb)>,
    pub cont: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

pub struct Canvas {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Cell>,
}

impl Canvas {
    pub fn new(cols: usize, rows: usize, bg: Rgb) -> Self {
        Self {
            cols,
            rows,
            cells: vec![
                Cell {
                    ch: ' ',
                    fg: (242, 238, 232),
                    bg,
                    bold: false,
                    half: None,
                    cont: false,
                };
                cols * rows
            ],
        }
    }

    fn index(&self, x: usize, y: usize) -> Option<usize> {
        if x < self.cols && y < self.rows {
            Some(y * self.cols + x)
        } else {
            None
        }
    }

    pub fn set(&mut self, x: usize, y: usize, ch: char, fg: Rgb, bg: Rgb, bold: bool) {
        let width = ch.width().unwrap_or(1).max(1);
        if width > 2 {
            return;
        }
        if width == 2 {
            if x + 1 >= self.cols {
                return;
            }
            if let Some(index) = self.index(x, y) {
                self.cells[index] = Cell {
                    ch,
                    fg,
                    bg,
                    bold,
                    half: None,
                    cont: false,
                };
                self.cells[index + 1] = Cell {
                    ch: ' ',
                    fg,
                    bg,
                    bold,
                    half: None,
                    cont: true,
                };
            }
        } else if let Some(index) = self.index(x, y) {
            self.cells[index] = Cell {
                ch,
                fg,
                bg,
                bold,
                half: None,
                cont: false,
            };
        }
    }

    pub fn put(&mut self, x: usize, y: usize, text: &str, fg: Rgb, bg: Rgb, bold: bool) -> usize {
        let mut cursor = x;
        for ch in text.chars() {
            if ch == '\n' {
                break;
            }
            let ch = if ch.is_control() { ' ' } else { ch };
            let width = ch.width().unwrap_or(1).max(1);
            if cursor + width > self.cols {
                break;
            }
            self.set(cursor, y, ch, fg, bg, bold);
            cursor += width;
        }
        cursor
    }

    pub fn center(&mut self, y: usize, text: &str, fg: Rgb, bg: Rgb, bold: bool) {
        let width = text.width();
        if width > self.cols {
            let text = truncate(text, self.cols);
            let x = (self.cols - text.width()) / 2;
            self.put(x, y, &text, fg, bg, bold);
        } else {
            self.put((self.cols - width) / 2, y, text, fg, bg, bold);
        }
    }

    pub fn put_right(&mut self, right: usize, y: usize, text: &str, fg: Rgb, bg: Rgb) {
        let width = text.width();
        if width >= right {
            self.put(0, y, &truncate(text, right), fg, bg, false);
        } else {
            self.put(right - width, y, text, fg, bg, false);
        }
    }

    pub fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, bg: Rgb) {
        for row in y..(y + h).min(self.rows) {
            for col in x..(x + w).min(self.cols) {
                if let Some(index) = self.index(col, row) {
                    self.cells[index] = Cell {
                        ch: ' ',
                        fg: (242, 238, 232),
                        bg,
                        bold: false,
                        half: None,
                        cont: false,
                    };
                }
            }
        }
    }

    pub fn hline(&mut self, x: usize, y: usize, w: usize, ch: char, fg: Rgb, bg: Rgb) {
        for col in x..(x + w).min(self.cols) {
            self.set(col, y, ch, fg, bg, false);
        }
    }

    pub fn box_plain(&mut self, rect: Rect, fg: Rgb, bg: Rgb) {
        self.fill(rect.x, rect.y, rect.w, rect.h, bg);
        if rect.w < 2 || rect.h < 2 {
            return;
        }
        self.hline(rect.x + 1, rect.y, rect.w - 2, '─', fg, bg);
        self.hline(rect.x + 1, rect.y + rect.h - 1, rect.w - 2, '─', fg, bg);
        for y in rect.y + 1..rect.y + rect.h - 1 {
            self.set(rect.x, y, '│', fg, bg, false);
            self.set(rect.x + rect.w - 1, y, '│', fg, bg, false);
        }
        self.set(rect.x, rect.y, '┌', fg, bg, false);
        self.set(rect.x + rect.w - 1, rect.y, '┐', fg, bg, false);
        self.set(rect.x, rect.y + rect.h - 1, '└', fg, bg, false);
        self.set(rect.x + rect.w - 1, rect.y + rect.h - 1, '┘', fg, bg, false);
    }

    pub fn half_cell(&mut self, x: usize, y: usize, top: Rgb, bottom: Rgb) {
        if let Some(index) = self.index(x, y) {
            self.cells[index] = Cell {
                ch: '▀',
                fg: top,
                bg: bottom,
                bold: false,
                half: Some((top, bottom)),
                cont: false,
            };
        }
    }

    pub fn render(&self, skip: Option<Rect>) -> String {
        let mut out = String::with_capacity(self.cols * self.rows * 4);
        out.push_str("\x1b[?2026h");
        for y in 0..self.rows {
            let _ = write!(out, "\x1b[{};1H", y + 1);
            let mut current: Option<(Rgb, Rgb, bool)> = None;
            for x in 0..self.cols {
                if let Some(rect) = skip
                    && y >= rect.y
                    && y < rect.y + rect.h
                    && x >= rect.x
                    && x < rect.x + rect.w
                {
                    if x == rect.x && rect.x + rect.w < self.cols {
                        let _ = write!(out, "\x1b[{};{}H", y + 1, rect.x + rect.w + 1);
                    }
                    continue;
                }
                let cell = &self.cells[y * self.cols + x];
                if cell.cont {
                    continue;
                }
                let style = (cell.fg, cell.bg, cell.bold);
                if current != Some(style) {
                    out.push_str("\x1b[0");
                    if cell.bold {
                        out.push_str(";1");
                    }
                    let _ = write!(
                        out,
                        ";38;2;{};{};{};48;2;{};{};{}m",
                        cell.fg.0, cell.fg.1, cell.fg.2, cell.bg.0, cell.bg.1, cell.bg.2
                    );
                    current = Some(style);
                }
                out.push(cell.ch);
            }
            out.push_str("\x1b[0m");
        }
        out.push_str("\x1b[?2026l");
        out
    }
}

pub fn truncate(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut width = 0;
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(1).max(1);
        if width + ch_width + 1 > max {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_characters_occupy_two_cells() {
        let mut canvas = Canvas::new(10, 2, (0, 0, 0));
        canvas.put(1, 0, "日本語", (255, 255, 255), (0, 0, 0), false);
        assert_eq!(canvas.cells[1].ch, '日');
        assert!(canvas.cells[2].cont);
        assert_eq!(canvas.cells[3].ch, '本');
        assert!(canvas.cells[4].cont);
    }

    #[test]
    fn wide_characters_do_not_wrap_at_the_edge() {
        let mut canvas = Canvas::new(3, 1, (0, 0, 0));
        canvas.put(2, 0, "日", (255, 255, 255), (0, 0, 0), false);
        assert_eq!(canvas.cells[2].ch, ' ');
    }

    #[test]
    fn truncation_uses_display_width() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("日本語", 5), "日本…");
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    fn control_characters_cannot_reach_the_terminal() {
        let mut canvas = Canvas::new(12, 1, (0, 0, 0));
        canvas.put(0, 0, "ok\x1b[31mred\x07", (255, 255, 255), (0, 0, 0), false);
        assert_eq!(canvas.cells[2].ch, ' ');
        assert_eq!(canvas.cells[3].ch, '[');
        assert_eq!(canvas.cells[10].ch, ' ');
        let rendered = canvas.render(None);
        assert!(!rendered.contains("ok\x1b[31mred"));
    }

    #[test]
    fn render_leaves_a_skipped_rect_untouched() {
        let mut canvas = Canvas::new(10, 3, (0, 0, 0));
        canvas.put(0, 1, "abcdefghij", (255, 255, 255), (0, 0, 0), false);
        let rendered = canvas.render(Some(Rect { x: 3, y: 1, w: 4, h: 1 }));
        assert!(rendered.contains("abc"));
        assert!(rendered.contains("\x1b[2;8H"));
        assert!(rendered.contains("hij"));
        assert!(!rendered.contains('d'));
        assert!(!rendered.contains('e'));
    }

    #[test]
    fn render_skips_continuation_cells() {
        let mut canvas = Canvas::new(4, 1, (0, 0, 0));
        canvas.put(0, 0, "日本", (255, 255, 255), (0, 0, 0), false);
        let rendered = canvas.render(None);
        assert!(rendered.contains('日'));
        assert!(!rendered.contains("日 日"));
    }
}
