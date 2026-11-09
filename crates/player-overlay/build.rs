use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

const PREFIX_ENV: &str = "RUST_FONTCONFIG_PREFIX";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={PREFIX_ENV}");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = env::var("TARGET").expect("cargo sets TARGET");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let prefix = env::var_os(PREFIX_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("../../.work/deps/rust-fontconfig").join(&target));
    let prefix = prefix.canonicalize().ok().filter(|prefix| prefix.join("lib/pkgconfig/rust-fontconfig.pc").is_file());

    let mut command = Command::new("pkg-config");
    command.args(["--libs", "rust-fontconfig", "harfbuzz", "freetype2"]);
    if let Some(prefix) = &prefix {
        println!("cargo:rerun-if-changed={}", prefix.join("lib").display());
        let mut search = vec![prefix.join("lib/pkgconfig")];
        search.extend(env::var_os("PKG_CONFIG_PATH").iter().flat_map(env::split_paths));
        command.env("PKG_CONFIG_PATH", env::join_paths(search).expect("pkg-config search path"));
    }
    let output = command.output().unwrap_or_else(|error| panic!("pkg-config could not run: {error}"));
    assert!(
        output.status.success(),
        "pkg-config --libs rust-fontconfig harfbuzz freetype2 failed ({PREFIX_ENV} or the system must provide rust-fontconfig; run scripts/build-rust-fontconfig-linux.sh): {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for flag in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        if let Some(path) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={}", search_path(Path::new(path), prefix.as_deref(), &out_dir));
        } else if let Some(name) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib=dylib={name}");
        }
    }
}

fn search_path(path: &Path, prefix: Option<&Path>, out_dir: &Path) -> String {
    if !prefix.is_some_and(|prefix| path.starts_with(prefix)) {
        return path.display().to_string();
    }
    // Cargo puts build-script search paths on the library path of `cargo test`
    // only when they lie inside the target directory.
    let link = out_dir.join("rust-fontconfig-lib");
    if fs::symlink_metadata(&link).is_ok() {
        fs::remove_file(&link).unwrap_or_else(|error| panic!("cannot replace {}: {error}", link.display()));
    }
    symlink(path, &link).unwrap_or_else(|error| panic!("cannot link {}: {error}", link.display()));
    link.display().to_string()
}
