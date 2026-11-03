mod art;
mod canvas;
mod term;

pub use art::{COVER_SIZE, Cover};

use std::io::IsTerminal;
use std::time::{Duration, Instant};

use art::Rgb;
use canvas::{Canvas, Rect, truncate};
use term::{ArtMode, Key, ShutdownFlag, Terminal, detect_art_mode, read_key};

const BG: Rgb = (10, 10, 14);
const FG: Rgb = (242, 238, 232);
const DIM: Rgb = (139, 136, 148);
const FAINT: Rgb = (82, 80, 94);
const BORDER: Rgb = (50, 50, 62);
const DEFAULT_ACCENT: Rgb = (255, 150, 106);

#[derive(Clone, Debug, Default)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: String,
    pub format: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub path: String,
    pub track: Track,
    pub pos: f64,
    pub duration: f64,
    pub playing: bool,
    pub volume: f64,
    pub muted: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            path: String::new(),
            track: Track::default(),
            pos: 0.0,
            duration: 0.0,
            playing: false,
            volume: 1.0,
            muted: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Pause,
    Seek(i32),
    Volume(i32),
    Mute,
    Next,
    Previous,
    Restart,
    Quit,
}

pub trait Player {
    fn snapshot(&mut self) -> Snapshot;
    fn control(&mut self, control: Control) -> Result<(), String>;
    fn cover_art(&mut self, path: &str) -> Option<Cover>;
    fn shutdown(&mut self) -> bool;
}

pub fn run(player: &mut dyn Player) -> Result<(), String> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("--tui needs an interactive terminal".to_owned());
    }

    let mode = detect_art_mode();
    let shutdown = ShutdownFlag::install()
        .map_err(|error| format!("cannot install terminal signal handlers: {error}"))?;
    let mut terminal =
        Terminal::enter(mode).map_err(|error| format!("cannot enter the terminal: {error}"))?;
    let mut model = Model::new(mode);
    let mut last_signature = None;

    let outcome = (|| -> Result<(), String> {
        loop {
            if shutdown.requested() {
                let _ = player.control(Control::Quit);
                break;
            }
            let snapshot = player.snapshot();
            let cover_key = (snapshot.path.clone(), snapshot.path.is_empty());
            if model.cover_key.as_ref() != Some(&cover_key) {
                model.cover_key = Some(cover_key);
                model.cover = if snapshot.path.is_empty() {
                    None
                } else {
                    player.cover_art(&snapshot.path)
                };
                model.accent = model
                    .cover
                    .as_ref()
                    .map(Cover::accent)
                    .unwrap_or(DEFAULT_ACCENT);
                model.blocks = None;
                terminal.invalidate_art();
            }
            model.snapshot = snapshot;
            model.expire();

            let (cols, rows) = Terminal::size().map_err(|error| error.to_string())?;
            let (canvas, rect) = model.frame(cols, rows, terminal.mode);
            let signature = model.signature(cols, rows, terminal.mode);
            if last_signature.as_ref() != Some(&signature) {
                terminal
                    .draw(&canvas, model.cover.as_ref(), rect)
                    .map_err(|error| format!("cannot draw the terminal: {error}"))?;
                last_signature = Some(signature);
            }

            match read_key(Duration::from_millis(120)) {
                Ok(Some(Key::Resize)) => {
                    terminal.clear();
                    terminal.invalidate_art();
                    last_signature = None;
                }
                Ok(Some(Key::Quit)) => {
                    let _ = player.control(Control::Quit);
                    break;
                }
                Ok(Some(key)) => {
                    let control = match key {
                        Key::Pause => Control::Pause,
                        Key::Seek(delta) => Control::Seek(delta),
                        Key::Volume(delta) => Control::Volume(delta),
                        Key::Mute => Control::Mute,
                        Key::Next => Control::Next,
                        Key::Previous => Control::Previous,
                        Key::Restart => Control::Restart,
                        Key::Quit | Key::Resize => unreachable!(),
                    };
                    match player.control(control) {
                        Ok(()) => model.hud(control),
                        Err(error) => model.notice = Some((error, Instant::now() + HUD_TIME)),
                    }
                    last_signature = None;
                }
                Ok(None) => {}
                Err(error) => return Err(format!("cannot read terminal input: {error}")),
            }

            if player.shutdown() {
                break;
            }
        }
        Ok(())
    })();

    terminal.leave();
    outcome
}

