# Player

Player is a local-only media player for macOS on Apple Silicon and for Linux x86-64 on musl. It uses a Rust playback engine, a pinned FFmpeg n9.0.2, libplacebo, and the mpv-compatible application surface ported in this repository. It plays local video, audio, subtitles, and playlists. It does not open network streams.

## Install and run

Tagged releases publish `Player-aarch64-apple-darwin.zip` for macOS and `player-x86_64-linux-musl.tar.gz` for Linux with `SHA256SUMS`. With the `~/usr` dotfiles, `sh dl player` installs the latest release after verifying the checksum.

Unzip the macOS app and open `Player.app` from Finder, or run a file directly:

```sh
Player.app/Contents/MacOS/player FILE
```

The bundle carries its own dylibs, Vulkan loader, and MoltenVK. It does not use Homebrew at runtime. It is ad-hoc signed and not notarized; macOS may require the Finder context-menu Open on first launch, or clearing the quarantine attribute.

On Linux, unpack the tarball and run `./player FILE`. The executable is mostly static and needs only musl, the Vulkan loader, Wayland client, ALSA, and libva at run time; the bundled README lists the runtime packages. See [Linux](docs/linux.md).

To build either artifact from source, see [Install and run](docs/install.md).

## Input

- Local filesystem paths, including spaces, Unicode, `:`, relative paths, symlinks, and mounted volumes.
- Local playlists (M3U, M3U8, PLS) and external audio or subtitle files.
- Network URLs are rejected before the stream layer with `Refusing non-local input`. The player makes no network connection.

The player does not read `mpv.conf`, `input.conf`, or scripts from user directories, and it has no scripting. Key bindings and defaults are built in. Pass options on the command line, or set `PLAYER_OPTIONS` to options that apply before the command line, for example `PLAYER_OPTIONS='--fs=no --volume=60'`.

## Controls

The window opens fullscreen; pass `--fs=no` or `--geometry=WxH` for a window. The on-screen controls provide play and pause, seek, volume and mute, fullscreen, and track selection. Open a file from the command line, Finder, or drag-and-drop; screenshots are available over IPC.

## Terminal UI

`player --tui FILE` plays in the current terminal instead of opening a window. Music starts here by default: when stdin and stdout are a terminal and the first playlist entry is audio with no video beyond cover art, the player enters the terminal UI without `--tui` (`--gui` opts out). It uses the alternate screen buffer, centers the embedded album art, and keeps the transport and metadata in one column:

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
- [Measurements](docs/metrics.md)
- [Manual checks](docs/manual-checklist.md)

For development, see [AGENTS.md](AGENTS.md). Open work is in [TODO.md](TODO.md).

## Limits

- Local files only: no network protocol, TLS, or remote resource load.
- Video decodes in software; hardware decode wiring is `PLAY-20`.
- Paths must be UTF-8.
- GUI visuals, HDR, Retina scaling, real mouse input, audibility, and hardware decode are owner-run checks in `docs/manual-checklist.md`; the automated gates are headless.
- Linux is x86-64 musl on Wayland only. glibc and X11 are out of scope.
