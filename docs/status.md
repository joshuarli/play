# Status

Player is a local-only media player. The macOS release artifact is `dist/Player.app`.

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
- Follow-on: A/V scheduler, general demux and decode orchestration, and renderer integration. Linux x86-64 musl gpu-next is the active campaign (`PLAN.md`).
- Linux L0.1: `scripts/linux-check.sh` exits 1 on Alpine x86_64; `meson setup build-linux` stops at the missing Linux source set (`osdep/language-posix.c` first).
- Linux L0.3: `scripts/build-linux.sh` builds `build-linux/mpv` on Alpine x86_64 against system libraries; `--vo=help` lists `gpu-next`, `--gpu-context=help` lists `waylandvk`, `--ao=help` lists `alsa`. Dynamic link to system FFmpeg and libplacebo is interim until L1.

## Evidence

- `docs/final-report.md`: release outcome, provenance, and limits.
- `docs/metrics.md`: measurement history and budget results.
- `docs/repo-audit.md`: security, license, and complexity audit.
- `TODO.md`: open defects and their fixes.

First command: `tests/run.sh suite && scripts/audit.sh`.
