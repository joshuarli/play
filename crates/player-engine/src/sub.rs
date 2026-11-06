use std::collections::HashSet;

/// FFmpeg's names for SubRip, the only subtitle format the player reads.
pub fn is_subrip(codec_name: &str) -> bool {
    matches!(codec_name, "subrip" | "srt")
}

pub const SUBRIP_CODEC: &str = "subrip";

#[derive(Debug, Clone)]
pub struct SubTrack {
    pub stream: Option<usize>,
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
    /// Position in the scheduler's subtitle list; `None` for subtitle
    /// streams in a format the player does not read.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// One subtitle track's cues: parsed up front for an external file, or
/// collected from demuxed packets for an embedded stream.
pub struct EngineSub {
    source: SubTrack,
    cues: Vec<Cue>,
    fed: HashSet<i64>,
    visible: bool,
}

impl EngineSub {
    pub fn open_embedded(info: &player_media::StreamInfo) -> Result<Self, String> {
        if !is_subrip(&info.codec_name) {
            return Err(format!("unsupported subtitle codec '{}'; only SubRip is supported", info.codec_name));
        }
        Ok(EngineSub {
            source: SubTrack { stream: Some(info.index), language: info.language.clone(), title: info.title.clone() },
            cues: Vec::new(),
            fed: HashSet::new(),
            visible: true,
        })
    }

    pub fn open_external(path: &str) -> Result<Self, String> {
        let extension = std::path::Path::new(path)
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase());
        if extension.as_deref() != Some("srt") {
            return Err(format!("unsupported subtitle file '{path}'; only SubRip (.srt) is supported"));
        }
        let content = std::fs::read(path).map_err(|error| format!("cannot read subtitle file '{path}': {error}"))?;
        let text = String::from_utf8(content).map_err(|_| format!("subtitle file '{path}' is not UTF-8"))?;
        let cues = parse_srt(&text).map_err(|error| format!("subtitle file '{path}': {error}"))?;
        let title = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_owned());
        Ok(EngineSub {
            source: SubTrack { stream: None, language: None, title: Some(title) },
            cues,
            fed: HashSet::new(),
            visible: true,
        })
    }

    pub fn source(&self) -> &SubTrack {
        &self.source
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Adds an embedded packet's cue once; demuxing the same span again
    /// after a seek re-delivers packets that are already known.
    pub fn feed(&mut self, payload: &[u8], start: f64, duration: f64) {
        let start_ms = (start * 1000.0).round() as i64;
        let duration_ms = (duration * 1000.0).round().max(0.0) as i64;
        if !self.fed.insert(start_ms) {
            return;
        }
        let text = markup_free(&String::from_utf8_lossy(payload));
        if text.trim().is_empty() {
            return;
        }
        let at = self.cues.partition_point(|cue| cue.start_ms <= start_ms);
        self.cues.insert(at, Cue { start_ms, end_ms: start_ms + duration_ms, text });
    }

    /// Every cue showing at `position_ms`, in start order, one per line.
    pub fn text_at(&self, position_ms: i64) -> String {
        if !self.visible {
            return String::new();
        }
        let shown = self.cues.partition_point(|cue| cue.start_ms <= position_ms);
        self.cues[..shown]
            .iter()
            .filter(|cue| position_ms < cue.end_ms)
            .map(|cue| cue.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn parse_timestamp(text: &str) -> Option<i64> {
    let text = text.trim();
    let (clock, millis) = text.split_once([',', '.'])?;
    let mut parts = clock.split(':');
    let (hours, minutes, seconds) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || millis.is_empty() || millis.len() > 3 {
        return None;
    }
    let number = |part: &str| -> Option<i64> {
        (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())).then(|| part.parse().ok()).flatten()
    };
    let (hours, minutes, seconds) = (number(hours)?, number(minutes)?, number(seconds)?);
    let millis = number(millis)? * 10i64.pow(3 - millis.len() as u32);
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
}

fn parse_timing(line: &str) -> Option<(i64, i64)> {
    let (start, rest) = line.split_once("-->")?;
    let end = rest.split_whitespace().next()?;
    Some((parse_timestamp(start)?, parse_timestamp(end)?))
}

/// Parses a SubRip file into cues sorted by start time. Cue numbers are
/// optional, text runs to the next blank line, and cues with no text or an
/// end before their start are skipped.
pub fn parse_srt(content: &str) -> Result<Vec<Cue>, String> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    let mut lines = normalized.lines().peekable();
    while let Some(line) = lines.next() {
        let timing = match parse_timing(line) {
            Some(timing) => timing,
            None => match lines.peek().and_then(|next| parse_timing(next)) {
                Some(timing) if line.trim().bytes().all(|byte| byte.is_ascii_digit()) => {
                    lines.next();
                    timing
                }
                _ => continue,
            },
        };
        let mut body = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                break;
            }
            body.push(lines.next().expect("peeked line"));
        }
        let (start_ms, end_ms) = timing;
        let text = markup_free(&body.join("\n"));
        if end_ms > start_ms && !text.trim().is_empty() {
            cues.push(Cue { start_ms, end_ms, text });
        }
    }
    if cues.is_empty() {
        return Err("no SubRip cues found".to_owned());
    }
    cues.sort_by_key(|cue| cue.start_ms);
    Ok(cues)
}

