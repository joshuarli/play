use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PROFILE: &str = "play-sdk";
pub const PIN_FILE: &str = "ffmpeg-sdk.lock.json";
const MANIFEST: &str = "share/ffmpeg-build/manifest.json";
const SDK_FILES: [&str; 8] = [
    "include/libavformat/avformat.h",
    "lib/pkgconfig/libavformat.pc",
    "lib/libavformat.a",
    "lib/libavcodec.a",
    "lib/libswscale.a",
    "lib/libavutil.a",
    "lib/libdav1d.a",
    "lib/libz.a",
];
const SYSTEM_LIBRARIES: [&str; 6] = ["m", "pthread", "dl", "atomic", "va", "va-drm"];
pub const LINUX_VA: [(&str, &str); 2] = [("va", "libva"), ("va-drm", "libva-drm")];

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
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
        let mut map = BTreeMap::new();
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

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub asset: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    pub repository: String,
    pub release: String,
    pub ffmpeg: String,
    pub targets: BTreeMap<String, Asset>,
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
        if json.get("contract") != Some(&Json::Number("1".into())) {
            return Err(format!("{PIN_FILE} contract must be 1"));
        }
        let profile = field("profile")?;
        if profile != PROFILE {
            return Err(format!("{PIN_FILE} profile is {profile}, want {PROFILE}"));
        }
        let Some(Json::Object(entries)) = json.get("targets") else {
            return Err(format!("{PIN_FILE} lacks a targets object"));
        };
        let mut targets = BTreeMap::new();
        for (target, entry) in entries {
            let asset = entry
                .get("asset")
                .and_then(Json::str)
                .ok_or_else(|| format!("{PIN_FILE} target {target} lacks asset"))?;
            let sha256 = entry
                .get("sha256")
                .and_then(Json::str)
                .filter(|sha| sha.len() == 64 && sha.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
                .ok_or_else(|| format!("{PIN_FILE} target {target} lacks a sha256"))?;
            targets.insert(
                target.clone(),
                Asset { asset: asset.to_owned(), sha256: sha256.to_owned() },
            );
        }
        Ok(Pin {
            repository: field("repository")?,
            release: field("release")?,
            ffmpeg: field("ffmpeg")?,
            targets,
        })
    }

    pub fn asset(&self, target: &str) -> Result<&Asset, String> {
        self.targets
            .get(target)
            .ok_or_else(|| format!("{PIN_FILE} has no FFmpeg SDK for target {target}"))
    }

    pub fn check_manifest(&self, manifest: &Json, target: &str) -> Result<(), String> {
        let asset = self.asset(target)?;
        let checks = [
            (&["profile"][..], PROFILE),
            (&["target"][..], target),
            (&["sources", "ffmpeg", "version"][..], self.ffmpeg.as_str()),
            (&["release", "tag"][..], self.release.as_str()),
            (&["release", "archive"][..], asset.asset.as_str()),
        ];
        for (keys, want) in checks {
            let found = manifest.path_str(keys);
            if found != Some(want) {
                return Err(format!(
                    "SDK manifest {} is {:?}, pin wants {want:?}",
                    keys.join("."),
                    found.unwrap_or("missing")
                ));
            }
        }
        Ok(())
    }
}

pub fn default_prefix(workspace: &Path, target: &str) -> PathBuf {
    workspace.join(".work/deps/ffmpeg-sdk").join(target)
}

pub fn check_prefix(prefix: &Path, pin: &Pin, target: &str) -> Result<(), String> {
    let manifest_path = prefix.join(MANIFEST);
    let text = std::fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "no FFmpeg SDK manifest at {} ({error}); run scripts/fetch-ffmpeg-sdk.sh {target}",
            manifest_path.display()
        )
    })?;
    let manifest = Json::parse(&text).map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    pin.check_manifest(&manifest, target)
        .map_err(|error| format!("{}: {error}", prefix.display()))?;
    for relative in SDK_FILES {
        if !prefix.join(relative).is_file() {
            return Err(format!("FFmpeg SDK {} lacks {relative}", prefix.display()));
        }
    }
    Ok(())
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    SearchNative(PathBuf),
    Static(String),
    System(String),
    Framework(String),
    Arg(String),
}

