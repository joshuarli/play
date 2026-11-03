use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use player_audio::sink::SinkKind;
use player_engine::video::PresentedFrame;
use player_engine::SchedulerEngine;
use player_ffi::engine::{EngineBackend, SharedEngine};
use player_lua::{CommandRequest, NativeValue, PlayerBackend, ScriptHost, ScriptModule};
use player_output::{ColorSpec, Overlay, OverlayRect, PixelLayout, Plane};

#[cfg(target_os = "linux")]
use player_display::{InputEvent, Window};
#[cfg(target_os = "macos")]
use player_display::{MacInput, MacWindow};

const ASSDRAW: &str = include_str!("../../../player/lua/assdraw.lua");
const OPTIONS: &str = include_str!("../../../player/lua/options.lua");
const OSC: &str = include_str!("../../../player/lua/osc.lua");

const OSD_INTERVAL: Duration = Duration::from_millis(50);
const GATE_FRAMES: u64 = 24;
const GATE_OVERLAY_INK: usize = 100;

struct Options {
    path: String,
    width: i32,
    height: i32,
    gate: bool,
    start: f64,
    pause: bool,
    fullscreen: bool,
    null_sink: bool,
}

fn parse_options() -> Result<Options, String> {
    let mut path = None;
    let mut width = 1280;
    let mut height = 720;
    let mut gate = false;
    let mut start: f64 = 0.0;
    let mut pause = false;
    let mut fullscreen = false;
    let mut null_sink = false;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--gate" => gate = true,
            "--pause" => pause = true,
            "--fullscreen" => fullscreen = true,
            "--ao=null" | "--ao" => {
                if argument == "--ao" && value()? != "null" {
                    return Err("only --ao=null is supported".to_owned());
                }
                null_sink = true;
            }
            "--width" => width = value()?.parse().map_err(|_| "bad width".to_owned())?,
            "--height" => height = value()?.parse().map_err(|_| "bad height".to_owned())?,
            "--start" => start = value()?.parse().map_err(|_| "bad start time".to_owned())?,
            "--help" | "-h" => {
                println!(
                    "usage: player-gui [--gate] [--ao=null] [--width N] [--height N]\n\
                     \x20                  [--start SECONDS] [--pause] [--fullscreen] FILE"
                );
                std::process::exit(0);
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown argument '{other}'"));
            }
            other => {
                if path.replace(other.to_owned()).is_some() {
                    return Err("only one file can be opened".to_owned());
                }
            }
        }
    }
    Ok(Options {
        path: path.ok_or_else(|| "a file argument is required".to_owned())?,
        width: width.max(64),
        height: height.max(64),
        gate,
        start: start.max(0.0),
        pause,
        fullscreen,
        null_sink,
    })
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Quit,
    Escape,
    Pause,
    Fullscreen,
    Seek(f64),
    Volume(f64),
    Mute,
    Osc,
    Subtitle,
}

enum HostEvent {
    Key {
        code: u32,
        pressed: bool,
        repeat: bool,
        command: bool,
        shift: bool,
    },
    PointerMotion,
    PointerLeave,
}

enum HostWindow {
    #[cfg(target_os = "linux")]
    Wayland(Window),
    #[cfg(target_os = "macos")]
    Mac(MacWindow),
}

