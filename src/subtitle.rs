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
        // Binary search for efficiency
        self.entries
            .iter()
            .find(|e| time_us >= e.start_us && time_us <= e.end_us)
            .map(|e| e.text.as_str())
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