pub fn plan_links(tokens: &[String], prefix: &Path, system_library: impl Fn(&str) -> Link) -> Result<Vec<Link>, String> {
    let lib = prefix.join("lib");
    let mut links: Vec<Link> = Vec::new();
    let mut push = |link: Link, links: &mut Vec<Link>| {
        if !links.contains(&link) {
            links.push(link);
        }
    };
    let mut tokens = tokens.iter();
    while let Some(token) = tokens.next() {
        if let Some(dir) = token.strip_prefix("-L") {
            let dir = PathBuf::from(dir);
            if !dir.starts_with(prefix) {
                return Err(format!("pkg-config search path {} is outside the SDK {}", dir.display(), prefix.display()));
            }
            push(Link::SearchNative(dir), &mut links);
        } else if let Some(name) = token.strip_prefix("-l") {
            if lib.join(format!("lib{name}.a")).is_file() {
                push(Link::Static(name.to_owned()), &mut links);
            } else if SYSTEM_LIBRARIES.contains(&name) {
                push(system_library(name), &mut links);
            } else {
                return Err(format!("pkg-config names -l{name}, but the SDK {} has no lib{name}.a", lib.display()));
            }
        } else if token == "-framework" {
            let name = tokens.next().ok_or("-framework without a name")?;
            push(Link::Framework(name.clone()), &mut links);
        } else if token == "-pthread" {
            push(Link::Arg(token.clone()), &mut links);
        } else {
            return Err(format!("unexpected pkg-config link token {token}"));
        }
    }
    Ok(links)
}

pub fn emit(links: &[Link]) {
    for link in links {
        match link {
            Link::SearchNative(dir) => println!("cargo:rustc-link-search=native={}", dir.display()),
            Link::Static(name) => println!("cargo:rustc-link-lib=static={name}"),
            Link::System(name) => println!("cargo:rustc-link-lib={name}"),
            Link::Framework(name) => println!("cargo:rustc-link-lib=framework={name}"),
            Link::Arg(arg) => println!("cargo:rustc-link-arg={arg}"),
        }
    }
}

