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
            InputError::Empty => write!(f, "Refusing non-local input: empty input"),
            InputError::RemoteScheme(scheme) => {
                write!(f, "Refusing non-local input: remote scheme: {scheme}")
            }
            InputError::Wrapper(wrapper) => {
                write!(f, "Refusing non-local input: protocol wrapper: {wrapper}")
            }
            InputError::InvalidFileUri => {
                write!(f, "Refusing non-local input: invalid file URI")
            }
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
            return Err(InputError::Wrapper(wrapper.to_string()));
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

    #[test]
    fn empty_input_is_refused_with_or_without_file_uris() {
        assert_eq!(classify_input("", false), Err(InputError::Empty));
        assert_eq!(classify_input("", true), Err(InputError::Empty));
    }

    #[test]
    fn every_wrapper_prefix_is_refused_in_any_case() {
        let wrappers = [
            "data:", "lavfi:", "edl:", "memory:", "fd:", "fdclose:", "av:",
            "avdevice:", "mf:", "null:", "slice:", "appending:", "mpv:",
            "ffmpeg:", "lavf:", "http:", "https:", "ftp:", "sftp:", "ssh:",
            "rtsp:", "rtmp:", "hls:", "dash:", "smb:",
        ];
        assert_eq!(wrappers, super::WRAPPERS);
        for wrapper in wrappers {
            for input in [format!("{wrapper}x"), format!("{}x", wrapper.to_ascii_uppercase())] {
                assert_eq!(classify_input(&input, false),
                           Err(InputError::Wrapper(wrapper.to_string())), "{input}");
                assert_eq!(classify_input(&input, true),
                           Err(InputError::Wrapper(wrapper.to_string())), "{input}");
            }
        }
    }

    #[test]
    fn schemes_are_matched_in_any_case_and_reported_lowercase() {
        assert_eq!(classify_input("HTTP://example.com/a", false),
                   Err(InputError::RemoteScheme("http".to_string())));
        assert_eq!(classify_input("x+y-z.w://host/a", true),
                   Err(InputError::RemoteScheme("x+y-z.w".to_string())));
        assert_eq!(classify_input("FILE:///tmp/a.mp4", false),
                   Err(InputError::RemoteScheme("file".to_string())));
        assert_eq!(local("FILE:///tmp/a.mp4", true), PathBuf::from("/tmp/a.mp4"));
    }

    #[test]
    fn a_separator_inside_a_path_is_not_a_scheme() {
        for input in ["1http://example.com/a.mp4", "/tmp/dir://a.mp4", "dir/x://a.mp4", "a b://c", "://a.mp4", "-x://a"] {
            assert_eq!(local(input, true), PathBuf::from(input), "{input}");
        }
    }

    #[test]
    fn file_uri_escapes_decode_in_either_hex_case() {
        assert_eq!(local("file:///tmp/a%2fb%2Fc%41", true), PathBuf::from("/tmp/a/b/cA"));
        assert_eq!(local("file:///tmp/%e3%81%82%C3%A9.mp4", true), PathBuf::from("/tmp/\u{3042}\u{e9}.mp4"));
        assert_eq!(local("file:///%41", true), PathBuf::from("/A"));
    }

    #[test]
    fn malformed_file_uri_escapes_are_refused() {
        for input in ["file:///a%", "file:///a%4", "file:///a%4g", "file:///a%zz", "file:///%ff", "file:///%c3"] {
            assert_eq!(classify_input(input, true), Err(InputError::InvalidFileUri), "{input}");
        }
    }
}