const HUD_TIME: Duration = Duration::from_millis(1300);

type HalfBlocks = Vec<(Rgb, Rgb)>;

struct Model {
    snapshot: Snapshot,
    cover_key: Option<(String, bool)>,
    cover: Option<Cover>,
    accent: Rgb,
    blocks: Option<(usize, usize, HalfBlocks)>,
    hud: Option<(String, Instant)>,
    notice: Option<(String, Instant)>,
}

impl Model {
    fn new(_mode: ArtMode) -> Self {
        Self {
            snapshot: Snapshot::default(),
            cover_key: None,
            cover: None,
            accent: DEFAULT_ACCENT,
            blocks: None,
            hud: None,
            notice: None,
        }
    }

    fn expire(&mut self) {
        let now = Instant::now();
        if self.hud.as_ref().is_some_and(|(_, until)| *until <= now) {
            self.hud = None;
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, until)| *until <= now)
        {
            self.notice = None;
        }
    }

    fn hud(&mut self, control: Control) {
        let text = match control {
            Control::Pause => {
                if self.snapshot.playing {
                    "PAUSED"
                } else {
                    "PLAYING"
                }
                .to_owned()
            }
            Control::Seek(delta) => {
                let target = (self.snapshot.pos + delta as f64).clamp(0.0, self.snapshot.duration);
                format!("SEEK  {}", clock(target))
            }
            Control::Volume(delta) => {
                let target = (self.snapshot.volume + delta as f64 / 100.0).clamp(0.0, 1.0);
                format!("VOL  {}%", (target * 100.0).round() as i32)
            }
            Control::Mute => {
                if self.snapshot.muted {
                    "SOUND ON"
                } else {
                    "MUTED"
                }
                .to_owned()
            }
            Control::Next | Control::Previous => "TRACK".to_owned(),
            Control::Restart => "RESTART".to_owned(),
            Control::Quit => return,
        };
        self.hud = Some((text, Instant::now() + HUD_TIME));
    }

    fn blocks(&mut self, width: usize, height: usize) -> Vec<(Rgb, Rgb)> {
        if let Some((w, h, blocks)) = &self.blocks
            && *w == width
            && *h == height
        {
            return blocks.clone();
        }
        let blocks = self
            .cover
            .as_ref()
            .map(|cover| cover.half_blocks(width, height))
            .unwrap_or_default();
        self.blocks = Some((width, height, blocks.clone()));
        blocks
    }

    fn signature(&self, cols: usize, rows: usize, mode: ArtMode) -> Signature {
        let hud = self
            .hud
            .as_ref()
            .map(|(text, _)| text.clone())
            .or_else(|| self.notice.as_ref().map(|(text, _)| text.clone()));
        Signature {
            cols,
            rows,
            mode,
            title: self.snapshot.track.title.clone(),
            artist: self.snapshot.track.artist.clone(),
            album: self.snapshot.track.album.clone(),
            year: self.snapshot.track.year.clone(),
            format: self.snapshot.track.format.clone(),
            path: self.snapshot.path.clone(),
            playing: self.snapshot.playing,
            muted: self.snapshot.muted,
            pos: (self.snapshot.pos * 10.0).round() as i64,
            duration: (self.snapshot.duration * 10.0).round() as i64,
            volume: (self.snapshot.volume * 100.0).round() as i64,
            accent: self.accent,
            cover: self.cover.is_some(),
            hud,
        }
    }

    fn frame(&mut self, cols: usize, rows: usize, mode: ArtMode) -> (Canvas, Rect) {
        let mut canvas = Canvas::new(cols, rows, BG);
        if cols < 30 || rows < 16 {
            canvas.center(rows / 2, "TERMINAL TOO SMALL", self.accent, BG, true);
            if rows / 2 + 1 < rows {
                canvas.center(rows / 2 + 1, "NEED 30 × 12", FAINT, BG, false);
            }
            return (canvas, Rect { x: 0, y: 0, w: 0, h: 0 });
        }

        let art_w = 52usize
            .min(cols.saturating_sub(20))
            .min(rows.saturating_sub(13).saturating_mul(2))
            .max(16)
            .min(cols);
        let art_h = (art_w / 2).max(4).min(rows);
        let block_h = art_h + 8;
        let top = rows.saturating_sub(block_h) / 2;
        let art_x = cols.saturating_sub(art_w) / 2;
        let rect = Rect {
            x: art_x,
            y: top,
            w: art_w,
            h: art_h,
        };

        self.draw_art(&mut canvas, rect, mode);

        if top >= 2 {
            canvas.put(2, 0, "◆", self.accent, BG, false);
            canvas.put(4, 0, "PLAYER", FAINT, BG, false);
            let format = if self.snapshot.track.format.is_empty() {
                "LOCAL FILE".to_owned()
            } else {
                format!("LOCAL · {}", self.snapshot.track.format.to_uppercase())
            };
            canvas.put_right(cols.saturating_sub(2), 0, &format, FAINT, BG);
        }

        let title_y = top + art_h + 1;
        let glyph = if self.snapshot.playing { "▶" } else { "‖" };
        let title = if self.snapshot.track.title.is_empty() {
            "NO TITLE"
        } else {
            &self.snapshot.track.title
        };
        let room = cols.saturating_sub(6);
        let title = truncate(title, room);
        let label_width = 1 + 2 + title.chars().map(char_width).sum::<usize>();
        let label_x = cols.saturating_sub(label_width) / 2;
        canvas.put(label_x, title_y, glyph, self.accent, BG, true);
        canvas.put(label_x + 3, title_y, &title, FG, BG, true);

        let meta_y = title_y + 1;
        center_segments(
            &mut canvas,
            meta_y,
            &[
                (self.snapshot.track.artist.as_str(), DIM),
                ("  ·  ", FAINT),
                (self.snapshot.track.album.as_str(), FAINT),
                ("  ·  ", FAINT),
                (self.snapshot.track.year.as_str(), FAINT),
            ],
            BG,
        );

        let bar_y = title_y + 3;
        draw_bar(
            &mut canvas,
            art_x,
            bar_y,
            art_w,
            if self.snapshot.duration > 0.0 {
                self.snapshot.pos / self.snapshot.duration
            } else {
                0.0
            },
            self.accent,
        );

        let times_y = bar_y + 1;
        let duration = if self.snapshot.duration > 0.0 {
            clock(self.snapshot.duration)
        } else {
            "--:--".to_owned()
        };
        canvas.put(art_x, times_y, &clock(self.snapshot.pos), FAINT, BG, false);
        canvas.put_right(art_x + art_w, times_y, &duration, FAINT, BG);

        let hint_y = times_y + 2;
        let status = self
            .notice
            .as_ref()
            .map(|(text, _)| text.as_str())
            .or_else(|| self.hud.as_ref().map(|(text, _)| text.as_str()));
        if let Some(status) = status {
            canvas.center(hint_y, status, self.accent, BG, false);
        } else {
            canvas.center(hint_y, hints(cols >= 76), FAINT, BG, false);
        }

        (canvas, rect)
    }

    fn draw_art(&mut self, canvas: &mut Canvas, rect: Rect, mode: ArtMode) {
        if self.cover.is_none() {
            canvas.fill(rect.x, rect.y, rect.w, rect.h, BG);
            if rect.w >= 6 && rect.h >= 3 {
                canvas.box_plain(rect, BORDER, BG);
                if rect.w >= 14 {
                    let x = rect.x + rect.w.saturating_sub(10) / 2;
                    let y = rect.y + rect.h / 2;
                    canvas.put(x, y, "NO ARTWORK", DIM, BG, false);
                }
            }
            return;
        }
        if mode == ArtMode::Blocks {
            let blocks = self.blocks(rect.w, rect.h);
            for row in 0..rect.h {
                for col in 0..rect.w {
                    if let Some((top, bottom)) = blocks.get(row * rect.w + col) {
                        canvas.half_cell(rect.x + col, rect.y + row, *top, *bottom);
                    }
                }
            }
        }
    }
}

