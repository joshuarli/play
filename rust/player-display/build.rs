fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "linux" {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        player_build::emit_linux_closure(
            "player-display",
            &dir.join("../player-output/linux-link.args"),
            &dir.join("../../.work/deps/linux/lib"),
        );
    }
}
