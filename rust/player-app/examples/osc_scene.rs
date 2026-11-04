use std::path::PathBuf;
use std::time::Instant;

use player_app::osc::{Chapter, Osc, Playback};
use player_backend::overlay::{OverlayRenderer, Scene};
use player_core::command::OscVisibility;

struct Options {
    base: PathBuf,
    output: PathBuf,
    width: u32,
    height: u32,
    playback: Playback,
    pointer: Option<(f64, f64)>,
    clicks: Vec<(f64, f64)>,
}

fn parse_args() -> Result<Options, String> {
    let mut base = None;
    let mut output = None;
    let mut width = 1280;
    let mut height = 720;
    let mut playback = Playback {
        paused: true,
        volume: 100.0,
        ..Playback::default()
    };
    let mut pointer = None;
    let mut clicks = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--muted" {
            playback.muted = true;
            continue;
        }
        if argument == "--playing" {
            playback.paused = false;
            continue;
        }
        if argument == "--fullscreen" {
            playback.fullscreen = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{argument} needs a value"))?;
        let number = |name: &str| value.parse::<f64>().map_err(|_| format!("bad {name} '{value}'"));
        match argument.as_str() {
            "--base" => base = Some(PathBuf::from(&value)),
            "--output" => output = Some(PathBuf::from(&value)),
            "--width" => width = number("width")? as u32,
            "--height" => height = number("height")? as u32,
            "--title" => playback.title = value.clone(),
            "--position" => playback.position = number("position")?,
            "--duration" => playback.duration = number("duration")?,
            "--speed" => playback.speed = number("speed")?,
            "--volume" => playback.volume = number("volume")?,
            "--audio-tracks" => playback.audio_tracks = number("audio-tracks")? as usize,
            "--sub-tracks" => playback.sub_tracks = number("sub-tracks")? as usize,
            "--aid" => playback.aid = Some(number("aid")? as i64),
            "--sid" => playback.sid = Some(number("sid")? as i64),
            "--chapter" => {
                let (time, title) = value
                    .split_once(':')
                    .and_then(|(time, title)| Some((time.parse().ok()?, title.to_owned())))
                    .ok_or_else(|| format!("bad --chapter '{value}'"))?;
                playback.chapters.push(Chapter { time, title });
            }
            "--playlist" => {
                let (position, count) = value
                    .split_once('/')
                    .and_then(|(position, count)| Some((position.parse().ok()?, count.parse().ok()?)))
                    .ok_or_else(|| format!("bad --playlist '{value}'"))?;
                playback.playlist_position = position;
                playback.playlist_count = count;
            }
            "--click" => {
                let (x, y) = value
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))
                    .ok_or_else(|| format!("bad --click '{value}'"))?;
                clicks.push((x, y));
            }
            "--pointer" => {
                let (x, y) = value
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))
                    .ok_or_else(|| format!("bad --pointer '{value}'"))?;
                pointer = Some((x, y));
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(Options {
        base: base.ok_or_else(|| "--base is required".to_owned())?,
        output: output.ok_or_else(|| "--output is required".to_owned())?,
        width,
        height,
        playback,
        pointer,
        clicks,
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
    let now = Instant::now();
    let mut osc = Osc::new(OscVisibility::Always, now);
    osc.update(options.playback.clone(), options.width, options.height, now);
    for (x, y) in &options.clicks {
        osc.pointer_move(*x, *y, now);
        osc.button(true);
        osc.button(false);
        osc.update(options.playback.clone(), options.width, options.height, now);
    }
    if let Some((x, y)) = options.pointer {
        osc.pointer_move(x, y, now);
    }
    let scene = Scene { subtitle: None, message: None, osc: osc.overlay() };
    if let Some(image) = OverlayRenderer::new()?.render(&scene, options.width, options.height)? {
        image.blend_onto(&mut frame, options.width, options.height)?;
    }
    std::fs::write(&options.output, &frame)
        .map_err(|error| format!("cannot write {}: {error}", options.output.display()))
}
