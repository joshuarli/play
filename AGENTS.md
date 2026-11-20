# Repository guide

Player is a local-only media player built on a Rust playback engine, the pinned shared FFmpeg n9.0.2 runtime, libplacebo, and an mpv-compatible application surface. The macOS release artifact is `dist/Player.app`; the Linux x86-64 musl artifact is `dist/player-x86_64-linux-musl.tar.xz`. The Cargo workspace manifest is at the repository root and the crates are under `crates/`.

## Commands

- `scripts/build.sh` builds the development player at `build/mpv` on macOS or `build-linux/mpv` on Linux. On macOS it and the Rust tests link `.work/deps/release`, so run `scripts/build-deps.sh` first; use the pinned container for Linux.
- `tests/run.sh suite` runs the full suite. `tests/run.sh <gate>` runs one gate.
- `scripts/audit.sh` runs the macOS FFmpeg manifest, build-surface, and local-only audits; use `scripts/audit-linux.sh` in the Linux container.
- `scripts/build-deps.sh` builds the pinned macOS release dependency closure under `.work/deps/release`.
- `scripts/release.sh` builds and bundles `dist/Player.app`.
- `make install` installs the macOS app and runtime under `~/Applications/Player`, or the Linux binary and libraries under `~/.local/opt/player` with a `~/.local/bin/player` link. See the install guide for overrides.
- `scripts/measure.sh final` checks the release budgets.
- `make linux` builds the Linux dependency prefix and tarball in the pinned Alpine image; `scripts/linux-container.sh COMMAND` runs any command there, such as `tests/run.sh suite` or `scripts/audit-linux.sh`.
- `cargo test` runs the Rust tests.
- `build/mpv --tui FILE` runs the terminal player; `build/mpv FILE` renders through libplacebo's native Metal backend on macOS (`--gpu-api=vulkan` selects Vulkan/MoltenVK).

## Verification

- Costs (M-series Mac, busy host, observations): `make test` 202 s warm; cold (empty `.work` and `target`) 481 s before it built the release app, which adds 287 s; it runs `build/mpv`, `scripts/release.sh`, then `tests/run.sh suite` (489 checks, workspace `cargo test` inside the `rust-link` gate). `make lint` 41 s warm, 187 s cold. Python unit tests are outside `make test`: `python3 -m unittest discover -s tests -p 'test_*.py'` 235 s, `-s scripts` 1 s (DSLP-19).
- Known red: two `fixtures` byte-hash checks for corrupt, hevc and mpeg4 fail with the host's Homebrew x265 and lame, also at base a4d901f (DSLP-18). Linux compares fixture structure only.
- CI: `ci.yml` (Linux musl container) runs `make test` on pull requests; `lint.yml` runs `make lint`; `macos.yml` builds and verifies the app and runs no tests.
- Guards by boundary (gates are `tests/run.sh GATE`; unit tests ship in each crate):
  - CLI flags, help text, exit codes: gates `cli`, `cli-help`; `crates/player-app/src/options.rs` tests.
  - Local-only input and wrappers: `local-only`, `local-paths`; `crates/player-core/src/input.rs` tests; `scripts/audit.sh`.
  - Commands, properties, JSON IPC socket (`--input-ipc-server`): `state`, `input`, `dispatch`, `osc-input`; `ipc.rs` tests. Malformed or oversized IPC lines: none.
  - Playlists (m3u, m3u8, pls, directories): `playlist` gate; `crates/player-app/src/playlist.rs` tests. Non-UTF-8, BOM or oversized playlist files: none.
  - Container and codec decode: `corpus`, `audio`, `cover-art`, `seek` over `tests/fixtures/manifest.json`; truncated and corrupt files: `failures`. Mutated or fuzzed media: none.
  - Release artifacts: `release` gate, `tests/test_release_archives.py`, `scripts/audit-linux.sh`.
  - Audio devices: ALSA null PCM in `linux-audio`; CoreAudio and real devices: none, owner checks in `docs/manual-checklist.md`.
- Performance workload: `scripts/measure.sh final` against the budgets in `docs/metrics.md` (CPU and RSS during playback), plus `scripts/bench.py`. Owner-run, not part of `make test`.
- Components (first-party lines; the repository is over 50k with vendored code): playback (14.2k: player-core, -media, -audio, -engine, -backend), presentation (8.1k: player-output, -display, -overlay), application (9.3k: player-app, -tui, -gui, -build), harness (9.6k: tests, scripts, docker, native).
- Vendored trees (`UPSTREAM.md` in each): `vendor/rusty-aac` is crates.io 2.0.1 from Remade-With-Rust/remade_ffmpeg_rs at `14e8508`, pristine; `vendor/ruopus` is crates.io 0.1.2 from jmg049/ruopus at `b7aab67`, modified (FFT); `vendor/symphonia` is joshuarli/symphonia at `b633620` (derived from pdeljanov/Symphonia 0.6.1), modified (AAC layouts, surround Opus). Fork differences were not diffed.

## Invariants

- Linux is x86-64 musl only. glibc is explicitly out of scope; do not add a glibc build or fallback.
- Subtitles are SubRip only, external `.srt` or embedded `subrip`. There is no ASS renderer, libass, FriBidi, or bundled font; text goes through Core Text on macOS and, on Linux, Laputa's shared rust-fontconfig library for font discovery and fallback with the system HarfBuzz and FreeType.
- Local input only. Network schemes, TLS, and network protocols are not built. The classifier is `crates/player-core/src/input.rs`.
- The player does not auto-load user config. There is no scripting: no interpreter, script host, or script API. The on-screen controller is native Rust in `crates/player-app/src/osc.rs`.
- Rust owns the playback pipeline (demux, decode, scheduling, audio, video rendering, subtitles, OSD), startup configuration, playlist, command validation, playback state, local file I/O, the application surface, and the OSC. The only C in-tree code is the macOS window bridge; everything else is a pinned native dependency.
- Native-boundary `unsafe` lives in `player-media`, `player-output`, `player-display`, `player-overlay`, `player-engine`, and `player-audio`. Each block states its safety argument.
- CI builds both release artifacts on GitHub. A `v*` tag runs `.github/workflows/release.yml`, which publishes the macOS app zip and the Linux tarball with `SHA256SUMS`. Gates are local commands. Do not notarize.
- `TODO.md` holds open work. Delete a ticket when it is done; git history is the record.

## Documents

- [docs/install.md](docs/install.md): build, run, release artifacts, and owner checks.
- [docs/linux.md](docs/linux.md): Linux runtime set, container, headless gates, ALSA, fixtures.
- [docs/architecture.md](docs/architecture.md): ownership boundaries, playback pipeline, OSC, renderer, release closure.
- [docs/scope.md](docs/scope.md): input boundary, retained and removed modules, CLI.
- [docs/ffmpeg.md](docs/ffmpeg.md): FFmpeg runtime pin, installed layout, fetch, link, and gates.
- [docs/metrics.md](docs/metrics.md): measurement history and release budgets.
- [docs/manual-checklist.md](docs/manual-checklist.md): owner-run visual and audio checks.
- [docs/tui-design.md](docs/tui-design.md): terminal UI layout, art transport, and limits.
- [docs/rfc/PLAY-30-osc-coordinates.md](docs/rfc/PLAY-30-osc-coordinates.md): draft OSC coordinate and injected-input contract.
- [TODO.md](TODO.md): open work.
