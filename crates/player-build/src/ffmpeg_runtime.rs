use std::path::{Path, PathBuf};
use std::process::Command;

use rustc_hash::FxHashMap;

pub const PROFILE: &str = "runtime";
pub const PIN_FILE: &str = "ffmpeg-runtime.lock.json";
pub const PREFIX_ENV: &str = "FFMPEG_PREFIX";
pub const RPATH_ENV: &str = "PLAYER_FFMPEG_RPATH";
pub const LINUX_DEFAULT_RPATH: &str = "$ORIGIN/../lib";
pub const MACOS_DEFAULT_RPATH: &str = "@executable_path/../../../lib";
const CONTRACT: &str = "2";
const MANIFEST: &str = "share/ffmpeg-build/manifest.json";
const HEADER: &str = "include/libavformat/avformat.h";

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(FxHashMap<String, Json>),
}

impl Json {
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut parser = Parser { bytes: text.as_bytes(), at: 0 };
        let value = parser.value()?;
        parser.space();
        if parser.at != parser.bytes.len() {
            return Err(format!("trailing data at byte {}", parser.at));
        }
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Json::String(value) => Some(value),
            _ => None,
        }
    }

    fn path_str(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().try_fold(self, |value, key| value.get(key))?.str()
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.space();
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", byte as char, self.at))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(format!("invalid literal at byte {}", self.at))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.bytes.get(self.at) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(byte) if *byte == b'-' || byte.is_ascii_digit() => {
                let start = self.at;
                while self.at < self.bytes.len()
                    && matches!(self.bytes[self.at], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.at += 1;
                }
                Ok(Json::Number(String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned()))
            }
            _ => Err(format!("unexpected input at byte {}", self.at)),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut map = FxHashMap::default();
        self.space();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Object(map));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.expect(b':')?;
            map.insert(key, self.value()?);
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(map));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.at)),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.at)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.at) else {
                return Err("unterminated string".into());
            };
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).map_err(|error| error.to_string()),
                b'\\' => {
                    let Some(&escape) = self.bytes.get(self.at) else {
                        return Err("unterminated escape".into());
                    };
                    self.at += 1;
                    match escape {
                        b'"' | b'\\' | b'/' => out.push(escape),
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = self
                                .bytes
                                .get(self.at..self.at + 4)
                                .ok_or("short \\u escape")?;
                            let code = u32::from_str_radix(&String::from_utf8_lossy(hex), 16)
                                .map_err(|error| error.to_string())?;
                            self.at += 4;
                            let ch = char::from_u32(code).unwrap_or('\u{fffd}');
                            out.extend_from_slice(ch.to_string().as_bytes());
                        }
                        _ => return Err(format!("invalid escape at byte {}", self.at)),
                    }
                }
                _ => out.push(byte),
            }
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

impl Platform {
    pub fn of_target(target: &str) -> Result<Platform, String> {
        if target.ends_with("-linux-musl") {
            Ok(Platform::Linux)
        } else if target.ends_with("-apple-darwin") {
            Ok(Platform::Macos)
        } else {
            Err(format!("no FFmpeg runtime platform for target {target}"))
        }
    }

