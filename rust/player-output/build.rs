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
    for package in ["libplacebo", "vulkan"] {
        let output = std::process::Command::new("pkg-config")
            .args(["--libs", package])
            .output()
            .unwrap_or_else(|_| panic!("player-output: pkg-config {package} failed"));
        assert!(
            output.status.success(),
            "player-output: pkg-config {package} failed"
        );
        emit_pkg_libs(&String::from_utf8_lossy(&output.stdout));
    }
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
