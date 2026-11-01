// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "aac", "adts", "aif", "aiff", "caf", "flac", "m4a", "m4b", "m4p", "mka", "mkv", "mp1", "mp2",
    "mp3", "mp4", "oga", "ogg", "opus", "wav", "wave", "webm", "wv",
];

pub struct Playlist {
    paths: Vec<PathBuf>,
    index: usize,
    from_directory: bool,
}

impl Playlist {
    pub fn from_input(input: &Path) -> io::Result<Self> {
        if input.as_os_str() == "-" || !input.is_dir() {
            return Ok(Self { paths: vec![input.to_path_buf()], index: 0, from_directory: false });
        }

        let mut paths = Vec::new();

        for entry in fs::read_dir(input)? {
            let entry = entry?;

            if !entry.file_type()?.is_file() {
                continue;
            }

            let name = entry.file_name();

            if is_hidden(&name) || !is_supported(&name) {
                continue;
            }

            paths.push(entry.path());
        }

        paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        if paths.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("no supported audio files found in '{}'", input.display()),
            ));
        }

        Ok(Self { paths, index: 0, from_directory: true })
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_directory(&self) -> bool {
        self.from_directory
    }

    pub fn current(&self) -> &Path {
        &self.paths[self.index]
    }

    pub fn current_index(&self) -> usize {
        self.index
    }

    pub fn next(&mut self) {
        self.index = (self.index + 1) % self.paths.len();
    }

    pub fn previous(&mut self) {
        self.index = if self.index == 0 { self.paths.len() - 1 } else { self.index - 1 };
    }

    pub fn advance(&mut self) -> bool {
        if self.index + 1 < self.paths.len() {
            self.index += 1;
            true
        }
        else {
            false
        }
    }
}

fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

fn is_supported(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            SUPPORTED_EXTENSIONS.iter().any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

#[cfg(test)]
mod tests {
    use super::Playlist;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "symphonia-play-playlist-{}-{id}",
                std::process::id()
            ));

            fs::create_dir_all(&path).expect("test directory should be created");

            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write(&self, name: &str) {
            fs::write(self.path.join(name), b"").expect("test file should be created");
        }

        fn create_dir(&self, name: &str) {
            fs::create_dir(self.path.join(name)).expect("test directory should be created");
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn names(playlist: &Playlist) -> Vec<String> {
        playlist
            .paths()
            .iter()
            .map(|path| path.file_name().and_then(OsStr::to_str).unwrap().to_string())
            .collect()
    }

    #[test]
    fn directory_playlist_filters_and_sorts_files() {
        let dir = TestDir::new();
        dir.write("b.mp3");
        dir.write("a.FLAC");
        dir.write("c.wav");
        dir.write(".hidden.mp3");
        dir.write("notes.txt");
        dir.write("track.ogg.bak");
        dir.create_dir("nested");
        fs::write(dir.path().join("nested").join("nested.mp3"), b"")
            .expect("nested test file should be created");

        let playlist = Playlist::from_input(dir.path()).expect("directory should form a playlist");

        assert!(playlist.is_directory());
        assert_eq!(playlist.len(), 3);
        assert_eq!(names(&playlist), ["a.FLAC", "b.mp3", "c.wav"]);
        assert_eq!(playlist.current_index(), 0);
    }

    #[test]
    fn directory_playlist_errors_when_no_supported_files_exist() {
        let dir = TestDir::new();
        dir.write(".hidden.mp3");
        dir.write("notes.txt");

        let error = match Playlist::from_input(dir.path()) {
            Ok(_) => panic!("empty playlist should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("no supported audio files"));
    }

    #[test]
    fn file_is_a_single_entry_playlist() {
        let dir = TestDir::new();
        dir.write("track.mp3");
        let path = dir.path().join("track.mp3");

        let playlist = Playlist::from_input(&path).expect("file should form a playlist");

        assert!(!playlist.is_directory());
        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.current(), path);
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let dir = TestDir::new();
        dir.write("one.mp3");
        dir.write("two.mp3");
        dir.write("three.mp3");

        let mut playlist =
            Playlist::from_input(dir.path()).expect("directory should form a playlist");

        assert_eq!(playlist.current_index(), 0);

        playlist.previous();
        assert_eq!(playlist.current_index(), 2);

        playlist.next();
        assert_eq!(playlist.current_index(), 0);

        playlist.next();
        playlist.next();
        assert_eq!(playlist.current_index(), 2);

        playlist.next();
        assert_eq!(playlist.current_index(), 0);
    }

    #[test]
    fn single_entry_playlist_next_and_previous_restart() {
        let dir = TestDir::new();
        dir.write("only.mp3");
        let mut playlist =
            Playlist::from_input(&dir.path().join("only.mp3")).expect("file should form a playlist");

        playlist.next();
        assert_eq!(playlist.current_index(), 0);

        playlist.previous();
        assert_eq!(playlist.current_index(), 0);
    }

    #[test]
    fn advance_stops_at_the_last_entry() {
        let dir = TestDir::new();
        dir.write("one.mp3");
        dir.write("two.mp3");
        let mut playlist =
            Playlist::from_input(dir.path()).expect("directory should form a playlist");

        assert!(playlist.advance());
        assert_eq!(playlist.current_index(), 1);
        assert!(!playlist.advance());
        assert_eq!(playlist.current_index(), 1);
    }
}
