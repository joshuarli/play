use std::fmt;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum InputPath {
    Local(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
pub enum InputError {
    Empty,
    RemoteScheme(String),
    Wrapper(String),
    InvalidFileUri,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputError::Empty => write!(f, "empty input"),
            InputError::RemoteScheme(scheme) => write!(f, "remote scheme: {scheme}"),
            InputError::Wrapper(wrapper) => write!(f, "protocol wrapper: {wrapper}"),
            InputError::InvalidFileUri => write!(f, "invalid file URI"),
        }
    }
}

const WRAPPERS: &[&str] = &[
    "data:", "lavfi:", "edl:", "memory:", "fd:", "fdclose:", "av:",
    "avdevice:", "mf:", "null:", "slice:", "appending:", "mpv:",
    "ffmpeg:", "lavf:", "http:", "https:", "ftp:", "sftp:", "ssh:",
    "rtsp:", "rtmp:", "hls:", "dash:", "smb:",
];

pub fn classify_input(input: &str, allow_file_uri: bool) -> Result<InputPath, InputError> {
    if input.is_empty() {
        return Err(InputError::Empty);
    }

    if let Some(pos) = input.find("://") {
        let scheme = &input[..pos];
        if valid_scheme(scheme) {
            if scheme.eq_ignore_ascii_case("file") && allow_file_uri {
                let path = percent_decode(&input[pos + 3..])?;
                if path.is_empty() {
                    return Err(InputError::InvalidFileUri);
                }
                return Ok(InputPath::Local(PathBuf::from(path)));
            }
            return Err(InputError::RemoteScheme(scheme.to_ascii_lowercase()));
        }
    }

    let lower = input.to_ascii_lowercase();
    for wrapper in WRAPPERS {
        if lower.starts_with(wrapper) {
            return Err(InputError::Wrapper((*wrapper).to_string()));
        }
    }

    Ok(InputPath::Local(PathBuf::from(input)))
}

fn valid_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() &&
        chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

fn percent_decode(input: &str) -> Result<String, InputError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(InputError::InvalidFileUri);
            }
            let hi = hex(bytes[i + 1]).ok_or(InputError::InvalidFileUri)?;
            let lo = hex(bytes[i + 2]).ok_or(InputError::InvalidFileUri)?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| InputError::InvalidFileUri)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_input, InputError, InputPath};
    use std::path::PathBuf;

    fn local(input: &str, allow_file_uri: bool) -> PathBuf {
        match classify_input(input, allow_file_uri) {
            Ok(InputPath::Local(path)) => path,
            other => panic!("expected local path, got {other:?}"),
        }
    }

    #[test]
    fn accepts_local_paths() {
        assert_eq!(local("/tmp/a.mp4", false), PathBuf::from("/tmp/a.mp4"));
        assert_eq!(local("relative:name.mp4", false),
                   PathBuf::from("relative:name.mp4"));
        assert_eq!(local("with space and ünïcode.mp4", false),
                   PathBuf::from("with space and ünïcode.mp4"));
    }

    #[test]
    fn rejects_remote_schemes() {
        for input in [
            "http://example.com/a.mp4", "https://example.com/a.mp4",
            "ftp://example.com/a.mp4", "sftp://example.com/a.mp4",
            "ssh://example.com/a.mp4", "rtsp://example.com/a.mp4",
            "rtmp://example.com/a.mp4", "hls://example.com/a.m3u8",
            "dash://example.com/a.mpd", "smb://example.com/a.mp4",
        ] {
            assert!(matches!(classify_input(input, false),
                             Err(InputError::RemoteScheme(_))));
        }
    }

    #[test]
    fn rejects_wrappers() {
        for input in [
            "data:text/plain,hello", "lavfi:color=red", "edl://%10%a.mp4",
            "memory://a", "fd://0", "http:/example.com/a.mp4",
        ] {
            assert!(matches!(classify_input(input, false),
                             Err(InputError::Wrapper(_)) | Err(InputError::RemoteScheme(_))));
        }
    }

    #[test]
    fn file_uri_requires_opt_in() {
        assert!(matches!(classify_input("file:///tmp/a.mp4", false),
                         Err(InputError::RemoteScheme(_))));
        assert_eq!(local("file:///tmp/a%20b.mp4", true),
                   PathBuf::from("/tmp/a b.mp4"));
        assert!(matches!(classify_input("file://", true),
                         Err(InputError::InvalidFileUri)));
    }
}
