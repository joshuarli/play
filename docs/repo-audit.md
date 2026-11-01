# Repository audit

Recorded 2026-11-01 at `7a8958b`.

## Layout

- `player/`, `stream/`, `demux/`, `filters/`, `sub/`, `video/`, `audio/`, `osdep/`, `input/`, `options/`, `misc/`: the retained C playback pipeline, platform glue, and multimedia integration.
- `rust/player-core`: pure typed control-plane logic (`config`, `playlist`, `input`, `command`, `state`, `local_file`).
- `rust/player-ffi`: the C boundary for startup, playlist handles, the Lua backend, command checking, state, and local files. `unsafe` is confined here and to `player-audio`.
- `rust/player-lua`: the mlua 0.12.2 Lua 5.2 host and embedded built-in modules.
- `rust/player-audio`: the Symphonia bridge.
- `rust/player-release`: the release-only cdylib that carries both FFI surfaces.
- `tests/`: the IPC harness, suite gates, fixtures, and audit scripts.
- `scripts/`: build, release, measure, audit, and checklist entry points.
- `docs/`: baseline, architecture, FFmpeg, scope, Lua API, metrics, licenses, and closure evidence.

## Build and test

- Development: `scripts/build.sh`, then `tests/run.sh suite`.
- Release: `scripts/release.sh`; the pinned closure is built by `scripts/build-deps.sh` into `.work/deps/release`.
- Audit: `scripts/audit.sh` runs the FFmpeg manifest, build, local-only, and release gates; budgets: `scripts/measure.sh final` and `scripts/measure.sh compare`.
- Clean rebuild evidence: the dependency prefix was moved aside and rebuilt from the pinned tarballs, then the app was rebuilt and the release gate passed.

## Security and correctness

- **Protocols:** `--list-protocols` reports one scheme, `file`. FFmpeg is configured `--disable-network`; no curl, TLS, CA, or network demuxer is linked. The URL matrix rejects network URLs before the stream layer. `--playlist` and `loadlist` bypass the classifier and rely on the stream layer, which fails closed on network schemes.
- **Dylibs:** the release closure contains no `/opt/homebrew` path and no `.work` build path. The app bundles the pinned closure, Vulkan loader, MoltenVK, and the Rust cdylib.
- **Lua:** exactly one `luaL_newstate` exists in the release path inside `libplayer_release.dylib`. The C host, external Lua library, and generated `.inc` host are deleted. The interpreter opens `io`, `os`, and `package`; it does not open `debug`.
- **Unexpected resources:** built-in Lua modules are embedded from `player/lua/`; there is no runtime resource extraction. User config and scripts are disabled by default and the gates pass `--no-config` where they can.
- **Subprocess and dlopen:** the retained C code contains the mpv `subprocess` command and the `.run` script backend (`load_run`). Both are reachable from an explicit script or an IPC client. No built-in module uses them. The cplugin loader is compiled out. These are local process capabilities, not a network capability.
- **Clipboard:** the clipboard property, command, and macOS bridge are compiled and reachable. The default `Ctrl+v` binding uses them.
- **Dependencies:** `cargo audit` reports no RustSec vulnerabilities for the 58 locked crates.
- **Licenses:** `docs/licenses.md` lists the shipped components. The Rust dependency graph is MIT/Apache-2.0/MPL-2.0/Zlib/Unicode-3.0; Symphonia is MPL-2.0 and its notices are preserved under `vendor/symphonia`.
- **Signature:** the app and every bundled dylib are ad-hoc signed; `codesign --verify --deep --strict` passes. Notarization and distribution signing are owner-run.

## Complexity accounting

- Retained C and headers: 216 C files and 137,228 lines under the retained source roots. This includes upstream mpv internals kept for the playback pipeline.
- Rust: 6,685 lines in 22 files under `rust/`. By crate: core 1,648, FFI 2,309, Lua 2,401, audio 318, release 2.
- FFI: 49 exported `player_ffi_*` and `player_audio_*` symbols; 49 `#[unsafe(no_mangle)]` declarations.
- Tests: 23 gates in `tests/harness.py`; the suite passes on the recorded host.
- Distributable: 28.2 MB; the executable is 9.64 MB, the framework closure is 16.36 MiB in 13 files, and the bundled icon is 1.39 MB.

## Known limits

This audit does not replace the owner's manual checklist for visual quality, audio audibility, real input devices, other hardware, or signing and notarization. Those remain scheduled, not silently passed. `TODO.md` records the defects found after this audit.
