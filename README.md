# Player

Player is a local-only media player for macOS on Apple Silicon and for Linux x86-64 on musl. It uses a Rust playback engine, a pinned FFmpeg n9.0.2, libplacebo, and the mpv-compatible application surface ported in this repository. It plays local video, audio, subtitles, and playlists. It does not open network streams.

## Install and run

Download the archive for your platform and `SHA256SUMS` from [Player releases](https://github.com/joshuarli/play/releases). Verify the archive against `SHA256SUMS`. With the `~/usr` dotfiles, `sh dl player` installs the latest release and verifies its checksum.

On macOS, unzip `Player-aarch64-apple-darwin.zip`. Download the matching Apple Silicon FFmpeg runtime from [ffmpeg-build releases](https://github.com/laputa-systems/ffmpeg-build/releases). The required release is in `Player.app/Contents/Info.plist` under `PlayerFFmpegRuntime`. Extract its `lib/` beside `Player.app`. Open the app from Finder, or run a file directly:

```sh
Player.app/Contents/MacOS/player FILE
```

The bundle carries its own dylibs, Vulkan loader, and MoltenVK, and loads the pinned FFmpeg runtime from `lib/` beside `Player.app`. It does not use Homebrew at runtime. It is ad-hoc signed and not notarized; macOS may require the Finder context-menu Open on first launch, or clearing the quarantine attribute.

On Linux, extract `player-x86_64-linux-musl.tar.gz`. Its README names the required [FFmpeg runtime](https://github.com/laputa-systems/ffmpeg-build/releases) and runtime packages. Place `player` in a `bin/` directory beside the runtime `lib/`, and put `librust_fontconfig.so.5.0.2` in that `lib/` or install the system rust-fontconfig package. Run `player FILE`.


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

macOS 27 on Apple Silicon, or Linux x86-64 on musl (Alpine or similar) with a Vulkan driver, a Wayland compositor, ALSA, and a fontconfig configuration with a font, plus the `librust_fontconfig` library from `laputa-systems/rust-fontconfig`.

## Help

Run `player --help` for command-line options. Report problems in [GitHub issues](https://github.com/joshuarli/play/issues). For source builds and developer documentation, see [AGENTS.md](AGENTS.md).

## Limits

Video currently decodes in software. Window rendering accepts a subset of 8-bit pixel formats; 10-bit input presentation is still open, and Metal output is SDR. Paths must be UTF-8. Linux requires musl and Wayland; glibc and X11 are not supported. [TODO.md](TODO.md) tracks the open playback and interaction limitations.
