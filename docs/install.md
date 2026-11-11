# Install and run

macOS builds `dist/Player.app`; Linux x86-64 on musl builds `dist/player` and the tarball. Sections below cover macOS unless titled Linux.

## Release artifacts

A `v*` tag runs `.github/workflows/release.yml`, which builds and publishes:

- `Player-aarch64-apple-darwin.zip`: `Player.app` for Apple Silicon.
- `player-x86_64-linux-musl.tar.gz`: the Linux x86-64 musl binary and its README.

Both need the FFmpeg runtime release named in `ffmpeg-runtime.lock.json`, installed in the layout described in [FFmpeg](ffmpeg.md#installed-layout).
- `SHA256SUMS`: the digests for both archives.

Verify the checksum before use. On a machine with the `~/usr` dotfiles, `sh dl player` installs the latest release and verifies it: `~/Applications/Player.app` plus `~/.local/bin/player` on macOS, or `~/.local/bin/player` on Linux. Set `PLAYER_APPLICATIONS_DIR` to change the macOS location. Remove a `make install` copy with `make uninstall` before the first `dl` install.

The macOS app is ad-hoc signed and not notarized. Open it from the Finder context menu on first launch, or clear the quarantine attribute.

## Linux

The build runs in Docker, in the pinned Alpine edge image (`docker/linux/Dockerfile`), through `scripts/linux-container.sh`:

```sh
make linux
```

`make linux` builds the static dependency prefix with `scripts/build-deps-linux.sh`, then `scripts/release-linux.sh` builds, audits, and writes `dist/player-x86_64-linux-musl.tar.gz` with a `.sha256` file. The tarball holds `player` and a `README` naming the FFmpeg runtime release and listing the musl runtime packages. `make install` builds the same, installs `player` and a copy of the runtime's libraries as `~/.local/opt/player/{bin/player,lib}`, and links `~/.local/bin/player` to it; set `PLAYER_HOME` or `BINDIR` to change them, and `make uninstall` removes both. Run a file with `player FILE`. The system needs a Vulkan driver, a Wayland compositor, ALSA, a fontconfig configuration with a font, and the `librust_fontconfig` library; `docs/linux.md` covers the runtime set and the headless gates.

## Release app

The macOS build needs the Xcode command-line tools, the pinned Rust toolchain, a Meson implementation and Ninja-compatible backend for SPIRV-Cross and libplacebo (`muon` and `samurai`/`samu` preferred; `meson` and `ninja` also work), pkg-config, Python 3, curl, and `shasum`; `scripts/dev-deps.sh` installs the pinned Homebrew build tools and the FFmpeg used to generate test fixtures. The development build and the Rust tests link the same pinned closure as the app, so build it first; `scripts/build.sh` and `cargo test` fail until it exists:

```sh
scripts/build-deps.sh
scripts/build.sh
build/mpv FILE
build/mpv --gpu-api=vulkan FILE
```

The macOS and Linux libplacebo recipes ignore the caller's `PYTHONPATH` and disable Python's user site packages while building. This keeps those locations from overriding the pinned packages in the source tree. The selected Meson and Python installations must work without those user packages.

Build the release app:

```sh
scripts/build-deps.sh
scripts/release.sh
```

`scripts/release.sh` also downloads and verifies the FFmpeg runtime pinned in `ffmpeg-runtime.lock.json` (see [FFmpeg](ffmpeg.md)); building it needs network access to GitHub releases once.

`make install` builds the same app and installs it with a copy of the runtime's libraries as `~/Applications/Player/{Player.app,lib}`.
Pass another location with `PLAYER_HOME`, or stage a package root with `DESTDIR`. `make uninstall` removes that directory.

The app is `dist/Player.app`, with `dist/lib` linking the runtime's libraries beside it. Open it from Finder, or run a file directly:

```sh
dist/Player.app/Contents/MacOS/player FILE
```

Music played from a terminal opens the terminal UI by default. To use it for any file, pass `--tui`:

```sh
dist/Player.app/Contents/MacOS/player --tui FILE
```

Kitty-capable terminals show the embedded cover at full resolution; others use truecolor half-blocks. See [Terminal UI](tui-design.md).

The bundle carries its own pinned dylibs, Vulkan loader, and MoltenVK, and its libplacebo includes the default Metal backend beside Vulkan (`--gpu-api=vulkan`). FFmpeg is not in the bundle: the executable loads the pinned shared runtime through `@rpath`, from `lib/` beside `Player.app`. It does not use Homebrew or a global FFmpeg. It is ad-hoc signed and not notarized. Do not distribute the build without a signing identity. It declares `MPVBUNDLE` and the supported document types in `Info.plist`.

## Local-only contract

Local paths are supported. Drag-and-drop converts a `file://` URL for an existing path; the CLI, `--playlist`, `loadlist`, IPC `loadfile`, playlists, and external files reject `file://`. Network URLs are rejected before the stream layer with `Refusing non-local input`. The player does not include network protocols, TLS, or a CA bundle.

Clipboard support is removed. Encoding, recording, and the lavfi filter graph are not built.

## Development

`nix develop` optionally provides pinned Muon, Meson, Samurai, Ninja, pkgconf, Python, and ripgrep on macOS arm64 and Linux x86-64. The macOS shell selects Apple Clang through the active Xcode tools. Install Rust and the fixture FFmpeg separately. macOS still needs Xcode and its SDK. Linux still needs Docker: Nix supplies host tools, and `make linux` builds in the pinned Alpine container. Nix is not required by the existing commands or the player.

```sh
scripts/build.sh
tests/run.sh suite
```

The development build is `build/mpv`; scratch files live under `.work/`. `scripts/release.sh` is the only supported way to produce the app. Do not hand-assemble `Contents/Frameworks`.

## Owner checks

```sh
scripts/manual-checklist.sh --fixture-dir .work/fixtures --release
```

Record results in `docs/manual-checklist.md`. Visual quality, real audio, real mouse and drag, HDR and Retina, and signing or notarization are not automated gates. `TODO.md` lists open user-facing defects.
