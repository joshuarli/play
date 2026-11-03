fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "linux" {
        return;
    }
    // Build-script link-args do not propagate to dependent crates the way
    // link-lib and link-search do, so the engine repeats the ALSA and
    // VA-API pairs that player-audio and player-media emit for their own
    // targets. Absolute paths because the musl cross-linker does not search
    // /usr/lib for -l names.
    println!("cargo:rustc-link-arg=/usr/lib/libasound.so");
    println!("cargo:rustc-link-arg=/usr/lib/libva.so");
    println!("cargo:rustc-link-arg=/usr/lib/libva-drm.so");
    // The renderer closure is owned by player-output's linux-link.args;
    // read it there instead of copying it here.
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let path = dir.join("../player-output/linux-link.args");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("player-engine: cannot read {}", path.display()));
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
            panic!("player-engine: bad directive in linux-link.args: {line}");
        }
    }
}
