fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "macos" {
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
    } else if target == "linux" {
        // The musl container ships only the shared ALSA library, so link it
        // by absolute path like the libva precedent in player-media: the
        // musl cross-linker does not search /usr/lib for -lasound.
        println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
        // Build-script link-args do not propagate to dependent crates the
        // way link-lib and link-search do, so the VA-API pair player-media
        // emits for its own targets is repeated here for ours.
        player_build::ffmpeg_sdk::emit_linux_va("player-audio");
    }
}
