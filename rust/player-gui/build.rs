fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "linux" {
        return;
    }
    println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    player_build::ffmpeg_sdk::emit_linux_va("player-gui");
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    for closure in ["../player-output/linux-link.args"] {
        player_build::emit_linux_closure(
            "player-gui",
            &dir.join(closure),
            &dir.join("../../.work/deps/linux/lib"),
        );
    }
}
