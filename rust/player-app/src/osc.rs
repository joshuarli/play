use std::fmt::Write as _;
use std::time::{Duration, Instant};

use player_backend::AssOverlay;
use player_core::command::{
    Adjust, Command, OscVisibility, SeekMode, Toggle, TrackKind,
};

const HIDE_TIMEOUT: Duration = Duration::from_millis(500);
const FADE_DURATION: Duration = Duration::from_millis(200);
const VOLUME_STEP: f64 = 5.0;
const WHEEL_SEEK_SECONDS: f64 = 5.0;
const ICON_FONT: &str = "mpv-osd-symbols";
const BAR_ALPHA: u8 = 0x70;
const BAR_RADIUS: f64 = 8.0;
const CIRCLE_HANDLE: f64 = 0.551_915_024_494;

mod icon {
    pub const PLAY: &str = "\u{e101}";
    pub const PAUSE: &str = "\u{e002}";
    pub const VOLUME: [&str; 4] = ["\u{e10b}", "\u{e10c}", "\u{e10d}", "\u{e10e}"];
    pub const MUTE: &str = "\u{e10a}";
    pub const AUDIO: &str = "\u{e106}";
    pub const SUBTITLE: &str = "\u{e107}";
    pub const FULLSCREEN: &str = "\u{e108}";
    pub const EXIT_FULLSCREEN: &str = "\u{e109}";
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    fn fraction_at(&self, x: f64) -> f64 {
        if self.w <= 0.0 {
            return 0.0;
        }
        ((x - self.x) / self.w).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Playback {
    pub paused: bool,
    pub fullscreen: bool,
    pub muted: bool,
    pub volume: f64,
    pub position: f64,
    pub duration: f64,
    pub audio_tracks: usize,
    pub sub_tracks: usize,
    pub aid: Option<i64>,
    pub sid: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LayoutKey {
    width: u32,
    height: u32,
    audio_control: bool,
    sub_control: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    key: LayoutKey,
    scale: f64,
    pub bar: Rect,
    pub play: Rect,
    pub elapsed: Rect,
    pub seek: Rect,
    pub duration: Rect,
    pub volume: Rect,
    pub audio: Option<Rect>,
    pub sub: Option<Rect>,
    pub fullscreen: Rect,
}

impl Layout {
    fn new(key: LayoutKey) -> Self {
        let width = key.width as f64;
        let height = key.height as f64;
        let scale = (height / 720.0).max(1.0);
        let pad = (12.0 * scale).floor();
        let button = (40.0 * scale).floor();
        let bar_height = (64.0 * scale).floor();
        let bar_y = height - bar_height - (8.0 * scale).floor();
        let button_y = bar_y + (bar_height - button) / 2.0;
        let control = |x: f64, w: f64| Rect {
            x,
            y: button_y,
            w,
            h: button,
        };
        let square = |x: f64| control(x, button);
        let track_width = (56.0 * scale).floor();
        let volume_width = (88.0 * scale).floor();
        let mut right = width - pad;
        let fullscreen = square(right - button);
        right = fullscreen.x - pad;
        let sub = key.sub_control.then(|| {
            let rect = control(right - track_width, track_width);
            right = rect.x - pad;
            rect
        });
        let audio = key.audio_control.then(|| {
            let rect = control(right - track_width, track_width);
            right = rect.x - pad;
            rect
        });
        let volume = control(right - volume_width, volume_width);
        right = volume.x - pad;
        let time_width = (70.0 * scale).floor();
        let duration = Rect {
            x: right - time_width,
            y: bar_y,
            w: time_width,
            h: bar_height,
        };
        let play = square(pad);
        let elapsed = Rect {
            x: play.x + button + pad,
            y: bar_y,
            w: time_width,
            h: bar_height,
        };
        let seek_start = elapsed.x + time_width + pad;
        let seek_end = duration.x - pad;
        let seek_height = (12.0 * scale).floor();
        let seek = Rect {
            x: seek_start,
            y: bar_y + (bar_height - seek_height) / 2.0,
            w: (seek_end - seek_start).max(40.0),
            h: seek_height,
        };
        let bar = Rect {
            x: pad,
            y: bar_y,
            w: width - 2.0 * pad,
            h: bar_height,
        };
        Layout {
            key,
            scale,
            bar,
            play,
            elapsed,
            seek,
            duration,
            volume,
            audio,
            sub,
            fullscreen,
        }
    }

    pub fn controls(&self) -> Vec<(&'static str, Rect)> {
        let mut controls = vec![
            ("play", self.play),
            ("seek", self.seek),
            ("volume", self.volume),
            ("fullscreen", self.fullscreen),
        ];
        if let Some(audio) = self.audio {
            controls.push(("audio", audio));
        }
        if let Some(sub) = self.sub {
            controls.push(("sub", sub));
        }
        controls
    }

    fn size(&self, base: f64) -> i64 {
        (base * self.scale).floor() as i64
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Hidden,
    Shown { hide_at: Option<Instant> },
    Fading { since: Instant },
}

pub enum Response {
    Pass,
    Consumed,
    Command(Command),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct View {
    layout: LayoutKey,
    paused: bool,
    fullscreen: bool,
    volume_icon: usize,
    volume: i64,
    elapsed: Option<i64>,
    duration: Option<i64>,
    progress_px: i64,
    aid: Option<i64>,
    sid: Option<i64>,
    tooltip: Option<(i64, i64)>,
    alpha: u8,
}

pub struct Osc {
    visibility: OscVisibility,
    phase: Phase,
    pointer: Option<(f64, f64)>,
    dragging: bool,
    playback: Playback,
    layout: Layout,
    now: Instant,
    rendered: Option<(View, AssOverlay)>,
    revision: u64,
}

impl Osc {
    pub fn new(visibility: OscVisibility, now: Instant) -> Self {
        let layout = Layout::new(LayoutKey {
            width: 1280,
            height: 720,
            audio_control: false,
            sub_control: false,
        });
        let mut osc = Osc {
            visibility,
            phase: Phase::Hidden,
            pointer: None,
            dragging: false,
            playback: Playback::default(),
            layout,
            now,
            rendered: None,
            revision: 0,
        };
        osc.show(now);
        osc
    }

    pub fn visibility(&self) -> OscVisibility {
        self.visibility
    }

    pub fn visible(&self) -> bool {
        self.phase != Phase::Hidden
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn set_visibility(&mut self, visibility: OscVisibility, now: Instant) {
        self.visibility = visibility;
        match visibility {
            OscVisibility::Never => self.phase = Phase::Hidden,
            OscVisibility::Auto | OscVisibility::Always => self.show(now),
        }
    }

    pub fn start_file(&mut self, now: Instant) {
        self.dragging = false;
        self.show(now);
    }

    pub fn update(&mut self, playback: Playback, width: u32, height: u32, now: Instant) {
        self.now = now;
        self.playback = playback;
        let key = LayoutKey {
            width: width.max(1),
            height: height.max(1),
            audio_control: playback.audio_tracks > 1,
            sub_control: playback.sub_tracks > 0,
        };
        if key != self.layout.key {
            self.layout = Layout::new(key);
        }
        match self.phase {
            Phase::Shown {
                hide_at: Some(deadline),
            } if now >= deadline => {
                if self.dragging || self.pointer_over_bar() {
                    self.phase = Phase::Shown {
                        hide_at: Some(now + HIDE_TIMEOUT),
                    };
                } else {
                    self.phase = Phase::Fading { since: now };
                }
            }
            Phase::Fading { since } if now.duration_since(since) >= FADE_DURATION => {
                self.phase = Phase::Hidden;
            }
            _ => {}
        }
    }

    pub fn pointer_move(&mut self, x: f64, y: f64, now: Instant) -> Response {
        self.pointer = Some((x, y));
        self.show(now);
        if self.dragging {
            return self.seek_command(x);
        }
        Response::Pass
    }

    pub fn pointer_leave(&mut self, now: Instant) {
        self.pointer = None;
        if self.visibility == OscVisibility::Auto && !self.dragging && self.visible() {
            self.phase = Phase::Fading { since: now };
        }
    }

    pub fn button(&mut self, pressed: bool) -> Response {
        if !pressed && self.dragging {
            self.dragging = false;
            return match self.pointer {
                Some((x, _)) => self.seek_command(x),
                None => Response::Consumed,
            };
        }
        let Some((x, y)) = self.pointer else {
            return Response::Pass;
        };
        if !self.visible() || !self.layout.bar.contains(x, y) {
            return Response::Pass;
        }
        let layout = &self.layout;
        if pressed {
            if layout.seek.contains(x, y) {
                self.dragging = true;
                return self.seek_command(x);
            }
            return Response::Consumed;
        }
        let command = if layout.play.contains(x, y) {
            Command::Pause(Toggle::Toggle)
        } else if layout.audio.is_some_and(|audio| audio.contains(x, y)) {
            Command::CycleTrack(TrackKind::Audio)
        } else if layout.sub.is_some_and(|sub| sub.contains(x, y)) {
            Command::CycleTrack(TrackKind::Subtitle)
        } else if layout.volume.contains(x, y) {
            Command::Mute(Toggle::Toggle)
        } else if layout.fullscreen.contains(x, y) {
            Command::Fullscreen(Toggle::Toggle)
        } else if layout.seek.contains(x, y) {
            return self.seek_command(x);
        } else {
            return Response::Consumed;
        };
        Response::Command(command)
    }

    pub fn wheel(&mut self, up: bool) -> Response {
        let Some((x, y)) = self.pointer else {
            return Response::Pass;
        };
        if !self.visible() {
            return Response::Pass;
        }
        let sign = if up { 1.0 } else { -1.0 };
        if self.layout.volume.contains(x, y) {
            Response::Command(Command::Volume(Adjust::Add(sign * VOLUME_STEP)))
        } else if self.layout.seek.contains(x, y) {
            Response::Command(Command::Seek {
                amount: sign * WHEEL_SEEK_SECONDS,
                mode: SeekMode::Relative,
                exact: false,
            })
        } else {
            Response::Pass
        }
    }

    pub fn overlay(&mut self) -> Option<&AssOverlay> {
        let Some(view) = self.view() else {
            if self.rendered.take().is_some() {
                self.revision += 1;
            }
            return None;
        };
        if self.rendered.as_ref().is_none_or(|(rendered, _)| *rendered != view) {
            let overlay = AssOverlay {
                events: self.events(&view),
                res_x: self.layout.key.width,
                res_y: self.layout.key.height,
            };
            self.rendered = Some((view, overlay));
            self.revision += 1;
        }
        self.rendered.as_ref().map(|(_, overlay)| overlay)
    }

    fn show(&mut self, now: Instant) {
        self.phase = match self.visibility {
            OscVisibility::Never => Phase::Hidden,
            OscVisibility::Always => Phase::Shown { hide_at: None },
            OscVisibility::Auto => Phase::Shown {
                hide_at: Some(now + HIDE_TIMEOUT),
            },
        };
    }

    fn pointer_over_bar(&self) -> bool {
        self.pointer
            .is_some_and(|(x, y)| self.layout.bar.contains(x, y))
    }

    fn seek_command(&self, x: f64) -> Response {
        let duration = self.playback.duration;
        if duration <= 0.0 {
            return Response::Consumed;
        }
        Response::Command(Command::Seek {
            amount: self.layout.seek.fraction_at(x) * duration,
            mode: SeekMode::Absolute,
            exact: true,
        })
    }

    fn opacity(&self) -> Option<f64> {
        match self.phase {
            Phase::Hidden => None,
            Phase::Shown { .. } => Some(1.0),
            Phase::Fading { since } => {
                let faded = self.now.duration_since(since).as_secs_f64() / FADE_DURATION.as_secs_f64();
                (faded < 1.0).then_some(1.0 - faded)
            }
        }
    }

    fn view(&self) -> Option<View> {
        let opacity = self.opacity()?;
        let playback = &self.playback;
        let seek = self.layout.seek;
        let progress = if playback.duration > 0.0 {
            (playback.position / playback.duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let tooltip = self
            .pointer
            .filter(|(x, y)| seek.contains(*x, *y) && playback.duration > 0.0)
            .map(|(x, _)| {
                (
                    x.round() as i64,
                    (seek.fraction_at(x) * playback.duration) as i64,
                )
            });
        Some(View {
            layout: self.layout.key,
            paused: playback.paused,
            fullscreen: playback.fullscreen,
            volume_icon: volume_icon(playback),
            volume: playback.volume.floor() as i64,
            elapsed: whole_seconds(playback.position),
            duration: whole_seconds(playback.duration),
            progress_px: (seek.w * progress).round() as i64,
            aid: playback.aid,
            sid: playback.sid,
            tooltip,
            alpha: (255.0 * (1.0 - opacity)).round() as u8,
        })
    }

    fn events(&self, view: &View) -> String {
        let layout = &self.layout;
        let mut ass = Ass::new(view.alpha);
        let bar = layout.bar;
        ass.shape("000000", BAR_ALPHA);
        ass.round_rect(bar.x, bar.y, bar.x + bar.w, bar.y + bar.h, BAR_RADIUS);
        let seek = layout.seek;
        ass.shape("333333", 0);
        ass.rect(seek.x, seek.y, seek.x + seek.w, seek.y + seek.h);
        ass.shape("FFFFFF", 0);
        ass.rect(seek.x, seek.y, seek.x + view.progress_px as f64, seek.y + seek.h);

        let text_y = bar.y + (bar.h - layout.size(20.0) as f64) / 2.0;
        let pause_icon = if view.paused { icon::PLAY } else { icon::PAUSE };
        ass.text(layout.play.x + layout.size(6.0) as f64, text_y, layout.size(28.0), Some(ICON_FONT), pause_icon);
        ass.text(layout.elapsed.x, text_y, layout.size(20.0), None, &clock(view.elapsed));
        ass.text(layout.duration.x, text_y, layout.size(20.0), None, &clock(view.duration));
        let volume_icon = if view.volume_icon == 0 {
            icon::MUTE
        } else {
            icon::VOLUME[view.volume_icon - 1]
        };
        ass.text(
            layout.volume.x + layout.size(2.0) as f64,
            text_y,
            layout.size(24.0),
            Some(ICON_FONT),
            &format!("{volume_icon} {}%", view.volume),
        );
        if let Some(audio) = layout.audio {
            ass.text(audio.x, text_y, layout.size(24.0), Some(ICON_FONT), &format!("{} {}", icon::AUDIO, track_label(view.aid)));
        }
        if let Some(sub) = layout.sub {
            ass.text(sub.x, text_y, layout.size(24.0), Some(ICON_FONT), &format!("{} {}", icon::SUBTITLE, track_label(view.sid)));
        }
        let fullscreen_icon = if view.fullscreen {
            icon::EXIT_FULLSCREEN
        } else {
            icon::FULLSCREEN
        };
        ass.text(layout.fullscreen.x + layout.size(2.0) as f64, text_y, layout.size(28.0), Some(ICON_FONT), fullscreen_icon);
        if let Some((x, seconds)) = view.tooltip {
            ass.tooltip(x, (seek.y - 6.0) as i64, layout.size(18.0), &clock(Some(seconds)));
        }
        ass.text
    }
}

fn whole_seconds(seconds: f64) -> Option<i64> {
    (seconds.is_finite() && seconds >= 0.0).then(|| seconds as i64)
}

fn volume_icon(playback: &Playback) -> usize {
    let volume = playback.volume;
    if playback.muted || volume <= 0.0 {
        0
    } else if volume <= 33.0 {
        1
    } else if volume <= 66.0 {
        2
    } else if volume <= 99.0 {
        3
    } else {
        4
    }
}

fn track_label(id: Option<i64>) -> String {
    id.map_or_else(|| "no".to_owned(), |id| id.to_string())
}

pub fn clock(seconds: Option<i64>) -> String {
    match seconds {
        None => "--:--".to_owned(),
        Some(seconds) if seconds >= 3600 => format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        ),
        Some(seconds) => format!("{:02}:{:02}", seconds / 60, seconds % 60),
    }
}

struct Ass {
    text: String,
    fade: u8,
}

impl Ass {
    fn new(fade: u8) -> Self {
        Ass {
            text: String::new(),
            fade,
        }
    }

    fn alpha(&self, base: u8) -> u8 {
        let opaque = (255 - base) as u32 * (255 - self.fade) as u32 / 255;
        255 - opaque as u8
    }

    fn event(&mut self) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
    }

    fn shape(&mut self, color: &str, alpha: u8) {
        self.event();
        let alpha = self.alpha(alpha);
        let _ = write!(
            self.text,
            "{{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H{color}&\\alpha&H{alpha:02X}&}}"
        );
    }

    fn text(&mut self, x: f64, y: f64, size: i64, font: Option<&str>, content: &str) {
        self.event();
        let alpha = self.alpha(0);
        let _ = write!(
            self.text,
            "{{\\an7\\pos({},{})\\fs{size}\\bord0\\shad0\\1c&HFFFFFF&\\alpha&H{alpha:02X}&",
            x as i64, y as i64
        );
        if let Some(font) = font {
            let _ = write!(self.text, "\\fn{font}");
        }
        let _ = write!(self.text, "}}{content}");
    }

    fn tooltip(&mut self, x: i64, y: i64, size: i64, content: &str) {
        self.event();
        let alpha = self.alpha(0);
        let _ = write!(
            self.text,
            "{{\\an2\\pos({x},{y})\\fs{size}\\bord1\\shad0\\1c&HFFFFFF&\\3c&H000000&\\alpha&H{alpha:02X}&}}{content}"
        );
    }

    fn coord(&mut self, x: f64, y: f64) {
        let _ = write!(self.text, " {} {}", (x * 8.0).ceil() as i64, (y * 8.0).ceil() as i64);
    }

    fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.text.push_str("{\\p4} m");
        self.coord(x0, y0);
        for (x, y) in [(x1, y0), (x1, y1), (x0, y1)] {
            self.text.push_str(" l");
            self.coord(x, y);
        }
        self.text.push_str("{\\p0}");
    }

    fn round_rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) {
        let handle = CIRCLE_HANDLE * radius;
        self.text.push_str("{\\p4} m");
        self.coord(x0 + radius, y0);
        self.line(x1 - radius, y0);
        self.curve([(x1 - radius + handle, y0), (x1, y0 + radius - handle), (x1, y0 + radius)]);
        self.line(x1, y1 - radius);
        self.curve([(x1, y1 - radius + handle), (x1 - radius + handle, y1), (x1 - radius, y1)]);
        self.line(x0 + radius, y1);
        self.curve([(x0 + radius - handle, y1), (x0, y1 - radius + handle), (x0, y1 - radius)]);
        self.line(x0, y0 + radius);
        self.curve([(x0, y0 + radius - handle), (x0 + radius - handle, y0), (x0 + radius, y0)]);
        self.text.push_str("{\\p0}");
    }

    fn line(&mut self, x: f64, y: f64) {
        self.text.push_str(" l");
        self.coord(x, y);
    }

    fn curve(&mut self, points: [(f64, f64); 3]) {
        self.text.push_str(" b");
        for (x, y) in points {
            self.coord(x, y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playback() -> Playback {
        Playback {
            volume: 100.0,
            position: 30.0,
            duration: 120.0,
            audio_tracks: 1,
            ..Playback::default()
        }
    }

    fn osc_at(now: Instant) -> Osc {
        let mut osc = Osc::new(OscVisibility::Auto, now);
        osc.update(playback(), 1280, 720, now);
        osc
    }

    fn center(rect: Rect) -> (f64, f64) {
        (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
    }

    fn click(osc: &mut Osc, rect: Rect, now: Instant) -> Option<Command> {
        let (x, y) = center(rect);
        osc.pointer_move(x, y, now);
        osc.button(true);
        match osc.button(false) {
            Response::Command(command) => Some(command),
            Response::Pass | Response::Consumed => None,
        }
    }

    #[test]
    fn layout_reproduces_the_bar_geometry_and_scales_with_height() {
        let osc = osc_at(Instant::now());
        let layout = osc.layout();
        assert_eq!(layout.play, Rect { x: 12.0, y: 660.0, w: 40.0, h: 40.0 });
        assert_eq!(layout.fullscreen.x, 1228.0);
        assert_eq!(layout.volume, Rect { x: 1128.0, y: 660.0, w: 88.0, h: 40.0 });
        assert!(layout.audio.is_none() && layout.sub.is_none());
        assert_eq!(layout.seek.x, 146.0);
        assert_eq!(layout.seek.x + layout.seek.w, 1034.0);

        let mut osc = osc_at(Instant::now());
        osc.update(
            Playback {
                audio_tracks: 2,
                sub_tracks: 1,
                ..playback()
            },
            2560,
            1440,
            Instant::now(),
        );
        let layout = osc.layout();
        assert_eq!(layout.play.w, 80.0);
        assert_eq!(layout.bar.y, 1440.0 - 128.0 - 16.0);
        let sub = layout.sub.expect("subtitle control shows with a subtitle");
        let audio = layout.audio.expect("audio control shows with two audio tracks");
        assert!(audio.x < sub.x && sub.x < layout.fullscreen.x);
        assert_eq!(layout.controls().len(), 6);
    }

    #[test]
    fn controls_map_to_commands() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        osc.update(Playback { audio_tracks: 2, sub_tracks: 1, ..playback() }, 1280, 720, now);
        let layout = *osc.layout();
        assert_eq!(click(&mut osc, layout.play, now), Some(Command::Pause(Toggle::Toggle)));
        assert_eq!(click(&mut osc, layout.volume, now), Some(Command::Mute(Toggle::Toggle)));
        assert_eq!(
            click(&mut osc, layout.fullscreen, now),
            Some(Command::Fullscreen(Toggle::Toggle))
        );
        assert_eq!(
            click(&mut osc, layout.audio.unwrap(), now),
            Some(Command::CycleTrack(TrackKind::Audio))
        );
        assert_eq!(
            click(&mut osc, layout.sub.unwrap(), now),
            Some(Command::CycleTrack(TrackKind::Subtitle))
        );
        osc.pointer_move(640.0, 100.0, now);
        assert!(matches!(osc.button(true), Response::Pass));
        assert!(matches!(osc.button(false), Response::Pass));
    }

    #[test]
    fn seek_bar_click_and_drag_seek_absolutely() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        let seek = osc.layout().seek;
        let (_, y) = center(seek);
        osc.pointer_move(seek.x + seek.w * 0.25, y, now);
        let Response::Command(Command::Seek { amount, mode, exact }) = osc.button(true) else {
            panic!("pressing the seek bar seeks");
        };
        assert!((amount - 30.0).abs() < 1e-9);
        assert_eq!((mode, exact), (SeekMode::Absolute, true));
        let Response::Command(Command::Seek { amount, .. }) =
            osc.pointer_move(seek.x + seek.w * 0.75, 100.0, now)
        else {
            panic!("dragging seeks even off the bar");
        };
        assert!((amount - 90.0).abs() < 1e-9);
        let Response::Command(Command::Seek { amount, .. }) =
            osc.pointer_move(seek.x + seek.w * 2.0, 100.0, now)
        else {
            panic!("dragging past the end seeks");
        };
        assert!((amount - 120.0).abs() < 1e-9);
        assert!(matches!(osc.button(false), Response::Command(Command::Seek { .. })));
        assert!(matches!(osc.pointer_move(seek.x, y, now), Response::Pass));
    }

    #[test]
    fn wheel_adjusts_volume_or_seeks_over_its_controls() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        let (x, y) = center(osc.layout().volume);
        osc.pointer_move(x, y, now);
        assert!(matches!(
            osc.wheel(true),
            Response::Command(Command::Volume(Adjust::Add(step))) if step == VOLUME_STEP
        ));
        let (x, y) = center(osc.layout().seek);
        osc.pointer_move(x, y, now);
        assert!(matches!(
            osc.wheel(false),
            Response::Command(Command::Seek { amount, mode: SeekMode::Relative, .. }) if amount == -WHEEL_SEEK_SECONDS
        ));
        osc.pointer_move(640.0, 100.0, now);
        assert!(matches!(osc.wheel(true), Response::Pass));
    }

    #[test]
    fn auto_hides_after_the_timeout_and_fades_out() {
        let start = Instant::now();
        let mut osc = osc_at(start);
        osc.pointer_move(640.0, 100.0, start);
        assert!(osc.visible());
        osc.update(playback(), 1280, 720, start + HIDE_TIMEOUT);
        assert!(osc.visible(), "fading still draws");
        let faded = start + HIDE_TIMEOUT + FADE_DURATION / 2;
        osc.update(playback(), 1280, 720, faded);
        let events = osc.overlay().expect("fading draws").events.clone();
        assert!(events.contains("\\alpha&H80&") || events.contains("\\alpha&H7F&"), "{events}");
        osc.update(playback(), 1280, 720, faded + FADE_DURATION);
        assert!(!osc.visible());
        assert!(osc.overlay().is_none());
    }

    #[test]
    fn hovering_the_bar_or_dragging_keeps_it_shown() {
        let start = Instant::now();
        let mut osc = osc_at(start);
        let (x, y) = center(osc.layout().play);
        osc.pointer_move(x, y, start);
        osc.update(playback(), 1280, 720, start + HIDE_TIMEOUT * 4);
        assert_eq!(osc.phase, Phase::Shown { hide_at: Some(start + HIDE_TIMEOUT * 5) });
        osc.pointer_leave(start + HIDE_TIMEOUT * 4);
        assert!(matches!(osc.phase, Phase::Fading { .. }));
    }

    #[test]
    fn visibility_modes_gate_display_and_clicks() {
        let start = Instant::now();
        let mut osc = osc_at(start);
        osc.set_visibility(OscVisibility::Always, start);
        osc.update(playback(), 1280, 720, start + HIDE_TIMEOUT * 10);
        assert!(osc.visible());
        osc.pointer_leave(start);
        assert!(osc.visible(), "always ignores pointer leave");

        osc.set_visibility(OscVisibility::Never, start);
        let play = osc.layout().play;
        assert_eq!(click(&mut osc, play, start), None);
        assert!(!osc.visible());
        assert!(osc.overlay().is_none());

        osc.set_visibility(OscVisibility::Auto, start);
        assert!(osc.visible());
    }

    #[test]
    fn overlay_rebuilds_only_when_the_view_changes() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        osc.set_visibility(OscVisibility::Always, now);
        let first = osc.overlay().expect("visible osc draws").events.clone();
        let revision = osc.revision();
        osc.update(Playback { position: 30.05, ..playback() }, 1280, 720, now);
        osc.overlay();
        assert_eq!(osc.revision(), revision, "a sub-second, sub-pixel change keeps the overlay");
        osc.update(Playback { position: 31.0, ..playback() }, 1280, 720, now);
        let second = osc.overlay().expect("visible osc draws").events.clone();
        assert_ne!(osc.revision(), revision);
        assert!(first.contains("00:30") && second.contains("00:31"));
        assert!(second.contains("02:00"));
        assert!(second.contains(icon::PAUSE) && second.contains("100%"));
        osc.update(Playback { paused: true, muted: true, ..playback() }, 1280, 720, now);
        let paused = osc.overlay().expect("visible osc draws").events.clone();
        assert!(paused.contains(icon::PLAY) && paused.contains(icon::MUTE));
    }

    #[test]
    fn hover_shows_a_tooltip_at_the_pointer() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        let seek = osc.layout().seek;
        osc.pointer_move(seek.x + seek.w / 2.0, seek.y + 1.0, now);
        let events = osc.overlay().expect("visible osc draws").events.clone();
        let x = (seek.x + seek.w / 2.0).round() as i64;
        assert!(events.contains(&format!("\\an2\\pos({x},")), "{events}");
        assert!(events.contains("01:00"), "{events}");
    }

    #[test]
    fn clock_formats_like_the_lua_osc() {
        assert_eq!(clock(None), "--:--");
        assert_eq!(clock(Some(61)), "01:01");
        assert_eq!(clock(Some(3661)), "01:01:01");
    }
}
