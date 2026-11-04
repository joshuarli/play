pub(crate) fn format_time_fmt(format: &str, time: f64) -> Result<String, String> {
    if time == i64::MIN as f64 {
        return Ok("unknown".to_owned());
    }
    let sign = if time < 0.0 { "-" } else { "" };
    let time = time.abs();
    let mut whole = time as i64;
    let mut milliseconds = ((time - whole as f64) * 1000.0).round() as i64;
    if milliseconds >= 1000 {
        milliseconds -= 1000;
        whole += 1;
    }
    let total_seconds = whole;
    let total_minutes = total_seconds / 60;
    let hours = total_seconds / 3600;
    let seconds_in_hour = total_seconds - hours * 3600;
    let minutes = seconds_in_hour / 60;
    let seconds = seconds_in_hour - minutes * 60;
    let mut output = String::new();
    let mut chars = format.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            output.push(character);
            continue;
        }
        match chars.next() {
            Some('h') => output.push_str(&format!("{sign}{hours}")),
            Some('H') => output.push_str(&format!("{sign}{hours:02}")),
            Some('m') => output.push_str(&format!("{sign}{total_minutes}")),
            Some('M') => output.push_str(&format!("{minutes:02}")),
            Some('s') => output.push_str(&format!("{sign}{total_seconds}")),
            Some('S') => output.push_str(&format!("{seconds:02}")),
            Some('T') => output.push_str(&format!("{milliseconds:03}")),
            Some('f') => output.push_str(&format!("{time:.6}")),
            Some('%') => output.push('%'),
            _ => return Err(format!("Invalid time format string '{format}'")),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::format_time_fmt;

    #[test]
    fn formats_time_like_the_c_helper() {
        assert_eq!(format_time_fmt("%H:%M:%S", 3661.5).unwrap(), "01:01:01");
        assert_eq!(format_time_fmt("%M:%S", 61.0).unwrap(), "01:01");
        assert_eq!(format_time_fmt("%s.%T", 5.25).unwrap(), "5.250");
        assert_eq!(format_time_fmt("%H:%M:%S", -61.0).unwrap(), "-00:01:01");
        assert!(format_time_fmt("%Q", 1.0).is_err());
    }
}
