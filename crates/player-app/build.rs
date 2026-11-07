fn main() {
    println!("cargo:rustc-link-lib=z");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let triple = std::env::var("TARGET").expect("cargo sets TARGET");
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    player_build::ffmpeg_runtime::emit_installed_rpath("player-app", &triple);
    if target == "macos" {
        player_build::emit_macos_prefix_rpath("player-app", &dir);
        player_build::ffmpeg_runtime::emit_macos_dev_rpath("player-app", &dir);
    }
    if target != "linux" {
        return;
    }
    println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    for closure in ["../player-output/linux-link.args"] {
        player_build::emit_linux_closure(
            "player-app",
            &dir.join(closure),
            &dir.join("../../.work/deps/linux/lib"),
        );
    }
}
