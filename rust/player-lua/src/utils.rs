use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::value::NativeValue;

const MAX_JSON_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtilsError;

pub fn split_path(path: &str) -> (String, String) {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        if path.starts_with('/') {
            return ("/".to_owned(), "/".to_owned());
        }
        return (String::new(), path.to_owned());
    }
    match trimmed.rfind('/') {
        Some(0) => ("/".to_owned(), trimmed[1..].to_owned()),
        Some(index) => (trimmed[..index].to_owned(), trimmed[index + 1..].to_owned()),
        None => (String::new(), trimmed.to_owned()),
    }
}

pub fn readdir(path: &str, filter: &str) -> Result<NativeValue, UtilsError> {
    let filter = match filter {
        "all" => Filter::All,
        "files" => Filter::Files,
        "dirs" => Filter::Dirs,
        "normal" => Filter::Normal,
        _ => return Err(UtilsError),
    };
    let mut names = Vec::new();
    if matches!(filter, Filter::All) {
        names.push(".".to_owned());
        names.push("..".to_owned());
    }
    let entries = fs::read_dir(path).map_err(|_| UtilsError)?;
    for entry in entries {
        let entry = entry.map_err(|_| UtilsError)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }
        if matches!(filter, Filter::All) {
            names.push(name);
            continue;
        }
        let metadata = entry.metadata().map_err(|_| UtilsError)?;
        let keep = match filter {
            Filter::Files => metadata.is_file(),
            Filter::Dirs => metadata.is_dir(),
            Filter::Normal => metadata.is_file() || metadata.is_dir(),
            Filter::All => true,
        };
        if keep {
            names.push(name);
        }
    }
    Ok(NativeValue::Array(
        names.into_iter().map(NativeValue::String).collect(),
    ))
}

pub fn file_info(path: &str) -> Result<NativeValue, UtilsError> {
    let metadata = fs::metadata(Path::new(path)).map_err(|_| UtilsError)?;
    Ok(NativeValue::Map(vec![
        ("mode".to_owned(), NativeValue::Int(metadata.mode() as i64)),
        ("size".to_owned(), NativeValue::Int(metadata.size() as i64)),
        ("atime".to_owned(), NativeValue::Int(metadata.atime())),
        ("mtime".to_owned(), NativeValue::Int(metadata.mtime())),
        ("ctime".to_owned(), NativeValue::Int(metadata.ctime())),
        ("is_file".to_owned(), NativeValue::Bool(metadata.is_file())),
        ("is_dir".to_owned(), NativeValue::Bool(metadata.is_dir())),
    ]))
}

enum Filter {
    All,
    Files,
    Dirs,
    Normal,
}

pub fn parse_json(text: &str, allow_trailing: bool) -> Result<(NativeValue, &str), UtilsError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        position: 0,
    };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if !allow_trailing && parser.position < parser.bytes.len() {
        return Err(UtilsError);
    }
    Ok((value, &text[parser.position..]))
}

pub fn format_json(value: &NativeValue) -> Result<String, UtilsError> {
    let mut output = String::new();
    write_value(value, &mut output, 0)?;
    Ok(output)
}

fn write_value(value: &NativeValue, output: &mut String, depth: usize) -> Result<(), UtilsError> {
    if depth > MAX_JSON_DEPTH {
        return Err(UtilsError);
    }
    match value {
        NativeValue::None => output.push_str("null"),
        NativeValue::Bool(value) => {
            output.push_str(if *value { "true" } else { "false" })
        }
        NativeValue::Int(value) => output.push_str(&value.to_string()),
        NativeValue::Double(value) => {
            if !value.is_finite() {
                return Err(UtilsError);
            }
            output.push_str(&value.to_string());
        }
        NativeValue::String(value) => write_string(value, output),
        NativeValue::Bytes(_) => return Err(UtilsError),
        NativeValue::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_value(value, output, depth + 1)?;
            }
            output.push(']');
        }
        NativeValue::Map(entries) => {
            output.push('{');
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_string(key, output);
                output.push(':');
                write_value(value, output, depth + 1)?;
            }
            output.push('}');
        }
        NativeValue::Unknown => return Err(UtilsError),
    }
    Ok(())
}

