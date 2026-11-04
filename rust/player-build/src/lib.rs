pub mod ffmpeg_sdk;

use std::path::Path;

pub fn emit_linux_closure(owner: &str, args_file: &Path, prefix: &Path) {
    let text = std::fs::read_to_string(args_file)
        .unwrap_or_else(|_| panic!("{owner}: cannot read {}", args_file.display()));
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let body = line.replace("{PREFIX}", &prefix.display().to_string());
        if let Some(arg) = body.strip_prefix("arg:") {
            println!("cargo:rustc-link-arg={arg}");
        } else if let Some(lib) = body.strip_prefix("lib:") {
            println!("cargo:rustc-link-lib={lib}");
        } else {
            panic!("{owner}: bad directive in {}: {line}", args_file.display());
        }
    }
}
