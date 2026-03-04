fn main() {
    for fw in [
        "AVFoundation",
        "CoreMedia",
        "CoreVideo",
        "CoreGraphics",
        "CoreText",
        "QuartzCore",
        "AppKit",
        "VideoToolbox",
        "AudioToolbox",
        "CoreFoundation",
    ] {
        println!("cargo:rustc-link-lib=framework={fw}");
    }
}
