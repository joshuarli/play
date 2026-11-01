# Status

Player is a local-only macOS media player. The release artifact is `dist/Player.app`.

## Current state

- The last full release measurement is at `7a8958b`.
- `tests/run.sh suite`: the full suite passes. The run includes the release gate.
- `scripts/audit.sh`: ok.
- `scripts/measure.sh final`: all published budgets pass.
- App: 26,807,168 B; executable 9,640,432 B; framework closure 17,166,736 B in 13 files.
- One `file` protocol and one Lua interpreter.

## Limits

- Local files only. No network protocol, TLS, CA bundle, or remote resource load.
- GUI, HDR and 10-bit color, subtitle shaping, Retina scaling, audio audibility, real mouse and drag, and signing or notarization are owner-run. See `docs/manual-checklist.md`.
- Clipboard commands are compiled and reachable. Encoding, recording, and the lavfi filter graph are compiled but unreachable because the release FFmpeg has no encoders or muxers.
- The `subprocess` command and the `.run` script backend are reachable from explicit scripts and IPC. No built-in module uses them.
- Follow-on: A/V scheduler, general demux and decode orchestration, renderer integration, and Linux.

## Evidence

- `docs/final-report.md`: release outcome, provenance, and limits.
- `docs/metrics.md`: measurement history and budget results.
- `docs/repo-audit.md`: security, license, and complexity audit.
- `TODO.md`: open defects and their fixes.

First command: `tests/run.sh suite && scripts/audit.sh`.
