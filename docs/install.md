# Install and run

macOS builds `dist/Player.app`; Linux x86-64 on musl builds `dist/player` and the tarball. Sections below cover macOS unless titled Linux.

## Release artifacts

A `v*` tag runs `.github/workflows/release.yml`, which builds and publishes:

- `Player-aarch64-apple-darwin.zip`: `Player.app` for Apple Silicon.
- `player-x86_64-linux-musl.tar.gz`: the Linux x86-64 musl binary and its README.
- `SHA256SUMS`: the digests for both archives.

Verify the checksum before use. On a machine with the `~/usr` dotfiles, `sh dl player` installs the latest release and verifies it: `~/Applications/Player.app` plus `~/.local/bin/player` on macOS, or `~/.local/bin/player` on Linux. Set `PLAYER_APPLICATIONS_DIR` to change the macOS location. Remove a `make install` copy with `make uninstall` before the first `dl` install.

The macOS app is ad-hoc signed and not notarized. Open it from the Finder context menu on first launch, or clear the quarantine attribute.

## Linux

The build runs in Docker, in the pinned Alpine edge image (`docker/linux/Dockerfile`), through `scripts/linux-container.sh`:

```sh
make linux
```

`make linux` builds the static dependency prefix with `scripts/build-deps-linux.sh`, then `scripts/release-linux.sh` builds, audits, and writes `dist/player-x86_64-linux-musl.tar.gz` with a `.sha256` file. The tarball holds `player` and a `README` listing the musl runtime packages. `make install` builds the same and copies `player` to `~/.local/bin`; set `BINDIR` to change it, and `make uninstall` removes it. Run a file with `player FILE`. The system needs a Vulkan driver, a Wayland compositor, ALSA, and fontconfig with a font; `docs/linux.md` covers the runtime set and the headless gates.

## Release app

The macOS build needs the Xcode command-line tools, the pinned Rust toolchain, Meson, Ninja, pkg-config, CMake, Python 3, curl, and `shasum`; `scripts/dev-deps.sh` installs the pinned Homebrew packages. Build the dependency closure and the app:

```sh
scripts/build-deps.sh
scripts/release.sh
```

`scripts/release.sh` also downloads and verifies the static FFmpeg SDK pinned in `ffmpeg-sdk.lock.json` (see [FFmpeg](ffmpeg.md)); building it needs network access to GitHub releases once.

`make install` builds the same app and copies it to `~/Applications/Player.app`.
Pass another location with `PREFIX`, such as `make install PREFIX=/Applications`,
or stage a package root with `DESTDIR`. `make uninstall` removes that copy.

The app is `dist/Player.app`. Open it from Finder, or run a file directly:

```sh
dist/Player.app/Contents/MacOS/player FILE
```

Music played from a terminal opens the terminal UI by default. To use it for any file, pass `--tui`:

```sh
dist/Player.app/Contents/MacOS/player --tui FILE
```

Kitty-capable terminals show the embedded cover at full resolution; others use truecolor half-blocks. See [Terminal UI](tui-design.md).

The bundle carries its own pinned dylibs, Vulkan loader, and MoltenVK; FFmpeg, dav1d, and zlib are linked statically into the executable. It does not use Homebrew, a global FFmpeg, or `usr` at runtime. It is ad-hoc signed and not notarized. Do not distribute the build without a signing identity. It declares `MPVBUNDLE` and the supported document types in `Info.plist`.

## Local-only contract

Local paths are supported. Drag-and-drop converts a `file://` URL for an existing path; the CLI, `--playlist`, `loadlist`, IPC `loadfile`, playlists, and external files reject `file://`. Network URLs are rejected before the stream layer with `Refusing non-local input`. The player does not include network protocols, TLS, or a CA bundle.

Clipboard support is removed. Encoding, recording, and the lavfi filter graph are not built.

## Development

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