pub fn pkg_config_static_libs(prefix: &Path, modules: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("pkg-config")
        .arg("--static")
        .arg("--libs")
        .args(modules)
        .env("PKG_CONFIG_LIBDIR", prefix.join("lib/pkgconfig"))
        .env_remove("PKG_CONFIG_PATH")
        .env_remove("PKG_CONFIG_SYSROOT_DIR")
        .output()
        .map_err(|error| format!("cannot run pkg-config: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "pkg-config --static --libs {} failed against {}: {}",
            modules.join(" "),
            prefix.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(split_pkg_config(&String::from_utf8_lossy(&output.stdout)))
}

pub fn linux_va_link_args() -> Result<Vec<String>, String> {
    LINUX_VA
        .iter()
        .map(|(library, module)| Ok(system_libdir(module)?.join(format!("lib{library}.so")).display().to_string()))
        .collect()
}

pub fn emit_linux_va(owner: &str) {
    let args = linux_va_link_args().unwrap_or_else(|error| panic!("{owner}: {error}"));
    for arg in args {
        println!("cargo:rustc-link-arg={arg}");
    }
}

pub fn system_libdir(module: &str) -> Result<PathBuf, String> {
    let output = Command::new("pkg-config")
        .args(["--variable=libdir", module])
        .output()
        .map_err(|error| format!("cannot run pkg-config: {error}"))?;
    let dir = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || dir.is_empty() {
        return Err(format!("pkg-config cannot find the system {module} libdir"));
    }
    Ok(PathBuf::from(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIN: &str = r#"{
      "contract": 1,
      "repository": "laputa-systems/ffmpeg-build",
      "release": "20261104",
      "profile": "play-sdk",
      "ffmpeg": "9.0.2",
      "targets": {
        "aarch64-apple-darwin": {
          "asset": "ffmpeg-9.0.2+20261104-aarch64-apple-darwin-play-sdk.tar.gz",
          "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
        }
      }
    }"#;

    fn manifest(profile: &str, target: &str, version: &str, tag: &str) -> Json {
        Json::parse(&format!(
            r#"{{"profile": "{profile}", "target": "{target}",
               "sources": {{"ffmpeg": {{"version": "{version}", "sha256": "x"}}}},
               "release": {{"tag": "{tag}", "archive": "ffmpeg-{version}+{tag}-{target}-{profile}.tar.gz"}},
               "components": {{"protocols": ["file"]}}, "flag": true, "none": null, "n": -1.5e3}}"#
        ))
        .unwrap()
    }

    #[test]
    fn pin_accepts_matching_manifest() {
        let pin = Pin::parse(PIN).unwrap();
        assert_eq!(pin.release, "20261104");
        let target = "aarch64-apple-darwin";
        assert_eq!(pin.check_manifest(&manifest("play-sdk", target, "9.0.2", "20261104"), target), Ok(()));
    }

    #[test]
    fn pin_rejects_wrong_profile_target_version_or_release() {
        let pin = Pin::parse(PIN).unwrap();
        let target = "aarch64-apple-darwin";
        for bad in [
            manifest("tools", target, "9.0.2", "20261104"),
            manifest("play-sdk", target, "9.0.1", "20261104"),
            manifest("play-sdk", target, "9.0.2", "local"),
        ] {
            assert!(pin.check_manifest(&bad, target).is_err());
        }
        assert!(pin.check_manifest(&manifest("play-sdk", "x86_64-unknown-linux-musl", "9.0.2", "20261104"), "x86_64-unknown-linux-musl").is_err());
    }

    #[test]
    fn pin_rejects_missing_sha_or_wrong_profile() {
        assert!(Pin::parse(&PIN.replace("0000000000000000000000000000000000000000000000000000000000000000", "abc")).is_err());
        assert!(Pin::parse(&PIN.replace("\"play-sdk\"", "\"tools\"")).is_err());
    }

    #[test]
    fn json_handles_escapes() {
        let json = Json::parse(r#"{"a": "x\"y\\zé", "b": [1, 2]}"#).unwrap();
        assert_eq!(json.get("a").and_then(Json::str), Some("x\"y\\zé"));
        assert!(Json::parse(r#"{"a": 1} trailing"#).is_err());
    }

    #[test]
    fn split_honors_escaped_spaces() {
        let tokens = split_pkg_config("-L/sdk/path\\ with\\ spaces/lib -lavformat  -framework VideoToolbox\n");
        assert_eq!(tokens, ["-L/sdk/path with spaces/lib", "-lavformat", "-framework", "VideoToolbox"]);
    }

    #[test]
    fn plan_links_static_from_sdk_and_rejects_foreign_paths() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.work/tmp/player-build-ffmpeg-sdk");
        let _ = std::fs::remove_dir_all(&root);
        let prefix = root.join("sdk with space");
        std::fs::create_dir_all(prefix.join("lib")).unwrap();
        std::fs::write(prefix.join("lib/libavformat.a"), b"").unwrap();
        std::fs::write(prefix.join("lib/libz.a"), b"").unwrap();
        let lib = format!("-L{}", prefix.join("lib").display());
        let tokens: Vec<String> = [lib.as_str(), "-lavformat", "-lz", "-lm", "-lz", "-framework", "CoreMedia", "-pthread"]
            .iter()
            .map(|token| token.to_string())
            .collect();
        let links = plan_links(&tokens, &prefix, |name| Link::System(name.to_owned())).unwrap();
        assert_eq!(
            links,
            [
                Link::SearchNative(prefix.join("lib")),
                Link::Static("avformat".into()),
                Link::Static("z".into()),
                Link::System("m".into()),
                Link::Framework("CoreMedia".into()),
                Link::Arg("-pthread".into()),
            ]
        );
        let foreign = vec!["-L/opt/homebrew/lib".to_owned()];
        assert!(plan_links(&foreign, &prefix, |name| Link::System(name.to_owned())).is_err());
        let missing_static = vec!["-ldav1d".to_owned()];
        assert!(plan_links(&missing_static, &prefix, |name| Link::System(name.to_owned())).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