impl HostWindow {
    fn open(title: &str, width: i32, height: i32) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            Window::open(title, width, height).map(HostWindow::Wayland)
        }
        #[cfg(target_os = "macos")]
        {
            MacWindow::open(title, width, height).map(HostWindow::Mac)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (title, width, height);
            Err("no window backend for this platform".to_owned())
        }
    }

    fn width(&self) -> i32 {
        match self {
            #[cfg(target_os = "linux")]
            HostWindow::Wayland(window) => window.width(),
            #[cfg(target_os = "macos")]
            HostWindow::Mac(window) => window.width(),
        }
    }

    fn height(&self) -> i32 {
        match self {
            #[cfg(target_os = "linux")]
            HostWindow::Wayland(window) => window.height(),
            #[cfg(target_os = "macos")]
            HostWindow::Mac(window) => window.height(),
        }
    }

    fn presented(&self) -> u64 {
        match self {
            #[cfg(target_os = "linux")]
            HostWindow::Wayland(window) => window.presented(),
            #[cfg(target_os = "macos")]
            HostWindow::Mac(window) => window.presented(),
        }
    }

    fn set_fullscreen(&mut self, fullscreen: bool) {
        match self {
            #[cfg(target_os = "linux")]
            HostWindow::Wayland(window) => window.set_fullscreen(fullscreen),
            #[cfg(target_os = "macos")]
            HostWindow::Mac(window) => window.set_fullscreen(fullscreen),
        }
    }

    fn poll(&mut self) -> Result<bool, String> {
        match self {
            #[cfg(target_os = "linux")]
            HostWindow::Wayland(window) => window.poll(),
            #[cfg(target_os = "macos")]
            HostWindow::Mac(window) => window.poll(),
        }
    }

    fn drain_input(&mut self) -> Result<Vec<HostEvent>, String> {
        match self {
            #[cfg(target_os = "linux")]
            HostWindow::Wayland(window) => {
                let events = window.drain_input()?;
                Ok(events
                    .into_iter()
                    .filter_map(|event| match event {
                        InputEvent::Key { code, pressed } => Some(HostEvent::Key {
                            code,
                            pressed,
                            repeat: false,
                            command: false,
                            shift: false,
                        }),
                        InputEvent::PointerMotion { .. } => Some(HostEvent::PointerMotion),
                        InputEvent::PointerLeave => Some(HostEvent::PointerLeave),
                        _ => None,
                    })
                    .collect())
            }
            #[cfg(target_os = "macos")]
            HostWindow::Mac(window) => Ok(window
                .drain_input()
                .into_iter()
                .filter_map(|event| match event {
                    MacInput::Key {
                        key_code,
                        modifiers,
                        pressed,
                        repeat,
                    } => Some(HostEvent::Key {
                        code: key_code as u32,
                        pressed,
                        repeat,
                        command: modifiers & (1 << 20) != 0,
                        shift: modifiers & (1 << 17) != 0,
                    }),
                    MacInput::PointerMotion { .. } => Some(HostEvent::PointerMotion),
                    MacInput::PointerLeave => Some(HostEvent::PointerLeave),
                })
                .collect()),
        }
    }

    fn present(
        &mut self,
        frame: &PresentedFrame,
        overlay: Option<&Overlay<'_>>,
    ) -> Result<(), String> {
        if frame.width == 0 || frame.height == 0 {
            return Ok(());
        }
        let width = frame.width as i32;
        let height = frame.height as i32;
        let planes = [Plane {
            data: &frame.rgba,
            stride: frame.width as usize * 4,
            width,
            height,
        }];
        match self {
            #[cfg(target_os = "linux")]
            HostWindow::Wayland(window) => window.present_planes(
                PixelLayout::Rgba8,
                ColorSpec::bt709_full(),
                &planes,
                width,
                height,
                overlay,
            ),
            #[cfg(target_os = "macos")]
            HostWindow::Mac(window) => window.present_planes(
                PixelLayout::Rgba8,
                ColorSpec::bt709_full(),
                &planes,
                width,
                height,
                overlay,
            ),
        }
    }
}

