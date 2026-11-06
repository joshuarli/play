use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalFileError {
    Open(String),
    Read(String),
    Write(String),
    Seek(String),
    Metadata(String),
}

impl fmt::Display for LocalFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocalFileError::Open(message) => write!(formatter, "open: {message}"),
            LocalFileError::Read(message) => write!(formatter, "read: {message}"),
            LocalFileError::Write(message) => write!(formatter, "write: {message}"),
            LocalFileError::Seek(message) => write!(formatter, "seek: {message}"),
            LocalFileError::Metadata(message) => write!(formatter, "metadata: {message}"),
        }
    }
}

impl std::error::Error for LocalFileError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekWhence {
    Start,
    Current,
    End,
}

#[derive(Debug)]
pub struct LocalFile {
    file: File,
    length: u64,
    position: u64,
    writable: bool,
}

impl LocalFile {
    pub fn open_read(path: &Path) -> Result<Self, LocalFileError> {
        let file = File::open(path).map_err(|error| LocalFileError::Open(error.to_string()))?;
        Self::from_file(file, false)
    }

    pub fn open_write(path: &Path) -> Result<Self, LocalFileError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|error| LocalFileError::Open(error.to_string()))?;
        Self::from_file(file, true)
    }

    fn from_file(file: File, writable: bool) -> Result<Self, LocalFileError> {
        let length = file
            .metadata()
            .map_err(|error| LocalFileError::Metadata(error.to_string()))?
            .len();
        Ok(Self {
            file,
            length,
            position: 0,
            writable,
        })
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, LocalFileError> {
        let read = self
            .file
            .read(buffer)
            .map_err(|error| LocalFileError::Read(error.to_string()))?;
        self.position += read as u64;
        Ok(read)
    }

    pub fn write(&mut self, buffer: &[u8]) -> Result<usize, LocalFileError> {
        if !self.writable {
            return Err(LocalFileError::Write("file is not writable".to_owned()));
        }
        let written = self
            .file
            .write(buffer)
            .map_err(|error| LocalFileError::Write(error.to_string()))?;
        self.position += written as u64;
        if self.position > self.length {
            self.length = self.position;
        }
        Ok(written)
    }

    pub fn seek(&mut self, offset: i64, whence: SeekWhence) -> Result<u64, LocalFileError> {
        let target = match whence {
            SeekWhence::Start => {
                if offset < 0 {
                    return Err(LocalFileError::Seek("negative start position".to_owned()));
                }
                SeekFrom::Start(offset as u64)
            }
            SeekWhence::Current => SeekFrom::Current(offset),
            SeekWhence::End => SeekFrom::End(offset),
        };
        let position = self
            .file
            .seek(target)
            .map_err(|error| LocalFileError::Seek(error.to_string()))?;
        self.position = position;
        Ok(position)
    }

    pub fn length(&mut self) -> Result<u64, LocalFileError> {
        self.length = self
            .file
            .metadata()
            .map_err(|error| LocalFileError::Metadata(error.to_string()))?
            .len();
        Ok(self.length)
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    pub fn is_eof(&mut self) -> Result<bool, LocalFileError> {
        let length = self.length()?;
        Ok(self.position >= length)
    }

    pub fn is_directory(path: &Path) -> Result<bool, LocalFileError> {
        Ok(std::fs::metadata(path)
            .map_err(|error| LocalFileError::Metadata(error.to_string()))?
            .is_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(".work/tmp")
            .join(format!("local-file-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn local_file_reads_short_and_seeks_beyond_eof() {
        let dir = test_dir("read");
        let path = dir.join("data.bin");
        fs::write(&path, b"abcdef").unwrap();
        let mut file = LocalFile::open_read(&path).unwrap();
        assert_eq!(file.length().unwrap(), 6);
        let mut buffer = [0u8; 16];
        assert_eq!(file.read(&mut buffer).unwrap(), 6);
        assert_eq!(&buffer[..6], b"abcdef");
        assert_eq!(file.read(&mut buffer).unwrap(), 0);
        assert_eq!(file.seek(100, SeekWhence::Start).unwrap(), 100);
        assert_eq!(file.read(&mut buffer).unwrap(), 0);
        assert_eq!(file.seek(10, SeekWhence::End).unwrap(), 16);
        assert_eq!(file.seek(2, SeekWhence::Current).unwrap(), 18);
        assert_eq!(file.position(), 18);
        assert!(file.is_eof().unwrap());
        assert!(file.seek(-1, SeekWhence::Start).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn local_file_writes_and_reports_unicode_paths() {
        let dir = test_dir("write");
        let path = dir.join("naïve 日本語.bin");
        let mut file = LocalFile::open_write(&path).unwrap();
        assert_eq!(file.write(b"hello").unwrap(), 5);
        assert_eq!(file.position(), 5);
        assert_eq!(file.length().unwrap(), 5);
        file.seek(0, SeekWhence::Start).unwrap();
        let mut buffer = [0u8; 8];
        assert_eq!(file.read(&mut buffer).unwrap(), 5);
        assert_eq!(&buffer[..5], b"hello");
        assert!(!LocalFile::is_directory(&path).unwrap());
        assert!(LocalFile::is_directory(&dir).unwrap());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn local_file_reports_permission_errors() {
        let dir = test_dir("permission");
        let path = dir.join("secret.bin");
        fs::write(&path, b"secret").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&path, permissions).unwrap();
        let error = LocalFile::open_read(&path).unwrap_err();
        assert!(matches!(error, LocalFileError::Open(_)), "{error}");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&path, permissions).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }
}