#[derive(PartialEq, Eq)]
struct Signature {
    cols: usize,
    rows: usize,
    mode: ArtMode,
    title: String,
    artist: String,
    album: String,
    year: String,
    format: String,
    path: String,
    playing: bool,
    muted: bool,
    pos: i64,
    duration: i64,
    volume: i64,
    accent: Rgb,
    cover: bool,
    hud: Option<String>,
}

fn center_segments(canvas: &mut Canvas, y: usize, segments: &[(&str, Rgb)], bg: Rgb) {
    let mut cleaned: Vec<(&str, Rgb)> = Vec::new();
    let mut pending_separator: Option<(&str, Rgb)> = None;
    for (index, (text, color)) in segments.iter().enumerate() {
        if index % 2 == 1 {
            pending_separator = Some((text, *color));
            continue;
        }
        if text.is_empty() {
            continue;
        }
        if !cleaned.is_empty()
            && let Some(separator) = pending_separator.take()
        {
            cleaned.push(separator);
        }
        cleaned.push((text, *color));
        pending_separator = None;
    }
    let total: usize = cleaned
        .iter()
        .map(|(text, _)| text.chars().map(char_width).sum::<usize>())
        .sum();
    if total > canvas.cols {
        let joined = cleaned
            .iter()
            .map(|(text, _)| *text)
            .collect::<String>();
        canvas.center(y, &truncate(&joined, canvas.cols), DIM, bg, false);
        return;
    }
    let mut x = (canvas.cols - total) / 2;
    for (text, color) in cleaned {
        x = canvas.put(x, y, text, color, bg, false);
    }
}