fn write_string(value: &str, output: &mut String) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                output.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn value(&mut self, depth: usize) -> Result<NativeValue, UtilsError> {
        if depth > MAX_JSON_DEPTH {
            return Err(UtilsError);
        }
        let byte = *self.bytes.get(self.position).ok_or(UtilsError)?;
        match byte {
            b'n' => {
                self.literal(b"null")?;
                Ok(NativeValue::None)
            }
            b't' => {
                self.literal(b"true")?;
                Ok(NativeValue::Bool(true))
            }
            b'f' => {
                self.literal(b"false")?;
                Ok(NativeValue::Bool(false))
            }
            b'"' => self.string().map(NativeValue::String),
            b'[' => self.array(depth),
            b'{' => self.object(depth),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(UtilsError),
        }
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), UtilsError> {
        if self.bytes.get(self.position..self.position + literal.len()) == Some(literal) {
            self.position += literal.len();
            Ok(())
        } else {
            Err(UtilsError)
        }
    }

    fn string(&mut self) -> Result<String, UtilsError> {
        self.position += 1;
        let mut output = String::new();
        loop {
            let byte = *self.bytes.get(self.position).ok_or(UtilsError)?;
            self.position += 1;
            match byte {
                b'"' => return Ok(output),
                b'\\' => {
                    let escape = *self.bytes.get(self.position).ok_or(UtilsError)?;
                    self.position += 1;
                    match escape {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{08}'),
                        b'f' => output.push('\u{0c}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            if (0xd800..0xdc00).contains(&first) {
                                if self.bytes.get(self.position..self.position + 2)
                                    != Some(b"\\u")
                                {
                                    return Err(UtilsError);
                                }
                                self.position += 2;
                                let second = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&second) {
                                    return Err(UtilsError);
                                }
                                let codepoint = 0x10000
                                    + ((first - 0xd800) << 10)
                                    + (second - 0xdc00);
                                output.push(char::from_u32(codepoint).ok_or(UtilsError)?);
                            } else {
                                output.push(char::from_u32(first).ok_or(UtilsError)?);
                            }
                        }
                        _ => return Err(UtilsError),
                    }
                }
                byte if byte < 0x20 => return Err(UtilsError),
                byte if byte < 0x80 => output.push(byte as char),
                _ => {
                    let start = self.position - 1;
                    let length = utf8_length(byte).ok_or(UtilsError)?;
                    let end = start + length;
                    let text = std::str::from_utf8(self.bytes.get(start..end).ok_or(UtilsError)?)
                        .map_err(|_| UtilsError)?;
                    output.push_str(text);
                    self.position = end;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, UtilsError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = *self.bytes.get(self.position).ok_or(UtilsError)?;
            self.position += 1;
            value = (value << 4)
                | match byte {
                    b'0'..=b'9' => (byte - b'0') as u32,
                    b'a'..=b'f' => (byte - b'a' + 10) as u32,
                    b'A'..=b'F' => (byte - b'A' + 10) as u32,
                    _ => return Err(UtilsError),
                };
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<NativeValue, UtilsError> {
        let start = self.position;
        if self.bytes.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        let digits = self.position;
        while matches!(self.bytes.get(self.position), Some(b'0'..=b'9')) {
            self.position += 1;
        }
        if self.position == digits {
            return Err(UtilsError);
        }
        let mut is_float = false;
        if self.bytes.get(self.position) == Some(&b'.') {
            is_float = true;
            self.position += 1;
            let fraction = self.position;
            while matches!(self.bytes.get(self.position), Some(b'0'..=b'9')) {
                self.position += 1;
            }
            if self.position == fraction {
                return Err(UtilsError);
            }
        }
        if matches!(self.bytes.get(self.position), Some(b'e' | b'E')) {
            is_float = true;
            self.position += 1;
            if matches!(self.bytes.get(self.position), Some(b'+' | b'-')) {
                self.position += 1;
            }
            let exponent = self.position;
            while matches!(self.bytes.get(self.position), Some(b'0'..=b'9')) {
                self.position += 1;
            }
            if self.position == exponent {
                return Err(UtilsError);
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.position]).map_err(|_| UtilsError)?;
        if is_float {
            Ok(NativeValue::Double(text.parse().map_err(|_| UtilsError)?))
        } else if let Ok(value) = text.parse::<i64>() {
            Ok(NativeValue::Int(value))
        } else {
            Ok(NativeValue::Double(text.parse().map_err(|_| UtilsError)?))
        }
    }

    fn array(&mut self, depth: usize) -> Result<NativeValue, UtilsError> {
        self.position += 1;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.bytes.get(self.position) == Some(&b']') {
            self.position += 1;
            return Ok(NativeValue::Array(values));
        }
        loop {
            values.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => {
                    self.position += 1;
                    self.skip_whitespace();
                }
                Some(b']') => {
                    self.position += 1;
                    return Ok(NativeValue::Array(values));
                }
                _ => return Err(UtilsError),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<NativeValue, UtilsError> {
        self.position += 1;
        self.skip_whitespace();
        let mut entries = Vec::new();
        if self.bytes.get(self.position) == Some(&b'}') {
            self.position += 1;
            return Ok(NativeValue::Map(entries));
        }
        loop {
            if self.bytes.get(self.position) != Some(&b'"') {
                return Err(UtilsError);
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.bytes.get(self.position) != Some(&b':') {
                return Err(UtilsError);
            }
            self.position += 1;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => {
                    self.position += 1;
                    self.skip_whitespace();
                }
                Some(b'}') => {
                    self.position += 1;
                    entries.sort_by(|left, right| left.0.cmp(&right.0));
                    return Ok(NativeValue::Map(entries));
                }
                _ => return Err(UtilsError),
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.position),
            Some(b' ' | b'\t' | b'\n' | b'\r')
        ) {
            self.position += 1;
        }
    }
}

fn utf8_length(first: u8) -> Option<usize> {
    match first {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_paths_like_the_c_helper() {
        assert_eq!(split_path("/a/b"), ("/a".to_owned(), "b".to_owned()));
        assert_eq!(split_path("/a/b/"), ("/a".to_owned(), "b".to_owned()));
        assert_eq!(split_path("b"), (String::new(), "b".to_owned()));
        assert_eq!(split_path("/b"), ("/".to_owned(), "b".to_owned()));
    }

    #[test]
    fn parses_and_formats_json() {
        let (value, rest) = parse_json(r#"{"a":1,"b":[true,null,"x"],"c":1.5}"#, false)
            .expect("json parses");
        assert_eq!(rest, "");
        assert_eq!(
            value,
            NativeValue::Map(vec![
                ("a".to_owned(), NativeValue::Int(1)),
                (
                    "b".to_owned(),
                    NativeValue::Array(vec![
                        NativeValue::Bool(true),
                        NativeValue::None,
                        NativeValue::String("x".to_owned()),
                    ]),
                ),
                ("c".to_owned(), NativeValue::Double(1.5)),
            ])
        );
        let text = format_json(&value).unwrap();
        let (round_trip, _) = parse_json(&text, false).unwrap();
        assert_eq!(round_trip, value);
    }

    #[test]
    fn handles_strings_and_trailing_data() {
        let (value, rest) = parse_json(r#""a\u00e9\n\"b" tail"#, true).unwrap();
        assert_eq!(value, NativeValue::String("aé\n\"b".to_owned()));
        assert_eq!(rest, "tail");
        assert!(parse_json("[1,", false).is_err());
        assert!(parse_json("true false", false).is_err());
    }

    #[test]
    fn reads_directory_entries() {
        let root = std::path::Path::new("../../.work/tmp/lua-utils-test");
        let _ = std::fs::remove_dir_all(root);
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("file"), b"x").unwrap();
        let entries = readdir(root.to_str().unwrap(), "normal").unwrap();
        let NativeValue::Array(entries) = entries else {
            panic!("readdir did not return an array");
        };
        let names: Vec<&str> = entries
            .iter()
            .filter_map(|entry| match entry {
                NativeValue::String(name) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"dir"));
        assert!(names.contains(&"file"));
        let info = file_info(root.join("file").to_str().unwrap()).unwrap();
        let NativeValue::Map(entries) = info else {
            panic!("file_info did not return a map");
        };
        assert!(entries.iter().any(|(key, value)| key == "is_file"
            && value == &NativeValue::Bool(true)));
        let _ = std::fs::remove_dir_all(root);
    }
}
