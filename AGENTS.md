# Repository guide

Player is a local-only media player built on the mpv v0.41.0 playback pipeline, a minimal pinned FFmpeg n9.0.2, and a Rust-owned control plane. The macOS release artifact is `dist/Player.app`; the Linux x86-64 musl artifact is `dist/player-x86_64-linux-musl.tar.gz`.

## Commands

- `scripts/build.sh` builds the development player at `build/mpv`.
- `tests/run.sh suite` runs the full suite. `tests/run.sh <gate>` runs one gate.
- `scripts/audit.sh` runs the FFmpeg manifest, build-surface, and local-only audits.
- `scripts/build-deps.sh` builds the pinned release dependency closure under `.work/deps/release`.
- `scripts/release.sh` builds and bundles `dist/Player.app`.
- `make install` copies the macOS app to `~/Applications` or the Linux binary to `~/.local/bin`.
- `scripts/measure.sh final` checks the release budgets.
- `make linux` builds the Linux dependency prefix and tarball in the pinned Alpine image; `scripts/linux-container.sh COMMAND` runs any command there, such as `tests/run.sh suite` or `scripts/audit-linux.sh`.
- `cargo test --manifest-path rust/Cargo.toml` runs the Rust tests.
- `build/mpv --tui FILE` runs the terminal player.

## Invariants

- Linux is x86-64 musl only. glibc is explicitly out of scope; do not add a glibc build or fallback.
- Local input only. Network schemes, TLS, and network protocols are not built. The classifier is `rust/player-core/src/input.rs`.
- The player does not auto-load user config or scripts. Explicit scripts are trusted code with the full command table, including `subprocess` and `run`.
- One Lua implementation: the vendored Lua 5.2 in `rust/player-lua`.
- Rust owns startup configuration, playlist, command validation, scalar playback state, local file I/O, and the Lua host. C owns the playback pipeline, platform glue, and multimedia integration.
- `unsafe` is confined to `rust/player-ffi` and `rust/player-audio`. Each block states its safety argument.
- CI builds both release artifacts on GitHub. A `v*` tag runs `.github/workflows/release.yml`, which publishes the macOS app zip and the Linux tarball with `SHA256SUMS`. Gates are local commands. Do not notarize.
- `TODO.md` holds open work. Delete a ticket when it is done; git history is the record.

## Documents

- [docs/install.md](docs/install.md): build, run, release artifacts, and owner checks.
- [docs/linux.md](docs/linux.md): Linux runtime set, container, headless gates, ALSA, fixtures.
- [docs/architecture.md](docs/architecture.md): ownership boundaries, FFI, script host, renderer, release closure.
- [docs/scope.md](docs/scope.md): input boundary, retained and removed modules, CLI, debt.
- [docs/ffmpeg.md](docs/ffmpeg.md): FFmpeg pin, allowlist, manifest gate.
- [docs/metrics.md](docs/metrics.md): measurement history and release budgets.
- [docs/status.md](docs/status.md): current state and evidence.
- [docs/final-report.md](docs/final-report.md): release outcome and known limits.
- [docs/baseline.md](docs/baseline.md): P0 baseline record.
- [docs/repo-audit.md](docs/repo-audit.md): dated security, license, and complexity audit.
- [docs/licenses.md](docs/licenses.md): shipped licenses.
- [docs/manual-checklist.md](docs/manual-checklist.md): owner-run visual and audio checks.
- [docs/lua-api.md](docs/lua-api.md): generated Lua API closure.
- [docs/tui-design.md](docs/tui-design.md): terminal UI layout, art transport, and limits.
- [TODO.md](TODO.md): open work.
