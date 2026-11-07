# Repository guide

Player is a local-only media player built on a Rust playback engine, the pinned shared FFmpeg n9.0.2 runtime, libplacebo, and an mpv-compatible application surface. The macOS release artifact is `dist/Player.app`; the Linux x86-64 musl artifact is `dist/player-x86_64-linux-musl.tar.gz`. The Cargo workspace manifest is at the repository root and the crates are under `crates/`.

## Commands

- `scripts/build.sh` builds the development player at `build/mpv`. On macOS it and the Rust tests link `.work/deps/release`, so run `scripts/build-deps.sh` first.
- `tests/run.sh suite` runs the full suite. `tests/run.sh <gate>` runs one gate.
- `scripts/audit.sh` runs the FFmpeg manifest, build-surface, and local-only audits.
- `scripts/build-deps.sh` builds the pinned release dependency closure under `.work/deps/release`.
- `scripts/release.sh` builds and bundles `dist/Player.app`.
- `make install` copies the macOS app to `~/Applications` or the Linux binary to `~/.local/bin`.
- `scripts/measure.sh final` checks the release budgets.
- `make linux` builds the Linux dependency prefix and tarball in the pinned Alpine image; `scripts/linux-container.sh COMMAND` runs any command there, such as `tests/run.sh suite` or `scripts/audit-linux.sh`.
- `cargo test` runs the Rust tests.
- `build/mpv --tui FILE` runs the terminal player; `build/mpv FILE` renders through libplacebo's native Metal backend on macOS (`--gpu-api=vulkan` selects Vulkan/MoltenVK).

## Invariants

- Linux is x86-64 musl only. glibc is explicitly out of scope; do not add a glibc build or fallback.
- Subtitles are SubRip only, external `.srt` or embedded `subrip`. There is no ASS renderer, libass, FriBidi, or bundled font; text goes through Core Text on macOS and the system Fontconfig, HarfBuzz, and FreeType on Linux.
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
- [docs/perf-campaign.md](docs/perf-campaign.md): performance measurement and optimization campaign plan.
- [TODO.md](TODO.md): open work.
