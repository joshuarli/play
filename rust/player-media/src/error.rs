use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum MediaError {
    Rejected(String),
    Open(String),
    Unsupported(String),
    Format(String),
    Io(String),
    Seek(String),
    TooLarge(&'static str),
    Eof,
}

impl fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaError::Rejected(message) => write!(formatter, "rejected: {message}"),
            MediaError::Open(message) => write!(formatter, "open: {message}"),
            MediaError::Unsupported(message) => {
                write!(formatter, "unsupported: {message}")
            }
            MediaError::Format(message) => write!(formatter, "format: {message}"),
            MediaError::Io(message) => write!(formatter, "io: {message}"),
            MediaError::Seek(message) => write!(formatter, "seek: {message}"),
            MediaError::TooLarge(what) => write!(formatter, "too large: {what}"),
            MediaError::Eof => write!(formatter, "end of file"),
        }
    }
}

impl std::error::Error for MediaError {}
