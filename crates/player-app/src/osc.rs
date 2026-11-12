use std::time::{Duration, Instant};

use player_core::command::{
    Adjust, Command, OscVisibility, SeekMode, SeekPrecision, Toggle, TrackKind,
};
use player_overlay::{self as overlay, Align, Anchor, Color, DisplayList, Outline, Point, Shape, TextBlock, TextStyle, Weight};

use crate::icon::{self, Icon, VolumeLevel};

const HIDE_TIMEOUT: Duration = Duration::from_millis(500);
const FADE_DURATION: Duration = Duration::from_millis(200);
const VOLUME_STEP: f64 = 5.0;
const WHEEL_SEEK_SECONDS: f64 = 10.0;
const CHAPTER_RESTART_SECONDS: f64 = 5.0;
const BASE_HEIGHT: f64 = 720.0;
const PAD_X: f64 = 9.0;
const PAD_Y: f64 = 3.0;
const BUTTON_WIDTH: f64 = 27.0;
const TRACK_WIDTH: f64 = 90.0;
const BAR_HEIGHT: f64 = 56.0;
const BAR_OVERSCAN: f64 = 2.0;
const TOP_BUTTON_WIDTH: f64 = 18.0;
const CACHE_WIDTH: f64 = 150.0;
const SEEK_GAP: f64 = 2.0;
const BAR_OPACITY: f64 = 175.0 / 255.0;
const TRACK_OPACITY: f64 = 35.0 / 255.0;
const DISABLED_OPACITY: f64 = 119.0 / 255.0;
const TOOLTIP_OUTLINE_OPACITY: f64 = 167.0 / 255.0;
const ICON_SIZE: f64 = 26.0;
const TOP_ICON_SIZE: f64 = 16.0;
const TITLE_SIZE: f64 = 15.0;
const TIMECODE_SIZE: f64 = 22.0;
const TOOLTIP_SIZE: f64 = 25.0;
const LABEL_SIZE: f64 = 22.0;
const LABEL_GAP: f64 = 4.0;

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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub time: f64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    pub title: String,
    pub paused: bool,
    pub fullscreen: bool,
    pub muted: bool,
    pub volume: f64,
    pub speed: f64,
    pub position: f64,
    pub duration: f64,
    pub audio_tracks: usize,
    pub sub_tracks: usize,
    pub aid: Option<i64>,
    pub sid: Option<i64>,
    pub playlist_position: usize,
    pub playlist_count: usize,
    pub chapters: Vec<Chapter>,
}

impl Default for Playback {
    fn default() -> Self {
        Playback {
            title: String::new(),
            paused: false,
            fullscreen: false,
            muted: false,
            volume: 0.0,
            speed: 1.0,
            position: 0.0,
            duration: 0.0,
            audio_tracks: 0,
            sub_tracks: 0,
            aid: None,
            sid: None,
            playlist_position: 1,
            playlist_count: 1,
            chapters: Vec::new(),
        }
    }
}

impl Playback {
    fn seekable(&self) -> bool {
        self.duration > 0.0
    }

    fn enabled(&self, control: Control) -> bool {
        match control {
            Control::Menu | Control::Title | Control::Play | Control::TimeElapsed => true,
            Control::Volume | Control::Fullscreen => true,
            Control::TimeRemaining | Control::Seek => self.seekable(),
            Control::PlaylistPrev => self.playlist_position > 1,
            Control::PlaylistNext => self.playlist_position < self.playlist_count,
            Control::ChapterPrev | Control::ChapterNext => !self.chapters.is_empty(),
            Control::Audio => self.audio_tracks > 0,
            Control::Sub => self.sub_tracks > 0,
        }
    }

    fn percent(&self) -> Option<f64> {
        self.seekable()
            .then(|| (self.position / self.duration * 100.0).clamp(0.0, 100.0))
    }

    fn chapter_target(&self, forward: bool) -> Option<f64> {
        if forward {
            return self
                .chapters
                .iter()
                .map(|chapter| chapter.time)
                .find(|time| *time > self.position);
        }
        let current = self
            .chapters
            .iter()
            .map(|chapter| chapter.time)
            .rfind(|time| *time <= self.position);
        match current {
            Some(start) if self.position - start > CHAPTER_RESTART_SECONDS => Some(start),
            Some(start) => Some(
                self.chapters
                    .iter()
                    .map(|chapter| chapter.time)
                    .rfind(|time| *time < start)
                    .unwrap_or(0.0),
            ),
            None => Some(0.0),
        }
    }

