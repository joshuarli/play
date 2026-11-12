fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    if target == "macos" {
        player_build::emit_macos_prefix_rpath("player-gui", &dir);
        player_build::ffmpeg_runtime::emit_macos_dev_rpath("player-gui", &dir);
    }
    if target != "linux" {
        return;
    }
    println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    let closure = "../player-output/linux-link.args";
    player_build::emit_linux_closure(
        "player-gui",
        &dir.join(closure),
        &dir.join("../../.work/deps/linux/lib"),
    );
}