fn key_action(code: u32, command: bool, shift: bool) -> Option<Action> {
    let base = code & 0xffff;
    if command && base == 12 {
        return Some(Action::Quit);
    }
    #[cfg(target_os = "macos")]
    let action = match base {
        49 => Some(Action::Pause),
        12 => Some(Action::Quit),
        53 => Some(Action::Escape),
        3 => Some(Action::Fullscreen),
        123 => Some(if shift {
            Action::Seek(-60.0)
        } else {
            Action::Seek(-5.0)
        }),
        124 => Some(if shift {
            Action::Seek(60.0)
        } else {
            Action::Seek(5.0)
        }),
        126 => Some(Action::Volume(5.0)),
        125 => Some(Action::Volume(-5.0)),
        46 => Some(Action::Mute),
        31 => Some(Action::Osc),
        38 => Some(Action::Subtitle),
        _ => None,
    };
    #[cfg(target_os = "linux")]
    let action = match base {
        57 => Some(Action::Pause),
        16 => Some(Action::Quit),
        1 => Some(Action::Escape),
        33 => Some(Action::Fullscreen),
        105 => Some(if shift {
            Action::Seek(-60.0)
        } else {
            Action::Seek(-5.0)
        }),
        106 => Some(if shift {
            Action::Seek(60.0)
        } else {
            Action::Seek(5.0)
        }),
        103 => Some(Action::Volume(5.0)),
        108 => Some(Action::Volume(-5.0)),
        50 => Some(Action::Mute),
        24 => Some(Action::Osc),
        36 => Some(Action::Subtitle),
        _ => None,
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let action = {
        let _ = (base, shift);
        None
    };
    action
}

fn run(command: &SharedEngine, args: &[&str]) -> Result<(), String> {
    let argv = args.iter().map(|arg| (*arg).to_owned()).collect();
    command
        .mutate(|backend| backend.command(CommandRequest::Args(argv)))
        .ok_or_else(|| "engine backend lock failed".to_owned())?
        .map(|_| ())
}

fn script_message(command: &SharedEngine, args: &[&str]) -> Result<(), String> {
    let mut argv = vec!["script-message".to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    command
        .mutate(|backend| backend.command(CommandRequest::Args(argv)))
        .ok_or_else(|| "engine backend lock failed".to_owned())?
        .map(|_| ())
}

fn overlay_hash(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn overlay_ink(data: &[u8]) -> usize {
    data.chunks_exact(4).filter(|pixel| pixel[3] > 0).count()
}

fn overlay_fits(osd: &Option<Vec<u8>>, width: i32, height: i32) -> bool {
    osd.as_ref()
        .is_some_and(|data| data.len() == width as usize * height as usize * 4)
}

fn run_player(options: Options) -> Result<(), String> {
    let title = Path::new(&options.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("player");
    let mut window = HostWindow::open(title, options.width, options.height)?;
    if options.fullscreen {
        window.set_fullscreen(true);
    }
    let sink = if options.null_sink {
        SinkKind::Null
    } else {
        SinkKind::Default
    };
    let engine = SchedulerEngine::load(&options.path, sink)?;
    if options.pause {
        engine.set_paused(true);
    }
    if options.start > 0.0 {
        engine.seek(options.start);
    }
    let mut backend = EngineBackend::open(engine);
    backend.set_overlay_mode();
    backend.set_display_size(
        window.width().max(1) as u32,
        window.height().max(1) as u32,
    );
    if options.fullscreen {
        backend
            .command(CommandRequest::Args(vec![
                "set".to_owned(),
                "fullscreen".to_owned(),
                "yes".to_owned(),
            ]))
            .map_err(|error| format!("cannot set fullscreen: {error}"))?;
    }
    let shared = SharedEngine::new(backend);
    let mut host = ScriptHost::spawn_with_modules(
        "osc",
        "@osc.lua",
        OSC,
        vec![
            ScriptModule::new("mp.assdraw", ASSDRAW),
            ScriptModule::new("mp.options", OPTIONS),
        ],
        shared.clone(),
    )
    .map_err(|error| format!("cannot start osc: {error}"))?;
    let sender = host.event_sender().sender();

    let gate_deadline = Instant::now() + Duration::from_secs(20);
    let mut gate_ready_at = None;
    let mut last_frame: Option<PresentedFrame> = None;
    let mut last_size = (0, 0);
    let mut osd: Option<Vec<u8>> = None;
    let mut osd_size = (0, 0);
    let mut osd_hash = 0;
    let mut last_osd = Instant::now()
        .checked_sub(OSD_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut pressed = HashSet::new();
    let mut quit = false;
    let mut applied_fullscreen = options.fullscreen;
    let mut last_visibility = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut last_report = Instant::now();
    let mut pumps = 0u64;

    while !quit {
        if window.poll()? {
            break;
        }
        for event in window.drain_input()? {
            match event {
                HostEvent::Key {
                    code,
                    pressed: down,
                    repeat,
                    command,
                    shift,
                } => {
                    if !down {
                        pressed.remove(&code);
                        continue;
                    }
                    if repeat || !pressed.insert(code) {
                        continue;
                    }
                    let action = key_action(code, command, shift);
                    match action {
                        Some(Action::Quit) => quit = true,
                        Some(Action::Escape) => {
                            if applied_fullscreen {
                                run(&shared, &["cycle", "fullscreen"])?;
                            } else {
                                quit = true;
                            }
                        }
                        Some(Action::Pause) => run(&shared, &["cycle", "pause"])?,
                        Some(Action::Fullscreen) => run(&shared, &["cycle", "fullscreen"])?,
                        Some(Action::Seek(delta)) => {
                            run(&shared, &["seek", &format!("{delta}"), "relative"])?
                        }
                        Some(Action::Volume(delta)) => {
                            run(&shared, &["add", "volume", &format!("{delta}")])?
                        }
                        Some(Action::Mute) => run(&shared, &["cycle", "mute"])?,
                        Some(Action::Osc) => {
                            script_message(&shared, &["osc-visibility", "cycle"])?
                        }
                        Some(Action::Subtitle) => run(&shared, &["cycle", "sub"])?,
                        None => {}
                    }
                }
                HostEvent::PointerMotion => {
                    let _ = script_message(&shared, &["osc-show"]);
                }
                HostEvent::PointerLeave => {
                    let _ = script_message(&shared, &["osc-hide"]);
                }
            }
        }
        if shared.inspect(|backend| backend.should_quit()).unwrap_or(false) {
            break;
        }
        let wanted_fullscreen = shared
            .inspect(|backend| backend.fullscreen())
            .unwrap_or(false);
        if wanted_fullscreen != applied_fullscreen {
            window.set_fullscreen(wanted_fullscreen);
            applied_fullscreen = wanted_fullscreen;
        }

        shared
            .pump(&sender)
            .map_err(|error| format!("engine pump failed: {error}"))?;
        pumps += 1;
        if let Some(result) = host.poll() {
            result.map_err(|error| format!("osc stopped: {error}"))?;
            break;
        }

        let frames = shared
            .mutate(|backend| backend.take_frames())
            .ok_or_else(|| "engine backend lock failed".to_owned())?;
        let new_frames = !frames.is_empty();
        if let Some(frame) = frames.into_iter().last() {
            last_frame = Some(frame);
        }

        let width = window.width().max(1);
        let height = window.height().max(1);
        let size = (width, height);
        let now = Instant::now();
        let mut osd_changed = false;
        if size != osd_size || now.duration_since(last_osd) >= OSD_INTERVAL {
            last_osd = now;
            let rendered = shared
                .mutate(|backend| {
                    backend.set_display_size(width as u32, height as u32);
                    backend.render_overlay(width as u32, height as u32, 1.0)
                })
                .ok_or_else(|| "engine backend lock failed".to_owned())??;
            let hash = rendered.as_deref().map(overlay_hash).unwrap_or(0);
            osd_changed = hash != osd_hash || size != osd_size;
            osd = rendered;
            osd_size = size;
            osd_hash = hash;
        }

        if let Some(frame) = &last_frame {
            if new_frames || osd_changed || size != last_size {
                if overlay_fits(&osd, width, height) {
                    let data = osd.as_deref().expect("overlay size checked");
                    let overlay = Overlay {
                        data,
                        stride: width as usize * 4,
                        width,
                        height,
                        dst: OverlayRect {
                            x0: 0.0,
                            y0: 0.0,
                            x1: width as f32,
                            y1: height as f32,
                        },
                    };
                    window.present(frame, Some(&overlay))?;
                } else {
                    window.present(frame, None)?;
                }
                last_size = size;
            }
        }

        if let Some(Some(error)) = shared.inspect(|backend| backend.engine_error()) {
            return Err(format!("engine error: {error}"));
        }

        if options.gate {
            let visibility = shared
                .inspect(|backend| backend.user_value("user-data/osc/visibility"))
                .flatten();
            if visibility != Some(NativeValue::String("always".to_owned()))
                && last_visibility.elapsed() >= Duration::from_millis(200)
            {
                last_visibility = Instant::now();
                script_message(&shared, &["osc-hide"])?;
                script_message(&shared, &["osc-visibility", "always"])?;
            }
            if last_report.elapsed() >= Duration::from_secs(1) {
                last_report = Instant::now();
                let status = shared
                    .inspect(|backend| backend.status())
                    .unwrap_or_default();
                eprintln!(
                    "player-gui gate: frames={} drained={} pumps={pumps} pos={:.2} ended={} error={:?} dims={}x{} tracks={} osd={}",
                    window.presented(),
                    status.presented,
                    status.position,
                    status.ended,
                    status.error,
                    status.video_width,
                    status.video_height,
                    status.tracks.len(),
                    osd.as_ref().map(|data| data.len()).unwrap_or(0),
                );
            }
            let ink = osd.as_deref().map(overlay_ink).unwrap_or(0);
            let status = shared
                .inspect(|backend| backend.status())
                .unwrap_or_default();
            let played_to_end = status.ended || status.position >= 2.5;
            if window.presented() >= GATE_FRAMES && ink >= GATE_OVERLAY_INK && played_to_end {
                if gate_ready_at.is_none() {
                    gate_ready_at = Some(Instant::now());
                } else if gate_ready_at.is_some_and(|ready| {
                    ready.elapsed() >= Duration::from_millis(200)
                }) {
                    println!(
                        "player-gui gate: PASS frames={} overlay_ink={ink}",
                        window.presented()
                    );
                    break;
                }
            }
            if Instant::now() >= gate_deadline {
                return Err(format!(
                    "gui gate timed out: frames={} overlay_ink={ink}",
                    window.presented()
                ));
            }
        }

        std::thread::sleep(Duration::from_millis(5));
    }

    let _ = host.stop();
    Ok(())
}

fn main() {
    let result = parse_options().and_then(run_player);
    if let Err(error) = result {
        eprintln!("player-gui: {error}");
        std::process::exit(1);
    }
}
