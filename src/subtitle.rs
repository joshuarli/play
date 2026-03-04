use std::path::{Path, PathBuf};

use anyhow::Result;

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
        let idx = self
            .entries
            .partition_point(|e| e.start_us <= time_us);
        if idx > 0 {
            let e = &self.entries[idx - 1];
            if time_us <= e.end_us {
                return Some(&e.text);
            }
        }
        None
    }
}

/// Parse an SRT file.
pub fn parse_srt(path: &Path) -> Result<Vec<SrtEntry>> {
    let content = std::fs::read_to_string(path)?;
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

    Ok(entries)
}

fn parse_timing_line(line: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return None;
    }
    let start = parse_srt_time(parts[0].trim())?;
    let end = parse_srt_time(parts[1].trim())?;
    Some((start, end))
}

fn parse_srt_time(s: &str) -> Option<i64> {
    // Format: HH:MM:SS,mmm
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].parse().ok()?;
    let m: f64 = parts[1].parse().ok()?;
    let secs: f64 = parts[2].parse().ok()?;
    Some(((h * 3600.0 + m * 60.0 + secs) * 1_000_000.0) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn write_temp_srt(content: &str) -> PathBuf {
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
            entries: vec![SrtEntry { start_us: 1_000_000, end_us: 2_000_000, text: "hi".into() }],
        };
        assert_eq!(track.text_at(500_000), None);
    }

    #[test]
    fn text_at_during_entry() {
        let track = SubtitleTrack {
            label: "test".into(),
            entries: vec![SrtEntry { start_us: 1_000_000, end_us: 2_000_000, text: "hi".into() }],
        };
        assert_eq!(track.text_at(1_500_000), Some("hi"));
    }

    #[test]
    fn text_at_between_and_after() {
        let track = SubtitleTrack {
            label: "test".into(),
            entries: vec![
                SrtEntry { start_us: 1_000_000, end_us: 2_000_000, text: "first".into() },
                SrtEntry { start_us: 3_000_000, end_us: 4_000_000, text: "second".into() },
            ],
        };
        assert_eq!(track.text_at(2_500_000), None); // between
        assert_eq!(track.text_at(5_000_000), None); // after
    }

    #[test]
    fn text_at_exact_boundary() {
        let track = SubtitleTrack {
            label: "test".into(),
            entries: vec![SrtEntry { start_us: 1_000_000, end_us: 2_000_000, text: "hi".into() }],
        };
        assert_eq!(track.text_at(1_000_000), Some("hi")); // start
        assert_eq!(track.text_at(2_000_000), Some("hi")); // end (inclusive)
    }
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
            if name.starts_with(&stem)
                && name.ends_with(".srt")
                && name != format!("{stem}.srt")
            {
                results.push(entry.path());
            }
        }
    }

    results
}
