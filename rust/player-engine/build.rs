fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "linux" {
        return;
    }
    // Build-script link-args do not propagate to dependent crates the way
    // link-lib and link-search do, so the engine repeats the pairs that
    // player-audio and player-media emit for their own targets: the
    // container ALSA library and the VA-API pair the Linux FFmpeg
    // hwcontext pulls. Absolute paths because the musl cross-linker does
    // not search /usr/lib for -l names.
    println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    println!("cargo:rustc-link-arg=/usr/lib/libva.so");
    println!("cargo:rustc-link-arg=/usr/lib/libva-drm.so");
}
