fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "macos" {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        player_build::emit_macos_prefix_rpath("player-backend", &dir);
    }
    if target != "linux" {
        return;
    }
    println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    player_build::ffmpeg_sdk::emit_linux_va("player-backend");
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    player_build::emit_linux_closure(
        "player-backend",
        &dir.join("../player-output/linux-link.args"),
        &dir.join("../../.work/deps/linux/lib"),
    );
}