/// Removes SubRip's HTML-style formatting tags (`<i>`, `<b>`, `<u>`,
/// `<font …>`, and their closers) and ASS-style `{\…}` override blocks,
/// which the renderer does not style, keeping all other text.
pub fn markup_free(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(['<', '{']) {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        let close = if tail.starts_with('<') { '>' } else { '}' };
        let end = tail.find(close);
        let tag = end.map(|end| &tail[1..end]);
        let is_markup = match (close, tag) {
            ('>', Some(tag)) => {
                let name = tag.trim_start_matches('/').split([' ', '\t']).next().unwrap_or("").to_ascii_lowercase();
                matches!(name.as_str(), "i" | "b" | "u" | "s" | "font")
            }
            ('}', Some(tag)) => tag.starts_with('\\'),
            _ => false,
        };
        if is_markup {
            rest = &tail[end.expect("markup has a closer") + 1..];
        } else {
            out.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim_matches('\n').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start_ms: i64, end_ms: i64, text: &str) -> Cue {
        Cue { start_ms, end_ms, text: text.to_owned() }
    }

    #[test]
    fn parses_numbered_multiline_cues() {
        let srt = "1\n00:00:00,200 --> 00:00:01,500\nHello from SubRip.\nSecond line\n\n2\n00:00:01,600 --> 00:00:02,800\nSecond cue.\n";
        assert_eq!(
            parse_srt(srt).unwrap(),
            vec![cue(200, 1500, "Hello from SubRip.\nSecond line"), cue(1600, 2800, "Second cue.")]
        );
    }

    #[test]
    fn tolerates_bom_crlf_missing_numbers_dots_and_coordinates() {
        let srt = "\u{feff}00:00:01.5 --> 00:00:02,000 X1:10 X2:20 Y1:0 Y2:5\r\nNo number\r\n\r\n\r\n7\r\n100:00:00,000 --> 100:00:01,000\r\nLong film\r\n";
        assert_eq!(parse_srt(srt).unwrap(), vec![cue(1500, 2000, "No number"), cue(360_000_000, 360_001_000, "Long film")]);
    }

    #[test]
    fn keeps_international_text_and_strips_formatting() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\n<i>مرحبا</i> <font color=\"#ff0000\">שלום</font>\n{\\an8}नमस्ते 你好 <3 {not a tag}\n";
        assert_eq!(parse_srt(srt).unwrap()[0].text, "مرحبا שלום\nनमस्ते 你好 <3 {not a tag}");
    }

    #[test]
    fn skips_broken_cues_and_rejects_files_without_any() {
        let srt = "1\n00:00:05,000 --> 00:00:04,000\nBackwards\n\n2\n00:00:06,000 --> 00:00:07,000\n\n3\n00:0x:08,000 --> 00:00:09,000\nBad time\n\n4\n00:00:10,000 --> 00:00:11,000\nGood\n";
        assert_eq!(parse_srt(srt).unwrap(), vec![cue(10_000, 11_000, "Good")]);
        assert!(parse_srt("nothing here\n").is_err());
        assert!(parse_srt("").is_err());
    }

    #[test]
    fn timestamps_need_three_fields_and_valid_ranges() {
        assert_eq!(parse_timestamp("00:00:01,500"), Some(1500));
        assert_eq!(parse_timestamp("01:02:03.04"), Some(3_723_040));
        assert_eq!(parse_timestamp("01:02,500"), None);
        assert_eq!(parse_timestamp("00:60:00,000"), None);
        assert_eq!(parse_timestamp("00:00:01"), None);
    }

    fn embedded() -> EngineSub {
        EngineSub {
            source: SubTrack { stream: Some(2), language: None, title: None },
            cues: Vec::new(),
            fed: HashSet::new(),
            visible: true,
        }
    }

    #[test]
    fn overlapping_cues_show_together_in_start_order() {
        let mut sub = embedded();
        sub.feed(b"second", 1.0, 2.0);
        sub.feed(b"first", 0.5, 1.0);
        assert_eq!(sub.text_at(400), "");
        assert_eq!(sub.text_at(700), "first");
        assert_eq!(sub.text_at(1200), "first\nsecond");
        assert_eq!(sub.text_at(1500), "second");
        assert_eq!(sub.text_at(3000), "");
    }

    #[test]
    fn refed_packets_are_ignored_and_hidden_tracks_show_nothing() {
        let mut sub = embedded();
        sub.feed(b"<b>once</b>", 1.0, 1.0);
        sub.feed(b"<b>once</b>", 1.0, 1.0);
        assert_eq!(sub.text_at(1500), "once");
        sub.set_visible(false);
        assert_eq!(sub.text_at(1500), "");
    }
}
