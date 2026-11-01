# Resume package

## Goal

A local-only macOS mpv fork with a minimal FFmpeg and a Rust-owned control plane. Protocols are `file://` only; encode/record/lavfi and clipboard are documented debt.

## Current state

P0.1–P8.3 are accepted. P8.4 is the final documentation pass. The release app is `dist/Player.app`; the full suite, audit, and final budget gate are green.

## First command

```
tests/run.sh suite && scripts/audit.sh
```

## Build from a fresh checkout

```
scripts/build-deps.sh
scripts/release.sh
scripts/measure.sh final
```

`scripts/build-deps.sh` builds the pinned closure under `.work/deps/release`; it needs CMake, Meson, Ninja, pkg-config, Python 3, curl, and the Xcode tools. Homebrew is for build tools only. `scripts/release.sh` writes `dist/Player.app`.

## Evidence

- `tests/run.sh suite`: 292 passed.
- `scripts/audit.sh`: ok, including `cargo audit`.
- `scripts/measure.sh final`: all budgets pass; `docs/metrics.md` and `docs/final-report.md`.
- `docs/status.md` for the accepted-slice table.
- `docs/repo-audit.md` for the security/license/complexity audit.
- `docs/jank-audit.md` for open user-facing items.
- `docs/manual-checklist.md` for owner-run checks; use `scripts/manual-checklist.sh --release`.

## Open owner items

- Manual GUI, audio, subtitle, HDR/Retina, and real input checks.
- Signing/notarization if the app is distributed.
- Jank findings 1–2 (Open URL dead end, missing failure feedback) are not fixed.
- Follow-on boundary: A/V scheduler, demux/decode orchestration, renderer integration, Linux.
