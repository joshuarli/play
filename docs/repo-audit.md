# Repository audit (P8.3)

## Layout

- `player/`, `stream/`, `demux/`, `filters/`, `sub/`, `video/`, `audio/`, `osdep/`, `input/`, `options/`, `misc/`: the retained C playback pipeline, platform glue, and multimedia integration.
- `rust/player-core`: pure typed control-plane logic (`config`, `playlist`, `input`, `command`, `state`, `local_file`).
- `rust/player-ffi`: the C boundary: startup, playlist handles, Lua backend, command check, state, local file. `unsafe` is confined here and to `player-audio`.
- `rust/player-lua`: the mlua 0.12.2 Lua 5.2 host and embedded built-in modules.
- `rust/player-audio`: Symphonia bridge.
- `rust/player-release`: release-only cdylib that carries both FFI surfaces.
- `tests/`: the IPC harness, suite gates, fixtures, and audit scripts.
- `scripts/`: build, release, measure, audit, and checklist entry points.
- `docs/`: baseline, architecture, FFmpeg, scope, Lua API, metrics, licenses, and closure evidence.

## Build and test

- Development: `scripts/build.sh`, then `tests/run.sh suite`.
- Release: `scripts/release.sh`; the pinned closure is built by `scripts/build-deps.sh` into `.work/deps/release`.
- Audit: `scripts/audit.sh`; final budgets: `scripts/measure.sh final` and `scripts/measure.sh compare`.
- Clean rebuild evidence: the dependency prefix was moved aside and rebuilt from the pinned tarballs in P8.1, then the app was rebuilt and the release gate passed.

## Security and correctness

- **Protocols:** `--list-protocols` reports one scheme, `file`. FFmpeg is configured `--disable-network`; no curl, TLS, CA, or network demuxer is linked. The URL matrix rejects network URLs before the stream layer.
- **Dylibs:** the release closure contains no `/opt/homebrew` path and no `.work` build path; `tests/run.sh release` scans every bundled dylib. The app bundles the pinned closure, Vulkan loader, MoltenVK, and the Rust cdylib.
- **Lua:** exactly one `luaL_newstate` exists in the release path (inside `libplayer_release.dylib`). The C host, external Lua library, and generated `.inc` host are deleted. The interpreter does not open `debug` or C-module loading.
- **Unexpected resources:** built-in Lua modules are embedded from `player/lua/`; there is no runtime resource extraction. User config/scripts are ordinary mpv behavior and are disabled by `--no-config` in all gates.
- **Subprocess and dlopen:** the retained C code still contains the mpv `subprocess` command, the `.run` script backend (`load_run`), and the cplugin loader (`load_cplugin`, unreachable with `cplugins` disabled). These are reachable only by trusted scripts, not by the built-in OSC or the local-file path. They are not a network capability and are recorded as accepted trust-boundary debt, not new exposure.
- **Dependencies:** `cargo audit` (RustSec) reports no vulnerabilities for the 58 locked crates.
- **Licenses:** `docs/licenses.md` lists the shipped components. The Rust dependency graph is MIT/Apache-2.0/MPL-2.0/Zlib/Unicode-3.0; Symphonia is MPL-2.0 and its notices are preserved under `vendor/symphonia`.
- **Signature:** the app and every bundled dylib are ad-hoc signed; `codesign --verify --deep --strict` passes. Notarization and distribution signing are owner-run.

## Complexity accounting

- Retained C/H: 216 C files; **137,228** lines under the retained source roots (includes upstream mpv internals kept for the playback pipeline).
- Rust: **6,685** lines in 22 files under `rust/`; by crate: core 1,648, FFI 2,309, Lua 2,401, audio 318, release 2.
- FFI: **49** exported `player_ffi_*`/`player_audio_*` symbols; 49 `#[unsafe(no_mangle)]` declarations.
- Tests: 23 gates in `tests/harness.py`; the full suite reports 292 cases.
- Distributable: **26.8 MB** (9.64 MB executable + 16.37 MiB closure in 13 files).

## Known limits

The audit does not replace the owner's manual checklist for visual quality, audio audibility, real input devices, other hardware, or signing/notarization. Those remain scheduled, not silently passed.
