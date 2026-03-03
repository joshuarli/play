use anyhow::{bail, Result};

/// Format microseconds as HH:MM:SS.
pub fn format_time(us: i64) -> String {
    let total_secs = (us.unsigned_abs() / 1_000_000) as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if us < 0 {
        format!("-{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{hours:02}:{mins:02}:{secs:02}")
    }
}

/// Parse a time string (HH:MM:SS, MM:SS, or bare seconds) into microseconds.
pub fn parse_time(s: &str) -> Result<i64> {
    let parts: Vec<&str> = s.split(':').collect();
    let secs: f64 = match parts.len() {
        1 => s.parse()?,
        2 => {
            let m: f64 = parts[0].parse()?;
            let s: f64 = parts[1].parse()?;
            m * 60.0 + s
        }
        3 => {
            let h: f64 = parts[0].parse()?;
            let m: f64 = parts[1].parse()?;
            let s: f64 = parts[2].parse()?;
            h * 3600.0 + m * 60.0 + s
        }
        _ => bail!("Invalid time format: {s}"),
    };
    Ok((secs * 1_000_000.0) as i64)
}

/// Convert ffmpeg timebase-based PTS to microseconds.
pub fn pts_to_us(pts: i64, time_base: ffmpeg_next::Rational) -> i64 {
    let num = time_base.numerator() as i64;
    let den = time_base.denominator() as i64;
    if den == 0 {
        return 0;
    }
    pts * num * 1_000_000 / den
}

/// Convert microseconds to ffmpeg timebase-based PTS.
#[allow(dead_code)]
pub fn us_to_pts(us: i64, time_base: ffmpeg_next::Rational) -> i64 {
    let num = time_base.numerator() as i64;
    let den = time_base.denominator() as i64;
    if num == 0 {
        return 0;
    }
    us * den / (num * 1_000_000)
}
