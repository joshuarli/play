use std::env;
use std::fs;
use std::path::PathBuf;

use player_build::ffmpeg_runtime::{self, Pin};

fn fail(message: String) -> ! {
    panic!("player-media: {message}")
}

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.join("../..");
    let target = env::var("TARGET").unwrap_or_else(|_| fail("cargo did not set TARGET".into()));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_else(|| fail("cargo did not set OUT_DIR".into())));
    let pin_path = workspace.join(ffmpeg_runtime::PIN_FILE);
    let pin_text = fs::read_to_string(&pin_path)
        .unwrap_or_else(|error| fail(format!("cannot read {}: {error}", pin_path.display())));
    let pin = Pin::parse(&pin_text).unwrap_or_else(|error| fail(error));

    let prefix = ffmpeg_runtime::prefix(&workspace, &target);
    let prefix = prefix.canonicalize().unwrap_or_else(|error| {
        fail(format!(
            "no FFmpeg runtime at {} ({error}); run scripts/fetch-ffmpeg-runtime.sh {target}",
            prefix.display()
        ))
    });
    ffmpeg_runtime::check_prefix(&prefix, &pin, &target).unwrap_or_else(|error| fail(error));

    let modules: Vec<&str> = pin.libraries.iter().map(String::as_str).collect();
    let tokens = ffmpeg_runtime::pkg_config_libs(&prefix, &modules).unwrap_or_else(|error| fail(error));
    let libraries = ffmpeg_runtime::shared_libraries(&tokens, &prefix, &pin).unwrap_or_else(|error| fail(error));

    // Cargo puts build-script search paths on the library path of `cargo test`
    // and `cargo run` only when they lie inside the target directory.
    let search = out_dir.join("ffmpeg-runtime-lib");
    if fs::symlink_metadata(&search).is_ok() {
        fs::remove_file(&search).unwrap_or_else(|error| fail(format!("cannot replace {}: {error}", search.display())));
    }
    std::os::unix::fs::symlink(prefix.join("lib"), &search)
        .unwrap_or_else(|error| fail(format!("cannot link {}: {error}", search.display())));
    println!("cargo:rustc-link-search=native={}", search.display());
    for library in libraries {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        ffmpeg_runtime::emit_macos_dev_rpath("player-media", &manifest_dir);
    }

    println!("cargo:rerun-if-env-changed={}", ffmpeg_runtime::PREFIX_ENV);
    println!("cargo:rerun-if-changed={}", pin_path.display());
    for relative in ["share/ffmpeg-build/manifest.json", "include", "lib"] {
        println!("cargo:rerun-if-changed={}", prefix.join(relative).display());
    }
}
