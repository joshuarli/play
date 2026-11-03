use std::path::PathBuf;
use std::time::{Duration, Instant};

use player_audio::sink::SinkKind;
use player_engine::SchedulerEngine;
use player_ffi::engine::{EngineBackend, SharedEngine};
use player_lua::queue::EventSender;
use player_lua::{PlayerBackend, ScriptHost, ScriptModule};

const ASSDRAW: &str = include_str!("../../../player/lua/assdraw.lua");
const OPTIONS: &str = include_str!("../../../player/lua/options.lua");
const OSC: &str = include_str!("../../../player/lua/osc.lua");

struct Options {
    fixture: String,
    base: PathBuf,
    output: PathBuf,
    width: u32,
    height: u32,
    position: f64,
    subtitle: Option<i64>,
    ass: Option<PathBuf>,
    osc: bool,
    text: Option<String>,
}

fn parse_args() -> Result<Options, String> {
    let mut fixture = None;
    let mut base = None;
    let mut output = None;
    let mut width = 320;
    let mut height = 240;
    let mut position = 1.0;
    let mut subtitle = None;
    let mut ass = None;
    let mut osc = true;
    let mut text = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--fixture" => fixture = Some(value()?),
            "--base" => base = Some(PathBuf::from(value()?)),
            "--output" => output = Some(PathBuf::from(value()?)),
            "--width" => width = value()?.parse().map_err(|_| "bad width".to_owned())?,
            "--height" => height = value()?.parse().map_err(|_| "bad height".to_owned())?,
            "--position" => position = value()?.parse().map_err(|_| "bad position".to_owned())?,
            "--sid" => subtitle = Some(value()?.parse().map_err(|_| "bad sid".to_owned())?),
            "--ass" => ass = Some(PathBuf::from(value()?)),
            "--osc" => {
                osc = match value()?.as_str() {
                    "yes" => true,
                    "no" => false,
                    other => return Err(format!("bad --osc value '{other}'")),
                }
            }
            "--text" => text = Some(value()?),
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(Options {
        fixture: fixture.ok_or_else(|| "--fixture is required".to_owned())?,
        base: base.ok_or_else(|| "--base is required".to_owned())?,
        output: output.ok_or_else(|| "--output is required".to_owned())?,
        width,
        height,
        position,
        subtitle,
        ass,
        osc,
        text,
    })
}

