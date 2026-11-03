fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "linux" {
        emit_linux_closure();
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

fn emit_linux_closure() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let path = dir.join("linux-link.args");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("player-output: cannot read {}", path.display()));
    let prefix = dir.join("../../.work/deps/linux/lib");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let body = line.replace("{PREFIX}", &prefix.display().to_string());
        if let Some(arg) = body.strip_prefix("arg:") {
            println!("cargo:rustc-link-arg={arg}");
        } else if let Some(lib) = body.strip_prefix("lib:") {
            println!("cargo:rustc-link-lib={lib}");
        } else {
            panic!("player-output: bad directive in linux-link.args: {line}");
        }
    }
}
