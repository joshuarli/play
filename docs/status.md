# Status

Player is a local-only media player. The macOS release artifact is `dist/Player.app`; the Linux x86-64 musl artifact is `dist/player-x86_64-linux-musl.tar.gz`.

## Current state

- The Rust port is at S7d: the engine owns scheduling, demux, decode, audio, video rendering, Lua, subtitles, OSD, OSC composition, and the TUI (including video files' audio). S7e (GUI) and S8 (legacy removal) remain.
- `osd-fidelity` passes 10/10 on macOS and in the Linux container; `linux-gpu-next` passes 38/38 including the OSD pixel diff.
- The app was rebuilt at `96331a9` and fully re-verified on the macOS host after the terminal UI work.
- `tests/run.sh suite`: 315 passed, 0 failed. The run includes the release gate.
- `scripts/audit.sh`: ok.
- `scripts/measure.sh final`: ok at `96331a9`; the host was quiet before and after the run.
- App: 28,437,647 B on disk; executable 9,707,520 B; framework closure 17,334,096 B in 13 files; bundled icon 1,387,652 B.
- One `file` protocol and one Lua interpreter.
- `--tui` plays in the terminal with centered embedded cover art. Kitty graphics are used on Ghostty, kitty, and WezTerm; other terminals use truecolor half-blocks. The TUI is audio-first and keeps `vo=null`.

## Releases

- `.github/workflows/release.yml` runs on a `v*` tag: the macOS and Linux build workflows run as reusable jobs, then one job publishes `Player-aarch64-apple-darwin.zip`, `player-x86_64-linux-musl.tar.gz`, and `SHA256SUMS`.
- `sh dl player` in `~/usr` installs the latest release on macOS and x86-64 Linux after verifying the checksum.
- `v0.41.0` is published on GitHub with `Player-aarch64-apple-darwin.zip`, `player-x86_64-linux-musl.tar.gz`, and `SHA256SUMS`; the macOS build, Linux build, and publish jobs all passed.

## Limits

- Local files only. No network protocol, TLS, CA bundle, or remote resource load.
- GUI, HDR and 10-bit color, subtitle shaping, Retina scaling, audio audibility, real mouse and drag, and signing or notarization are owner-run. See `docs/manual-checklist.md`.
- Clipboard support is removed. Encoding, recording, and the lavfi filter graph are deleted (sources, build rows, and registrations removed).
- The `subprocess` command and the `.run` script backend are reachable from explicit scripts and IPC. No built-in module uses them.
- The terminal UI has no sixel output and does not load external cover files; it renders the embedded picture only. Quit, Ctrl-C, and SIGTERM/SIGHUP restore the terminal.
- Follow-on: Rust-owned TUI and GUI paths, then removal of the legacy engine.
- Linux: glibc is explicitly out of scope. aarch64, X11, and a real-GPU, zero-copy VA-API, real-compositor, and real-ALSA check are outside the automated gates. The headless gate uses lavapipe with `--gpu-sw` and `mailbox` swap because headless Weston never releases FIFO buffers.

## Linux

- `scripts/linux-container.sh tests/run.sh suite`: passes on Alpine x86_64 with 307 checks, including `linux-gpu-next` under headless Weston with lavapipe, `linux-audio` on the ALSA null PCM, `linux-local-only` under `unshare -rn`, `linux-audit`, and `tui`.
- `scripts/audit-linux.sh`: ok. `dist/player` needs exactly nine libraries: musl, Vulkan, Wayland client and cursor, xkbcommon, ALSA, and libva, libva-drm, and libva-wayland.
- `--hwdec` defaults to `auto` on Linux and `no` on macOS. `suite` passes with and without a GPU render node in the container.
- `tests/run.sh linux-vaapi` (needs a GPU render node, not part of `suite`): passes on an AMD Raphael iGPU for h264, hevc, vp9, and av1 with `vaapi-copy`.
- Fixtures match the tracked stream structure; their bytes differ from macOS and between Linux hosts because the encoders select threads and SIMD paths at run time.
- Measurements are in `docs/metrics.md`; the runtime set, headless gates, and fixtures are in `docs/linux.md`.
