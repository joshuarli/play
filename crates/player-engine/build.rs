fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "macos" {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        player_build::emit_macos_prefix_rpath("player-engine", &dir);
    }
    if target != "linux" {
        return;
    }
    // Build-script link-args do not propagate to dependent crates the way
    // link-lib and link-search do, so the engine repeats the ALSA and
    // VA-API pairs that player-audio and player-media emit for their own
    // targets. Absolute paths because the musl cross-linker does not search
    // /usr/lib for -l names.
    println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    player_build::ffmpeg_sdk::emit_linux_va("player-engine");
    // The renderer closure is owned by player-output's linux-link.args;
    // read it there instead of copying it here.
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    player_build::emit_linux_closure(
        "player-engine",
        &dir.join("../player-output/linux-link.args"),
        &dir.join("../../.work/deps/linux/lib"),
    );
}
