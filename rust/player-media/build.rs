use std::env;
use std::fs;
use std::path::PathBuf;

use player_build::ffmpeg_sdk::{self, Link, Pin};

const MODULES: [&str; 4] = ["libavformat", "libavcodec", "libswscale", "libavutil"];
const LINUX_SYSTEM_VA: [(&str, &str); 2] = [("va", "libva"), ("va-drm", "libva-drm")];

fn fail(message: String) -> ! {
    panic!("player-media: {message}")
}

fn main() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = env::var("TARGET").unwrap_or_else(|_| fail("cargo did not set TARGET".into()));
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let pin_path = workspace.join(ffmpeg_sdk::PIN_FILE);
    let pin_text = fs::read_to_string(&pin_path)
        .unwrap_or_else(|error| fail(format!("cannot read {}: {error}", pin_path.display())));
    let pin = Pin::parse(&pin_text).unwrap_or_else(|error| fail(error));

    let prefix = match env::var_os("FFMPEG_PREFIX") {
        Some(prefix) => PathBuf::from(prefix),
        None => ffmpeg_sdk::default_prefix(&workspace, &target),
    };
    let prefix = prefix
        .canonicalize()
        .unwrap_or_else(|error| {
            fail(format!(
                "no FFmpeg SDK at {} ({error}); run scripts/fetch-ffmpeg-sdk.sh {target}",
                prefix.display()
            ))
        });
    ffmpeg_sdk::check_prefix(&prefix, &pin, &target).unwrap_or_else(|error| fail(error));

    let tokens = ffmpeg_sdk::pkg_config_static_libs(&prefix, &MODULES).unwrap_or_else(|error| fail(error));
    let links = ffmpeg_sdk::plan_links(&tokens, &prefix, |name| {
        match LINUX_SYSTEM_VA.iter().find(|(library, _)| *library == name) {
            Some((library, module)) if target_os == "linux" => {
                let libdir = ffmpeg_sdk::system_libdir(module).unwrap_or_else(|error| fail(error));
                Link::Arg(libdir.join(format!("lib{library}.so")).display().to_string())
            }
            _ => Link::System(name.to_owned()),
        }
    })
    .unwrap_or_else(|error| fail(error));
    ffmpeg_sdk::emit(&links);

    println!("cargo:rerun-if-env-changed=FFMPEG_PREFIX");
    println!("cargo:rerun-if-changed={}", pin_path.display());
    for relative in ["share/ffmpeg-build/manifest.json", "include", "lib"] {
        println!("cargo:rerun-if-changed={}", prefix.join(relative).display());
    }
}
