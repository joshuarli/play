# Player

Player is a local-only media player for macOS on Apple Silicon and for Linux x86-64 on musl. It uses the mpv v0.41.0 playback pipeline, a pinned FFmpeg n9.0.2, and a Rust-owned control plane. It plays local video, audio, subtitles, and playlists. It does not open network streams.

## Install and run

Tagged releases publish `Player-aarch64-apple-darwin.zip` for macOS and `player-x86_64-linux-musl.tar.gz` for Linux with `SHA256SUMS`. With the `~/usr` dotfiles, `sh dl player` installs the latest release after verifying the checksum.

Build the pinned dependency closure and the app:

```sh
scripts/build-deps.sh
scripts/release.sh
```

`make install` builds the app and copies it to `~/Applications/Player.app`.

The app is `dist/Player.app`. Open it from Finder, or run a file directly:

```sh
dist/Player.app/Contents/MacOS/player FILE
```

The bundle carries its own dylibs, Vulkan loader, MoltenVK, and the Rust cdylib. It does not use Homebrew at runtime. It is ad-hoc signed and not notarized; macOS may require the Finder context-menu Open on first launch, or clearing the quarantine attribute.

On Linux, `make linux` builds `dist/player-x86_64-linux-musl.tar.gz` in a pinned Alpine container, and `make install` copies `player` to `~/.local/bin`. The executable is mostly static and needs only musl, the Vulkan loader, Wayland, xkbcommon, and ALSA at run time; see [Linux](docs/linux.md).

## Input

- Local filesystem paths, including spaces, Unicode, `:`, relative paths, symlinks, and mounted volumes.
- Local playlists (M3U, M3U8, PLS) and external audio or subtitle files.
- Network URLs are rejected before the stream layer with `Refusing non-local input`. The player makes no network connection.

The player does not read `mpv.conf`, `input.conf`, or scripts from user directories. Pass options on the command line and load scripts explicitly with `--script=FILE`.

## Controls

The on-screen controls provide play and pause, seek, volume and mute, fullscreen, and track selection. The macOS menu provides the same playback controls, file opening, screenshots, and window sizing. Clipboard paste of a local path is available from the default key bindings.

## Requirements

macOS 27 on Apple Silicon. Building needs the Xcode command-line tools, Meson, Ninja, pkg-config, CMake, Python 3, curl, and `shasum`.

Linux x86-64 on musl (Alpine or similar) with a Vulkan driver, a Wayland compositor, and ALSA. Hardware decode is on by default (`--hwdec=auto`) and needs a VA-API driver such as `mesa-va-gallium` or `intel-media-driver`; without one, or with `--hwdec=no`, it decodes in software. Building needs Docker; the toolchain lives in the pinned image. glibc systems cannot run the Linux build, and glibc support is explicitly out of scope.

## Help and development

- [Install and run](docs/install.md)
- [Linux](docs/linux.md)
- [Current status](docs/status.md)
- [Architecture](docs/architecture.md)
- [Scope and retained code](docs/scope.md)
- [Metrics](docs/metrics.md)
- [Release report](docs/final-report.md)

Development build: `scripts/build.sh` on macOS, `scripts/linux-container.sh scripts/build-linux.sh` on Linux. Tests: `tests/run.sh suite` (`scripts/linux-container.sh tests/run.sh suite` on Linux). Audit: `scripts/audit.sh` on macOS, `scripts/audit-linux.sh` on Linux. See [AGENTS.md](AGENTS.md) for the repository map.

## License

GPL-2.0-or-later. mpv and the bundled components keep their licenses; see [licenses](docs/licenses.md). This fork is not affiliated with the mpv project.
