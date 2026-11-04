# Repository audit

Recorded 2026-11-03 after S9 and the C ABI cleanup: the C playback engine is retired and Cargo is the only player build.

## Layout

- `rust/player-core`: typed application logic (`config`, `playlist`, `input`, `command`, `state`, `local_file`).
- `rust/player-media`: FFmpeg demuxing and video decode.
- `rust/player-audio`: Symphonia decode, mixing, resampling, the audio clock, and the CoreAudio and ALSA sinks.
- `rust/player-engine`: scheduler, playback state machine, frame pacing, subtitles, and the session worker.
- `rust/player-ass` and `rust/player-output`: libass tracks and rendering, and the libplacebo/Vulkan renderer.
- `rust/player-display`: the macOS Cocoa/Metal bridge and the Linux Wayland window.
- `rust/player-tui`: the terminal renderer and input surface.
- `rust/player-app`: the player executable, CLI, IPC, playlists, properties and events, commands, the native OSC, GUI and TUI modes, Finder open, drag-and-drop, and screenshots.
- `rust/player-backend`: the Rust engine backend used by `player-app`. The retired C ABI, its headers, and its C test harness are gone.
- `tests/`: the IPC harness, suite gates, fixtures, and audit scripts.
- `scripts/`: build, release, measure, audit, and checklist entry points.
- `docs/`: baseline, architecture, FFmpeg, scope, metrics, licenses, and status.

## Build and test

- Development: `scripts/build.sh`, then `tests/run.sh suite`. The build is a Cargo `player-app` binary installed at `build/mpv`.
- Release: `scripts/release.sh` builds the `dist` Cargo profile and bundles `dist/Player.app`; `scripts/build-deps.sh` builds the pinned closure under `.work/deps/release`.
- Linux: `make linux` builds the musl prefix and tarball in the pinned Alpine image.
- Audit: `scripts/audit.sh` checks the FFmpeg manifest, build surface, local-only behavior, and release bundle; `scripts/measure.sh final` enforces budgets.
- Clean rebuild evidence: the dependency prefix was moved aside and rebuilt from the pinned tarballs, then the app was rebuilt and the release gate passed.

## Security and correctness

- **Protocols:** `--list-protocols` reports one scheme, `file`. FFmpeg is configured `--disable-network`; no curl, TLS, CA, or network demuxer is linked. `player-core::input::classify_input` rejects every remote URL and protocol wrapper at startup, IPC `loadfile`, playlist loading, playlist entries, and external files.
- **Dylibs:** the release closure contains no `/opt/homebrew` path and no `.work` build path. The app bundles the pinned closure, Vulkan loader, and MoltenVK. There is no Rust dylib and no libmpv.
- **Scripting:** none. `scripts/audit.sh` fails on any `lua_`/`luaL_` symbol in the development or release executable, any `mlua`, `lua-src`, or `luajit` crate in `Cargo.lock`, and any Lua API reference in `rust/`; `scripts/audit-linux.sh` checks the Linux executable.
- **Unexpected resources:** the only embedded resource is the OSD symbol font; there is no runtime resource extraction. User config is never read, and the gates pass `--no-config` where they can.
- **Subprocess:** the player spawns no process. The `subprocess` and `run` commands were removed with scripting.
- **Clipboard:** removed in S7a. No clipboard property, command, backend, bridge, binding, or Wayland selection sync remains in the tree.
- **Dependencies:** `cargo audit` reports no RustSec vulnerabilities for the 90 locked crates.
- **Licenses:** `docs/licenses.md` lists the shipped components. The Rust dependency graph is MIT/Apache-2.0/MPL-2.0/Zlib/Unicode-3.0; Symphonia is MPL-2.0 and its notices are preserved under `vendor/symphonia`.
- **Signature:** the app and every bundled dylib are ad-hoc signed; `codesign --verify --deep --strict` passes. Notarization and distribution signing are owner-run.
- **Local-only audit:** `scripts/audit.sh` checks the source tree and the linked symbols for a retired C engine, a second Lua, unsupported libraries, and network protocols. `scripts/audit-linux.sh` does the same for the musl artifact and rejects any `NEEDED` entry outside the documented set.

## Complexity accounting

- Rust: 26,576 lines in 82 files under `rust/`.
- C ABI: none. No `extern "C"` export, header, or C test harness remains.
- In-tree C: only the Objective-C window bridge.
- Tests: 37 gates in `tests/harness.py`; the suite passes on the recorded host.
- Distributable: 24.1 MB on disk; the executable is 7,600,640 B, the framework closure is 15,127,360 B in 12 dylibs, and the bundled icon is 1.39 MB.
- Linux: the stripped musl executable is 24,471,888 B and needs seven shared libraries.

## Known limits

This audit does not replace the owner's manual checklist for visual quality, audio audibility, hardware decode, real input devices, other hardware, or signing and notarization. Those remain scheduled, not silently passed. `TODO.md` records the defects found after this audit.

## Linux

- **Layout:** `docker/linux/Dockerfile` (pinned image), `scripts/{linux-container,linux-check,build-deps-linux,build-linux,audit-linux,release-linux,measure-linux}.sh`, `tests/wayland.py`, and `.github/workflows/linux.yml`.
- **Network:** `audit-linux.sh` finds no `getaddrinfo`, `connect`, TLS, or curl import or library string, exactly one `file` protocol, and no network scheme.
- **Dynamic set:** seven `NEEDED` entries (musl, libgcc, Vulkan loader, Wayland client, ALSA, libva, libva-drm), no RPATH or RUNPATH, every entry resolves, and no `/work/` or checkout path appears in the binary.
- **Unsafe:** confined to the native-boundary crates; each block states its safety argument.
- **Scripts:** the container wrapper runs with `seccomp=unconfined` and `apparmor=unconfined` so the local-only gate can create a user and network namespace. That widens the build container, which handles only repository code and pinned sources.
- **Advisories:** `cargo audit` was not run on Linux; it is not installed in the image.
