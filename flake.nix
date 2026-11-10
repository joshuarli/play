{
  description = "Player development tools";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" ];
      shell = system:
        let
          pkgs = import nixpkgs { inherit system; };
          pkgconf = pkgs.pkgconf.override {
            pkg-config = pkgs.pkgconf-unwrapped.overrideAttrs (_: {
              version = "3.0.10";
              src = pkgs.fetchurl {
                url = "https://distfiles.ariadne.space/pkgconf/pkgconf-3.0.10.tar.xz";
                hash = "sha256-k3Gy9jctpXTUxtyW/psUYwN7z2r8tmccFVWv3nsX4t4=";
              };
            });
            extraBuildCommands = ''ln -s pkgconf "$out/bin/pkg-config"'';
          };
        in pkgs.mkShellNoCC {
          packages = with pkgs; [
            muon meson samurai ninja pkgconf python3 ripgrep
          ];
          shellHook = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
            player_native_cc="$(xcrun --find clang)" || exit 1
            player_native_cxx="$(xcrun --find clang++)" || exit 1
            export CC="$player_native_cc"
            export CXX="$player_native_cxx"
          '';
        };
    in {
      devShells = nixpkgs.lib.genAttrs systems (system: { default = shell system; });
    };
}
