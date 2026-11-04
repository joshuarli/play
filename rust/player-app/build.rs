fn main() {
    println!("cargo:rustc-link-lib=z");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "linux" {
        return;
    }
    println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    println!("cargo:rustc-link-arg=/usr/lib/libva.so");
    println!("cargo:rustc-link-arg=/usr/lib/libva-drm.so");
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    for closure in ["../player-output/linux-link.args", "../player-ass/ass-link.args"] {
        player_build::emit_linux_closure(
            "player-app",
            &dir.join(closure),
            &dir.join("../../.work/deps/linux/lib"),
        );
    }
}
