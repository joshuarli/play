use std::path::{Path, PathBuf};

use player_core::input::{classify_input, InputPath};

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub title: Option<String>,
}

#[derive(Debug, Default)]
pub struct Playlist {
    entries: Vec<Entry>,
    current: Option<usize>,
}

impl Playlist {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn set_current(&mut self, index: Option<usize>) {
        self.current = index.filter(|index| *index < self.entries.len());
    }

    pub fn current_entry(&self) -> Option<&Entry> {
        self.current.and_then(|index| self.entries.get(index))
    }

    pub fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    pub fn insert(&mut self, index: usize, entries: Vec<Entry>) {
        let index = index.min(self.entries.len());
        if let Some(current) = self.current.as_mut()
            && index <= *current
        {
            *current += entries.len();
        }
        self.entries.splice(index..index, entries);
    }

    pub fn append(&mut self, entries: Vec<Entry>) {
        self.entries.extend(entries);
    }

    pub fn remove(&mut self, index: usize) -> Option<Entry> {
        if index >= self.entries.len() {
            return None;
        }
        let entry = self.entries.remove(index);
        if let Some(current) = self.current {
            if index < current {
                self.current = Some(current - 1);
            } else if index == current {
                self.current = None;
            }
        }
        Some(entry)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
    }

    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        if from >= self.entries.len() || to >= self.entries.len() {
            return false;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        if let Some(current) = self.current {
            self.current = Some(match current {
                _ if current == from => to,
                _ if from < current && to >= current => current - 1,
                _ if from > current && to <= current => current + 1,
                _ => current,
            });
        }
        true
    }

    pub fn next_index(&self) -> Option<usize> {
        self.current.map(|index| index + 1).filter(|index| *index < self.entries.len())
    }

    pub fn previous_index(&self) -> Option<usize> {
        self.current.and_then(|index| index.checked_sub(1))
    }
}

pub fn local_entry(path: &str) -> Result<Entry, String> {
    match classify_input(path, false) {
        Ok(InputPath::Local(path)) => Ok(Entry {
            path: path.to_string_lossy().into_owned(),
            title: None,
        }),
        Err(error) => Err(error.to_string()),
    }
}

const DIRECTORY_MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "m4a", "m4b", "mov", "mkv", "mka", "webm", "avi", "mp3", "flac", "wav", "ogg",
    "oga", "opus",
];

pub fn entries_for_path(path: &str) -> Result<Vec<Entry>, String> {
    if let InputPath::Local(local) = classify_input(path, false).map_err(|error| error.to_string())?
        && local.is_dir()
    {
        return directory_entries(&local);
    }
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "m3u" | "m3u8" | "pls") {
        read_playlist(path)
    } else {
        local_entry(path).map(|entry| vec![entry])
    }
}

fn directory_entries(directory: &Path) -> Result<Vec<Entry>, String> {
    let listing = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read directory {}: {error}", directory.display()))?;
    let mut files = Vec::new();
    for item in listing {
        let item = item
            .map_err(|error| format!("cannot read directory {}: {error}", directory.display()))?;
        let file = item.path();
        let media = file
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                DIRECTORY_MEDIA_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
            });
        if media && file.is_file() {
            files.push(file);
        }
    }
    if files.is_empty() {
        return Err(format!("directory {} has no playable files", directory.display()));
    }
    files.sort();
    Ok(files
        .into_iter()
        .map(|file| Entry {
            path: file.to_string_lossy().into_owned(),
            title: None,
        })
        .collect())
}

pub fn read_playlist(path: &str) -> Result<Vec<Entry>, String> {
    let source = match classify_input(path, false) {
        Ok(InputPath::Local(path)) => path,
        _ => return Err("Refusing non-local playlist".to_owned()),
    };
    let bytes = std::fs::read(&source)
        .map_err(|error| format!("cannot read playlist {}: {error}", source.display()))?;
    let text = decode_playlist(&bytes);
    let base = source.parent().unwrap_or_else(|| Path::new("."));
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let entries = match extension.as_str() {
        "pls" => parse_pls(&text, base),
        _ => parse_m3u(&text, base),
    };
    if entries.is_empty() {
        return Err(format!("playlist {} has no entries", source.display()));
    }
    Ok(entries)
}

/// Classic m3u and pls files are Latin-1 or a system code page and often carry
/// a UTF-8 byte order mark. Valid UTF-8 is kept as written; each byte that is
/// not part of a valid sequence is read as the Latin-1 character with that value.
fn decode_playlist(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        text.extend(chunk.invalid().iter().map(|byte| char::from(*byte)));
    }
    text
}

