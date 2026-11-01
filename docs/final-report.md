# Final report

## Outcome

This fork is a local-only macOS media player built on mpv v0.41.0, a minimal FFmpeg n9.0.2, and a Rust-owned control plane. The release artifact is `dist/Player.app`: a self-contained 26.8 MB app with a 9.64 MB executable, a 16.37 MiB framework closure, an arm64-thinned MoltenVK, and no Homebrew runtime dependency.

The campaign ran P0.1 through P8.3. P8.4 adds this report, the install/resume material, and the final documentation pass.

## What is owned by Rust

- **Configuration and startup:** typed `StartupConfig`; C applies supported fields and stops parsing them.
- **Playlist:** order, current entry, IDs, next/previous, remove/transfer; C entry pointers are mapped by opaque handles with talloc ownership preserved.
- **Input and commands:** parsed input vocabulary and validation; C executes supported commands.
- **State and events:** pause, fullscreen, mute, volume, speed, time-pos, duration, EOF, and the change decision; C property getters serve the Rust store.
- **Local file:** open/read/write/seek/size/EOF with typed errors; `stream_file` is a compact backend over the FFI handle.
- **Lua:** one mlua 0.12.2 Lua 5.2 host, embedded built-in modules, one interpreter. The C host is deleted.

The release app links the Rust plane as one `libplayer_release.dylib`; the dev build keeps staticlibs. Track-list payloads and the client event queue remain at the C API boundary.

## Verification

| Command | Result |
|---|---|
| `tests/run.sh suite` | 292 passed, 0 failed |
| `scripts/audit.sh` | ok |
| `scripts/measure.sh final` | final: ok (all P8 budgets) |
| `scripts/measure.sh compare` | compare: ok |
| `tests/run.sh release` | 7 passed, 0 failed |

P8 budgets:

| Budget | Actual |
|---|---|
| Distributable ≤ 50% of P0 (29,542,136 B) | 26,807,168 B |
| Executable ≤ 10 MB | 9,640,432 B |
| First frame ≤ 1 s | 220.07 ms |
| 1080p CPU ≤ baseline + 10% (0.231 s) | 0.23 s median |
| 1080p RSS ≤ 300 MB | 216.88 MB |
| Seek ≤ 250 ms | 3.21 ms |
| Frame drops | 0 |
| Network schemes | 0 |
| Duplicate Lua interpreters | 0 (one interpreter) |

Final measurement at `7a8958b`: cold 162.11 ms, warm 115.73 ms, idle RSS 84.78 MB, 1080p RSS 216.88 MB, 1080p CPU 0.23 s, 4K CPU 0.55 s, seek 3.21 ms, exit 4.24 ms, no audio underruns. These are headless timings; the host was loaded during the run, so the absolute values are evidence, not a product promise.

## Build and dependency provenance

- mpv v0.41.0, `41f6a645068483470267271e1d09966ca3b9f413`.
- FFmpeg n9.0.2, `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`, minimal allowlist, `file` protocol only.
- libass 0.17.5 (CoreText, no fontconfig), libplacebo 7.387.0, shaderc 2026.4 (MinSizeRel), dav1d 1.5.4, FreeType 2.15.0, FriBiDi 1.0.17, HarfBuzz 15.0.0, Vulkan-Headers/Loader 1.4.363.0, MoltenVK 1.4.2 (checksum-pinned binary).
- `scripts/build-deps.sh` pins source URLs and SHA-256 values and was exercised from a clean prefix in P8.1. Homebrew is used for build tools, not runtime dylibs.
- `scripts/release.sh` builds the Rust cdylib, configures against the release prefix, bundles the recursive closure, rewrites install names to `@rpath`, embeds the MoltenVK ICD, writes `Info.plist`, and ad-hoc signs the app.
- `player/main.c` sets `VK_DRIVER_FILES`/`VK_ICD_FILENAMES` from the bundle before renderer initialization.

## Known limitations and debt

- Local files only. No network protocol, encode/record/lavfi, or clipboard.
- GUI, HDR/10-bit color, subtitle shaping/positioning, Retina scaling, real audio audibility, real mouse/drag, and signing/notarization are owner-run (`docs/manual-checklist.md`).
- Two high-impact jank items are open: the `Open URL…` menu dead end and missing visible playback-failure feedback. See `docs/jank-audit.md`.
- The retained C still contains the `subprocess` command, `.run` script backend, and disabled cplugin loader; they are reachable only by trusted scripts, not by built-in code or the local-file path.
- Follow-on boundary: A/V scheduler, general demux/decode orchestration, renderer integration (7.6–7.8), and Linux.

## Reproduction

```
scripts/build-deps.sh
scripts/release.sh
tests/run.sh suite
scripts/audit.sh
scripts/measure.sh final
```

The development loop is `scripts/build.sh` plus `tests/run.sh suite`. Scratch artifacts live under `.work/`; `dist/` is disposable and rebuilt by `scripts/release.sh`.
