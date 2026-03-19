//! Subtitle parser: external SRT files and embedded stream decoding.
//!
//! Parses SubRip (.srt) files and decodes embedded text subtitle streams
//! (SRT-in-MKV, ASS/SSA, WebVTT, MOV text) into a sorted list of
//! [`SrtEntry`] cues.  [`SubtitleTrack::text_at`] uses `partition_point`
//! for O(log n) lookup of the active subtitle at any given PTS.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::subtitle::Rect;
use ffmpeg_sys_next as ffs;

/// A single SRT subtitle entry.
#[derive(Debug, Clone)]
pub struct SrtEntry {
    pub start_us: i64,
    pub end_us: i64,
    pub text: String,
}

/// A subtitle track (either embedded or external SRT).
#[derive(Debug, Clone)]
pub struct SubtitleTrack {
    pub label: String,
    pub entries: Vec<SrtEntry>,
}

impl SubtitleTrack {
    /// Get the subtitle text at a given time (microseconds).
    pub fn text_at(&self, time_us: i64) -> Option<&str> {
        let idx = self.entries.partition_point(|e| e.start_us <= time_us);
        if idx > 0 {
            let e = &self.entries[idx - 1];
            if time_us <= e.end_us {
                return Some(&e.text);
            }
        }
        None
    }
}

/// Parse an SRT file. Tries UTF-8 first, falls back to Latin-1 (ISO-8859-1).
pub fn parse_srt(path: &Path) -> Result<Vec<SrtEntry>> {
    let bytes = std::fs::read(path)?;
    let content = match std::str::from_utf8(&bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    };
    Ok(parse_srt_content(&content))
}

/// Parse SRT subtitle content from a string.
pub fn parse_srt_content(content: &str) -> Vec<SrtEntry> {
    let mut entries = Vec::new();
    let mut lines = content.lines().peekable();

    while lines.peek().is_some() {
        // Skip empty lines and sequence number
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() || line.trim().parse::<u32>().is_ok() {
                lines.next();
            } else {
                break;
            }
        }

        // Parse timestamp line: "00:01:23,456 --> 00:01:25,789"
        let Some(timing_line) = lines.next() else {
            break;
        };

        let Some((start, end)) = parse_timing_line(timing_line) else {
            continue;
        };

        // Collect text lines until empty line or EOF
        let mut text_lines = Vec::new();
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() {
                lines.next();
                break;
            }
            text_lines.push(*line);
            lines.next();
        }

        if !text_lines.is_empty() {
            entries.push(SrtEntry {
                start_us: start,
                end_us: end,
                text: text_lines.join("\n"),
            });
        }
    }

    entries
}

fn parse_timing_line(line: &str) -> Option<(i64, i64)> {
    let (left, right) = line.split_once("-->")?;
    let start = parse_srt_time(left.trim())?;
    let end = parse_srt_time(right.trim())?;
    Some((start, end))
}

fn parse_srt_time(s: &str) -> Option<i64> {
    // Format: HH:MM:SS,mmm
    let s = s.replace(',', ".");
    let (h_str, rest) = s.split_once(':')?;
    let (m_str, secs_str) = rest.split_once(':')?;
    let h: f64 = h_str.parse().ok()?;
    let m: f64 = m_str.parse().ok()?;
    let secs: f64 = secs_str.parse().ok()?;
    Some(((h * 3600.0 + m * 60.0 + secs) * 1_000_000.0) as i64)
}

/// Look for SRT files alongside a video file.
/// Checks: video.srt, video.*.srt
pub fn find_srt_files(video_path: &Path) -> Vec<PathBuf> {
    let stem = match video_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };
    let dir = video_path.parent().unwrap_or(Path::new("."));

    let mut results = Vec::new();

    // Check video.srt
    let direct = dir.join(format!("{stem}.srt"));
    if direct.exists() {
        results.push(direct);
    }

    // Check video.*.srt (e.g., video.en.srt)
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&stem) && name.ends_with(".srt") && name != format!("{stem}.srt") {
                results.push(entry.path());
            }
        }
    }

    results
}

/// Bitmap subtitle codecs that cannot be decoded to text.
const BITMAP_CODECS: &[&str] = &["dvd_subtitle", "hdmv_pgs_subtitle", "dvb_subtitle", "xsub"];

