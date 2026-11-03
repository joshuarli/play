fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "linux" {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        player_build::emit_linux_closure(
            "player-display",
            &dir.join("../player-output/linux-link.args"),
            &dir.join("../../.work/deps/linux/lib"),
        );
        return;
    }
    if target == "macos" {
        cc::Build::new()
            .file("bridge/metal_window.m")
            .flag("-fobjc-arc")
            .compile("player_display_bridge");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=QuartzCore");
    }
}
