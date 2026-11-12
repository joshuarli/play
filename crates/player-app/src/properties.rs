use player_engine::{EngineStatus, TrackKind};
use player_core::command::{OscVisibility, OscVisibilityChange};
use player_media::{ChapterInfo, StreamInfo, StreamKind};

use crate::app::App;
use crate::osc::Rect;
use crate::value::NativeValue;

pub struct Probe {
    pub duration: f64,
    pub metadata: Vec<(String, String)>,
    pub chapters: Vec<ChapterInfo>,
    pub streams: Vec<StreamInfo>,
    pub format: String,
    pub video_frame_rate: Option<f64>,
    pub video_geometry: player_media::VideoGeometry,
}

impl Probe {
    pub fn displays_video(&self) -> bool {
        let mut video = self
            .streams
            .iter()
            .filter(|stream| stream.kind == StreamKind::Video)
            .peekable();
        if video.peek().is_none() {
            return false;
        }
        let has_audio = self.streams.iter().any(|stream| stream.kind == StreamKind::Audio);
        !has_audio || video.any(|stream| !is_cover_art(stream))
    }

    pub fn is_music(&self) -> bool {
        self.streams.iter().any(|stream| stream.kind == StreamKind::Audio) && !self.displays_video()
    }
}

fn is_cover_art(stream: &StreamInfo) -> bool {
    stream.attached || matches!(stream.codec_name.as_str(), "png" | "bmp" | "tiff" | "webp")
}

pub fn probe(path: &str) -> Result<Probe, String> {
    let session = player_media::MediaSession::open(path).map_err(|error| error.to_string())?;
    let duration = session
        .streams()
        .iter()
        .filter(|stream| {
            matches!(
                stream.kind,
                StreamKind::Audio | StreamKind::Video
            )
        })
        .filter_map(|stream| stream.duration_seconds)
        .reduce(f64::max)
        .filter(|duration| *duration > 0.0)
        .unwrap_or_else(|| session.duration_seconds().unwrap_or(0.0))
        .max(0.0);
    let mut metadata = session.metadata();
    let format = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let streams = session.streams();
    let video_frame_rate = session.video_frame_rate();
    let video_geometry = streams.iter().find(|stream| stream.kind == StreamKind::Video)
        .map(|stream| session.video_geometry(stream.index)).unwrap_or_default();
    if matches!(format.as_str(), "mp3" | "ogg" | "m4a") {
        for stream in &streams {
            if stream.kind != StreamKind::Audio {
                continue;
            }
            for (key, value) in session.stream_metadata(stream.index) {
                let key = key.to_ascii_lowercase();
                if matches!(key.as_str(), "encoder" | "handler_name" | "language")
                    && !metadata.iter().any(|(known, _)| known.eq_ignore_ascii_case(&key))
                {
                    metadata.push((key, value));
                }
            }
        }
    }
    let chapters = session.chapters();
    if matches!(format.as_str(), "mkv" | "webm") {
        metadata.retain(|(key, _)| !key.eq_ignore_ascii_case("encoder"));
    }
    Ok(Probe {
        duration,
        metadata,
        chapters,
        streams,
        format,
        video_frame_rate,
        video_geometry,
    })
}

