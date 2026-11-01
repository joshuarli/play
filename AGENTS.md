# Repository guide

Player is a local-only macOS media player: the mpv v0.41.0 playback pipeline, a minimal pinned FFmpeg n9.0.2, and a Rust-owned control plane. The release artifact is `dist/Player.app`.

## Commands

- `scripts/build.sh` builds the development player at `build/mpv`.
- `tests/run.sh suite` runs the full suite. `tests/run.sh <gate>` runs one gate.
- `scripts/audit.sh` runs the FFmpeg manifest, build-surface, and local-only audits.
- `scripts/build-deps.sh` builds the pinned release dependency closure under `.work/deps/release`.
- `scripts/release.sh` builds and bundles `dist/Player.app`.
- `make install` runs that build and copies the app to `~/Applications`.
- `scripts/measure.sh final` checks the release budgets.
- `cargo test --manifest-path rust/Cargo.toml` runs the Rust tests.

## Invariants

- Local input only. Network schemes, TLS, and network protocols are not built. The classifier is `rust/player-core/src/input.rs`.
- The player does not auto-load user config or scripts. Explicit scripts are trusted code with the full command table, including `subprocess` and `run`.
- One Lua implementation: the vendored Lua 5.2 in `rust/player-lua`.
- Rust owns startup configuration, playlist, command validation, scalar playback state, local file I/O, and the Lua host. C owns the playback pipeline, platform glue, and multimedia integration.
- `unsafe` is confined to `rust/player-ffi` and `rust/player-audio`. Each block states its safety argument.
- CI builds the macOS release on GitHub. Gates are local commands. Do not publish or notarize.
- `TODO.md` holds open work. Delete a ticket when it is done; git history is the record.

## Documents

- `docs/install.md`: build, run, and owner checks.
- `docs/architecture.md`: ownership boundaries, FFI, script host, renderer, release closure.
- `docs/scope.md`: retained and removed input surfaces, CLI, local-only boundary, debt.
- `docs/ffmpeg.md`: FFmpeg pin, allowlist, manifest gate.
- `docs/metrics.md`: measured baseline and release numbers.
- `docs/baseline.md`: P0 baseline record.
- `docs/status.md`: current state and evidence.
- `docs/final-report.md`: release outcome and known limits.
- `docs/licenses.md`: shipped licenses.
- `docs/manual-checklist.md`: owner-run visual and audio checks.
- `docs/lua-api.md`: generated Lua API audit.
- `docs/repo-audit.md`: dated security, license, and complexity evidence.
- `PLAN.md`: completed campaign record, not the current plan.
