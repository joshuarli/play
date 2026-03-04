use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};

/// Current wall-clock time in milliseconds (monotonic-ish, for OSD timeouts).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

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
    (pts as i128 * num as i128 * 1_000_000 / den as i128) as i64
}

/// Convert microseconds to ffmpeg timebase-based PTS.
#[allow(dead_code)]
pub fn us_to_pts(us: i64, time_base: ffmpeg_next::Rational) -> i64 {
    let num = time_base.numerator() as i64;
    let den = time_base.denominator() as i64;
    if num == 0 {
        return 0;
    }
    (us as i128 * den as i128 / (num as i128 * 1_000_000)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use ffmpeg_next::Rational;

    // --- format_time ---

    #[test]
    fn format_time_zero() {
        assert_eq!(format_time(0), "00:00:00");
    }

    #[test]
    fn format_time_positive() {
        // 1h 23m 45s
        let us = (1 * 3600 + 23 * 60 + 45) * 1_000_000;
        assert_eq!(format_time(us), "01:23:45");
    }

    #[test]
    fn format_time_negative() {
        let us = -((2 * 60 + 30) * 1_000_000);
        assert_eq!(format_time(us), "-00:02:30");
    }

    #[test]
    fn format_time_boundary() {
        // 59:59:59
        let us = (59 * 3600 + 59 * 60 + 59) * 1_000_000;
        assert_eq!(format_time(us), "59:59:59");
    }

    #[test]
    fn format_time_large() {
        // 100 hours
        let us: i64 = 100 * 3600 * 1_000_000;
        assert_eq!(format_time(us), "100:00:00");
    }

    #[test]
    fn format_time_truncates_subsecond() {
        // 1.999s should display as 1s
        assert_eq!(format_time(1_999_999), "00:00:01");
    }

    // --- parse_time ---

    #[test]
    fn parse_time_bare_seconds() {
        assert_eq!(parse_time("90").unwrap(), 90_000_000);
    }

    #[test]
    fn parse_time_mm_ss() {
        assert_eq!(parse_time("1:30").unwrap(), 90_000_000);
    }

    #[test]
    fn parse_time_hh_mm_ss() {
        assert_eq!(parse_time("1:02:03").unwrap(), (3600 + 120 + 3) * 1_000_000);
    }

    #[test]
    fn parse_time_fractional() {
        assert_eq!(parse_time("1.5").unwrap(), 1_500_000);
    }

    #[test]
    fn parse_time_invalid() {
        assert!(parse_time("not:a:time:stamp").is_err());
        assert!(parse_time("abc").is_err());
    }

    // --- pts_to_us / us_to_pts ---

    #[test]
    fn pts_to_us_90k_timebase() {
        let tb = Rational::new(1, 90000);
        // 90000 ticks = 1 second = 1_000_000 us
        assert_eq!(pts_to_us(90000, tb), 1_000_000);
    }

    #[test]
    fn pts_to_us_48k_timebase() {
        let tb = Rational::new(1, 48000);
        assert_eq!(pts_to_us(48000, tb), 1_000_000);
    }

    #[test]
    fn pts_to_us_zero_denominator() {
        let tb = Rational::new(0, 0);
        assert_eq!(pts_to_us(12345, tb), 0);
    }

    #[test]
    fn us_to_pts_90k_timebase() {
        let tb = Rational::new(1, 90000);
        assert_eq!(us_to_pts(1_000_000, tb), 90000);
    }

    #[test]
    fn us_to_pts_zero_numerator() {
        let tb = Rational::new(0, 48000);
        assert_eq!(us_to_pts(1_000_000, tb), 0);
    }

    #[test]
    fn pts_us_round_trip() {
        let tb = Rational::new(1, 90000);
        let original_us: i64 = 5_000_000; // 5 seconds
        let pts = us_to_pts(original_us, tb);
        let recovered = pts_to_us(pts, tb);
        assert_eq!(recovered, original_us);
    }

    #[test]
    fn pts_to_us_large_value_no_overflow() {
        let tb = Rational::new(1, 90000);
        // 8 hours in 90kHz ticks — would overflow with i64 multiply
        let pts: i64 = 8 * 3600 * 90000;
        let us = pts_to_us(pts, tb);
        assert_eq!(us, 8 * 3600 * 1_000_000);
    }
}