fn draw_bar(canvas: &mut Canvas, x: usize, y: usize, width: usize, fraction: f64, accent: Rgb) {
    if width < 3 {
        return;
    }
    let fraction = fraction.clamp(0.0, 1.0);
    let filled = (fraction * width as f64).round() as usize;
    for column in 0..width {
        let ch = if column < filled { '━' } else { '─' };
        let color = if column < filled { accent } else { FAINT };
        canvas.put(x + column, y, &ch.to_string(), color, BG, false);
    }
    let knob = filled.min(width - 1);
    canvas.put(x + knob, y, "●", accent, BG, false);
}

fn hints(wide: bool) -> &'static str {
    if wide {
        "space pause   ←/→ seek   +/− volume   m mute   n/p track   q quit"
    } else {
        "space pause  ·  ←→ seek  ·  +− vol  ·  n/p track  ·  q quit"
    }
}

fn clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--:--".to_owned();
    }
    let seconds = seconds.round() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn char_width(ch: char) -> usize {
    use unicode_width::UnicodeWidthChar;
    ch.width().unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(title: &str) -> Snapshot {
        Snapshot {
            path: "/music/a.flac".to_owned(),
            track: Track {
                title: title.to_owned(),
                artist: "Mira Vale".to_owned(),
                album: "Low Light".to_owned(),
                year: "2026".to_owned(),
                format: "flac".to_owned(),
            },
            pos: 102.0,
            duration: 238.0,
            playing: true,
            volume: 0.42,
            muted: false,
        }
    }

    fn model(title: &str) -> Model {
        let mut model = Model::new(ArtMode::Blocks);
        model.snapshot = snapshot(title);
        model
    }

    #[test]
    fn layout_matches_the_mock_at_136_by_39() {
        let mut model = model("Paper Satellites");
        let (canvas, rect) = model.frame(136, 39, ArtMode::Blocks);
        assert_eq!((rect.w, rect.h, rect.x, rect.y), (52, 26, 42, 2));
        assert!(canvas.render(None).contains("Paper Satellites"));
        assert!(canvas.render(None).contains("LOCAL · FLAC"));
        assert!(canvas.render(None).contains("space pause"));
    }

    #[test]
    fn tiny_terminals_get_a_message_instead_of_a_broken_frame() {
        let mut model = model("Paper Satellites");
        let (canvas, rect) = model.frame(24, 8, ArtMode::Blocks);
        assert_eq!(rect.w, 0);
        assert!(canvas.render(None).contains("TERMINAL TOO SMALL"));
    }

    #[test]
    fn clock_formats_and_rejects_unknown_durations() {
        assert_eq!(clock(102.0), "1:42");
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(f64::NAN), "--:--");
        assert_eq!(clock(-1.0), "--:--");
    }

    #[test]
    fn long_titles_are_truncated_to_the_screen() {
        let mut model = model(&"X".repeat(200));
        let (canvas, _) = model.frame(80, 24, ArtMode::Blocks);
        assert!(canvas.render(None).contains('…'));
    }

    #[test]
    fn hud_expires() {
        let mut model = model("Paper Satellites");
        model.hud(Control::Volume(5));
        assert!(model.hud.is_some());
        model.hud.as_mut().unwrap().1 = Instant::now() - Duration::from_millis(1);
        model.expire();
        assert!(model.hud.is_none());
    }
}
