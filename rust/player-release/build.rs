fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup");
        println!("cargo:rustc-link-arg=-Wl,-install_name,@rpath/libplayer_release.dylib");
    }
}
