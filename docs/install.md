# Install and run

macOS builds `dist/Player.app`; Linux x86-64 on musl builds `dist/player` and the tarball. Sections below cover macOS unless titled Linux.

## Release artifacts

A `v*` tag runs `.github/workflows/release.yml`, which builds and publishes:

- `Player-aarch64-apple-darwin.zip`: `Player.app` for Apple Silicon.
- `player-x86_64-linux-musl.tar.gz`: the Linux x86-64 musl binary and its README.

- `SHA256SUMS`: the digests for both archives.

Both archives need the FFmpeg runtime release named in `ffmpeg-runtime.lock.json`, installed in the layout described in [FFmpeg](ffmpeg.md#installed-layout).

Verify the checksum before use. On a machine with the `~/usr` dotfiles, `sh dl player` installs the latest release and verifies it: `~/Applications/Player.app` plus `~/.local/bin/player` on macOS, or `~/.local/bin/player` on Linux. Set `PLAYER_APPLICATIONS_DIR` to change the macOS location. Remove a `make install` copy with `make uninstall` before the first `dl` install.

The macOS app is ad-hoc signed and not notarized. Open it from the Finder context menu on first launch, or clear the quarantine attribute.

## Linux

The build runs in Docker, in the pinned Alpine image (`docker/linux/Dockerfile`), through `scripts/linux-container.sh`:

```sh
make linux
```

`make linux` builds the static dependency prefix with `scripts/build-deps-linux.sh`, then `scripts/release-linux.sh` builds, audits, and writes `dist/player-x86_64-linux-musl.tar.gz` with a `.sha256` file. The tarball holds `player` and a `README` naming the FFmpeg runtime release and listing the musl runtime packages. `make install` builds the same, installs `player` and a copy of the runtime's libraries as `~/.local/opt/player/{bin/player,lib}`, and links `~/.local/bin/player` to it; set `PLAYER_HOME` or `BINDIR` to change them, and `make uninstall` removes both. Run a file with `player FILE`. The system needs a Vulkan driver, a Wayland compositor, ALSA, a fontconfig configuration with a font, and the `librust_fontconfig` library; `docs/linux.md` covers the runtime set and the headless gates.

## Release app

The macOS build needs the Xcode command-line tools, the pinned Rust toolchain, pkg-config, Python 3, curl, `unzip`, and `shasum`; `scripts/dev-deps.sh` installs the pinned Homebrew pkgconf and the FFmpeg used to generate test fixtures. SPIRV-Cross and libplacebo build with the `meson` and `ninja` from the gluon release pinned in `scripts/fetch-gluon.sh`, which downloads and checksums it into `.work/tools/gluon`; the Linux build uses the same release's musl binaries. No Python Meson, Ninja, samurai, or muon is needed. The development build and the Rust tests link the same pinned closure as the app, so build it first; `scripts/build.sh` and `cargo test` fail until it exists:

```sh
scripts/build-deps.sh
scripts/build.sh
build/mpv FILE
build/mpv --gpu-api=vulkan FILE
```

The macOS and Linux libplacebo generators use the pinned Python packages in the source tree, ignoring the caller's `PYTHONPATH` and Python user site. The other build tools retain the caller's Python environment.

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

Kitty-capable terminals display a 320×320 rendition of the embedded cover; others use truecolor half-blocks. See [Terminal UI](tui-design.md).

The bundle carries its own pinned dylibs, Vulkan loader, and MoltenVK, and its libplacebo includes the default Metal backend beside Vulkan (`--gpu-api=vulkan`). FFmpeg is not in the bundle: the executable loads the pinned shared runtime through `@rpath`, from `lib/` beside `Player.app`. It does not use Homebrew or a global FFmpeg. It is ad-hoc signed and not notarized, matching the published release artifacts. It declares `MPVBUNDLE` and the supported document types in `Info.plist`.

## Local-only contract

Local paths are supported. Drag-and-drop converts a `file://` URL for an existing path; the CLI, `--playlist`, `loadlist`, IPC `loadfile`, playlists, and external files reject `file://`. Network URLs are rejected before the stream layer with `Refusing non-local input`. The player does not include network protocols, TLS, or a CA bundle.

Clipboard support is removed. Encoding, recording, and the lavfi filter graph are not built.

## Development

```sh
scripts/build.sh
tests/run.sh suite
```

The macOS development build is `build/mpv`; Linux uses `build-linux/mpv`. Scratch files live under `.work/`. `scripts/release.sh` is the supported way to produce the macOS app. Do not hand-assemble `Contents/Frameworks`.

Keep Linux Cargo target directories private to each checkout or worktree. The container mounts every checkout at `/work`, so sharing a target directory can let Cargo's mtime checks reuse artifacts from a different source tree.

## Build policy

Use the canonical Cargo and native build/release entrypoints, pinned inputs, and conservative stamp-based invalidation. The runtime and rust-fontconfig fetch/build helpers retain their current pinned-input reuse checks; they do not implement a general integrity-checked, concurrent artifact store. Arbitrary Cargo configuration, compiler wrappers, GCC specs, and hostile concurrent mutation are outside the supported cache contract.

The owner rejected the incremental-cache prototype because the small gain on rare native-recipe edits did not justify its extra machinery and slower unchanged dependency runs; everyday Rust edits did not improve. Keep that no-go decision in force unless the owner reopens it. The kache Cargo shim, optional Nix shell, and reverted cache-hardening prototypes are not part of the supported build. Cargo, the native Apple toolchain, and the pinned Alpine musl build remain the foundations; no replacement build system, remote cache service, or Nix prerequisite was accepted.

The experiments were parked with no retry condition. Their refs are `campaign/build-cache`, `campaign/build-cache-shim`, `campaign/container-identity-probe`, `campaign/gcc-prefix-player-probe`, `campaign/ffmpeg-cache`, `campaign/fontconfig-rustc-identity`, `campaign/build-nix`, and `campaign/nix-shell`; related kache fork work was retained in `~/d/kache`. Git history holds the measurements and rejected implementations.

## Owner checks

```sh
scripts/manual-checklist.sh --fixture-dir .work/fixtures --release
```

Record results in `docs/manual-checklist.md`. Visual quality, real audio, real mouse and drag, HDR and Retina, and signing or notarization are not automated gates. `TODO.md` lists open user-facing defects.

IPC socket paths must be unused. Socket entries remain after close, including a clean exit; remove them explicitly before starting another server at the same path. Automatic unlinking cannot safely distinguish a concurrent replacement on both supported operating systems. Automatic screenshots reserve the next unused `shotNNNN.png` name without overwriting files or following symlinks. `screenshot-to-file` retains explicit overwrite behavior.
