# Release report

## Outcome

This fork is a local-only macOS media player built on mpv v0.41.0, a minimal FFmpeg n9.0.2, and a Rust-owned control plane. The release artifact is `dist/Player.app`: a self-contained 26.8 MB app with a 9.64 MB executable, a 17,166,736 B framework closure, an arm64-thinned MoltenVK, and no Homebrew runtime dependency.

## Rust ownership

- **Configuration and startup:** typed `StartupConfig`; C applies the supported fields and stops parsing them.
- **Playlist:** order, current entry, IDs, next and previous, remove and transfer. C entry pointers are mapped through opaque handles with talloc ownership preserved.
- **Input and commands:** parsed input vocabulary and validation. C executes the supported commands.
- **State and events:** pause, fullscreen, mute, volume, speed, time-pos, duration, EOF, and the change decision. C property getters serve the Rust store.
- **Local file:** open, read, write, seek, size, and EOF with typed errors. `stream_file` is a compact backend over the FFI handle.
- **Lua:** one mlua 0.12.2 Lua 5.2 host with embedded built-in modules and one interpreter.

The release app links the Rust plane as `libplayer_release.dylib`. The development build keeps static libraries. Track-list payloads and the client event queue remain at the C API boundary.

## Verification

| Command | Result |
|---|---|
| `tests/run.sh suite` | 292 passed, 0 failed |
| `scripts/audit.sh` | ok |
| `scripts/measure.sh final` | final: ok |
| `scripts/measure.sh compare` | compare: ok |
| `tests/run.sh release` | 7 passed, 0 failed |

| Budget | Limit | Actual |
|---|---:|---:|
| Distributable | 29,542,136 B | 26,807,168 B |
| Executable | 10,000,000 B | 9,640,432 B |
| First frame | 1 s | 220.07 ms |
| 1080p CPU | 0.231 s | 0.23 s, inside run noise |
| 1080p RSS | 300 MB | 216.88 MB |
| Seek | 250 ms | 3.21 ms |
| Network schemes | 0 | 0 |
| Duplicate Lua interpreters | 0 | 0 |

The final measurement is at `7a8958b`: cold 162.11 ms, warm 115.73 ms, idle RSS 84.78 MB, 1080p RSS 216.88 MB, 1080p CPU 0.23 s, 4K CPU 0.55 s, seek 3.21 ms, exit 4.24 ms, no audio underruns. These are headless timings. The host was loaded during the run, so the absolute values are evidence, not a product promise.

## Build and provenance

- mpv v0.41.0, `41f6a645068483470267271e1d09966ca3b9f413`.
- FFmpeg n9.0.2, `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`, minimal allowlist, `file` protocol only.
- libass 0.17.5 with CoreText and no fontconfig, libplacebo 7.387.0, shaderc 2026.4 (MinSizeRel), dav1d 1.5.4, FreeType 2.15.0, FriBiDi 1.0.17, HarfBuzz 15.0.0, Vulkan-Headers and Loader 1.4.363.0, MoltenVK 1.4.2 from a checksum-pinned binary.
- `scripts/build-deps.sh` pins source URLs and SHA-256 values. Homebrew supplies build tools, not runtime dylibs.
- `scripts/release.sh` builds the Rust cdylib, configures against the release prefix, bundles the recursive closure, rewrites install names to `@rpath`, embeds the MoltenVK ICD, writes `Info.plist`, and ad-hoc signs the app.
- `player/main.c` sets `VK_DRIVER_FILES` and `VK_ICD_FILENAMES` from the bundle before renderer initialization.

## Known limits

- Local files only. No network protocol, TLS, CA bundle, or remote resource load.
- GUI, HDR and 10-bit color, subtitle shaping, Retina scaling, audio audibility, real mouse and drag, and signing or notarization are owner-run. See `docs/manual-checklist.md`.
- Encoding, recording, and the lavfi filter graph are compiled but unreachable: the release FFmpeg has no encoders or muxers, and the CLI has no entry point.
- Clipboard commands are compiled and reachable. The default `Ctrl+v` binding uses them.
- The `subprocess` command and the `.run` script backend are compiled and reachable from explicit scripts and IPC. `select.lua`, loaded by default, uses both; the built-in OSC does not.
- The current bundle omits `MPVBUNDLE`, so the Cocoa layer runs it as an accessory app. `TODO.md` tracks the fix.
- Follow-on: A/V scheduler, general demux and decode orchestration, renderer integration, and Linux.

## Reproduction

```sh
scripts/build-deps.sh
scripts/release.sh
tests/run.sh suite
scripts/audit.sh
scripts/measure.sh final
```

The development loop is `scripts/build.sh` plus `tests/run.sh suite`. Scratch artifacts live under `.work/`; `dist/` is disposable and rebuilt by `scripts/release.sh`.
