use std::path::PathBuf;
use std::time::{Duration, Instant};

use player_app::osc::{Osc, Playback};
use player_audio::sink::SinkKind;
use player_backend::engine::EngineBackend;
use player_core::command::OscVisibility;
use player_engine::{SchedulerEngine, TrackKind};

struct Options {
    fixture: String,
    base: PathBuf,
    output: PathBuf,
    width: u32,
    height: u32,
    position: f64,
    window: (u32, u32),
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
    let mut window = None;
    let mut osc = true;
    let mut text = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{argument} needs a value"));
        match argument.as_str() {
            "--fixture" => fixture = Some(value()?),
            "--base" => base = Some(PathBuf::from(value()?)),
            "--output" => output = Some(PathBuf::from(value()?)),
            "--width" => width = value()?.parse().map_err(|_| "bad width".to_owned())?,
            "--height" => height = value()?.parse().map_err(|_| "bad height".to_owned())?,
            "--position" => position = value()?.parse().map_err(|_| "bad position".to_owned())?,
            "--window" => {
                let text = value()?;
                let (width, height) = text
                    .split_once('x')
                    .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
                    .ok_or_else(|| format!("bad --window '{text}'"))?;
                window = Some((width, height));
            }
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
        window: window.unwrap_or((width, height)),
        osc,
        text,
    })
}

fn main() -> Result<(), String> {
    let options = parse_args()?;
    let mut frame = std::fs::read(&options.base)
        .map_err(|error| format!("cannot read {}: {error}", options.base.display()))?;
    let expected = options.width as usize * options.height as usize * 4;
    if frame.len() != expected {
        return Err(format!(
            "{} holds {} bytes, expected {expected}",
            options.base.display(),
            frame.len()
        ));
    }
    let duration = player_app::properties::probe(&options.fixture)?.duration;
    let engine = SchedulerEngine::load(&options.fixture, SinkKind::Null, player_engine::StartState::default())
        .map_err(|error| format!("cannot open {}: {error}", options.fixture))?;
    let mut backend = EngineBackend::open(engine);
    backend.set_paused(true);
    backend.seek_to(options.position);
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        backend.latest_frame();
        let status = backend.status();
        if status.paused && !status.tracks.is_empty() && (status.position - options.position).abs() < 0.05 {
            break status;
        }
        if Instant::now() >= deadline {
            return Err(format!("engine did not settle at {}: {status:?}", options.position));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if let Some(text) = &options.text {
        backend.show_text(text, Duration::from_secs(5));
    }
    let mut osc = options.osc.then(|| Osc::new(OscVisibility::Always, Instant::now()));
    if let Some(osc) = &mut osc {
        let count = |kind: TrackKind| status.tracks.iter().filter(|track| track.kind == kind).count();
        let audio_tracks = count(TrackKind::Audio);
        let playback = Playback {
            title: std::path::Path::new(&options.fixture)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            paused: true,
            volume: 100.0,
            position: status.position,
            duration,
            audio_tracks,
            sub_tracks: count(TrackKind::Subtitle),
            aid: (audio_tracks > 0).then_some(1),
            ..Playback::default()
        };
        let (width, height) = options.window;
        osc.update(playback, width, height, Instant::now());
    }
    let overlay = osc.as_mut().and_then(Osc::overlay);
    backend.compose(&mut frame, options.width, options.height, overlay)?;
    std::fs::write(&options.output, &frame)
        .map_err(|error| format!("cannot write {}: {error}", options.output.display()))
}