/// Decode all text subtitles from an embedded stream. Returns an empty Vec
/// for bitmap formats or streams that produce no text output.
pub fn decode_embedded_subtitles(
    path: &Path,
    stream_index: usize,
    codec_name: &str,
) -> Result<Vec<SrtEntry>> {
    if BITMAP_CODECS.contains(&codec_name) {
        return Ok(Vec::new());
    }

    let mut ictx = ffmpeg::format::input(path)
        .with_context(|| format!("Failed to open for subtitle decode: {}", path.display()))?;
    let stream = ictx
        .stream(stream_index)
        .ok_or_else(|| anyhow::anyhow!("Subtitle stream {stream_index} not found"))?;

    let codec_ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .context("Failed to create subtitle codec context")?;
    let mut decoder = codec_ctx
        .decoder()
        .subtitle()
        .context("Failed to open subtitle decoder")?;

    let mut entries = Vec::new();
    let mut sub = ffmpeg::Subtitle::new();

    for (s, packet) in ictx.packets() {
        if s.index() != stream_index {
            continue;
        }
        match decoder.decode(&packet, &mut sub) {
            Ok(true) => {
                let pts_us = sub.pts().unwrap_or(0);
                let start_us = pts_us + sub.start() as i64 * 1000;
                let end_us = pts_us + sub.end() as i64 * 1000;

                let text = extract_subtitle_text(&sub);
                // SAFETY: avsubtitle_free releases internally-allocated rects.
                // Must be called after each successful decode since ffmpeg_next's
                // Subtitle type has no Drop impl for the rect array.
                unsafe { ffs::avsubtitle_free(sub.as_mut_ptr()) };

                if !text.is_empty() && end_us > start_us {
                    entries.push(SrtEntry {
                        start_us,
                        end_us,
                        text,
                    });
                }
            }
            Ok(false) => {}
            Err(e) => log::debug!("Subtitle decode error: {e}"),
        }
    }

    entries.sort_by_key(|e| e.start_us);
    Ok(entries)
}

/// Extract plain text from decoded subtitle rects.
fn extract_subtitle_text(sub: &ffmpeg::Subtitle) -> String {
    let mut parts = Vec::new();
    for rect in sub.rects() {
        match rect {
            Rect::Text(t) => {
                let s = t.get().trim();
                if !s.is_empty() {
                    parts.push(s.to_string());
                }
            }
            Rect::Ass(a) => {
                let s = strip_ass_tags(a.get());
                if !s.is_empty() {
                    parts.push(s);
                }
            }
            _ => {}
        }
    }
    parts.join("\n")
}