fn drive(
    shared: &SharedEngine,
    sender: &EventSender,
    host: &mut ScriptHost,
    timeout: Duration,
    done: impl Fn() -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        shared
            .pump(sender)
            .map_err(|error| format!("pump failed: {error}"))?;
        if let Some(result) = host.poll() {
            result.map_err(|error| format!("osc stopped: {error}"))?;
        }
        if done() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("timed out driving osc".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn main() -> Result<(), String> {
    let options = parse_args()?;
    let base = std::fs::read(&options.base)
        .map_err(|error| format!("cannot read {}: {error}", options.base.display()))?;
    let expected = options.width as usize * options.height as usize * 4;
    if base.len() != expected {
        return Err(format!(
            "{} holds {} bytes, expected {expected}",
            options.base.display(),
            base.len()
        ));
    }

    let engine = SchedulerEngine::load(&options.fixture, SinkKind::Null)
        .map_err(|error| format!("cannot open {}: {error}", options.fixture))?;
    engine.set_paused(true);
    engine.seek(options.position.max(0.0));
    let mut backend = EngineBackend::open(engine);
    backend.set_display_size(options.width, options.height);
    if let Some(id) = options.subtitle {
        backend
            .set_property("sid", player_lua::NativeValue::Int(id))
            .map_err(|error| format!("cannot select subtitle {id}: {error}"))?;
    }
    let shared = SharedEngine::new(backend);
    if options.text.is_some() || !options.osc {
        if let Some(text) = &options.text {
            shared
                .mutate(|backend| {
                    backend.command(player_lua::CommandRequest::Args(vec![
                        "show-text".to_owned(),
                        text.clone(),
                        "5000".to_owned(),
                        "0".to_owned(),
                    ]))
                })
                .ok_or_else(|| "engine backend lock failed".to_owned())?
                .map_err(|error| format!("cannot show text: {error}"))?;
        }
        let mut frame = base;
        shared
            .mutate(|backend| backend.compose(&mut frame, options.width, options.height, 1.0))
            .ok_or_else(|| "engine backend lock failed".to_owned())?
            .map_err(|error| format!("cannot compose osd: {error}"))?;
        std::fs::write(&options.output, &frame)
            .map_err(|error| format!("cannot write {}: {error}", options.output.display()))?;
        let error = shared.inspect(|backend| backend.osd_error().cloned()).flatten();
        if let Some(message) = error {
            return Err(message);
        }
        return Ok(());
    }
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

    drive(
        &shared,
        &sender,
        &mut host,
        Duration::from_secs(20),
        || {
            shared
                .inspect(|backend| {
                    backend
                        .enabled_events()
                        .iter()
                        .any(|name| name == "client-message")
                })
                .unwrap_or(false)
        },
    )?;
    let settle = Instant::now() + Duration::from_millis(800);
    drive(
        &shared,
        &sender,
        &mut host,
        Duration::from_secs(20),
        || Instant::now() >= settle,
    )?;
    shared
        .mutate(|backend| {
            backend
                .command(player_lua::CommandRequest::Args(vec![
                    "script-message".to_owned(),
                    "osc-visibility".to_owned(),
                    "always".to_owned(),
                ]))
                .map_err(|error| format!("cannot show osc: {error}"))
        })
        .ok_or_else(|| "engine backend lock failed".to_owned())??;

    let position = options.position;
    drive(
        &shared,
        &sender,
        &mut host,
        Duration::from_secs(20),
        || {
            shared
                .inspect(|backend| {
                    backend.user_value("user-data/osc/visibility")
                        == Some(player_lua::NativeValue::String("always".to_owned()))
                })
                .unwrap_or(false)
        },
    )?;
    drive(
        &shared,
        &sender,
        &mut host,
        Duration::from_secs(20),
        || {
            shared
                .inspect(|backend| {
                    let ready = backend.overlay(1).map(|overlay| {
                        overlay.data.contains("100%")
                            && overlay.data.contains("mpv-osd-symbols")
                            && overlay.data.contains(&format_time(position))
                    });
                    ready.unwrap_or(false)
                })
                .unwrap_or(false)
        },
    )?;
    let deadline = Instant::now() + Duration::from_millis(1200);
    drive(
        &shared,
        &sender,
        &mut host,
        Duration::from_secs(20),
        || Instant::now() >= deadline,
    )?;

    if let Some(path) = &options.ass {
        let data = shared
            .inspect(|backend| backend.overlay(1).map(|overlay| overlay.data.clone()))
            .flatten()
            .ok_or_else(|| "osc overlay is missing".to_owned())?;
        std::fs::write(path, data)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    std::thread::sleep(Duration::from_millis(1200));
    shared.pump(&sender).map_err(|error| format!("pump failed: {error}"))?;

    let mut frame = base;
    shared
        .mutate(|backend| backend.compose(&mut frame, options.width, options.height, 1.0))
        .ok_or_else(|| "engine backend lock failed".to_owned())?
        .map_err(|error| format!("cannot compose osd: {error}"))?;
    std::fs::write(&options.output, &frame)
        .map_err(|error| format!("cannot write {}: {error}", options.output.display()))?;

    let error = shared.inspect(|backend| backend.osd_error().cloned()).flatten();
    if let Some(message) = error {
        return Err(message);
    }

    shared
        .mutate(|backend| {
            backend.command(player_lua::CommandRequest::Args(vec!["quit".to_owned()]))
        })
        .ok_or_else(|| "engine backend lock failed".to_owned())?
        .map_err(|error| format!("quit failed: {error}"))?;
    let _ = shared.pump(&sender);
    let _ = host.wait_timeout(Duration::from_secs(5));
    Ok(())
}

fn format_time(seconds: f64) -> String {
    let whole = seconds.abs() as i64;
    if whole >= 3600 {
        format!("{:02}:{:02}:{:02}", whole / 3600, (whole / 60) % 60, whole % 60)
    } else {
        format!("{:02}:{:02}", whole / 60, whole % 60)
    }
}
