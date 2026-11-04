# Status

Player is a local-only media player. The macOS release artifact is `dist/Player.app`; the Linux x86-64 musl artifact is `dist/player-x86_64-linux-musl.tar.gz`.

## Current state

- The Rust port is complete through S9. The C playback engine and Meson player build are gone; Cargo builds both artifacts. The Rust engine owns scheduling, demux, decode, audio, video rendering, Lua, subtitles, OSD, OSC composition, GUI, and TUI. The `player-app` binary owns the application layer (CLI, IPC, playlists, chapters, track switching, properties/events, OSC dispatch, native GUI window, TUI, Finder open, drag-and-drop, and PNG screenshots).
- `tests/run.sh suite` on macOS: 343 passed, 0 failed.
- `tests/run.sh suite` in the Alpine container: 334 passed, 0 failed.
- `scripts/audit.sh`: ok. `scripts/audit-linux.sh`: ok.
- `scripts/measure.sh final`: final: ok. Distributable 22,744,448 B; first frame 35.11 ms; 1080p RSS 219.75 MB; seek 42.21 ms.
- App: 24,140,241 B on disk; executable 7,617,088 B; framework closure 15,127,360 B in 12 dylibs; bundled icon 1,387,652 B.
- One `file` protocol and one Lua interpreter.
- `--tui` plays in the terminal with centered embedded cover art. Kitty graphics are used on Ghostty, kitty, and WezTerm; other terminals use truecolor half-blocks. The TUI is audio-first and keeps `vo=null`.

## Releases

- `.github/workflows/release.yml` runs on a `v*` tag: the macOS and Linux build workflows run as reusable jobs, then one job publishes `Player-aarch64-apple-darwin.zip`, `player-x86_64-linux-musl.tar.gz`, and `SHA256SUMS`.
- `sh dl player` in `~/usr` installs the latest release on macOS and x86-64 Linux after verifying the checksum.
- `v0.41.0` is published on GitHub with `Player-aarch64-apple-darwin.zip`, `player-x86_64-linux-musl.tar.gz`, and `SHA256SUMS`; the macOS build, Linux build, and publish jobs all passed.

## Limits

- Local files only. No network protocol, TLS, CA bundle, or remote resource load.
- The scheduler requests software video decode. `player-media` carries VideoToolbox and VA-API paths and tests; wiring the scheduler and reporting `hwdec-current` is `PLAY-20`.
- GUI, HDR and 10-bit color, subtitle shaping, Retina scaling, audio audibility, hardware decode, real mouse and drag, and signing or notarization are owner-run. See `docs/manual-checklist.md`.
- Clipboard support is removed. Encoding, recording, and the lavfi filter graph are deleted.
- The `subprocess` command and the `.run` script backend are reachable from explicit scripts and IPC. No built-in module uses them.
- The terminal UI has no sixel output and does not load external cover files; it renders the embedded picture only. Quit, Ctrl-C, and SIGTERM/SIGHUP restore the terminal.
- The UTF-8-only path API remains.
- Linux: glibc is explicitly out of scope. aarch64, X11, and a real-GPU, zero-copy VA-API, real-compositor, and real-ALSA check are outside the automated gates. The headless gate uses lavapipe with `--gpu-sw` and `mailbox` swap because headless Weston never releases FIFO buffers.

## Linux

- `scripts/linux-container.sh tests/run.sh suite`: 334 passed, 0 failed.
- `scripts/audit-linux.sh`: ok. `dist/player` needs seven shared libraries: musl, libgcc, Vulkan loader, Wayland client, ALSA, libva, and libva-drm. The tarball lists six Alpine runtime packages.
- `scripts/measure-linux.sh final`: stripped 24,475,984 B; 1080p RSS 216.09 MB; first frame 75.34 ms; seek 36.05 ms.
- `--hwdec` defaults to the compatibility value `auto` and `hwdec-current` reports `no` while the scheduler decodes in software.
- `tests/run.sh linux-gpu-next` passes 41/41 on headless Weston with lavapipe, including the OSD pixel diff, subtitles, window scaling, and the GUI gate.
- `tests/run.sh linux-vaapi` (needs a GPU render node, not part of `suite`) is an owner check.
- Fixtures match the tracked stream structure; their bytes differ from macOS and between Linux hosts because the encoders select threads and SIMD paths at run time.
- Measurements are in `docs/metrics.md`; the runtime set, headless gates, and fixtures are in `docs/linux.md`.
