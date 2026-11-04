use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let output = Command::new("pkg-config")
        .args(["--libs", "fontconfig", "harfbuzz", "freetype2"])
        .output()
        .unwrap_or_else(|error| panic!("pkg-config could not run: {error}"));
    assert!(
        output.status.success(),
        "pkg-config --libs fontconfig harfbuzz freetype2 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for flag in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        if let Some(path) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        } else if let Some(name) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib=dylib={name}");
        }
    }
}