/// Strip ASS/SSA formatting from decoded subtitle text.
///
/// Handles two forms:
/// - Full dialogue lines: `ReadOrder,Layer,Style,Name,ML,MR,MV,Effect,Text`
///   (9 comma-separated fields; text is everything after the 8th comma)
/// - Inline override tags: `{\b1}`, `{\i0}`, `{\an8}`, `{\pos(320,50)}`, etc.
/// - ASS line breaks: `\N` and `\n` → real newlines
pub fn strip_ass_tags(s: &str) -> String {
    // Skip the 9-field ASS dialogue prefix if present
    let text = skip_ass_prefix(s);

    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            // Skip until closing brace
            for c2 in chars.by_ref() {
                if c2 == '}' {
                    break;
                }
            }
        } else if c == '\\' {
            match chars.peek() {
                Some('N' | 'n') => {
                    chars.next();
                    out.push('\n');
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// If `s` looks like a full ASS dialogue event (9+ comma-delimited fields),
/// return the text after the 8th comma. Otherwise return `s` unchanged.
fn skip_ass_prefix(s: &str) -> &str {
    let mut commas = 0;
    for (i, c) in s.char_indices() {
        if c == ',' {
            commas += 1;
            if commas == 8 {
                return &s[i + 1..];
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn write_temp_srt(content: &str) -> PathBuf {
        write_temp_srt_bytes(content.as_bytes())
    }

    fn write_temp_srt_bytes(content: &[u8]) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let id = std::process::id();
        let path = std::env::temp_dir().join(format!("play_test_{id}_{n}.srt"));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parse_srt_basic() {
        let srt = "\
1
00:00:01,000 --> 00:00:03,000
Hello world

2
00:00:05,000 --> 00:00:07,500
Second subtitle
";
        let f = write_temp_srt(srt);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].start_us, 1_000_000);
        assert_eq!(entries[0].end_us, 3_000_000);
        assert_eq!(entries[0].text, "Hello world");
        assert_eq!(entries[1].start_us, 5_000_000);
        assert_eq!(entries[1].end_us, 7_500_000);
        assert_eq!(entries[1].text, "Second subtitle");
    }

    #[test]
    fn parse_srt_multiline_text() {
        let srt = "\
1
00:00:01,000 --> 00:00:03,000
Line one
Line two
";
        let f = write_temp_srt(srt);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "Line one\nLine two");
    }

    #[test]
    fn parse_srt_empty_file() {
        let f = write_temp_srt("");
        let entries = parse_srt(&f).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_srt_no_trailing_blank_line() {
        let srt = "\
1
00:00:01,000 --> 00:00:02,000
No newline at end";
        let f = write_temp_srt(srt);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "No newline at end");
    }

    #[test]
    fn parse_srt_bom_prefix() {
        let srt = "\u{FEFF}1
00:00:00,500 --> 00:00:01,500
BOM test";
        let f = write_temp_srt(srt);
        let entries = parse_srt(&f).unwrap();
        // BOM is part of the sequence number line, which is skipped by the
        // u32-parse check. The entry should still parse.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "BOM test");
    }

    #[test]
    fn text_at_before_first() {
        let track = SubtitleTrack {
            label: "test".into(),
            entries: vec![SrtEntry {
                start_us: 1_000_000,
                end_us: 2_000_000,
                text: "hi".into(),
            }],
        };
        assert_eq!(track.text_at(500_000), None);
    }

    #[test]
    fn text_at_during_entry() {
        let track = SubtitleTrack {
            label: "test".into(),
            entries: vec![SrtEntry {
                start_us: 1_000_000,
                end_us: 2_000_000,
                text: "hi".into(),
            }],
        };
        assert_eq!(track.text_at(1_500_000), Some("hi"));
    }

    #[test]
    fn text_at_between_and_after() {
        let track = SubtitleTrack {
            label: "test".into(),
            entries: vec![
                SrtEntry {
                    start_us: 1_000_000,
                    end_us: 2_000_000,
                    text: "first".into(),
                },
                SrtEntry {
                    start_us: 3_000_000,
                    end_us: 4_000_000,
                    text: "second".into(),
                },
            ],
        };
        assert_eq!(track.text_at(2_500_000), None); // between
        assert_eq!(track.text_at(5_000_000), None); // after
    }

    #[test]
    fn text_at_exact_boundary() {
        let track = SubtitleTrack {
            label: "test".into(),
            entries: vec![SrtEntry {
                start_us: 1_000_000,
                end_us: 2_000_000,
                text: "hi".into(),
            }],
        };
        assert_eq!(track.text_at(1_000_000), Some("hi")); // start
        assert_eq!(track.text_at(2_000_000), Some("hi")); // end (inclusive)
    }

    #[test]
    fn text_at_empty_track() {
        let track = SubtitleTrack {
            label: "empty".into(),
            entries: vec![],
        };
        assert_eq!(track.text_at(0), None);
        assert_eq!(track.text_at(1_000_000), None);
    }

    #[test]
    fn text_at_adjacent_entries() {
        let track = SubtitleTrack {
            label: "test".into(),
            entries: vec![
                SrtEntry {
                    start_us: 1_000_000,
                    end_us: 2_000_000,
                    text: "first".into(),
                },
                SrtEntry {
                    start_us: 2_000_001,
                    end_us: 3_000_000,
                    text: "second".into(),
                },
            ],
        };
        assert_eq!(track.text_at(2_000_000), Some("first"));
        assert_eq!(track.text_at(2_000_001), Some("second"));
    }

    #[test]
    fn parse_srt_extra_blank_lines() {
        let srt = "\n\n\
1
00:00:01,000 --> 00:00:02,000
Hello


2
00:00:03,000 --> 00:00:04,000
World

";
        let f = write_temp_srt(srt);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Hello");
        assert_eq!(entries[1].text, "World");
    }

    #[test]
    fn parse_srt_time_precision() {
        let srt = "\
1
01:02:03,456 --> 01:02:04,789
precise";
        let f = write_temp_srt(srt);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(
            entries[0].start_us,
            (3600 + 2 * 60 + 3) * 1_000_000 + 456_000
        );
        assert_eq!(entries[0].end_us, (3600 + 2 * 60 + 4) * 1_000_000 + 789_000);
    }

    #[test]
    fn parse_srt_malformed_timing_skipped() {
        let srt = "\
1
not a timing line
This should be skipped

2
00:00:01,000 --> 00:00:02,000
Valid entry
";
        let f = write_temp_srt(srt);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "Valid entry");
    }

    #[test]
    fn parse_srt_latin1_fallback() {
        // ISO-8859-1 encoded: "Héllo wörld" with bytes that are invalid UTF-8
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"1\r\n");
        bytes.extend_from_slice(b"00:00:01,000 --> 00:00:03,000\r\n");
        // "Héllo wörld" in Latin-1: é=0xE9, ö=0xF6
        bytes.extend_from_slice(&[
            b'H', 0xE9, b'l', b'l', b'o', b' ', b'w', 0xF6, b'r', b'l', b'd',
        ]);
        bytes.extend_from_slice(b"\r\n");

        let f = write_temp_srt_bytes(&bytes);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "H\u{e9}llo w\u{f6}rld");
    }

    #[test]
    fn parse_srt_latin1_with_crlf() {
        // Real-world pattern: Latin-1 SRT with CRLF line endings
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"1\r\n");
        bytes.extend_from_slice(b"00:00:01,000 --> 00:00:02,000\r\n");
        // "ça va" in Latin-1: ç=0xE7
        bytes.extend_from_slice(&[0xE7, b'a', b' ', b'v', b'a']);
        bytes.extend_from_slice(b"\r\n\r\n");
        bytes.extend_from_slice(b"2\r\n");
        bytes.extend_from_slice(b"00:00:03,000 --> 00:00:04,000\r\n");
        // "über" in Latin-1: ü=0xFC
        bytes.extend_from_slice(&[0xFC, b'b', b'e', b'r']);
        bytes.extend_from_slice(b"\r\n");

        let f = write_temp_srt_bytes(&bytes);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "\u{e7}a va");
        assert_eq!(entries[1].text, "\u{fc}ber");
    }

    #[test]
    fn parse_srt_utf8_still_works() {
        // UTF-8 with multi-byte characters should still parse correctly
        let srt = "\
1
00:00:01,000 --> 00:00:02,000
日本語テスト
";
        let f = write_temp_srt(srt);
        let entries = parse_srt(&f).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "日本語テスト");
    }

    // --- strip_ass_tags ---

    #[test]
    fn strip_ass_plain_text() {
        assert_eq!(strip_ass_tags("Hello world"), "Hello world");
    }

    #[test]
    fn strip_ass_override_tags() {
        assert_eq!(strip_ass_tags(r"{\b1}bold{\b0} text"), "bold text");
        assert_eq!(strip_ass_tags(r"{\i1}italic{\i0}"), "italic");
        assert_eq!(strip_ass_tags(r"{\an8}top text"), "top text");
        assert_eq!(strip_ass_tags(r"{\pos(320,50)}positioned"), "positioned");
    }

    #[test]
    fn strip_ass_line_breaks() {
        assert_eq!(strip_ass_tags(r"line one\Nline two"), "line one\nline two");
        assert_eq!(strip_ass_tags(r"line one\nline two"), "line one\nline two");
    }

    #[test]
    fn strip_ass_dialogue_prefix() {
        // Full ASS dialogue line: 9 fields separated by commas, text after 8th comma
        let ass = "0,0,Default,,0,0,0,,The actual subtitle text";
        assert_eq!(strip_ass_tags(ass), "The actual subtitle text");
    }

    #[test]
    fn strip_ass_dialogue_with_tags() {
        let ass = r"0,0,Default,,0,0,0,,{\b1}Bold{\b0} and {\i1}italic{\i0}";
        assert_eq!(strip_ass_tags(ass), "Bold and italic");
    }

    #[test]
    fn strip_ass_dialogue_with_line_break() {
        let ass = r"0,0,Default,,0,0,0,,First line\NSecond line";
        assert_eq!(strip_ass_tags(ass), "First line\nSecond line");
    }

    #[test]
    fn strip_ass_empty_input() {
        assert_eq!(strip_ass_tags(""), "");
    }

    #[test]
    fn strip_ass_only_tags() {
        assert_eq!(strip_ass_tags(r"{\an8}{\pos(320,50)}"), "");
    }

    #[test]
    fn strip_ass_commas_in_text() {
        // Text field itself may contain commas
        let ass = "0,0,Default,,0,0,0,,Hello, world, how are you?";
        assert_eq!(strip_ass_tags(ass), "Hello, world, how are you?");
    }

    #[test]
    fn find_srt_files_discovers_variants() {
        let dir = std::env::temp_dir().join(format!("play_srt_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("movie.mp4");
        std::fs::write(&video, b"").unwrap();
        std::fs::write(dir.join("movie.srt"), b"").unwrap();
        std::fs::write(dir.join("movie.en.srt"), b"").unwrap();

        let found = find_srt_files(&video);
        assert!(found.len() >= 2);
        assert!(found.iter().any(|p| p.ends_with("movie.srt")));
        assert!(found.iter().any(|p| p.ends_with("movie.en.srt")));

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }
}
