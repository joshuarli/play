# Rust Port: Migration Charter

Reference commit: `ee5021c` (tree clean except staged campaign doc at preflight).
Preflight doc: `campaign-rust.md`. This file is the durable resume point.

## Current phase

S0 (ground and oracle). Next: S1 (Rust owns the control path).

## Rust-owned components

- Startup parsing and validation: `player-core::config`, `command` (subset), `state`, `playlist`, `input`, `local_file`.
- FFI surface: `rust/player-ffi` (command, state, playlist, local file, startup, TUI, Lua, mpv handles).
- Audio decode bridge: `rust/player-audio` (Symphonia packets to PCM).
- Script host: `rust/player-lua` (mlua, Lua 5.2, vendored).
- Terminal UI: `rust/player-tui` (renderer; engine still C).

## Remaining mpv C dependencies

Everything else: `player/` playloop, audio, video, command, demux orchestration,
`demux/`, `stream/`, `sub/`, `video/`, `audio/`, `filters/`, `osdep/` platform
glue, client API, IPC, OSD/OSC delivery.

## S0 baseline (reference: legacy engine at `ee5021c`)

Host: macOS 27.0.1 arm64, Xcode 27.2, SDK 27.0, Meson 1.12.1, Python 3.15.1,
Rust nightly-1.100.0 (`nightly-2026-09-15`). `perf-gate-ready` exit 0.

- `cargo test --manifest-path rust/Cargo.toml`: pass (~1 s warm).
- `tests/run.sh suite`: 316 passed, 0 failed (~2m20s).
- `scripts/audit.sh`: ok (~27 s).
- `scripts/measure.sh final`: ok (~1m17s).
  - cold 68.31 ms (spread 10.38), warm 66.83 ms (spread 2.51)
  - first frame 1080p 154.31 ms (spread 11.16)
  - idle RSS 83.84 MB, 1080p RSS 213.31 MB, CPU 0.26 s (spread 0.13)
  - seek 28.99 ms, exit 6.37 ms, audio underruns 0
  - executable 9,440,304 B, distributable 26,774,400 B, budgets pass
  - protocols 1 (`file`), network 0, one Lua interpreter
- Linux (Alpine x86-64 container): `make linux` ok (~39 s, deps cached),
  `tests/run.sh suite` 310 passed 0 failed (~2m4s),
  `scripts/audit-linux.sh` ok (~2 s).
- `tests/run.sh ffi`: 3 passed (~16 s).
- `tests/trace.py --check`: 33/33 goldens match, stable over 6 consecutive
  macOS runs and green in the Linux container.

Oracle traces: `tests/trace.py` drives the reference binary over all 34
fixtures (21 corpus, 3 audio, 3 subtitle via companions, 2 cover, 3 failure;
2 local-path fixtures stay with the `local-paths` gate). Goldens in
`tests/traces/` (33 files, committed). `traces` gate runs in both suites.
Known reference nondeterminism, normalized in `canon_order` with a
`--selftest`: seek/playback-restart adjacency, doubled playback-restart,
post-EOF teardown events, macOS-only startup `idle`, duplicate pause
property-change delivery. Pause behavior asserts the exact True-to-False
transition; event order asserts kinds and pre-EOF order.

Fixture corpus: reproducible from `tests/fixtures/gen.sh` plus
`tests/fixtures/manifest.json`. Measurement fixtures under `.work/fixtures/`.

## Known platform gaps

- Real-GPU VA-API (`linux-vaapi` gate, needs render node), VideoToolbox
  hardware paths, audible output, HDR/color, manual-checklist owner runs:
  all on the Needs environment list, none blocking.
- Non-UTF-8 path arguments rejected before the classifier (open, unchanged).
- Clipboard removal lands in S7 (owner decision 2026-11-03).

## perf-gate-ready

Exit 0 before the S0 measurements above. A busy host defers measurement;
a deferred run never counts as a pass.