    fn identity_key(self) -> &'static str {
        match self {
            Platform::Linux => "soname",
            Platform::Macos => "install_name",
        }
    }

    fn identity_matches(self, library: &str, identity: &str) -> bool {
        let major = match self {
            Platform::Linux => identity.strip_prefix(library).and_then(|rest| rest.strip_prefix(".so.")),
            Platform::Macos => identity
                .strip_prefix("@rpath/")
                .and_then(|rest| rest.strip_prefix(library))
                .and_then(|rest| rest.strip_prefix('.'))
                .and_then(|rest| rest.strip_suffix(".dylib")),
        };
        major.is_some_and(|major| !major.is_empty() && major.bytes().all(|byte| byte.is_ascii_digit()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    pub repository: String,
    pub release: String,
    pub ffmpeg: String,
    pub libraries: Vec<String>,
    pub sha256: FxHashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLibrary {
    pub name: String,
    pub file: String,
    pub links: Vec<String>,
    pub identity: String,
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn strings(json: Option<&Json>) -> Option<Vec<String>> {
    match json {
        Some(Json::Array(items)) => items.iter().map(|item| item.str().map(str::to_owned)).collect(),
        _ => None,
    }
}

impl Pin {
    pub fn parse(text: &str) -> Result<Pin, String> {
        let json = Json::parse(text)?;
        let field = |key: &str| {
            json.get(key)
                .and_then(Json::str)
                .map(str::to_owned)
                .ok_or_else(|| format!("{PIN_FILE} lacks string field {key}"))
        };
        if json.get("contract") != Some(&Json::Number(CONTRACT.into())) {
            return Err(format!("{PIN_FILE} contract must be {CONTRACT}"));
        }
        let profile = field("profile")?;
        if profile != PROFILE {
            return Err(format!("{PIN_FILE} profile is {profile}, want {PROFILE}"));
        }
        let libraries = strings(json.get("libraries"))
            .filter(|libraries| !libraries.is_empty())
            .ok_or_else(|| format!("{PIN_FILE} lacks a libraries list"))?;
        if let Some(bad) = libraries.iter().find(|name| !name.starts_with("lib") || name.len() <= 3) {
            return Err(format!("{PIN_FILE} library {bad:?} is not a lib<name> library"));
        }
        let Some(Json::Object(entries)) = json.get("sha256") else {
            return Err(format!("{PIN_FILE} lacks a sha256 object"));
        };
        let mut sha256 = FxHashMap::default();
        for (target, digest) in entries {
            let digest = digest
                .str()
                .filter(|digest| is_sha256(digest))
                .ok_or_else(|| format!("{PIN_FILE} sha256 for {target} is not a lowercase SHA-256"))?;
            sha256.insert(target.clone(), digest.to_owned());
        }
        Ok(Pin {
            repository: field("repository")?,
            release: field("release")?,
            ffmpeg: field("ffmpeg")?,
            libraries,
            sha256,
        })
    }

    pub fn runtime_release(&self) -> String {
        format!("{}+{}", self.ffmpeg, self.release)
    }

    pub fn archive_root(&self, target: &str) -> String {
        format!("ffmpeg-{}-{target}-{PROFILE}", self.runtime_release())
    }

    pub fn archive(&self, target: &str) -> String {
        format!("{}.tar.gz", self.archive_root(target))
    }

    pub fn sha256(&self, target: &str) -> Result<&str, String> {
        self.sha256.get(target).map(String::as_str).ok_or_else(|| {
            format!("{PIN_FILE} pins no FFmpeg runtime archive for {target}; add its sha256")
        })
    }

    pub fn check_manifest(&self, manifest: &Json, target: &str) -> Result<Vec<RuntimeLibrary>, String> {
        let platform = Platform::of_target(target)?;
        let archive = self.archive(target);
        let checks = [
            (&["profile"][..], PROFILE),
            (&["target"][..], target),
            (&["sources", "ffmpeg", "version"][..], self.ffmpeg.as_str()),
            (&["release", "tag"][..], self.release.as_str()),
            (&["release", "archive"][..], archive.as_str()),
        ];
        for (keys, want) in checks {
            let found = manifest.path_str(keys);
            if found != Some(want) {
                return Err(format!(
                    "runtime manifest {} is {:?}, pin wants {want:?}",
                    keys.join("."),
                    found.unwrap_or("missing")
                ));
            }
        }
        let libraries = manifest.get("libraries").ok_or("runtime manifest lacks libraries")?;
        self.libraries
            .iter()
            .map(|name| {
                let entry = libraries
                    .get(name)
                    .ok_or_else(|| format!("runtime manifest has no library {name}"))?;
                let file = entry
                    .get("file")
                    .and_then(Json::str)
                    .ok_or_else(|| format!("runtime manifest library {name} lacks file"))?;
                let links = strings(entry.get("links"))
                    .ok_or_else(|| format!("runtime manifest library {name} lacks links"))?;
                let identity = entry
                    .get(platform.identity_key())
                    .and_then(Json::str)
                    .filter(|identity| platform.identity_matches(name, identity))
                    .ok_or_else(|| {
                        format!("runtime manifest library {name} lacks a {} for {target}", platform.identity_key())
                    })?;
                Ok(RuntimeLibrary {
                    name: name.clone(),
                    file: file.to_owned(),
                    links,
                    identity: identity.to_owned(),
                })
            })
            .collect()
    }
}

pub fn prefix(workspace: &Path, target: &str) -> PathBuf {
    match std::env::var_os(PREFIX_ENV) {
        Some(prefix) => PathBuf::from(prefix),
        None => workspace.join(".work/deps/ffmpeg-runtime").join(target),
    }
}

pub fn check_prefix(prefix: &Path, pin: &Pin, target: &str) -> Result<Vec<RuntimeLibrary>, String> {
    let manifest_path = prefix.join(MANIFEST);
    let text = std::fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "no FFmpeg runtime manifest at {} ({error}); run scripts/fetch-ffmpeg-runtime.sh {target}",
            manifest_path.display()
        )
    })?;
    let manifest = Json::parse(&text).map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let libraries = pin
        .check_manifest(&manifest, target)
        .map_err(|error| format!("{}: {error}", prefix.display()))?;
    let mut required = vec![HEADER.to_owned()];
    for library in &libraries {
        required.push(format!("lib/pkgconfig/{}.pc", library.name));
        required.push(library.file.clone());
        required.extend(library.links.iter().cloned());
    }
    for relative in required {
        if !prefix.join(&relative).is_file() {
            return Err(format!("FFmpeg runtime {} lacks {relative}", prefix.display()));
        }
    }
    let lib = prefix.join("lib");
    let entries = std::fs::read_dir(&lib).map_err(|error| format!("cannot list {}: {error}", lib.display()))?;
    for entry in entries {
        let name = entry.map_err(|error| error.to_string())?.file_name();
        if Path::new(&name).extension().is_some_and(|extension| extension == "a") {
            return Err(format!(
                "FFmpeg runtime {} carries static archive {}; Player links only the shared runtime",
                prefix.display(),
                name.to_string_lossy()
            ));
        }
    }
    Ok(libraries)
}

pub fn split_pkg_config(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    in_token = true;
                }
            }
            ch if ch.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            ch => {
                current.push(ch);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

pub fn shared_libraries(tokens: &[String], prefix: &Path, pin: &Pin) -> Result<Vec<String>, String> {
    let lib = prefix.join("lib");
    let mut names: Vec<String> = Vec::new();
    for token in tokens {
        if let Some(dir) = token.strip_prefix("-L") {
            let resolved = Path::new(dir)
                .canonicalize()
                .map_err(|error| format!("pkg-config search path {dir}: {error}"))?;
            if resolved != lib {
                return Err(format!("pkg-config search path {dir} is not the runtime {}", lib.display()));
            }
        } else if let Some(name) = token.strip_prefix("-l") {
            if !pin.libraries.iter().any(|library| library.strip_prefix("lib") == Some(name)) {
                return Err(format!("pkg-config names -l{name}, which {PIN_FILE} does not list in libraries"));
            }
            if !names.iter().any(|known| known == name) {
                names.push(name.to_owned());
            }
        } else {
            return Err(format!("unexpected pkg-config link token {token}"));
        }
    }
    Ok(names)
}

pub fn pkg_config_libs(prefix: &Path, modules: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("pkg-config")
        .arg("--libs")
        .args(modules)
        .env("PKG_CONFIG_LIBDIR", prefix.join("lib/pkgconfig"))
        .env_remove("PKG_CONFIG_PATH")
        .env_remove("PKG_CONFIG_SYSROOT_DIR")
        .output()
        .map_err(|error| format!("cannot run pkg-config: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "pkg-config --libs {} failed against {}: {}",
            modules.join(" "),
            prefix.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(split_pkg_config(&String::from_utf8_lossy(&output.stdout)))
}

pub fn installed_rpath(platform: Platform, value: Option<&str>) -> Result<Option<String>, String> {
    let (default, roots): (&str, &[&str]) = match platform {
        Platform::Linux => (LINUX_DEFAULT_RPATH, &["$ORIGIN"]),
        Platform::Macos => (MACOS_DEFAULT_RPATH, &["@executable_path", "@loader_path"]),
    };
    let Some(value) = value else {
        return Ok(Some(default.to_owned()));
    };
    if value == "none" && platform == Platform::Linux {
        return Ok(None);
    }
    let relative = roots
        .iter()
        .any(|root| value == *root || value.strip_prefix(root).is_some_and(|rest| rest.starts_with('/')));
    if !relative || value.contains([',', ':']) {
        return Err(format!(
            "{RPATH_ENV}={value:?} must be one path under {}{}",
            roots.join(" or "),
            if platform == Platform::Linux { ", or none" } else { "" }
        ));
    }
    Ok(Some(value.to_owned()))
}

pub fn emit_installed_rpath(owner: &str, target: &str) {
    println!("cargo:rerun-if-env-changed={RPATH_ENV}");
    let platform = Platform::of_target(target).unwrap_or_else(|error| panic!("{owner}: {error}"));
    let value = std::env::var(RPATH_ENV).ok();
    let rpath = installed_rpath(platform, value.as_deref()).unwrap_or_else(|error| panic!("{owner}: {error}"));
    match (platform, rpath) {
        (Platform::Linux, Some(path)) => println!("cargo:rustc-link-arg-bins=-Wl,--enable-new-dtags,-rpath,{path}"),
        (Platform::Macos, Some(path)) => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{path}"),
        (_, None) => {}
    }
}

pub fn emit_macos_dev_rpath(owner: &str, manifest_dir: &Path) {
    println!("cargo:rerun-if-env-changed={PREFIX_ENV}");
    let target = std::env::var("TARGET").unwrap_or_else(|_| panic!("{owner}: cargo did not set TARGET"));
    let prefix = prefix(&manifest_dir.join("../.."), &target);
    let lib = prefix.join("lib").canonicalize().unwrap_or_else(|error| {
        panic!(
            "{owner}: no FFmpeg runtime at {} ({error}); run scripts/fetch-ffmpeg-runtime.sh {target}",
            prefix.display()
        )
    });
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX: &str = "x86_64-unknown-linux-musl";
    const MACOS: &str = "aarch64-apple-darwin";
    const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn pin_text() -> String {
        format!(
            r#"{{
              "contract": 2,
              "repository": "laputa-systems/ffmpeg-build",
              "profile": "runtime",
              "ffmpeg": "9.0.2",
              "release": "20261107",
              "libraries": ["libavformat", "libavutil"],
              "sha256": {{"{LINUX}": "{SHA}"}}
            }}"#
        )
    }

    fn manifest(target: &str, tag: &str, identity: &str) -> Json {
        let soname = |library: &str| match Platform::of_target(target).unwrap() {
            Platform::Linux => format!(r#""soname": "{library}{identity}""#),
            Platform::Macos => format!(r#""install_name": "@rpath/{library}{identity}""#),
        };
        Json::parse(&format!(
            r#"{{"profile": "runtime", "target": "{target}",
               "sources": {{"ffmpeg": {{"version": "9.0.2", "sha256": "x"}}}},
               "release": {{"tag": "{tag}", "archive": "ffmpeg-9.0.2+{tag}-{target}-runtime.tar.gz"}},
               "libraries": {{
                 "libavformat": {{"file": "lib/libavformat.so.63.1.102", "links": ["lib/libavformat.so"], {}, "sha256": "x"}},
                 "libavutil": {{"file": "lib/libavutil.so.61.1.102", "links": ["lib/libavutil.so"], {}, "sha256": "x"}}
               }},
               "flag": true, "none": null, "n": -1.5e3}}"#,
            soname("libavformat"),
            soname("libavutil"),
        ))
        .unwrap()
    }

    #[test]
    fn pin_names_the_runtime_release_and_its_archive() {
        let pin = Pin::parse(&pin_text()).unwrap();
        assert_eq!(pin.runtime_release(), "9.0.2+20261107");
        assert_eq!(pin.archive(LINUX), "ffmpeg-9.0.2+20261107-x86_64-unknown-linux-musl-runtime.tar.gz");
        assert_eq!(pin.sha256(LINUX), Ok(SHA));
        assert!(pin.sha256(MACOS).is_err());
    }

    #[test]
    fn pin_accepts_a_matching_manifest_and_returns_the_pinned_libraries() {
        let pin = Pin::parse(&pin_text()).unwrap();
        let libraries = pin.check_manifest(&manifest(LINUX, "20261107", ".so.63"), LINUX).unwrap();
        let identities: Vec<_> = libraries.iter().map(|library| library.identity.as_str()).collect();
        assert_eq!(identities, ["libavformat.so.63", "libavutil.so.63"]);
        let macos = pin.check_manifest(&manifest(MACOS, "20261107", ".63.dylib"), MACOS).unwrap();
        assert_eq!(macos[0].identity, "@rpath/libavformat.63.dylib");
    }

    #[test]
    fn pin_rejects_another_release_target_or_library_identity() {
        let pin = Pin::parse(&pin_text()).unwrap();
        assert!(pin.check_manifest(&manifest(LINUX, "local", ".so.63"), LINUX).is_err());
        assert!(pin.check_manifest(&manifest(MACOS, "20261107", ".63.dylib"), LINUX).is_err());
        assert!(pin.check_manifest(&manifest(LINUX, "20261107", ".so"), LINUX).is_err());
        assert!(pin.check_manifest(&manifest(LINUX, "20261107", ".a"), LINUX).is_err());
    }

    #[test]
    fn pin_rejects_another_profile_contract_digest_or_empty_libraries() {
        let text = pin_text();
        for bad in [
            text.replace("\"runtime\"", "\"play-sdk\""),
            text.replace("\"contract\": 2", "\"contract\": 1"),
            text.replace(SHA, "abc"),
            text.replace("[\"libavformat\", \"libavutil\"]", "[]"),
        ] {
            assert!(Pin::parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn json_handles_escapes() {
        let json = Json::parse(r#"{"a": "x\"y\\zé", "b": [1, 2]}"#).unwrap();
        assert_eq!(json.get("a").and_then(Json::str), Some("x\"y\\zé"));
        assert!(Json::parse(r#"{"a": 1} trailing"#).is_err());
    }

    #[test]
    fn split_honors_escaped_spaces() {
        let tokens = split_pkg_config("-L/sdk/path\\ with\\ spaces/lib -lavformat  -lavutil\n");
        assert_eq!(tokens, ["-L/sdk/path with spaces/lib", "-lavformat", "-lavutil"]);
    }

    #[test]
    fn shared_libraries_accepts_only_the_runtime_lib_dir_and_pinned_libraries() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.work/tmp/player-build-ffmpeg-runtime");
        let _ = std::fs::remove_dir_all(&root);
        let prefix = root.join("runtime with space");
        std::fs::create_dir_all(prefix.join("lib/pkgconfig")).unwrap();
        let prefix = prefix.canonicalize().unwrap();
        let pin = Pin::parse(&pin_text()).unwrap();
        let via_pcfiledir = format!("-L{}", prefix.join("lib/pkgconfig/../../lib").display());
        let tokens: Vec<String> = [via_pcfiledir.as_str(), "-lavformat", "-lavutil", "-lavformat"]
            .iter()
            .map(|token| token.to_string())
            .collect();
        assert_eq!(shared_libraries(&tokens, &prefix, &pin).unwrap(), ["avformat", "avutil"]);
        for bad in [
            vec![format!("-L{}", root.display())],
            vec!["-lswresample".to_owned()],
            vec!["-lm".to_owned()],
            vec!["-framework".to_owned(), "CoreMedia".to_owned()],
        ] {
            assert!(shared_libraries(&bad, &prefix, &pin).is_err(), "{bad:?}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_rpath_is_relative_to_the_executable() {
        assert_eq!(installed_rpath(Platform::Linux, None), Ok(Some("$ORIGIN/../lib".into())));
        assert_eq!(installed_rpath(Platform::Linux, Some("none")), Ok(None));
        assert_eq!(installed_rpath(Platform::Linux, Some("$ORIGIN/../lib/ffmpeg")), Ok(Some("$ORIGIN/../lib/ffmpeg".into())));
        assert_eq!(installed_rpath(Platform::Macos, None), Ok(Some("@executable_path/../../../lib".into())));
        for (platform, bad) in [
            (Platform::Linux, "/work/.work/deps/ffmpeg-runtime/lib"),
            (Platform::Linux, "$ORIGINAL/lib"),
            (Platform::Linux, "$ORIGIN/../lib:/usr/lib"),
            (Platform::Linux, ""),
            (Platform::Macos, "none"),
            (Platform::Macos, "/Users/me/.local/lib"),
        ] {
            assert!(installed_rpath(platform, Some(bad)).is_err(), "{bad}");
        }
    }
}
