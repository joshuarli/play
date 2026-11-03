# Player

Player is a local-only media player for macOS on Apple Silicon and for Linux x86-64 on musl. It uses the mpv v0.41.0 playback pipeline, a pinned FFmpeg n9.0.2, and a Rust-owned control plane. It plays local video, audio, subtitles, and playlists. It does not open network streams.

## Install and run

Tagged releases publish `Player-aarch64-apple-darwin.zip` for macOS and `player-x86_64-linux-musl.tar.gz` for Linux with `SHA256SUMS`. With the `~/usr` dotfiles, `sh dl player` installs the latest release after verifying the checksum.

Unzip the macOS app and open `Player.app` from Finder, or run a file directly:

```sh
Player.app/Contents/MacOS/player FILE
```

The bundle carries its own dylibs, Vulkan loader, MoltenVK, and the Rust cdylib. It does not use Homebrew at runtime. It is ad-hoc signed and not notarized; macOS may require the Finder context-menu Open on first launch, or clearing the quarantine attribute.

On Linux, unpack the tarball and run `./player FILE`. The executable is mostly static and needs only musl, the Vulkan loader, Wayland, xkbcommon, ALSA, and libva at run time; the bundled README lists the runtime packages. See [Linux](docs/linux.md).

To build either artifact from source, see [Install and run](docs/install.md).

## Input

- Local filesystem paths, including spaces, Unicode, `:`, relative paths, symlinks, and mounted volumes.
- Local playlists (M3U, M3U8, PLS) and external audio or subtitle files.
- Network URLs are rejected before the stream layer with `Refusing non-local input`. The player makes no network connection.

The player does not read `mpv.conf`, `input.conf`, or scripts from user directories. Pass options on the command line and load scripts explicitly with `--script=FILE`.

## Controls

The on-screen controls provide play and pause, seek, volume and mute, fullscreen, and track selection. The macOS menu provides the same playback controls, file opening, screenshots, and window sizing.

## Terminal UI

`player --tui FILE` plays in the current terminal instead of opening a window. It uses the alternate screen buffer, centers the embedded album art, and keeps the transport and metadata in one column:

- **space** pause, **←/→** seek, **↑/↓** volume, **m** mute, **n/p** previous or next playlist entry, **r** restart, **q** quit.
- On Ghostty, kitty, and WezTerm the cover is sent with the kitty graphics protocol. Terminals without it get a truecolor half-block rendering. `PLAYER_TUI_ART=blocks` forces the fallback.
- Inside tmux, enable passthrough for pixel art: `tmux set -g allow-passthrough on`. Without it the UI stays in half-block mode.
- The terminal UI is audio-first. Video files play their audio with a placeholder instead of a video frame.

## Requirements

macOS 27 on Apple Silicon, or Linux x86-64 on musl (Alpine or similar) with a Vulkan driver, a Wayland compositor, ALSA, and fontconfig with a font.

## Help

- [Install and run](docs/install.md)
- [Linux](docs/linux.md)
- [Scope](docs/scope.md)
- [Terminal UI design](docs/tui-design.md)
- [Licenses](docs/licenses.md)

For development, see [AGENTS.md](AGENTS.md).

## License

GPL-2.0-or-later. mpv and the bundled components keep their licenses; see [licenses](docs/licenses.md). This fork is not affiliated with the mpv project.
