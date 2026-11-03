fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "linux" {
        let prefix = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../.work/deps/linux/lib");
        println!("cargo:rustc-link-arg={}/libplacebo.a", prefix.display());
        println!("cargo:rustc-link-arg={}/libshaderc_combined.a", prefix.display());
        println!("cargo:rustc-link-arg=/usr/lib/libstdc++.a");
        println!("cargo:rustc-link-arg=/usr/lib/libvulkan.so");
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=m");
        return;
    }
    let output = std::process::Command::new("pkg-config")
        .args(["--libs", "libplacebo"])
        .output()
        .unwrap_or_else(|_| panic!("player-output: pkg-config libplacebo failed"));
    assert!(
        output.status.success(),
        "player-output: pkg-config libplacebo failed"
    );
    let libs = String::from_utf8_lossy(&output.stdout);
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
