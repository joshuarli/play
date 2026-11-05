fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "linux" {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        player_build::emit_linux_closure(
            "player-output",
            &dir.join("linux-link.args"),
            &dir.join("../../.work/deps/linux/lib"),
        );
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let prefix = player_build::release_prefix("player-output", &dir);
    let pkgconfig = prefix.join("lib/pkgconfig");
    for package in ["libplacebo", "vulkan"] {
        println!("cargo:rerun-if-changed={}", pkgconfig.join(format!("{package}.pc")).display());
        let output = std::process::Command::new("pkg-config")
            .args(["--libs", package])
            .env("PKG_CONFIG_LIBDIR", &pkgconfig)
            .env_remove("PKG_CONFIG_PATH")
            .output()
            .unwrap_or_else(|_| panic!("player-output: pkg-config {package} failed"));
        assert!(
            output.status.success(),
            "player-output: pkg-config {package} failed"
        );
        emit_pkg_libs(&String::from_utf8_lossy(&output.stdout));
    }
    player_build::emit_macos_prefix_rpath("player-output", &dir);
}

fn emit_pkg_libs(libs: &str) {
    for token in libs.split_whitespace() {
        if let Some(dir) = token.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(name) = token.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={name}");
        } else if let Some(framework) = token.strip_prefix("-framework") {
            println!("cargo:rustc-link-lib=framework={framework}");
        } else {
            println!("cargo:rustc-link-arg={token}");
        }
    }
}
