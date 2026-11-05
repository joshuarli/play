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
        for (offset, entry) in entries.into_iter().enumerate() {
            self.entries.insert(index + offset, entry);
        }
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
    let text = std::fs::read_to_string(&source)
        .map_err(|error| format!("cannot read playlist {}: {error}", source.display()))?;
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