fn parse_m3u(text: &str, base: &Path) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut title = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            title = info
                .split_once(',')
                .map(|(_, title)| title.trim().to_owned())
                .filter(|title| !title.is_empty());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(entry) = resolve_entry(line, base, title.take()) {
            entries.push(entry);
        }
    }
    entries
}

fn parse_pls(text: &str, base: &Path) -> Vec<Entry> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.to_ascii_lowercase().starts_with("file") {
            continue;
        }
        if let Some(entry) = resolve_entry(value.trim(), base, None) {
            entries.push(entry);
        }
    }
    entries
}

fn resolve_entry(value: &str, base: &Path, title: Option<String>) -> Option<Entry> {
    if let Err(error) = classify_input(value, false) {
        eprintln!("Refusing non-local playlist entry: {error}");
        return None;
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    let path = path.to_string_lossy().into_owned();
    local_entry(&path).ok().map(|entry| Entry { title, ..entry })
}

#[cfg(test)]
mod insertion_tests {
    use super::*;
    fn entry(path: &str) -> Entry { Entry { path: path.to_owned(), title: None } }

    #[test]
    fn insertion_preserves_active_entry_and_navigation() {
        for (index, inserted, expected) in [(0, 2, 3), (1, 2, 3), (2, 2, 1), (99, 2, 1), (0, 0, 1)] {
            let mut list = Playlist::new();
            list.append(vec![entry("A"), entry("B"), entry("C")]);
            list.set_current(Some(1));
            list.insert(index, (0..inserted).map(|_| entry("X")).collect());
            assert_eq!(list.current(), Some(expected));
            assert_eq!(list.current_entry().unwrap().path, "B");
            assert_eq!(list.entries()[list.next_index().unwrap()].path, if index == 2 { "X" } else { "C" });
            assert_eq!(list.entries()[list.previous_index().unwrap()].path, if index == 1 { "X" } else { "A" });
        }
        let mut list = Playlist::new();
        list.insert(99, vec![entry("X")]);
        assert_eq!(list.current(), None);
    }
}

#[cfg(test)]
mod read_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.work/tmp")
                .join(format!("playlist-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn read(&self, name: &str, contents: &[u8]) -> Result<Vec<Entry>, String> {
            let playlist = self.0.join(name);
            std::fs::write(&playlist, contents).unwrap();
            read_playlist(playlist.to_str().unwrap())
        }
        fn paths(&self, names: &[&str]) -> Vec<String> {
            names.iter().map(|name| self.0.join(name).to_string_lossy().into_owned()).collect()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
    }

    fn paths(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.path.as_str()).collect()
    }

    #[test]
    fn utf8_bom_does_not_become_an_entry() {
        let scratch = Scratch::new();
        let entries = scratch.read("bom.m3u", b"\xef\xbb\xbf#EXTM3U\n#EXTINF:3,Tone\na.wav\n").unwrap();
        assert_eq!(paths(&entries), scratch.paths(&["a.wav"]));
        assert_eq!(entries[0].title.as_deref(), Some("Tone"));
        let entries = scratch.read("bom.pls", b"\xef\xbb\xbf[playlist]\nFile1=a.wav\n").unwrap();
        assert_eq!(paths(&entries), scratch.paths(&["a.wav"]));
    }

    #[test]
    fn latin1_entries_are_read_without_losing_valid_ones() {
        let scratch = Scratch::new();
        let entries = scratch.read("latin1.m3u", b"#EXTM3U\ncaf\xe9.wav\na.wav\n").unwrap();
        assert_eq!(paths(&entries), scratch.paths(&["caf\u{e9}.wav", "a.wav"]));
    }

    #[test]
    fn valid_utf8_is_unchanged() {
        let scratch = Scratch::new();
        let entries = scratch.read("utf8.m3u8", "#EXTINF:1,Caf\u{e9}\ncaf\u{e9} \u{65e5}\u{672c}.wav\n".as_bytes()).unwrap();
        assert_eq!(paths(&entries), scratch.paths(&["caf\u{e9} \u{65e5}\u{672c}.wav"]));
        assert_eq!(entries[0].title.as_deref(), Some("Caf\u{e9}"));
    }

    #[test]
    fn empty_and_bom_only_playlists_are_rejected() {
        let scratch = Scratch::new();
        assert!(scratch.read("empty.m3u", b"").unwrap_err().ends_with("has no entries"));
        assert!(scratch.read("bom-only.m3u", b"\xef\xbb\xbf").unwrap_err().ends_with("has no entries"));
    }
}
