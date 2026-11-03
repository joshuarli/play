use player_ass::{Library, RenderedImage, Renderer, Track};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubCodec {
    Ass,
    SubRip,
    WebVtt,
}

impl SubCodec {
    pub fn for_name(name: &str) -> Option<SubCodec> {
        match name {
            "ass" | "ssa" => Some(SubCodec::Ass),
            "subrip" | "srt" => Some(SubCodec::SubRip),
            "webvtt" | "web_vtt" | "vtt" => Some(SubCodec::WebVtt),
            _ => None,
        }
    }

    pub fn for_extension(path: &str) -> Option<SubCodec> {
        let extension = path.rsplit('.').next().unwrap_or("").to_lowercase();
        match extension.as_str() {
            "ass" | "ssa" => Some(SubCodec::Ass),
            "srt" => Some(SubCodec::SubRip),
            "vtt" => Some(SubCodec::WebVtt),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SubCodec::Ass => "ass",
            SubCodec::SubRip => "subrip",
            SubCodec::WebVtt => "webvtt",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubTrack {
    pub stream: Option<usize>,
    pub codec: SubCodec,
    pub language: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Video,
    Subtitle,
}

#[derive(Debug, Clone)]
pub struct TrackInfo {
    /// 1-based id within the track type, matching mpv's per-type track ids.
    /// Audio and video count demux order; subtitles count load order, so an
    /// added external subtitle never moves an existing id.
    pub id: usize,
    pub kind: TrackKind,
    /// Demux stream index plus one for embedded tracks; synthetic
    /// demux-size-based ids for external files.
    pub src_id: usize,
    pub codec: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub selected: bool,
    pub external: bool,
    pub sub_index: Option<usize>,
}

impl TrackKind {
    pub fn name(self) -> &'static str {
        match self {
            TrackKind::Audio => "audio",
            TrackKind::Video => "video",
            TrackKind::Subtitle => "sub",
        }
    }
}

struct Retained {
    start_ms: i64,
    end_ms: i64,
    payload: Vec<u8>,
}

#[derive(Clone)]
enum ExternalSource {
    Ass(String),
    Cues(Vec<(i64, i64, String)>),
}

pub struct EngineSub {
    renderer: Renderer,
    track: Track,
    source: SubTrack,
    external: Option<ExternalSource>,
    retained: Vec<Retained>,
    fed: std::collections::HashSet<(usize, i64)>,
    visible: bool,
}

impl EngineSub {
    fn open(library: &Library, source: SubTrack, header: &str) -> Result<Self, String> {
        let renderer = Renderer::new(library)?;
        let mut track = Track::new(library)?;
        track.process_data(header)?;
        Ok(EngineSub {
            renderer,
            track,
            source,
            external: None,
            retained: Vec::new(),
            fed: std::collections::HashSet::new(),
            visible: true,
        })
    }

    pub fn open_embedded(
        library: &Library,
        info: &player_media::StreamInfo,
        playres: (i32, i32),
    ) -> Result<Self, String> {
        let codec = SubCodec::for_name(&info.codec_name)
            .ok_or_else(|| format!("unsupported subtitle codec '{}'", info.codec_name))?;
        let source = SubTrack {
            stream: Some(info.index),
            codec,
            language: info.language.clone(),
            title: info.title.clone(),
        };
        let extradata = &info.extradata;
        match codec {
            SubCodec::Ass => {
                let mut sub = Self::open(library, source, &ass_header(playres))?;
                let header = String::from_utf8_lossy(extradata);
                sub.track.process_data(&header)?;
                Ok(sub)
            }
            SubCodec::SubRip | SubCodec::WebVtt => {
                Self::open(library, source, &text_header(playres))
            }
        }
    }

    pub fn open_external(
        library: &Library,
        path: &str,
        playres: (i32, i32),
    ) -> Result<Self, String> {
        let codec = SubCodec::for_extension(path)
            .ok_or_else(|| format!("unsupported subtitle file '{path}'"))?;
        let content = std::fs::read(path)
            .map_err(|error| format!("cannot read subtitle file '{path}': {error}"))?;
        let text = String::from_utf8(content)
            .map_err(|_| format!("subtitle file '{path}' is not UTF-8"))?;
        let source = SubTrack {
            stream: None,
            codec,
            language: None,
            title: None,
        };
        match codec {
            SubCodec::Ass => {
                let mut sub = Self::open(library, source, &ass_header(playres))?;
                sub.track.process_data(&text)?;
                sub.external = Some(ExternalSource::Ass(text));
                Ok(sub)
            }
            SubCodec::SubRip | SubCodec::WebVtt => {
                let mut sub = Self::open(library, source, &text_header(playres))?;
                let cues = parse_cues(&text, codec)?;
                for (start, end, event) in &cues {
                    let dialogue =
                        dialogue_event(event, ms_to_seconds(*start), ms_to_seconds(*end));
                    sub.track
                        .process_data(&dialogue)
                        .map_err(|error| format!("subtitle cue rejected: {error}"))?;
                }
                sub.external = Some(ExternalSource::Cues(cues));
                Ok(sub)
            }
        }
    }

    pub fn source(&self) -> &SubTrack {
        &self.source
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn feed(
        &mut self,
        stream: usize,
        payload: &[u8],
        start: f64,
        duration: f64,
    ) -> Result<(), String> {
        let start_ms = (start * 1000.0).round() as i64;
        let duration_ms = (duration * 1000.0).round().max(0.0) as i64;
        if !self.fed.insert((stream, start_ms)) {
            return Ok(());
        }
        match self.source.codec {
            SubCodec::Ass => {
                self.track.process_chunk(payload, start_ms, duration_ms);
                self.retained.push(Retained {
                    start_ms,
                    end_ms: start_ms + duration_ms,
                    payload: payload.to_vec(),
                });
                Ok(())
            }
            SubCodec::SubRip | SubCodec::WebVtt => {
                let text = String::from_utf8_lossy(payload);
                let event = dialogue_event(&text, start, start + duration);
                self.track
                    .process_data(&event)
                    .map_err(|error| format!("subtitle cue rejected: {error}"))?;
                self.retained.push(Retained {
                    start_ms,
                    end_ms: start_ms + duration_ms,
                    payload: event.into_bytes(),
                });
                Ok(())
            }
        }
    }

    pub fn flush(&mut self, target_ms: i64) -> Result<(), String> {
        self.track.flush_events();
        self.retained.retain(|event| event.end_ms > target_ms);
        let replay: Vec<(Vec<u8>, i64, i64)> = self
            .retained
            .iter()
            .map(|event| {
                (
                    event.payload.clone(),
                    event.start_ms,
                    event.end_ms - event.start_ms,
                )
            })
            .collect();
        for (payload, start, duration) in &replay {
            self.replay(payload, *start, *duration)?;
        }
        self.refeed_external(Some(target_ms))
    }

    pub fn reseed(&mut self) -> Result<(), String> {
        self.retained.clear();
        self.fed.clear();
        self.track.flush_events();
        self.refeed_external(None)
    }

    fn refeed_external(&mut self, target_ms: Option<i64>) -> Result<(), String> {
        let show_all = target_ms.is_none();
        match self.external.clone() {
            Some(ExternalSource::Ass(text)) => {
                self.track.process_data(&text)?;
            }
            Some(ExternalSource::Cues(cues)) => {
                for (start, end, text) in &cues {
                    if show_all || target_ms.is_some_and(|target| *end > target) {
                        let dialogue =
                            dialogue_event(text, ms_to_seconds(*start), ms_to_seconds(*end));
                        self.track
                            .process_data(&dialogue)
                            .map_err(|error| format!("subtitle cue rejected: {error}"))?;
                    }
                }
            }
            None => {}
        }
        Ok(())
    }

    fn replay(&mut self, payload: &[u8], start_ms: i64, duration_ms: i64) -> Result<(), String> {
        match self.source.codec {
            SubCodec::Ass => {
                self.track.process_chunk(payload, start_ms, duration_ms);
                Ok(())
            }
            _ => self
                .track
                .process_data(std::str::from_utf8(payload).map_err(|_| {
                    "retained subtitle cue is not UTF-8".to_owned()
                })?)
                .map_err(|error| format!("subtitle cue rejected: {error}")),
        }
    }

    pub fn render(
        &mut self,
        width: i32,
        height: i32,
        par: f64,
        now_ms: i64,
    ) -> Result<Vec<RenderedImage>, String> {
        if !self.visible {
            return Ok(Vec::new());
        }
        self.renderer.render(
            &self.track,
            width,
            height,
            Some((width, height)),
            par,
            now_ms,
        )
    }
}

fn ass_header(playres: (i32, i32)) -> String {
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {}\nPlayResY: {}\nScaledBorderAndShadow: yes\n",
        playres.0, playres.1
    )
}

fn text_header(playres: (i32, i32)) -> String {
    format!(
        "{}\n[V4+ Styles]\n\
        Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
        Style: Default,Arial,48,&H00FFFFFF,&H000019FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
        \n\
        [Events]\n\
        Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        ass_header(playres)
    )
}

fn ass_time(seconds: f64) -> String {
    let clamped = seconds.max(0.0);
    let whole = clamped.trunc() as u64;
    let centis = ((clamped - whole as f64) * 100.0).round() as u64;
    format!(
        "{}:{:02}:{:02}.{:02}",
        whole / 3600,
        (whole / 60) % 60,
        whole % 60,
        centis.min(99)
    )
}

fn ms_to_seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

fn dialogue_event(text: &str, start: f64, end: f64) -> String {
    let clean = text.replace('\n', "\\N").replace('\r', "");
    format!(
        "Dialogue: 0,{},{},Default,,0,0,0,,{}\n",
        ass_time(start),
        ass_time(end),
        clean
    )
}

fn parse_timestamp(text: &str) -> Option<f64> {
    let normalized = text.trim().replace(',', ".");
    let mut parts = normalized.split(':');
    let (hours, minutes, seconds) = match (parts.next(), parts.next(), parts.next(), parts.next())
    {
        (Some(h), Some(m), Some(s), None) => {
            (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?)
        }
        (Some(m), Some(s), None, _) => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    if !(0.0..60.0).contains(&minutes) || !(0.0..60.0).contains(&seconds) || hours < 0.0 {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn parse_cues(content: &str, codec: SubCodec) -> Result<Vec<(i64, i64, String)>, String> {
    let mut cues = Vec::new();
    let mut blocks = content.replace("\r\n", "\n").replace('\r', "\n");
    if codec == SubCodec::WebVtt {
        if let Some(stripped) = blocks.strip_prefix("WEBVTT") {
            blocks = stripped.to_string();
        }
    }
    for block in blocks.split("\n\n") {
        let mut lines = block.lines().filter(|line| !line.trim().is_empty());
        let Some(first) = lines.next() else { continue };
        if first.trim_start().starts_with("NOTE") {
            continue;
        }
        let (timing, text) = if first.contains("-->") {
            (first, lines.collect::<Vec<_>>().join("\n"))
        } else if let Some(second) = lines.next() {
            if !second.contains("-->") {
                continue;
            }
            (second, lines.collect::<Vec<_>>().join("\n"))
        } else {
            continue;
        };
        let mut arrow = timing.split("-->");
        let (Some(start), Some(end)) = (arrow.next(), arrow.next()) else {
            continue;
        };
        let (Some(start), Some(end)) = (
            parse_timestamp(start.split_whitespace().next().unwrap_or("")),
            parse_timestamp(end.split_whitespace().next().unwrap_or("")),
        ) else {
            continue;
        };
        if end <= start || text.trim().is_empty() {
            continue;
        }
        cues.push((
            (start * 1000.0).round() as i64,
            (end * 1000.0).round() as i64,
            text,
        ));
    }
    if cues.is_empty() {
        return Err("no subtitle cues found".to_owned());
    }
    Ok(cues)
}

#[cfg(test)]
mod tests {
    use super::{ass_time, dialogue_event, parse_cues, parse_timestamp, SubCodec};

    const SRT: &str = "1\n00:00:00,200 --> 00:00:01,500\nHello from SubRip.\n\n2\n00:00:01,600 --> 00:00:02,800\nSecond cue.\n";

    const VTT: &str = "WEBVTT\n\n00:00:00.200 --> 00:00:01.500\nHello from WebVTT.\n\nNOTE a comment\n\n00:00:01.600 --> 00:00:02.800 align:start\nSecond cue.\n";

    #[test]
    fn timestamps_parse_both_separators() {
        assert_eq!(parse_timestamp("00:00:01,500"), Some(1.5));
        assert_eq!(parse_timestamp("00:00:01.500"), Some(1.5));
        assert_eq!(parse_timestamp("01:02.500"), Some(62.5));
        assert_eq!(parse_timestamp("nope"), None);
        assert_eq!(parse_timestamp("00:99:01.500"), None);
    }

    #[test]
    fn ass_time_formats_centiseconds() {
        assert_eq!(ass_time(1.5), "0:00:01.50");
        assert_eq!(ass_time(62.25), "0:01:02.25");
        assert_eq!(ass_time(-1.0), "0:00:00.00");
    }

    #[test]
    fn srt_converts_to_dialogues() {
        let cues = parse_cues(SRT, SubCodec::SubRip).expect("srt parses");
        assert_eq!(cues.len(), 2);
        let event = dialogue_event(&cues[0].2, 0.2, 1.5);
        assert_eq!(
            event,
            "Dialogue: 0,0:00:00.20,0:00:01.50,Default,,0,0,0,,Hello from SubRip.\n"
        );
    }

    #[test]
    fn vtt_converts_ignoring_header_and_settings() {
        let cues = parse_cues(VTT, SubCodec::WebVtt).expect("vtt parses");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0], (200, 1500, "Hello from WebVTT.".to_owned()));
        assert_eq!(cues[1].2, "Second cue.");
    }

    #[test]
    fn garbage_has_no_cues() {
        assert!(parse_cues("nothing here\n", SubCodec::SubRip).is_err());
        assert!(parse_cues("1\nno arrow here\ntext\n", SubCodec::SubRip).is_err());
    }
}
