fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "macos" {
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        player_build::ffmpeg_runtime::emit_macos_dev_rpath("player-audio", &dir);
    } else if target == "linux" {
        // The musl container ships only the shared ALSA library, so link it
        // by absolute path: the musl cross-linker does not search /usr/lib
        // for -lasound.
        println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    }
}
