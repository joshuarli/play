# Player

Player is a local-only media player for macOS on Apple Silicon. It uses the mpv v0.41.0 playback pipeline, a pinned FFmpeg n9.0.2, and a Rust-owned control plane. It plays local video, audio, subtitles, and playlists. It does not open network streams.

## Install and run

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

The bundle carries its own dylibs, Vulkan loader, MoltenVK, and the Rust cdylib. It does not use Homebrew at runtime. It is ad-hoc signed and not notarized; do not distribute the build without a signing identity.

## Input

- Local filesystem paths, including spaces, Unicode, `:`, relative paths, symlinks, and mounted volumes.
- Local playlists (M3U, M3U8, PLS) and external audio or subtitle files.
- Network URLs are rejected before the stream layer with `Refusing non-local input`. The player makes no network connection.

The player does not read `mpv.conf`, `input.conf`, or scripts from user directories. Pass options on the command line and load scripts explicitly with `--script=FILE`.

## Controls

The on-screen controls provide play and pause, seek, volume and mute, fullscreen, and track selection. The macOS menu provides the same playback controls, file opening, screenshots, and window sizing. Clipboard paste of a local path is available from the default key bindings.

## Requirements

macOS 27 on Apple Silicon. Building needs the Xcode command-line tools, Meson, Ninja, pkg-config, CMake, Python 3, curl, and `shasum`.

## Help and development

- [Install and run](docs/install.md)
- [Current status](docs/status.md)
- [Architecture](docs/architecture.md)
- [Scope and retained code](docs/scope.md)
- [Metrics](docs/metrics.md)
- [Release report](docs/final-report.md)

Development build: `scripts/build.sh`. Tests: `tests/run.sh suite`. Audit: `scripts/audit.sh`. See [AGENTS.md](AGENTS.md) for the repository map.

## License

GPL-2.0-or-later. mpv and the bundled components keep their licenses; see [licenses](docs/licenses.md). This fork is not affiliated with the mpv project.
