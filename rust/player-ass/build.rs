fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "linux" {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        player_build::emit_linux_closure(
            "player-ass",
            &dir.join("ass-link.args"),
            &dir.join("../../.work/deps/linux/lib"),
        );
        return;
    }
    let output = std::process::Command::new("pkg-config")
        .args(["--libs", "libass"])
        .output()
        .unwrap_or_else(|_| panic!("player-ass: pkg-config libass failed"));
    assert!(
        output.status.success(),
        "player-ass: pkg-config libass failed"
    );
    for token in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        if let Some(dir) = token.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(name) = token.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={name}");
        } else {
            println!("cargo:rustc-link-arg={token}");
        }
    }
}
