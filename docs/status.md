# Status

Player is a local-only media player. The macOS release artifact is `dist/Player.app`; the Linux x86-64 musl artifact is `dist/player-x86_64-linux-musl.tar.gz`.

## Current state

- The last full release measurement is at `7a8958b`. The app was rebuilt after the backlog; its sizes are below.
- `tests/run.sh suite`: the full suite passes. The run includes the release gate.
- `scripts/audit.sh`: ok.
- `scripts/measure.sh final`: the published timing budgets pass at `7a8958b`.
- App: 28,186,443 B; executable 9,640,096 B; framework closure 17,150,320 B in 13 files; bundled icon 1,387,652 B.
- One `file` protocol and one Lua interpreter.

## Limits

- Local files only. No network protocol, TLS, CA bundle, or remote resource load.
- GUI, HDR and 10-bit color, subtitle shaping, Retina scaling, audio audibility, real mouse and drag, and signing or notarization are owner-run. See `docs/manual-checklist.md`.
- Clipboard commands are compiled and reachable. Encoding, recording, and the lavfi filter graph are compiled but unreachable because the release FFmpeg has no encoders or muxers.
- The `subprocess` command and the `.run` script backend are reachable from explicit scripts and IPC. No built-in module uses them.
- Follow-on: A/V scheduler, general demux and decode orchestration, and renderer integration.
- Linux: glibc is explicitly out of scope. aarch64, X11, and a real-GPU, zero-copy VA-API, real-compositor, and real-ALSA check are outside the automated gates. The headless gate uses lavapipe with `--gpu-sw` and `mailbox` swap because headless Weston never releases FIFO buffers.

## Linux

- `scripts/linux-container.sh tests/run.sh suite`: passes on Alpine x86_64, including `linux-gpu-next` under headless Weston with lavapipe, `linux-audio` on the ALSA null PCM, `linux-local-only` under `unshare -rn`, and `linux-audit`.
- `scripts/audit-linux.sh`: ok. `dist/player` needs exactly nine libraries: musl, Vulkan, Wayland client and cursor, xkbcommon, ALSA, and libva, libva-drm, and libva-wayland.
- `tests/run.sh linux-vaapi` (needs a GPU render node, not part of `suite`): passes on an AMD Raphael iGPU for h264, hevc, vp9, and av1 with `vaapi-copy`.
- Fixtures regenerate deterministically with Alpine's FFmpeg 9.0.2; their hashes differ from macOS (`tests/fixtures/hashes-linux.json`).
- Measurements are in `docs/metrics.md`; the runtime set, headless gates, and fixtures are in `docs/linux.md`.

## Evidence

- `docs/final-report.md`: release outcome, provenance, and limits.
- `docs/metrics.md`: measurement history and budget results.
- `docs/repo-audit.md`: security, license, and complexity audit.
- `TODO.md`: open defects and their fixes.

First command: `tests/run.sh suite && scripts/audit.sh`.