pub fn get(app: &App, name: &str) -> Result<NativeValue, String> {
    let status = app.status();
    match name {
        "pause" => Ok(NativeValue::Bool(app.paused)),
        "speed" => Ok(NativeValue::Double(app.speed)),
        "control-state" => Ok(NativeValue::String(format!(
            "pause={} volume={:.3} speed={:.3}",
            if app.paused { 1 } else { 0 },
            app.volume,
            app.speed
        ))),
        "time-pos" | "playback-time" => Ok(NativeValue::Double(status.position)),
        "duration" => Ok(NativeValue::Double(
            app.probe.as_ref().map_or(status.duration, |probe| probe.duration),
        )),
        "percent-pos" => {
            let duration = app.probe.as_ref().map_or(status.duration, |probe| probe.duration);
            let percent = if duration > 0.0 {
                status.position / duration * 100.0
            } else {
                0.0
            };
            Ok(NativeValue::Double(percent))
        }
        "volume" => Ok(NativeValue::Double(app.volume)),
        "mute" => Ok(NativeValue::Bool(app.muted)),
        "fullscreen" => Ok(NativeValue::Bool(app.fullscreen)),
        "eof-reached" => Ok(NativeValue::Bool(app.eof_reached)),
        "vo-configured" => Ok(NativeValue::Bool(
            app.backend.is_some() && app.probe.as_ref().is_some_and(Probe::displays_video),
        )),
        "ao-configured" => Ok(NativeValue::Bool(
            app.backend.is_some()
                && app
                    .probe
                    .as_ref()
                    .is_some_and(|probe| probe.streams.iter().any(|stream| stream.kind == StreamKind::Audio)),
        )),
        "video-format" => Ok(NativeValue::String("yuv420p".to_owned())),
        "audio-format" => Ok(NativeValue::String("floatp".to_owned())),
        "video-out-params" => Ok(video_params(&app.probe, app.presented_video_geometry())),
        "audio-out-params" => Ok(audio_params(&app.probe)),
        "idle-active" => Ok(NativeValue::Bool(app.backend.is_none())),
        "core-idle" => Ok(NativeValue::Bool(app.backend.is_none())),
        "keep-open" => Ok(NativeValue::Bool(app.options.keep_open)),
        "path" => Ok(NativeValue::String(
            app.playlist
                .current_entry()
                .map(|entry| entry.path.clone())
                .unwrap_or_default(),
        )),
        "filename" => Ok(NativeValue::String(
            app.playlist
                .current_entry()
                .and_then(|entry| {
                    std::path::Path::new(&entry.path)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_default(),
        )),
        "media-title" => Ok(NativeValue::String(
            app.playlist
                .current_entry()
                .and_then(|entry| {
                    entry.title.clone().or_else(|| {
                        std::path::Path::new(&entry.path)
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
                })
                .unwrap_or_default(),
        )),
        "metadata" => Ok(metadata(&app.probe)),
        "chapter-list" => Ok(chapters(&app.probe)),
        "chapter" => Ok(NativeValue::Int(chapter_at(app, status.position))),
        "track-list" => Ok(track_list(app, &status)),
        "sid" => Ok(sid(&status)),
        "aid" => Ok(if app.aid_disabled {
            NativeValue::Bool(false)
        } else {
            app.aid
                .map(NativeValue::Int)
                .unwrap_or_else(|| aid(&status))
        }),
        "sub-visibility" => Ok(NativeValue::Bool(
            app.backend.as_ref().is_none_or(|backend| backend.sub_visible()),
        )),
        "video-codec" => Ok(NativeValue::String(codec(&app.probe, StreamKind::Video))),
        "audio-codec" => Ok(NativeValue::String(codec(&app.probe, StreamKind::Audio))),
        "width" => Ok(NativeValue::Int(video_dimension(&app.probe, 0))),
        "height" => Ok(NativeValue::Int(video_dimension(&app.probe, 1))),
        "video-params" => Ok(video_params(&app.probe, app.presented_video_geometry())),
        "audio-params" => Ok(audio_params(&app.probe)),
        "file-format" => Ok(NativeValue::String(
            app.probe.as_ref().map(|probe| probe.format.clone()).unwrap_or_default(),
        )),
        "demuxer" => Ok(NativeValue::String(demuxer(app))),
        "playlist" => Ok(playlist(app)),
        "playlist-count" => Ok(NativeValue::Int(app.playlist.count() as i64)),
        "playlist-pos" | "playlist-current-pos" => Ok(app
            .playlist
            .current()
            .map(|index| NativeValue::Int(index as i64))
            .unwrap_or(NativeValue::Int(-1))),
        "playlist-playing-pos" => Ok(app
            .playlist
            .current()
            .map(|index| NativeValue::Int(index as i64))
            .unwrap_or(NativeValue::Int(-1))),
        "sub-text" => Ok(NativeValue::String(
            app.backend
                .as_ref()
                .map(|backend| backend.status().subtitle_text)
                .unwrap_or_default(),
        )),
        "osd-dimensions" => Ok(osd_dimensions(app)),
        "window-scale" => Ok(NativeValue::Double(app.window_scale)),
        "osd-width" => Ok(NativeValue::Int(dimensions(app).0)),
        "osd-height" => Ok(NativeValue::Int(dimensions(app).1)),
        "mouse-pos" => Ok(NativeValue::Map(vec![
            ("x".to_owned(), NativeValue::Double(0.0)),
            ("y".to_owned(), NativeValue::Double(0.0)),
            ("hover".to_owned(), NativeValue::Bool(false)),
        ])),
        "pid" => Ok(NativeValue::Int(std::process::id() as i64)),
        "working-directory" => Ok(NativeValue::String(
            std::env::current_dir()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )),
        "input-bindings" => Ok(input_bindings()),
        "options/ao" => Ok(NativeValue::String(if app.options.null_audio {
            "null".to_owned()
        } else {
            "auto".to_owned()
        })),
        "options/gpu-api" => Ok(NativeValue::String(app.options.gpu_api.name().to_owned())),
        "options/vo" => Ok(NativeValue::String(if app.options.null_video || app.options.no_video {
            "null".to_owned()
        } else {
            "auto".to_owned()
        })),
        "audio-device-list" => Ok(NativeValue::Array(vec![NativeValue::Map(vec![
            ("name".to_owned(), NativeValue::String("auto".to_owned())),
            ("description".to_owned(), NativeValue::String("Autoselect".to_owned())),
        ])])),
        "audio-device" => Ok(NativeValue::String(
            app.options
                .audio_device
                .clone()
                .unwrap_or_else(|| "auto".to_owned()),
        )),
        "hwdec" => Ok(NativeValue::Array(vec![NativeValue::String(
            app.options.hwdec.clone(),
        )])),
        "hwdec-current" => Ok(NativeValue::String(match status.hardware_decoder {
            Some(player_media::HwRequest::VideoToolbox) => "videotoolbox",
            Some(player_media::HwRequest::VaApi) => "vaapi",
            Some(player_media::HwRequest::None) | None => "no",
        }.to_owned())),
        "hwdec-software-fallback" => Ok(NativeValue::String("yes".to_owned())),
        "seekable" => Ok(NativeValue::Bool(true)),
        "partially-seekable" => Ok(NativeValue::Bool(false)),
        "cache-buffering-state" => Ok(NativeValue::Int(100)),
        "paused-for-cache" => Ok(NativeValue::Bool(false)),
        "demuxer-cache-state" => Ok(demuxer_cache_state(status.position)),
        "current-vo" => current_vo(&status).map(NativeValue::String),
        "current-ao" => current_ao(&status).map(NativeValue::String),
        "frame-drop-count" | "vo-drop-frame-count" => {
            Ok(NativeValue::Int(app.dropped_frames() as i64))
        }
        "decoder-frame-drop-count" => Ok(NativeValue::Int(status.evicted as i64)),
        "estimated-vf-fps" => estimated_vf_fps(&status).map(NativeValue::Double),
        "container-fps" => container_fps(&app.probe).map(NativeValue::Double),
        "audio-underrun-count" => Ok(NativeValue::Int(status.audio_underruns as i64)),
        "chapter-metadata" => Ok(NativeValue::Map(Vec::new())),
        "osc-visibility" => Ok(NativeValue::String(
            osc(app)?.visibility().name().to_owned(),
        )),
        "osc-visible" => Ok(NativeValue::Bool(osc(app)?.visible())),
        "osc-areas" => Ok(NativeValue::Map(
            osc(app)?
                .areas()
                .into_iter()
                .map(|(name, rect)| (name.to_owned(), rect_value(rect)))
                .collect(),
        )),
        _ if name.starts_with("options/") => Ok(NativeValue::String(String::new())),
        _ => Err("property not found".to_owned()),
    }
}

pub fn set(app: &mut App, name: &str, value: &NativeValue) -> Result<(), String> {
    match (name, value) {
        ("pause", NativeValue::Bool(paused)) => {
            app.set_paused(*paused)?;
            Ok(())
        }
        ("volume", NativeValue::Double(volume)) => {
            app.set_volume(*volume as f32);
            Ok(())
        }
        ("volume", NativeValue::Int(volume)) => {
            app.set_volume(*volume as f32);
            Ok(())
        }
        ("speed", NativeValue::Double(speed)) => {
            app.set_speed(*speed);
            Ok(())
        }
        ("speed", NativeValue::Int(speed)) => {
            app.set_speed(*speed as f64);
            Ok(())
        }
        ("mute", NativeValue::Bool(muted)) => {
            app.set_muted(*muted);
            Ok(())
        }
        ("fullscreen", NativeValue::Bool(fullscreen)) => {
            app.fullscreen = *fullscreen;
            Ok(())
        }
        ("window-scale", NativeValue::Double(scale)) => {
            set_window_scale(app, *scale);
            Ok(())
        }
        ("window-scale", NativeValue::Int(scale)) => {
            set_window_scale(app, *scale as f64);
            Ok(())
        }
        ("aid", NativeValue::Int(id)) => app.select_audio(*id),
        ("aid", NativeValue::Bool(false)) => {
            app.disable_audio();
            Ok(())
        }
        ("aid", NativeValue::Bool(true)) => {
            app.auto_audio();
            Ok(())
        }
        ("aid", NativeValue::String(name)) if name == "auto" => {
            app.auto_audio();
            Ok(())
        }
        ("aid", NativeValue::String(name)) if matches!(name.as_str(), "no" | "off" | "none") => {
            app.disable_audio();
            Ok(())
        }
        ("sid", NativeValue::Int(id)) => app.select_sub(Some(*id)),
        ("sid", NativeValue::Bool(false)) => app.select_sub(None),
        ("sid", NativeValue::String(name)) => match name.as_str() {
            "auto" => {
                app.select_first_sub();
                Ok(())
            }
            "no" | "off" | "none" => app.select_sub(None),
            _ => Err(format!("unknown subtitle selection '{name}'")),
        },
        ("sub-visibility", NativeValue::Bool(visible)) => {
            app.set_sub_visible(*visible);
            Ok(())
        }
        ("osc-visibility", NativeValue::String(mode)) => {
            let mode = OscVisibility::parse(mode)
                .ok_or_else(|| format!("osc-visibility is auto, always, or never, not '{mode}'"))?;
            app.set_osc_visibility(OscVisibilityChange::Set(mode))
        }
        ("keep-open", NativeValue::Bool(keep_open)) => {
            app.options.keep_open = *keep_open;
            Ok(())
        }
        _ => Err(format!("property is not settable: {name}")),
    }
}

fn set_window_scale(app: &mut App, scale: f64) {
    let scale = scale.clamp(0.1, 10.0);
    let width = video_dimension(&app.probe, 0);
    let height = video_dimension(&app.probe, 1);
    if width > 0 && height > 0 {
        app.window_scale = scale;
        let width = (width as f64 * scale).round() as i32;
        let height = (height as f64 * scale).round() as i32;
        app.logical_display = Some((width as u32, height as u32));
        app.set_display(width as u32, height as u32);
        if let Some(window) = app.window.as_mut() {
            window.resize(width, height);
        }
    }
}

fn osc(app: &App) -> Result<&crate::osc::Osc, String> {
    app.osc
        .as_ref()
        .ok_or_else(|| "property unavailable: the OSC is disabled".to_owned())
}

fn unavailable(name: &str) -> String {
    format!("property unavailable: {name}")
}

fn container_fps(probe: &Option<Probe>) -> Result<f64, String> {
    probe
        .as_ref()
        .and_then(|probe| probe.video_frame_rate)
        .ok_or_else(|| unavailable("container-fps"))
}

fn estimated_vf_fps(status: &EngineStatus) -> Result<f64, String> {
    status
        .estimated_vf_fps
        .ok_or_else(|| unavailable("estimated-vf-fps"))
}

fn current_vo(status: &EngineStatus) -> Result<String, String> {
    status
        .video_output
        .map(str::to_owned)
        .ok_or_else(|| unavailable("current-vo"))
}

fn current_ao(status: &EngineStatus) -> Result<String, String> {
    status
        .audio_output
        .map(str::to_owned)
        .ok_or_else(|| unavailable("current-ao"))
}

fn demuxer_cache_state(position: f64) -> NativeValue {
    NativeValue::Map(vec![("reader-pts".to_owned(), NativeValue::Double(position))])
}

fn rect_value(rect: Rect) -> NativeValue {
    NativeValue::Map(vec![
        ("x".to_owned(), NativeValue::Double(rect.x)),
        ("y".to_owned(), NativeValue::Double(rect.y)),
        ("w".to_owned(), NativeValue::Double(rect.w)),
        ("h".to_owned(), NativeValue::Double(rect.h)),
    ])
}

pub fn to_bool(value: &NativeValue) -> bool {
    matches!(value, NativeValue::Bool(true))
}

fn metadata(probe: &Option<Probe>) -> NativeValue {
    let mut fields = Vec::new();
    if let Some(probe) = probe {
        for (key, value) in &probe.metadata {
            fields.push((
                key.to_ascii_lowercase(),
                NativeValue::String(value.clone()),
            ));
        }
    }
    NativeValue::Map(fields)
}

fn chapters(probe: &Option<Probe>) -> NativeValue {
    let Some(probe) = probe else {
        return NativeValue::Array(Vec::new());
    };
    NativeValue::Array(
        probe
            .chapters
            .iter()
            .map(|chapter| {
                NativeValue::Map(vec![
                    ("title".to_owned(), NativeValue::String(chapter.title.clone().unwrap_or_default())),
                    ("time".to_owned(), NativeValue::Double(chapter.start_seconds)),
                ])
            })
            .collect(),
    )
}

fn chapter_at(app: &App, position: f64) -> i64 {
    let Some(probe) = &app.probe else {
        return -1;
    };
    probe
        .chapters
        .iter()
        .enumerate()
        .rev()
        .find(|(_, chapter)| chapter.start_seconds <= position + 0.001)
        .map(|(index, _)| index as i64)
        .unwrap_or(-1)
}

fn track_list(app: &App, status: &EngineStatus) -> NativeValue {
    if status.tracks.is_empty() {
        return probe_track_list(app);
    }
    let mut tracks: Vec<(usize, NativeValue)> = Vec::new();
    for track in &status.tracks {
        let stream = app.probe.as_ref().and_then(|probe| {
            probe
                .streams
                .iter()
                .find(|stream| stream.index + 1 == track.src_id)
        });
        let kind = match track.kind {
            TrackKind::Audio => "audio",
            TrackKind::Video => "video",
            TrackKind::Subtitle => "sub",
        };
        let mut fields = vec![
            ("id".to_owned(), NativeValue::Int(track.id as i64)),
            ("type".to_owned(), NativeValue::String(kind.to_owned())),
            ("src-id".to_owned(), NativeValue::Int(track.src_id as i64)),
            (
                "codec".to_owned(),
                NativeValue::String(track_codec(app, track.kind, &track.codec)),
            ),
            (
                "selected".to_owned(),
                NativeValue::Bool(if track.kind == TrackKind::Audio {
                    !app.aid_disabled && app.aid == Some(track.id as i64)
                } else {
                    track.selected
                }),
            ),
            ("external".to_owned(), NativeValue::Bool(track.external)),
            (
                "default".to_owned(),
                NativeValue::Bool(stream.is_some_and(|stream| stream.default)),
            ),
            (
                "forced".to_owned(),
                NativeValue::Bool(stream.is_some_and(|stream| stream.forced)),
            ),
        ];
        if let Some(language) = &track.language {
            if language != "und" {
                fields.push(("lang".to_owned(), NativeValue::String(language.clone())));
            }
        }
        if let Some(title) = &track.title {
            fields.push(("title".to_owned(), NativeValue::String(title.clone())));
        }
        if let Some(stream) = stream {
            if stream.kind == StreamKind::Audio {
                fields.push((
                    "demux-channel-count".to_owned(),
                    NativeValue::Int(stream.channels as i64),
                ));
                fields.push((
                    "demux-samplerate".to_owned(),
                    NativeValue::Int(stream.sample_rate as i64),
                ));
            }
            if stream.kind == StreamKind::Video {
                fields.push(("demux-w".to_owned(), NativeValue::Int(stream.width as i64)));
                fields.push(("demux-h".to_owned(), NativeValue::Int(stream.height as i64)));
            }
        }
        tracks.push((track.src_id, NativeValue::Map(fields)));
    }
    if let Some(probe) = &app.probe {
        for stream in &probe.streams {
            let src_id = stream.index + 1;
            if !stream.attached
                || stream.kind != StreamKind::Video
                || tracks.iter().any(|(known, _)| *known == src_id)
            {
                continue;
            }
            tracks.push((
                src_id,
                NativeValue::Map(vec![
                    ("id".to_owned(), NativeValue::Int(1)),
                    ("type".to_owned(), NativeValue::String("video".to_owned())),
                    ("src-id".to_owned(), NativeValue::Int(src_id as i64)),
                    (
                        "codec".to_owned(),
                        NativeValue::String(stream.codec_name.clone()),
                    ),
                    ("selected".to_owned(), NativeValue::Bool(false)),
                    ("external".to_owned(), NativeValue::Bool(false)),
                    ("default".to_owned(), NativeValue::Bool(stream.default)),
                    ("forced".to_owned(), NativeValue::Bool(stream.forced)),
                ]),
            ));
        }
    }
    tracks.sort_by_key(|(src_id, _)| *src_id);
    NativeValue::Array(tracks.into_iter().map(|(_, track)| track).collect())
}

fn track_codec(app: &App, kind: TrackKind, codec: &str) -> String {
    if kind == TrackKind::Subtitle
        && codec == "webvtt"
        && app
            .probe
            .as_ref()
            .is_some_and(|probe| probe.format == "webm")
    {
        return "webvtt-webm".to_owned();
    }
    codec.to_owned()
}

fn probe_track_list(app: &App) -> NativeValue {
    let Some(probe) = &app.probe else {
        return NativeValue::Array(Vec::new());
    };
    let mut tracks = Vec::new();
    let mut video_id = 0;
    let mut audio_id = 0;
    let mut sub_id = 0;
    for stream in &probe.streams {
        let (kind, id) = match stream.kind {
            StreamKind::Video => {
                video_id += 1;
                ("video", video_id)
            }
            StreamKind::Audio => {
                audio_id += 1;
                ("audio", audio_id)
            }
            StreamKind::Subtitle => {
                sub_id += 1;
                ("sub", sub_id)
            }
            _ => continue,
        };
        let selected = match stream.kind {
            StreamKind::Video => !stream.attached && video_id == 1,
            StreamKind::Audio => !app.aid_disabled && app.aid == Some(audio_id as i64),
            _ => false,
        };
        let codec = match stream.kind {
            StreamKind::Subtitle
                if stream.codec_name == "webvtt" && probe.format == "webm" =>
            {
                "webvtt-webm".to_owned()
            }
            _ => stream.codec_name.clone(),
        };
        let mut fields = vec![
            ("id".to_owned(), NativeValue::Int(id)),
            ("type".to_owned(), NativeValue::String(kind.to_owned())),
            ("src-id".to_owned(), NativeValue::Int(stream.index as i64 + 1)),
            ("codec".to_owned(), NativeValue::String(codec)),
            ("selected".to_owned(), NativeValue::Bool(selected)),
            ("external".to_owned(), NativeValue::Bool(false)),
            ("default".to_owned(), NativeValue::Bool(stream.default)),
            ("forced".to_owned(), NativeValue::Bool(stream.forced)),
        ];
        if let Some(language) = &stream.language {
            if language != "und" {
                fields.push(("lang".to_owned(), NativeValue::String(language.clone())));
            }
        }
        if let Some(title) = &stream.title {
            fields.push(("title".to_owned(), NativeValue::String(title.clone())));
        }
        if stream.kind == StreamKind::Audio {
            fields.push((
                "demux-channel-count".to_owned(),
                NativeValue::Int(stream.channels as i64),
            ));
            fields.push((
                "demux-samplerate".to_owned(),
                NativeValue::Int(stream.sample_rate as i64),
            ));
        }
        if stream.kind == StreamKind::Video {
            fields.push(("demux-w".to_owned(), NativeValue::Int(stream.width as i64)));
            fields.push(("demux-h".to_owned(), NativeValue::Int(stream.height as i64)));
        }
        tracks.push(NativeValue::Map(fields));
    }
    NativeValue::Array(tracks)
}

fn sid(status: &EngineStatus) -> NativeValue {
    let subtitles: Vec<_> = status
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect();
    if subtitles.is_empty() {
        return NativeValue::String("auto".to_owned());
    }
    match subtitles.iter().find(|track| track.selected) {
        Some(track) => NativeValue::Int(track.id as i64),
        None => NativeValue::Bool(false),
    }
}

fn aid(status: &EngineStatus) -> NativeValue {
    status
        .tracks
        .iter()
        .find(|track| track.kind == TrackKind::Audio && track.selected)
        .map(|track| NativeValue::Int(track.id as i64))
        .unwrap_or(NativeValue::Bool(false))
}

fn codec(probe: &Option<Probe>, kind: StreamKind) -> String {
    probe
        .as_ref()
        .and_then(|probe| probe.streams.iter().find(|stream| stream.kind == kind))
        .map(|stream| stream.codec_name.clone())
        .unwrap_or_default()
}

fn video_dimension(probe: &Option<Probe>, axis: usize) -> i64 {
    probe
        .as_ref()
        .and_then(|probe| probe.streams.iter().find(|stream| stream.kind == StreamKind::Video))
        .map(|stream| if axis == 0 { stream.width as i64 } else { stream.height as i64 })
        .unwrap_or(0)
}

fn video_params(probe: &Option<Probe>, presented: Option<(u32, u32, player_output::VideoGeometry)>) -> NativeValue {
    let Some(stream) = probe
        .as_ref()
        .and_then(|probe| probe.streams.iter().find(|stream| stream.kind == StreamKind::Video))
    else {
        return NativeValue::Map(Vec::new());
    };
    let (width, height, geometry) = if let Some((width, height, geometry)) = presented {
        (width as i64, height as i64, geometry)
    } else {
        let metadata = probe.as_ref().expect("stream belongs to probe").video_geometry;
        (stream.width.max(0) as i64, stream.height.max(0) as i64,
            player_output::VideoGeometry { pixel_aspect: metadata.pixel_aspect, rotation: metadata.rotation })
    };
    let display_width = (width as f64 * geometry.pixel_aspect).round() as i64;
    let (dw, dh) = if geometry.rotation == 90 || geometry.rotation == 270 {
        (height, display_width)
    } else { (display_width, height) };
    let aspect = if dh > 0 {
        ((dw as f64 / dh as f64) * 100.0).round() / 100.0
    } else {
        0.0
    };
    let sar = if height > 0 { ((width as f64 / height as f64) * 100.0).round() / 100.0 } else { 0.0 };
    let sar_name = if (sar - 1.0).abs() < 0.01 { "Square" } else if (sar - 4.0 / 3.0).abs() < 0.01 { "4:3" } else { "other" };
    let aspect_name = if (aspect - 1.0).abs() < 0.01 {
        "Square"
    } else if (aspect - 4.0 / 3.0).abs() < 0.01 {
        "4:3"
    } else {
        "other"
    };
    let (pixelformat, colormatrix, gamma, colorlevels, chroma, bpp) =
        match stream.codec_name.as_str() {
            "png" => ("rgb24", "rgb", "srgb", "full", "mpeg1/jpeg", 24),
            "mjpeg" | "jpeg" => (
                "yuv420p",
                "bt.601",
                "srgb",
                "full",
                "mpeg1/jpeg",
                12,
            ),
            _ => (
                "yuv420p",
                "bt.601",
                "bt.1886",
                "limited",
                "mpeg2/4/h264",
                12,
            ),
        };
    NativeValue::Map(vec![
        ("w".to_owned(), NativeValue::Int(width)),
        ("h".to_owned(), NativeValue::Int(height)),
        ("dw".to_owned(), NativeValue::Int(dw)),
        ("dh".to_owned(), NativeValue::Int(dh)),
        ("crop-x".to_owned(), NativeValue::Int(0)),
        ("crop-y".to_owned(), NativeValue::Int(0)),
        ("crop-w".to_owned(), NativeValue::Int(width)),
        ("crop-h".to_owned(), NativeValue::Int(height)),
        ("pixelformat".to_owned(), NativeValue::String(pixelformat.to_owned())),
        ("par".to_owned(), NativeValue::Double(geometry.pixel_aspect)),
        ("sar".to_owned(), NativeValue::Double(sar)),
        ("sar-name".to_owned(), NativeValue::String(sar_name.to_owned())),
        ("aspect".to_owned(), NativeValue::Double(aspect)),
        ("aspect-name".to_owned(), NativeValue::String(aspect_name.to_owned())),
        ("colormatrix".to_owned(), NativeValue::String(colormatrix.to_owned())),
        ("primaries".to_owned(), NativeValue::String("bt.709".to_owned())),
        ("gamma".to_owned(), NativeValue::String(gamma.to_owned())),
        ("colorlevels".to_owned(), NativeValue::String(colorlevels.to_owned())),
        ("chroma-location".to_owned(), NativeValue::String(chroma.to_owned())),
        ("light".to_owned(), NativeValue::String("display".to_owned())),
        ("stereo-in".to_owned(), NativeValue::String("mono".to_owned())),
        ("average-bpp".to_owned(), NativeValue::Int(bpp)),
        ("alpha".to_owned(), NativeValue::String("none".to_owned())),
        ("rotate".to_owned(), NativeValue::Int(geometry.rotation as i64)),
        ("sig-peak".to_owned(), NativeValue::Double(0.0)),
    ])
}

fn audio_params(probe: &Option<Probe>) -> NativeValue {
    let Some(stream) = probe
        .as_ref()
        .and_then(|probe| probe.streams.iter().find(|stream| stream.kind == StreamKind::Audio))
    else {
        return NativeValue::Map(Vec::new());
    };
    let (channels, hr_channels) =
        channel_names(&probe_format(probe), stream.channels, stream.channel_mask);
    NativeValue::Map(vec![
        ("channel-count".to_owned(), NativeValue::Int(stream.channels as i64)),
        ("channels".to_owned(), NativeValue::String(channels)),
        ("format".to_owned(), NativeValue::String("float".to_owned())),
        ("hr-channels".to_owned(), NativeValue::String(hr_channels)),
        ("samplerate".to_owned(), NativeValue::Int(stream.sample_rate as i64)),
    ])
}

fn probe_format(probe: &Option<Probe>) -> String {
    probe
        .as_ref()
        .map(|probe| probe.format.clone())
        .unwrap_or_default()
}

fn channel_names(format: &str, channels: i32, mask: u64) -> (String, String) {
    if matches!(format, "mkv" | "webm") || (mask == 0 && channels != 1 && channels != 2 && channels != 6)
    {
        return (format!("unknown{channels}"), "empty".to_owned());
    }
    match channels {
        1 => ("mono".to_owned(), "mono".to_owned()),
        2 => ("stereo".to_owned(), "stereo".to_owned()),
        6 => ("5.1".to_owned(), "5.1".to_owned()),
        other => (format!("unknown{other}"), "empty".to_owned()),
    }
}

fn demuxer(app: &App) -> String {
    match app.probe.as_ref().map(|probe| probe.format.as_str()) {
        Some("mkv" | "webm") => "matroska".to_owned(),
        Some("mp4" | "m4a" | "mov") => "mov".to_owned(),
        Some("avi") => "avi".to_owned(),
        Some("mp3") => "mp3".to_owned(),
        Some("flac") => "flac".to_owned(),
        Some("ogg" | "opus") => "ogg".to_owned(),
        Some("wav") => "wav".to_owned(),
        Some(other) => other.to_owned(),
        None => String::new(),
    }
}

fn playlist(app: &App) -> NativeValue {
    NativeValue::Array(
        app.playlist
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                NativeValue::Map(vec![
                    ("filename".to_owned(), NativeValue::String(entry.path.clone())),
                    (
                        "current".to_owned(),
                        NativeValue::Bool(app.playlist.current() == Some(index)),
                    ),
                    (
                        "playing".to_owned(),
                        NativeValue::Bool(app.playlist.current() == Some(index)),
                    ),
                ])
            })
            .collect(),
    )
}

fn dimensions(app: &App) -> (i64, i64) {
    if let Some((width, height)) = app.display {
        return (width as i64, height as i64);
    }
    if app.options.null_video || app.options.no_video {
        return (0, 0);
    }
    (video_dimension(&app.probe, 0), video_dimension(&app.probe, 1))
}

fn osd_dimensions(app: &App) -> NativeValue {
    let (width, height) = dimensions(app);
    let aspect = if height > 0 { width as f64 / height as f64 } else { 0.0 };
    NativeValue::Map(vec![
        ("w".to_owned(), NativeValue::Int(width)),
        ("h".to_owned(), NativeValue::Int(height)),
        ("par".to_owned(), NativeValue::Double(1.0)),
        ("aspect".to_owned(), NativeValue::Double(aspect)),
        ("mt".to_owned(), NativeValue::Int(0)),
        ("mb".to_owned(), NativeValue::Int(0)),
        ("ml".to_owned(), NativeValue::Int(0)),
        ("mr".to_owned(), NativeValue::Int(0)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::Options;

    fn probe(video_frame_rate: Option<f64>) -> Probe {
        Probe {
            duration: 3.0,
            metadata: Vec::new(),
            chapters: Vec::new(),
            streams: Vec::new(),
            format: "mp4".to_owned(),
            video_frame_rate,
            video_geometry: player_media::VideoGeometry::default(),
        }
    }

    #[test]
    fn video_params_report_coded_and_rotated_display_geometry() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../player-media/tests/fixtures/geometry-rotated.mp4");
        let probe = Some(super::probe(path.to_str().unwrap()).unwrap());
        let NativeValue::Map(values) = video_params(&probe, None) else { panic!("map") };
        let value = |key: &str| values.iter().find(|(name, _)| name == key).map(|(_, value)| value).unwrap();
        assert_eq!(value("w"), &NativeValue::Int(64));
        assert_eq!(value("h"), &NativeValue::Int(32));
        assert_eq!(value("dw"), &NativeValue::Int(32));
        assert_eq!(value("dh"), &NativeValue::Int(128));
        assert_eq!(value("par"), &NativeValue::Double(2.0));
        assert_eq!(value("sar"), &NativeValue::Double(2.0));
        assert_eq!(value("aspect"), &NativeValue::Double(0.25));
        assert_eq!(value("rotate"), &NativeValue::Int(270));
    }

    #[test]
    fn unavailable_properties_return_the_unavailable_error() {
        let app = App::new(Options::default());
        for name in ["container-fps", "estimated-vf-fps", "current-vo", "current-ao"] {
            let error = get(&app, name).expect_err(name);
            assert!(
                error.starts_with("property unavailable:"),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn container_fps_comes_from_the_probed_video_stream() {
        let mut app = App::new(Options::default());
        app.probe = Some(probe(Some(24.0)));
        assert_eq!(
            get(&app, "container-fps"),
            Ok(NativeValue::Double(24.0))
        );
        app.probe = Some(probe(None));
        assert!(
            get(&app, "container-fps").is_err(),
            "an unknown video rate is unavailable"
        );
    }

    #[test]
    fn estimated_vf_fps_uses_the_engine_measurement() {
        let mut status = EngineStatus::default();
        assert!(estimated_vf_fps(&status).is_err());
        status.estimated_vf_fps = Some(23.976);
        assert_eq!(estimated_vf_fps(&status), Ok(23.976));
    }

    #[test]
    fn current_outputs_name_the_backends_that_opened() {
        let mut status = EngineStatus::default();
        assert!(current_vo(&status).is_err());
        assert!(current_ao(&status).is_err());
        status.video_output = Some("null");
        status.audio_output = Some("coreaudio");
        assert_eq!(current_vo(&status).as_deref(), Ok("null"));
        assert_eq!(current_ao(&status).as_deref(), Ok("coreaudio"));
    }

    #[test]
    fn demuxer_cache_state_omits_an_unmeasured_cache_duration() {
        let NativeValue::Map(fields) = demuxer_cache_state(1.5) else {
            panic!("demuxer-cache-state is a map");
        };
        assert!(fields.iter().any(|(key, _)| key == "reader-pts"));
        assert!(!fields.iter().any(|(key, _)| key == "cache-duration"));
        assert!(!fields.iter().any(|(key, _)| key == "cache-end"));
    }
}

fn input_bindings() -> NativeValue {
    let bindings = [
        ("SPACE", "cycle pause"),
        ("f", "cycle fullscreen"),
        ("9", "add volume 2"),
        ("0", "add volume -2"),
        ("m", "cycle mute"),
        ("[", "add speed -0.1"),
        ("]", "add speed 0.1"),
        (".", "frame-step"),
        ("q", "quit"),
        ("o", "cycle osc-visibility"),
    ];
    let values: Vec<NativeValue> = bindings
        .iter()
        .map(|(key, command)| {
            NativeValue::Map(vec![
                ("key".to_owned(), NativeValue::String((*key).to_owned())),
                ("cmd".to_owned(), NativeValue::String((*command).to_owned())),
                ("section".to_owned(), NativeValue::String("default".to_owned())),
                ("is_weak".to_owned(), NativeValue::Bool(false)),
                ("owner".to_owned(), NativeValue::String("builtin".to_owned())),
            ])
        })
        .collect();
    NativeValue::Array(values)
}