    fn chapter_title_at(&self, seconds: f64) -> Option<&str> {
        self.chapters
            .iter()
            .rev()
            .find(|chapter| seconds >= chapter.time)
            .map(|chapter| chapter.title.as_str())
            .filter(|title| !title.is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Menu,
    PlaylistPrev,
    PlaylistNext,
    Title,
    ChapterPrev,
    ChapterNext,
    Play,
    TimeElapsed,
    TimeRemaining,
    Audio,
    Sub,
    Volume,
    Fullscreen,
    Seek,
}

impl Control {
    const ALL: [Control; 14] = [
        Control::Menu,
        Control::PlaylistPrev,
        Control::PlaylistNext,
        Control::Title,
        Control::ChapterPrev,
        Control::ChapterNext,
        Control::Play,
        Control::TimeElapsed,
        Control::TimeRemaining,
        Control::Audio,
        Control::Sub,
        Control::Volume,
        Control::Fullscreen,
        Control::Seek,
    ];

    fn name(self) -> &'static str {
        match self {
            Control::Menu => "menu",
            Control::PlaylistPrev => "playlist_prev",
            Control::PlaylistNext => "playlist_next",
            Control::Title => "title",
            Control::ChapterPrev => "chapter_prev",
            Control::ChapterNext => "chapter_next",
            Control::Play => "play",
            Control::TimeElapsed => "time_elapsed",
            Control::TimeRemaining => "time_remaining",
            Control::Audio => "audio",
            Control::Sub => "sub",
            Control::Volume => "volume",
            Control::Fullscreen => "fullscreen",
            Control::Seek => "seek",
        }
    }

    fn exposed(self) -> bool {
        !matches!(self, Control::Menu | Control::Title)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Geometry {
    x: f64,
    y: f64,
    an: u8,
    w: f64,
    h: f64,
}

impl Geometry {
    fn anchor(&self) -> Anchor {
        let along = |index: u8| match index {
            0 => Align::Start,
            1 => Align::Center,
            _ => Align::End,
        };
        let vertical = match (self.an - 1) / 3 {
            0 => Align::End,
            1 => Align::Center,
            _ => Align::Start,
        };
        Anchor { x: along((self.an - 1) % 3), y: vertical }
    }

    fn origin(&self) -> Point {
        Point { x: self.x, y: self.y }
    }

    fn center(&self) -> Point {
        let hitbox = self.hitbox();
        Point { x: hitbox.x + hitbox.w / 2.0, y: hitbox.y + hitbox.h / 2.0 }
    }

    fn hitbox(&self) -> Rect {
        let left = match (self.an - 1) % 3 {
            0 => self.x,
            1 => self.x - self.w / 2.0,
            _ => self.x - self.w,
        };
        let top = match (self.an - 1) / 3 {
            0 => self.y - self.h,
            1 => self.y - self.h / 2.0,
            _ => self.y,
        };
        Rect {
            x: left,
            y: top,
            w: self.w,
            h: self.h,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LayoutKey {
    width: u32,
    height: u32,
    milliseconds: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    key: LayoutKey,
    res_x: f64,
    res_y: f64,
    bar: Geometry,
    show_zone: Rect,
    menu: Geometry,
    playlist_prev: Geometry,
    playlist_next: Geometry,
    title: Geometry,
    chapter_prev: Geometry,
    chapter_next: Geometry,
    play: Geometry,
    time_elapsed: Geometry,
    time_remaining: Geometry,
    audio: Geometry,
    sub: Geometry,
    volume: Geometry,
    fullscreen: Geometry,
    seek: Geometry,
}

impl Layout {
    fn new(key: LayoutKey) -> Self {
        let aspect = key.width as f64 / key.height as f64;
        let timecode_width = if key.milliseconds { 170.0 } else { 110.0 };
        let minimum_width = (BUTTON_WIDTH + PAD_X) * 5.0
            + (timecode_width + PAD_X) * 4.0
            + (TRACK_WIDTH + PAD_X) * 2.0;
        let mut res_y = BASE_HEIGHT;
        let mut res_x = res_y * aspect;
        if res_x < minimum_width {
            res_y = minimum_width / aspect;
            res_x = res_y * aspect;
        }
        let bar = Geometry {
            x: -BAR_OVERSCAN,
            y: res_y - (BAR_HEIGHT - BAR_OVERSCAN),
            an: 7,
            w: res_x + 2.0 * BAR_OVERSCAN,
            h: BAR_HEIGHT,
        };
        let line1 = bar.y + (9.0 + PAD_Y);
        let line2 = bar.y + (36.0 + PAD_Y);
        let top = |x: f64, w: f64| Geometry {
            x,
            y: line1,
            an: 4,
            w,
            h: TOP_BUTTON_WIDTH - PAD_Y,
        };
        let row = |x: f64, w: f64, an: u8| Geometry {
            x,
            y: line2,
            an,
            w,
            h: 36.0 - 2.0 * PAD_Y,
        };
        let menu = top(bar.x + PAD_X + 4.0, TOP_BUTTON_WIDTH);
        let playlist_prev = top(menu.x + menu.w + PAD_X, TOP_BUTTON_WIDTH);
        let playlist_next = top(playlist_prev.x + playlist_prev.w + PAD_X, TOP_BUTTON_WIDTH);
        let title_left = playlist_next.x + playlist_next.w + PAD_X;
        let bar_right = bar.x + bar.w;
        let title_right = bar_right - PAD_X - CACHE_WIDTH - PAD_X;
        let title = top(title_left, title_right - title_left);

        let play = row(bar.x + PAD_X, BUTTON_WIDTH, 4);
        let chapter_prev = row(play.x + play.w + PAD_X, BUTTON_WIDTH, 4);
        let chapter_next = row(chapter_prev.x + chapter_prev.w + PAD_X, BUTTON_WIDTH, 4);
        let time_elapsed = row(
            chapter_next.x + chapter_next.w + PAD_X + timecode_width,
            timecode_width,
            6,
        );
        let seek_left = time_elapsed.x + PAD_X;
        let fullscreen = row(bar_right - BUTTON_WIDTH - PAD_X, BUTTON_WIDTH, 4);
        let volume = row(fullscreen.x - fullscreen.w - PAD_X, BUTTON_WIDTH, 4);
        let sub = row(volume.x - TRACK_WIDTH - PAD_X, TRACK_WIDTH, 4);
        let audio = row(sub.x - sub.w - PAD_X, TRACK_WIDTH, 4);
        let time_remaining = row(audio.x - PAD_X - timecode_width - 10.0, timecode_width, 4);
        let seek_right = time_remaining.x - PAD_X;
        let seek = row(seek_left, (seek_right - seek_left).max(0.0), 4);

        let show_zone = Rect {
            x: 0.0,
            y: bar.y / 2.0,
            w: res_x,
            h: res_y - bar.y / 2.0,
        };
        Layout {
            key,
            res_x,
            res_y,
            bar,
            show_zone,
            menu,
            playlist_prev,
            playlist_next,
            title,
            chapter_prev,
            chapter_next,
            play,
            time_elapsed,
            time_remaining,
            audio,
            sub,
            volume,
            fullscreen,
            seek,
        }
    }

    fn geometry(&self, control: Control) -> Geometry {
        match control {
            Control::Menu => self.menu,
            Control::PlaylistPrev => self.playlist_prev,
            Control::PlaylistNext => self.playlist_next,
            Control::Title => self.title,
            Control::ChapterPrev => self.chapter_prev,
            Control::ChapterNext => self.chapter_next,
            Control::Play => self.play,
            Control::TimeElapsed => self.time_elapsed,
            Control::TimeRemaining => self.time_remaining,
            Control::Audio => self.audio,
            Control::Sub => self.sub,
            Control::Volume => self.volume,
            Control::Fullscreen => self.fullscreen,
            Control::Seek => self.seek,
        }
    }

    fn to_virtual(self, x: f64, y: f64) -> (f64, f64) {
        (
            x * self.res_x / self.key.width as f64,
            y * self.res_y / self.key.height as f64,
        )
    }

    fn to_window(self, rect: Rect) -> Rect {
        let scale_x = self.key.width as f64 / self.res_x;
        let scale_y = self.key.height as f64 / self.res_y;
        Rect {
            x: rect.x * scale_x,
            y: rect.y * scale_y,
            w: rect.w * scale_x,
            h: rect.h * scale_y,
        }
    }

    fn seek_percent(&self, virtual_x: f64) -> f64 {
        let hitbox = self.seek.hitbox();
        scale_value(
            hitbox.x + SEEK_GAP,
            hitbox.x + hitbox.w - SEEK_GAP,
            0.0,
            100.0,
            virtual_x,
        )
        .clamp(0.0, 100.0)
    }

    fn seek_offset(&self, percent: f64) -> f64 {
        let width = self.seek.w;
        scale_value(0.0, 100.0, SEEK_GAP, width - SEEK_GAP, percent)
            .clamp(SEEK_GAP, width - SEEK_GAP)
    }
}

fn scale_value(x0: f64, x1: f64, y0: f64, y1: f64, value: f64) -> f64 {
    let slope = (y1 - y0) / (x1 - x0);
    let intercept = y0 - slope * x0;
    slope * value + intercept
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

#[derive(Debug, Clone, PartialEq)]
struct Tooltip {
    x: f64,
    y: f64,
    an: u8,
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
struct View {
    layout: LayoutKey,
    title: String,
    play_icon: Icon,
    volume_icon: Icon,
    fullscreen_icon: Icon,
    elapsed: String,
    remaining: Option<String>,
    progress: Option<f64>,
    chapter_marks: Vec<f64>,
    audio_label: String,
    sub_label: String,
    enabled: [bool; 14],
    tooltip: Option<Tooltip>,
    animation: Option<u8>,
}

impl View {
    fn enabled(&self, control: Control) -> bool {
        self.enabled[control as usize]
    }
}

struct Rendered {
    view: View,
    stable: DisplayList,
    dynamic: DisplayList,
    full: Option<DisplayList>,
}

pub struct Osc {
    visibility: OscVisibility,
    phase: Phase,
    pointer: Option<(f64, f64)>,
    active: Option<Control>,
    milliseconds: bool,
    show_remaining: bool,
    playback: Playback,
    size: (u32, u32),
    layout: Layout,
    now: Instant,
    rendered: Option<Rendered>,
    revision: u64,
}

impl Osc {
    pub fn new(visibility: OscVisibility, now: Instant) -> Self {
        let size = (1280, 720);
        let layout = Layout::new(LayoutKey {
            width: size.0,
            height: size.1,
            milliseconds: false,
        });
        let mut osc = Osc {
            visibility,
            phase: Phase::Hidden,
            pointer: None,
            active: None,
            milliseconds: false,
            show_remaining: true,
            playback: Playback::default(),
            size,
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

    pub fn areas(&self) -> Vec<(&'static str, Rect)> {
        Control::ALL
            .into_iter()
            .filter(|control| control.exposed() && self.playback.enabled(*control))
            .map(|control| {
                let hitbox = self.layout.geometry(control).hitbox();
                (control.name(), self.layout.to_window(hitbox))
            })
            .collect()
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
        self.active = None;
        self.show(now);
    }

    pub fn update(&mut self, playback: Playback, width: u32, height: u32, now: Instant) {
        self.now = now;
        self.playback = playback;
        self.size = (width.max(1), height.max(1));
        self.relayout();
        match self.phase {
            Phase::Shown {
                hide_at: Some(deadline),
            } if now >= deadline => {
                if self.active.is_some() || self.pointer_over_bar() {
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
        let (virtual_x, virtual_y) = self.layout.to_virtual(x, y);
        if self.layout.show_zone.contains(virtual_x, virtual_y) {
            self.show(now);
        }
        if self.active == Some(Control::Seek) {
            return self.seek_command(virtual_x);
        }
        Response::Pass
    }

    pub fn pointer_leave(&mut self, now: Instant) {
        self.pointer = None;
        if self.visibility == OscVisibility::Auto && self.active.is_none() && self.visible() {
            self.phase = Phase::Fading { since: now };
        }
    }

    pub fn button(&mut self, pressed: bool) -> Response {
        let pointer = self.pointer.map(|(x, y)| self.layout.to_virtual(x, y));
        if !pressed {
            let Some(active) = self.active.take() else {
                return if self.visible() && self.pointer_over_bar() {
                    Response::Consumed
                } else {
                    Response::Pass
                };
            };
            let Some((x, y)) = pointer else {
                return Response::Consumed;
            };
            if active == Control::Seek {
                return self.seek_command(x);
            }
            if self.control_at(x, y) == Some(active) {
                return self.activate(active);
            }
            return Response::Consumed;
        }
        let Some((x, y)) = pointer else {
            return Response::Pass;
        };
        if !self.visible() || !self.layout.bar.hitbox().contains(x, y) {
            return Response::Pass;
        }
        if let Some(control) = self.control_at(x, y) {
            self.active = Some(control);
            if control == Control::Seek {
                return self.seek_command(x);
            }
        }
        Response::Consumed
    }

    pub fn wheel(&mut self, up: bool) -> Response {
        let Some((x, y)) = self.pointer.map(|(x, y)| self.layout.to_virtual(x, y)) else {
            return Response::Pass;
        };
        if !self.visible() {
            return Response::Pass;
        }
        let sign = if up { 1.0 } else { -1.0 };
        match self.control_at(x, y) {
            Some(Control::Volume) => {
                Response::Command(Command::Volume(Adjust::Add(sign * VOLUME_STEP)))
            }
            Some(Control::Seek) => Response::Command(Command::Seek {
                amount: sign * WHEEL_SEEK_SECONDS,
                mode: SeekMode::Relative,
                precision: SeekPrecision::Default,
            }),
            _ => Response::Pass,
        }
    }

    pub fn overlay(&mut self) -> Option<&DisplayList> {
        self.refresh();
        let rendered = self.rendered.as_mut()?;
        if rendered.full.is_none() {
            let mut full = rendered.stable.clone();
            full.items.extend(rendered.dynamic.items.iter().cloned());
            rendered.full = Some(full);
        }
        rendered.full.as_ref()
    }

    /// The static prefix and the tooltip/progress suffix, in paint order.
    pub fn layers(&mut self) -> Option<(&DisplayList, &DisplayList)> {
        self.refresh();
        self.rendered.as_ref().map(|rendered| (&rendered.stable, &rendered.dynamic))
    }

    fn refresh(&mut self) {
        let Some(view) = self.view() else {
            if self.rendered.take().is_some() {
                self.revision += 1;
            }
            return;
        };
        if self.rendered.as_ref().is_none_or(|rendered| rendered.view != view) {
            let (stable, dynamic) = self.display(&view);
            self.rendered = Some(Rendered { view, stable, dynamic, full: None });
            self.revision += 1;
        }
    }

    fn relayout(&mut self) {
        let key = LayoutKey {
            width: self.size.0,
            height: self.size.1,
            milliseconds: self.milliseconds,
        };
        if key != self.layout.key {
            if key.width != self.layout.key.width || key.height != self.layout.key.height {
                self.active = None;
            }
            self.layout = Layout::new(key);
        }
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
        self.pointer.is_some_and(|(x, y)| {
            let (x, y) = self.layout.to_virtual(x, y);
            self.layout.bar.hitbox().contains(x, y)
        })
    }

    fn control_at(&self, x: f64, y: f64) -> Option<Control> {
        Control::ALL.into_iter().find(|control| {
            self.playback.enabled(*control)
                && self.layout.geometry(*control).hitbox().contains(x, y)
        })
    }

    fn activate(&mut self, control: Control) -> Response {
        let command = match control {
            Control::Play => Command::Pause(Toggle::Toggle),
            Control::PlaylistPrev => Command::PlaylistPrevious,
            Control::PlaylistNext => Command::PlaylistNext,
            Control::ChapterPrev | Control::ChapterNext => {
                let forward = control == Control::ChapterNext;
                match self.playback.chapter_target(forward) {
                    Some(amount) => Command::Seek {
                        amount,
                        mode: SeekMode::Absolute,
                        precision: SeekPrecision::Exact,
                    },
                    None => return Response::Consumed,
                }
            }
            Control::Audio => Command::CycleTrack(TrackKind::Audio),
            Control::Sub => Command::CycleTrack(TrackKind::Subtitle),
            Control::Volume => Command::Mute(Toggle::Toggle),
            Control::Fullscreen => Command::Fullscreen(Toggle::Toggle),
            Control::TimeElapsed => {
                self.milliseconds = !self.milliseconds;
                self.relayout();
                return Response::Consumed;
            }
            Control::TimeRemaining => {
                self.show_remaining = !self.show_remaining;
                return Response::Consumed;
            }
            Control::Menu | Control::Title | Control::Seek => return Response::Consumed,
        };
        Response::Command(command)
    }

    fn seek_command(&self, virtual_x: f64) -> Response {
        let duration = self.playback.duration;
        if duration <= 0.0 {
            return Response::Consumed;
        }
        Response::Command(Command::Seek {
            amount: self.layout.seek_percent(virtual_x) / 100.0 * duration,
            mode: SeekMode::Absolute,
            precision: SeekPrecision::Keyframes,
        })
    }

    fn animation(&self) -> Option<Option<u8>> {
        match self.phase {
            Phase::Hidden => None,
            Phase::Shown { .. } => Some(None),
            Phase::Fading { since } => {
                let faded =
                    self.now.duration_since(since).as_secs_f64() / FADE_DURATION.as_secs_f64();
                (faded < 1.0).then_some(Some((255.0 * faded) as u8))
            }
        }
    }

    fn view(&self) -> Option<View> {
        let animation = self.animation()?;
        let playback = &self.playback;
        let layout = &self.layout;
        let pointer = self.pointer.map(|(x, y)| layout.to_virtual(x, y));
        let over_seek = pointer
            .filter(|(x, y)| playback.seekable() && layout.seek.hitbox().contains(*x, *y));
        let hovered_seconds = match (self.active, over_seek, pointer) {
            (Some(Control::Seek), _, Some((x, _))) => Some(x),
            (None, Some((x, _)), _) => Some(x),
            _ => None,
        }
        .map(|x| playback.duration * (layout.seek_percent(x) / 100.0));
        let title = hovered_seconds
            .and_then(|seconds| playback.chapter_title_at(seconds))
            .map(|chapter| format!("Chapter: {chapter}"))
            .unwrap_or_else(|| self.title_text());
        let tooltip = over_seek.map(|(x, _)| {
            let percent = layout.seek_percent(x);
            let hitbox = layout.seek.hitbox();
            let (an, x) = if percent > 50.0 {
                (6, x - 5.0)
            } else {
                (4, x + 10.0)
            };
            Tooltip {
                x,
                y: hitbox.y + layout.seek.h / 2.0,
                an,
                text: format_time(playback.duration * (percent / 100.0), false),
            }
        });
        let remaining = playback.seekable().then(|| {
            if self.show_remaining {
                let speed = if playback.speed > 0.0 {
                    playback.speed
                } else {
                    1.0
                };
                let remaining = ((playback.duration - playback.position) / speed).max(0.0);
                format!("-{}", format_time(remaining, self.milliseconds))
            } else {
                format_time(playback.duration, self.milliseconds)
            }
        });
        let chapter_marks = playback
            .chapters
            .iter()
            .filter(|_| playback.seekable())
            .map(|chapter| chapter.time / playback.duration * 100.0)
            .filter(|percent| *percent > 0.0 && *percent < 100.0)
            .collect();
        let mut enabled = [false; 14];
        for control in Control::ALL {
            enabled[control as usize] = playback.enabled(control);
        }
        Some(View {
            layout: layout.key,
            title,
            play_icon: if playback.paused { Icon::Play } else { Icon::Pause },
            volume_icon: volume_icon(playback),
            fullscreen_icon: if playback.fullscreen { Icon::ExitFullscreen } else { Icon::Fullscreen },
            elapsed: format_time(playback.position, self.milliseconds),
            remaining,
            progress: playback
                .percent()
                .map(|percent| (layout.seek_offset(percent) * 8.0).ceil() / 8.0),
            chapter_marks,
            audio_label: track_label(playback.aid, playback.audio_tracks),
            sub_label: track_label(playback.sid, playback.sub_tracks),
            enabled,
            tooltip,
            animation,
        })
    }

    fn title_text(&self) -> String {
        let playback = &self.playback;
        let title = if playback.playlist_count > 1 {
            format!(
                "[{}/{}] {}",
                playback.playlist_position, playback.playlist_count, playback.title
            )
        } else {
            playback.title.clone()
        };
        title.replace('\n', " ")
    }

    fn display(&self, view: &View) -> (DisplayList, DisplayList) {
        let layout = &self.layout;
        let fade = view.animation.map_or(1.0, |animation| 1.0 - animation as f64 / 255.0);
        let white = |opacity: f64| Color::WHITE.with_opacity(opacity * fade);
        let enabled = |control: Control| if view.enabled(control) { white(1.0) } else { white(DISABLED_OPACITY) };
        let scale = self.size.1 as f64 / layout.res_y;
        let mut list = DisplayList::new(self.size.0, self.size.1, scale);

        list.fill(Shape::Rect(overlay_rect(layout.bar.hitbox())), Color::BLACK.with_opacity(BAR_OPACITY * fade));
        let seek = layout.seek.hitbox();
        list.fill(Shape::Rect(overlay_rect(seek)), white(TRACK_OPACITY));

        icon::draw(&mut list, Icon::Menu, layout.menu.center(), TOP_ICON_SIZE, enabled(Control::Menu));
        icon::draw(
            &mut list,
            Icon::PlaylistPrev,
            layout.playlist_prev.center(),
            TOP_ICON_SIZE,
            enabled(Control::PlaylistPrev),
        );
        icon::draw(
            &mut list,
            Icon::PlaylistNext,
            layout.playlist_next.center(),
            TOP_ICON_SIZE,
            enabled(Control::PlaylistNext),
        );
        let title = layout.title;
        let title_text = if view.title.is_empty() { "mpv".to_owned() } else { view.title.clone() };
        list.push(overlay::Item::Text {
            origin: title.origin(),
            anchor: title.anchor(),
            block: text(title_text, TITLE_SIZE, white(1.0), None),
            clip: Some(overlay_rect(title.hitbox())),
        });

        icon::draw(&mut list, view.play_icon, layout.play.center(), ICON_SIZE, white(1.0));
        icon::draw(&mut list, Icon::ChapterPrev, layout.chapter_prev.center(), ICON_SIZE, enabled(Control::ChapterPrev));
        icon::draw(&mut list, Icon::ChapterNext, layout.chapter_next.center(), ICON_SIZE, enabled(Control::ChapterNext));
        let elapsed = layout.time_elapsed;
        list.text(elapsed.origin(), elapsed.anchor(), text(view.elapsed.clone(), TIMECODE_SIZE, white(1.0), None));
        icon::draw(&mut list, view.fullscreen_icon, layout.fullscreen.center(), ICON_SIZE, white(1.0));
        icon::draw(&mut list, view.volume_icon, layout.volume.center(), ICON_SIZE, white(1.0));
        for (control, track_icon, label) in [
            (Control::Sub, Icon::Subtitle, &view.sub_label),
            (Control::Audio, Icon::Audio, &view.audio_label),
        ] {
            let geometry = layout.geometry(control);
            let color = enabled(control);
            let hitbox = geometry.hitbox();
            let icon_center = Point { x: hitbox.x + ICON_SIZE / 2.0, y: geometry.center().y };
            icon::draw(&mut list, track_icon, icon_center, ICON_SIZE, color);
            list.text(
                Point { x: hitbox.x + ICON_SIZE + LABEL_GAP, y: geometry.center().y },
                Anchor { x: Align::Start, y: Align::Center },
                text(label.clone(), LABEL_SIZE, color, None),
            );
        }
        if let Some(remaining) = &view.remaining {
            let geometry = layout.time_remaining;
            list.text(geometry.origin(), geometry.anchor(), text(remaining.clone(), TIMECODE_SIZE, white(1.0), None));
        }

        let seek_color = enabled(Control::Seek);
        for percent in &view.chapter_marks {
            let offset = seek.x + layout.seek_offset(*percent);
            let half = SEEK_GAP / 0.5 / 2.0;
            let (top, bottom) = (seek.y, seek.y + seek.h);
            list.fill(
                Shape::Path(overlay::Path::polygon(&[(offset - half, top), (offset + half, top), (offset, top + SEEK_GAP)])),
                seek_color,
            );
            list.fill(
                Shape::Path(overlay::Path::polygon(&[
                    (offset - half, bottom),
                    (offset, bottom - SEEK_GAP),
                    (offset + half, bottom),
                ])),
                seek_color,
            );
        }
        let mut dynamic = DisplayList::new(self.size.0, self.size.1, scale);
        if let Some(progress) = view.progress {
            dynamic.fill(
                Shape::Rect(overlay::Rect::new(
                    seek.x + SEEK_GAP,
                    seek.y + SEEK_GAP,
                    (progress - SEEK_GAP).max(0.0),
                    seek.h - 2.0 * SEEK_GAP,
                )),
                seek_color,
            );
        }
        if let Some(tooltip) = &view.tooltip {
            let anchor = Geometry { x: tooltip.x, y: tooltip.y, an: tooltip.an, w: 0.0, h: 0.0 }.anchor();
            let outline = Outline { width: 1.0, color: Color::BLACK.with_opacity(TOOLTIP_OUTLINE_OPACITY * fade) };
            dynamic.text(
                Point { x: tooltip.x, y: tooltip.y },
                anchor,
                text(tooltip.text.clone(), TOOLTIP_SIZE, white(1.0), Some(outline)),
            );
        }
        (list, dynamic)
    }
}

fn overlay_rect(rect: Rect) -> overlay::Rect {
    overlay::Rect::new(rect.x, rect.y, rect.w, rect.h)
}

fn text(content: String, size: f64, color: Color, outline: Option<Outline>) -> TextBlock {
    TextBlock {
        text: content,
        style: TextStyle { size, weight: Weight::Regular, color, outline },
        max_width: None,
        align: Align::Start,
    }
}

fn volume_icon(playback: &Playback) -> Icon {
    if playback.volume == 0.0 || playback.muted {
        return Icon::Mute;
    }
    let level = (playback.volume / (100.0 / 3.0)).ceil().clamp(1.0, 4.0) as usize;
    Icon::Volume([VolumeLevel::Low, VolumeLevel::Medium, VolumeLevel::High, VolumeLevel::Boosted][level - 1])
}

fn track_label(id: Option<i64>, count: usize) -> String {
    id.map_or_else(|| format!("-/{count}"), |id| format!("{id}/{count}"))
}

pub fn format_time(seconds: f64, milliseconds: bool) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let magnitude = seconds.abs();
    let whole = magnitude as i64;
    let mut millis = ((magnitude - whole as f64) * 1000.0).round() as i64;
    let mut total = whole;
    if millis >= 1000 {
        millis -= 1000;
        total += 1;
    }
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if milliseconds {
        format!("{sign}{hours:02}:{minutes:02}:{secs:02}.{millis:03}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playback() -> Playback {
        Playback {
            title: "Title".to_owned(),
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

    fn area(osc: &Osc, name: &str) -> Rect {
        osc.areas()
            .into_iter()
            .find(|(candidate, _)| *candidate == name)
            .unwrap_or_else(|| panic!("{name} is not an enabled control"))
            .1
    }

    fn center(rect: Rect) -> (f64, f64) {
        (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
    }

    fn click(osc: &mut Osc, name: &str, now: Instant) -> Option<Command> {
        let (x, y) = center(area(osc, name));
        osc.pointer_move(x, y, now);
        osc.button(true);
        match osc.button(false) {
            Response::Command(command) => Some(command),
            Response::Pass | Response::Consumed => None,
        }
    }

    fn texts(list: &DisplayList) -> String {
        list.items
            .iter()
            .filter_map(|item| match item {
                overlay::Item::Text { block, .. } => Some(block.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tooltip(list: &DisplayList) -> (Point, Anchor, String) {
        match list.items.last() {
            Some(overlay::Item::Text { origin, anchor, block, .. }) if block.style.outline.is_some() => {
                (*origin, *anchor, block.text.clone())
            }
            other => panic!("the tooltip draws last: {other:?}"),
        }
    }

    fn names(osc: &Osc) -> Vec<&'static str> {
        osc.areas().into_iter().map(|(name, _)| name).collect()
    }

    #[test]
    fn bottom_bar_geometry_follows_the_reference_layout_at_720p() {
        let osc = osc_at(Instant::now());
        assert_eq!(area(&osc, "play"), Rect { x: 7.0, y: 690.0, w: 27.0, h: 30.0 });
        assert_eq!(area(&osc, "volume"), Rect { x: 1210.0, y: 690.0, w: 27.0, h: 30.0 });
        assert_eq!(area(&osc, "fullscreen"), Rect { x: 1246.0, y: 690.0, w: 27.0, h: 30.0 });
        assert_eq!(area(&osc, "seek"), Rect { x: 234.0, y: 690.0, w: 640.0, h: 30.0 });
        assert_eq!(area(&osc, "audio").x, 1012.0);
        assert_eq!(osc.layout.bar.hitbox(), Rect { x: -2.0, y: 666.0, w: 1284.0, h: 56.0 });
    }

    #[test]
    fn layout_scales_with_the_window_and_widens_the_canvas_for_narrow_windows() {
        let mut osc = osc_at(Instant::now());
        osc.update(playback(), 2560, 1440, Instant::now());
        let play = area(&osc, "play");
        assert_eq!(play, Rect { x: 14.0, y: 1380.0, w: 54.0, h: 60.0 });

        osc.update(playback(), 640, 640, Instant::now());
        let layout = osc.layout;
        assert!((layout.res_x - 854.0).abs() < 1e-9 && (layout.res_y - 854.0).abs() < 1e-9);
        let fullscreen = area(&osc, "fullscreen");
        assert!(fullscreen.x + fullscreen.w <= 640.0 + 1e-9, "{fullscreen:?}");
    }

    #[test]
    fn controls_are_enabled_like_the_reference_osc() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        assert_eq!(
            names(&osc),
            ["play", "time_elapsed", "time_remaining", "audio", "volume", "fullscreen", "seek"]
        );
        osc.update(
            Playback {
                audio_tracks: 2,
                sub_tracks: 1,
                playlist_position: 2,
                playlist_count: 3,
                chapters: vec![Chapter { time: 0.0, title: String::new() }],
                ..playback()
            },
            1280,
            720,
            now,
        );
        assert_eq!(
            names(&osc),
            [
                "playlist_prev",
                "playlist_next",
                "chapter_prev",
                "chapter_next",
                "play",
                "time_elapsed",
                "time_remaining",
                "audio",
                "sub",
                "volume",
                "fullscreen",
                "seek",
            ]
        );
        osc.update(Playback { duration: 0.0, ..playback() }, 1280, 720, now);
        assert!(!names(&osc).contains(&"seek") && !names(&osc).contains(&"time_remaining"));
    }

    #[test]
    fn controls_map_to_commands() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        osc.update(
            Playback {
                audio_tracks: 2,
                sub_tracks: 1,
                playlist_position: 2,
                playlist_count: 3,
                ..playback()
            },
            1280,
            720,
            now,
        );
        assert_eq!(click(&mut osc, "play", now), Some(Command::Pause(Toggle::Toggle)));
        assert_eq!(click(&mut osc, "volume", now), Some(Command::Mute(Toggle::Toggle)));
        assert_eq!(
            click(&mut osc, "fullscreen", now),
            Some(Command::Fullscreen(Toggle::Toggle))
        );
        assert_eq!(
            click(&mut osc, "audio", now),
            Some(Command::CycleTrack(TrackKind::Audio))
        );
        assert_eq!(
            click(&mut osc, "sub", now),
            Some(Command::CycleTrack(TrackKind::Subtitle))
        );
        assert_eq!(click(&mut osc, "playlist_prev", now), Some(Command::PlaylistPrevious));
        assert_eq!(click(&mut osc, "playlist_next", now), Some(Command::PlaylistNext));
        osc.pointer_move(640.0, 400.0, now);
        assert!(matches!(osc.button(true), Response::Pass));
        assert!(matches!(osc.button(false), Response::Pass));
    }

    #[test]
    fn chapter_buttons_seek_to_chapter_starts() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        let chapters = vec![
            Chapter { time: 0.0, title: "A".to_owned() },
            Chapter { time: 20.0, title: "B".to_owned() },
            Chapter { time: 60.0, title: "C".to_owned() },
        ];
        let seek = |osc: &mut Osc, name: &str, position: f64| {
            osc.update(
                Playback { position, chapters: chapters.clone(), ..playback() },
                1280,
                720,
                now,
            );
            match click(osc, name, now) {
                Some(Command::Seek { amount, mode: SeekMode::Absolute, .. }) => amount,
                other => panic!("expected an absolute seek, got {other:?}"),
            }
        };
        assert_eq!(seek(&mut osc, "chapter_next", 30.0), 60.0);
        assert_eq!(seek(&mut osc, "chapter_prev", 30.0), 20.0);
        assert_eq!(seek(&mut osc, "chapter_prev", 22.0), 0.0);
        assert_eq!(seek(&mut osc, "chapter_prev", 2.0), 0.0);
        osc.update(
            Playback { position: 70.0, chapters, ..playback() },
            1280,
            720,
            now,
        );
        assert_eq!(click(&mut osc, "chapter_next", now), None);
    }

    #[test]
    fn seek_bar_click_and_drag_seek_absolutely() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        let seek = area(&osc, "seek");
        let (_, y) = center(seek);
        let span = (seek.x + 2.0, seek.w - 4.0);
        osc.pointer_move(span.0 + span.1 * 0.25, y, now);
        let Response::Command(Command::Seek { amount, mode, precision }) = osc.button(true) else {
            panic!("pressing the seek bar seeks");
        };
        assert!((amount - 30.0).abs() < 1e-9);
        assert_eq!((mode, precision), (SeekMode::Absolute, SeekPrecision::Keyframes));
        let Response::Command(Command::Seek { amount, .. }) =
            osc.pointer_move(span.0 + span.1 * 0.75, 100.0, now)
        else {
            panic!("dragging seeks even off the bar");
        };
        assert!((amount - 90.0).abs() < 1e-9);
        let Response::Command(Command::Seek { amount, .. }) =
            osc.pointer_move(span.0 + span.1 * 2.0, 100.0, now)
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
        let (x, y) = center(area(&osc, "volume"));
        osc.pointer_move(x, y, now);
        assert!(matches!(
            osc.wheel(true),
            Response::Command(Command::Volume(Adjust::Add(step))) if step == VOLUME_STEP
        ));
        let (x, y) = center(area(&osc, "seek"));
        osc.pointer_move(x, y, now);
        assert!(matches!(
            osc.wheel(false),
            Response::Command(Command::Seek { amount, mode: SeekMode::Relative, .. }) if amount == -WHEEL_SEEK_SECONDS
        ));
        osc.pointer_move(640.0, 400.0, now);
        assert!(matches!(osc.wheel(true), Response::Pass));
    }

    #[test]
    fn timecodes_toggle_milliseconds_and_total_time() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        osc.set_visibility(OscVisibility::Always, now);
        let events = texts(osc.overlay().expect("visible osc draws"));
        assert!(events.contains("00:00:30") && events.contains("-00:01:30"), "{events}");
        let seek_before = area(&osc, "seek");

        assert_eq!(click(&mut osc, "time_elapsed", now), None);
        osc.update(playback(), 1280, 720, now);
        let events = texts(osc.overlay().expect("visible osc draws"));
        assert!(events.contains("00:00:30.000") && events.contains("-00:01:30.000"), "{events}");
        assert!(area(&osc, "seek").w < seek_before.w);

        assert_eq!(click(&mut osc, "time_remaining", now), None);
        let events = texts(osc.overlay().expect("visible osc draws"));
        assert!(events.contains("00:02:00.000") && !events.contains("-00:01:30"), "{events}");
    }

    #[test]
    fn auto_hides_after_the_timeout_and_fades_out() {
        let start = Instant::now();
        let mut osc = osc_at(start);
        osc.pointer_move(640.0, 600.0, start);
        assert!(osc.visible());
        osc.update(playback(), 1280, 720, start + HIDE_TIMEOUT);
        assert!(osc.visible(), "fading still draws");
        let faded = start + HIDE_TIMEOUT + FADE_DURATION / 2;
        osc.update(playback(), 1280, 720, faded);
        let events = texts(osc.overlay().expect("fading draws"));
        let bar_alpha = match &osc.overlay().expect("fading draws").items[0] {
            overlay::Item::Fill { color, .. } => color.a,
            other => panic!("the bar comes first: {other:?}"),
        };
        assert!((86..=89).contains(&bar_alpha), "half-faded bar alpha {bar_alpha}");
        assert!(!events.is_empty());
        osc.update(playback(), 1280, 720, faded + FADE_DURATION);
        assert!(!osc.visible());
        assert!(osc.overlay().is_none());
    }

    #[test]
    fn pointer_motion_above_the_dead_zone_does_not_show_the_osc() {
        let start = Instant::now();
        let mut osc = osc_at(start);
        osc.update(playback(), 1280, 720, start + HIDE_TIMEOUT);
        osc.update(playback(), 1280, 720, start + HIDE_TIMEOUT + FADE_DURATION);
        assert!(!osc.visible());
        osc.pointer_move(640.0, 100.0, start);
        assert!(!osc.visible());
        osc.pointer_move(640.0, 340.0, start);
        assert!(osc.visible());
    }

    #[test]
    fn hovering_the_bar_or_dragging_keeps_it_shown() {
        let start = Instant::now();
        let mut osc = osc_at(start);
        let (x, y) = center(area(&osc, "play"));
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
        assert_eq!(click(&mut osc, "play", start), None);
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
        let first = texts(osc.overlay().expect("visible osc draws"));
        let revision = osc.revision();
        osc.update(Playback { position: 29.9999, ..playback() }, 1280, 720, now);
        osc.overlay();
        assert_eq!(osc.revision(), revision, "a sub-pixel change keeps the overlay");
        osc.update(Playback { position: 31.0, ..playback() }, 1280, 720, now);
        let second = texts(osc.overlay().expect("visible osc draws"));
        assert_ne!(osc.revision(), revision);
        assert!(first.contains("00:00:30") && second.contains("00:00:31"));
        assert!(second.contains("-00:01:29"));
        let view = osc.view().expect("visible osc has a view");
        assert_eq!((view.play_icon, view.volume_icon), (Icon::Pause, Icon::Volume(VolumeLevel::High)));
        osc.update(Playback { paused: true, muted: true, ..playback() }, 1280, 720, now);
        osc.overlay().expect("visible osc draws");
        let view = osc.view().expect("visible osc has a view");
        assert_eq!((view.play_icon, view.volume_icon), (Icon::Play, Icon::Mute));
    }

    #[test]
    fn title_shows_the_playlist_position_and_the_hovered_chapter() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        osc.set_visibility(OscVisibility::Always, now);
        osc.update(
            Playback {
                playlist_position: 2,
                playlist_count: 3,
                chapters: vec![
                    Chapter { time: 0.0, title: "One".to_owned() },
                    Chapter { time: 60.0, title: "Two {x}".to_owned() },
                ],
                ..playback()
            },
            1280,
            720,
            now,
        );
        let events = texts(osc.overlay().expect("visible osc draws"));
        assert!(events.contains("[2/3] Title"), "{events}");
        let seek = area(&osc, "seek");
        osc.pointer_move(seek.x + seek.w * 0.9, seek.y + 5.0, now);
        let events = texts(osc.overlay().expect("visible osc draws"));
        assert!(events.contains("Chapter: Two {x}"), "{events}");
        osc.update(Playback { title: String::new(), ..playback() }, 1280, 720, now);
        osc.pointer_move(640.0, 400.0, now);
        assert!(texts(osc.overlay().expect("visible osc draws")).contains("mpv"));
    }

    #[test]
    fn hover_shows_a_tooltip_that_flips_at_the_middle() {
        let now = Instant::now();
        let mut osc = osc_at(now);
        let seek = area(&osc, "seek");
        osc.pointer_move(seek.x + seek.w * 0.25, seek.y + 1.0, now);
        let (origin, anchor, text) = tooltip(osc.overlay().expect("visible osc draws"));
        assert_eq!(text, "00:00:29");
        assert!((origin.x - (seek.x + seek.w * 0.25 + 10.0)).abs() < 1e-9 && origin.y == 705.0, "{origin:?}");
        assert_eq!(anchor, Anchor { x: Align::Start, y: Align::Center });
        osc.pointer_move(seek.x + seek.w * 0.75, seek.y + 1.0, now);
        let (origin, anchor, text) = tooltip(osc.overlay().expect("visible osc draws"));
        assert_eq!(text, "00:01:30");
        assert!((origin.x - (seek.x + seek.w * 0.75 - 5.0)).abs() < 1e-9, "{origin:?}");
        assert_eq!(anchor, Anchor { x: Align::End, y: Align::Center });
    }

    #[test]
    fn time_formats_like_mpv() {
        assert_eq!(format_time(0.0, false), "00:00:00");
        assert_eq!(format_time(61.9, false), "00:01:01");
        assert_eq!(format_time(61.9996, false), "00:01:02");
        assert_eq!(format_time(3661.0, false), "01:01:01");
        assert_eq!(format_time(3661.5, true), "01:01:01.500");
        assert_eq!(format_time(-5.0, false), "-00:00:05");
    }

    #[test]
    fn volume_icon_follows_the_reference_thresholds() {
        let at = |volume: f64, muted: bool| volume_icon(&Playback { volume, muted, ..playback() });
        assert_eq!(at(0.0, false), Icon::Mute);
        assert_eq!(at(80.0, true), Icon::Mute);
        assert_eq!(at(10.0, false), Icon::Volume(VolumeLevel::Low));
        assert_eq!(at(40.0, false), Icon::Volume(VolumeLevel::Medium));
        assert_eq!(at(100.0, false), Icon::Volume(VolumeLevel::High));
        assert_eq!(at(130.0, false), Icon::Volume(VolumeLevel::Boosted));
    }
}
