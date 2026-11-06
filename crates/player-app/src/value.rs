const MAX_JSON_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum NativeValue {
    None,
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Array(Vec<NativeValue>),
    Map(Vec<(String, NativeValue)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonError;

pub fn parse_json(text: &str) -> Result<NativeValue, JsonError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        position: 0,
    };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.position < parser.bytes.len() {
        return Err(JsonError);
    }
    Ok(value)
}

pub fn format_json(value: &NativeValue) -> Result<String, JsonError> {
    let mut output = String::new();
    write_value(value, &mut output, 0)?;
    Ok(output)
}

fn write_value(value: &NativeValue, output: &mut String, depth: usize) -> Result<(), JsonError> {
    if depth > MAX_JSON_DEPTH {
        return Err(JsonError);
    }
    match value {
        NativeValue::None => output.push_str("null"),
        NativeValue::Bool(value) => {
            output.push_str(if *value { "true" } else { "false" })
        }
        NativeValue::Int(value) => output.push_str(&value.to_string()),
        NativeValue::Double(value) => {
            if !value.is_finite() {
                return Err(JsonError);
            }
            output.push_str(&value.to_string());
        }
        NativeValue::String(value) => write_string(value, output),
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
    fn value(&mut self, depth: usize) -> Result<NativeValue, JsonError> {
        if depth > MAX_JSON_DEPTH {
            return Err(JsonError);
        }
        let byte = *self.bytes.get(self.position).ok_or(JsonError)?;
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
            _ => Err(JsonError),
        }
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), JsonError> {
        if self.bytes.get(self.position..self.position + literal.len()) == Some(literal) {
            self.position += literal.len();
            Ok(())
        } else {
            Err(JsonError)
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.position += 1;
        let mut output = String::new();
        loop {
            let byte = *self.bytes.get(self.position).ok_or(JsonError)?;
            self.position += 1;
            match byte {
                b'"' => return Ok(output),
                b'\\' => {
                    let escape = *self.bytes.get(self.position).ok_or(JsonError)?;
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
                                    return Err(JsonError);
                                }
                                self.position += 2;
                                let second = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&second) {
                                    return Err(JsonError);
                                }
                                let codepoint = 0x10000
                                    + ((first - 0xd800) << 10)
                                    + (second - 0xdc00);
                                output.push(char::from_u32(codepoint).ok_or(JsonError)?);
                            } else {
                                output.push(char::from_u32(first).ok_or(JsonError)?);
                            }
                        }
                        _ => return Err(JsonError),
                    }
                }
                byte if byte < 0x20 => return Err(JsonError),
                byte if byte < 0x80 => output.push(byte as char),
                _ => {
                    let start = self.position - 1;
                    let length = utf8_length(byte).ok_or(JsonError)?;
                    let end = start + length;
                    let text = std::str::from_utf8(self.bytes.get(start..end).ok_or(JsonError)?)
                        .map_err(|_| JsonError)?;
                    output.push_str(text);
                    self.position = end;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = *self.bytes.get(self.position).ok_or(JsonError)?;
            self.position += 1;
            value = (value << 4)
                | match byte {
                    b'0'..=b'9' => (byte - b'0') as u32,
                    b'a'..=b'f' => (byte - b'a' + 10) as u32,
                    b'A'..=b'F' => (byte - b'A' + 10) as u32,
                    _ => return Err(JsonError),
                };
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<NativeValue, JsonError> {
        let start = self.position;
        if self.bytes.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        let digits = self.position;
        while matches!(self.bytes.get(self.position), Some(b'0'..=b'9')) {
            self.position += 1;
        }
        if self.position == digits {
            return Err(JsonError);
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
                return Err(JsonError);
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
                return Err(JsonError);
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.position]).map_err(|_| JsonError)?;
        if is_float {
            Ok(NativeValue::Double(text.parse().map_err(|_| JsonError)?))
        } else if let Ok(value) = text.parse::<i64>() {
            Ok(NativeValue::Int(value))
        } else {
            Ok(NativeValue::Double(text.parse().map_err(|_| JsonError)?))
        }
    }

    fn array(&mut self, depth: usize) -> Result<NativeValue, JsonError> {
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
                _ => return Err(JsonError),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<NativeValue, JsonError> {
        self.position += 1;
        self.skip_whitespace();
        let mut entries = Vec::new();
        if self.bytes.get(self.position) == Some(&b'}') {
            self.position += 1;
            return Ok(NativeValue::Map(entries));
        }
        loop {
            if self.bytes.get(self.position) != Some(&b'"') {
                return Err(JsonError);
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.bytes.get(self.position) != Some(&b':') {
                return Err(JsonError);
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
                _ => return Err(JsonError),
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
    fn parses_and_formats_json() {
        let value = parse_json(r#"{"a":1,"b":[true,null,"x"],"c":1.5}"#).expect("json parses");
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
        let round_trip = parse_json(&text).unwrap();
        assert_eq!(round_trip, value);
    }

    #[test]
    fn decodes_escapes_and_rejects_trailing_data() {
        let value = parse_json(r#""a\u00e9\n\"b""#).unwrap();
        assert_eq!(value, NativeValue::String("aé\n\"b".to_owned()));
        assert!(parse_json("[1,").is_err());
        assert!(parse_json("true false").is_err());
    }
}
