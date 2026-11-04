use std::env;
use std::path::PathBuf;
use std::process::Command;

fn prefix_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(prefix) = env::var("FFMPEG_PREFIX") {
        out.push(PathBuf::from(prefix));
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    // The work tree is shared with the Linux container, so the macOS prefix
    // must never be selected there: choose per target OS.
    let platform: &[&str] = if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        &[".work/deps/linux/ffmpeg"]
    } else {
        &[".work/deps/ffmpeg-full", ".work/deps/release/ffmpeg"]
    };
    for relative in platform {
        out.push(workspace.join(relative));
    }
    out
}

fn pkg_config_libs(name: &str) -> Vec<String> {
    let output = Command::new("pkg-config")
        .arg("--libs")
        .arg(name)
        .output()
        .unwrap_or_else(|_| panic!("player-media: pkg-config {name} failed"));
    assert!(
        output.status.success(),
        "player-media: pkg-config {name} failed"
    );
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn main() {
    let prefix = prefix_candidates()
        .into_iter()
        .find(|candidate| candidate.join("include/libavformat/avformat.h").is_file())
        .unwrap_or_else(|| {
            panic!(
                "player-media: no pinned FFmpeg prefix found; \
                 set FFMPEG_PREFIX or run scripts/build-ffmpeg.sh"
            )
        });
    let lib = prefix.join("lib");
    assert!(
        lib.join("libavformat.a").is_file(),
        "player-media: static libavformat missing in {}",
        lib.display()
    );
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=avformat");
    println!("cargo:rustc-link-lib=static=avcodec");
    // Album art decodes to RGB24 at thumbnail size. libswscale is already part
    // of the pinned FFmpeg closure; no new runtime library is added.
    println!("cargo:rustc-link-lib=static=swscale");
    println!("cargo:rustc-link-lib=static=avutil");
    // The static libavcodec pulls the bundled dav1d decoder objects, so the
    // same decoder closure as the C link is required even though demuxing
    // never decodes.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for token in pkg_config_libs("dav1d") {
            if let Some(dir) = token.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={dir}");
            } else if let Some(name) = token.strip_prefix("-l") {
                println!("cargo:rustc-link-lib={name}");
            } else if let Some(name) = token.strip_prefix("-framework") {
                println!("cargo:rustc-link-lib=framework={name}");
            }
        }
        for framework in [
            "VideoToolbox",
            "CoreFoundation",
            "CoreMedia",
            "CoreVideo",
            "CoreServices",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    } else {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let linux_lib = workspace.join(".work/deps/linux/lib");
        if linux_lib.join("libdav1d.a").is_file() {
            println!("cargo:rustc-link-search=native={}", linux_lib.display());
            println!("cargo:rustc-link-lib=static=dav1d");
        }
        // The Linux FFmpeg enables VA-API, so its hwcontext pulls the
        // container's system libva, matching the C link. The paths are
        // passed by absolute name because the musl cross-linker does not
        // search /usr/lib for -lva.
        println!("cargo:rustc-link-arg=/usr/lib/libva.so");
        println!("cargo:rustc-link-arg=/usr/lib/libva-drm.so");
    }
    println!("cargo:rustc-link-lib=z");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rerun-if-env-changed=FFMPEG_PREFIX");
}
