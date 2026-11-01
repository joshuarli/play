# Release report

## Outcome

This fork is a local-only macOS media player built on mpv v0.41.0, a minimal FFmpeg n9.0.2, and a Rust-owned control plane. The release artifact is `dist/Player.app`: a self-contained 28.2 MB app with a 9.64 MB executable, a 17,150,384 B framework closure, a bundled icon, an arm64-thinned MoltenVK, and no Homebrew runtime dependency.

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
| `tests/run.sh suite` | 303 passed, 0 failed |
| `scripts/audit.sh` | ok |
| `scripts/measure.sh final` | final: ok |
| `tests/run.sh release` | 10 passed, 0 failed |

| Budget | Limit | Actual |
|---|---:|---:|
| Distributable | 29,542,136 B | 26,790,480 B |
| Executable | 10,000,000 B | 9,640,096 B |
| First frame | 1 s | 154.3 ms |
| 1080p CPU | 0.231 s | 0.16 s |
| 1080p RSS | 300 MB | 216.09 MB |
| Seek | 250 ms | 23.99 ms |
| Network schemes | 0 | 0 |
| Duplicate Lua interpreters | 0 | 0 |

Sizes and timings are from the `cbb4ec2` rebuild.

The final measurement is at `cbb4ec2`: cold 72.01 ms, warm 72.78 ms, idle RSS 84.33 MB, 1080p RSS 216.09 MB, 1080p CPU 0.16 s, 4K CPU 0.43 s, seek 23.99 ms, exit 3.08 ms, no audio underruns. These are headless timings. The host was quiet before and after the run (`perf-gate-ready` exited 0).

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
- The `subprocess` command and the `.run` script backend are compiled and reachable from explicit scripts and IPC. No built-in module uses them.
- Follow-on: A/V scheduler, general demux and decode orchestration, renderer integration. Linux has its own section below.

## Reproduction

```sh
scripts/build-deps.sh
scripts/release.sh
tests/run.sh suite
scripts/audit.sh
scripts/measure.sh final
```

The development loop is `scripts/build.sh` plus `tests/run.sh suite`. Scratch artifacts live under `.work/`; `dist/` is disposable and rebuilt by `scripts/release.sh`.

## Linux x86-64 musl

### Outcome

The Linux build is a mostly static `x86_64-unknown-linux-musl` executable: the mpv pipeline, the pinned minimal FFmpeg n9.0.2, the Rust control plane as one staticlib, one Lua 5.2 interpreter, `vo=gpu-next` over Vulkan and Wayland, and ALSA. The release is `dist/player-x86_64-linux-musl.tar.gz` (player and README) with a SHA-256 file. The stripped executable is 26,355,208 B and needs exactly nine shared libraries: musl, `libvulkan.so.1`, `libwayland-client.so.0`, `libwayland-cursor.so.0`, `libxkbcommon.so.0`, `libasound.so.2`, and, for VA-API hardware decode, `libva.so.2`, `libva-drm.so.2`, and `libva-wayland.so.2`.

### Verification

| Command | Result |
|---|---|
| `scripts/linux-container.sh tests/run.sh suite` | 297 passed, 0 failed (includes `linux-gpu-next`, `linux-audio`, `linux-local-only`, `linux-osc`, `linux-audit`) |
| `scripts/linux-container.sh scripts/audit-linux.sh` | ok; the extra-`NEEDED` negative case failed as expected |
| `scripts/linux-container.sh tests/run.sh ffmpeg-audit` | 19 passed |
| `cargo test --target x86_64-unknown-linux-musl` | 60 passed |
| `make linux`, `make install` | tarball extracts, checksum matches, installed player runs |

| Budget | Limit | Actual |
|---|---:|---:|
| Stripped binary | 50,000,000 B | 26,355,208 B |
| Direct dynamic libraries | 9 (6 before VA-API) | 9 |
| 1080p RSS | 300 MB | 128.94 MB |
| Network schemes | 0 | 0 |
| Duplicate Lua interpreters | 0 | 0 |

Cold start, warm start, first frame, and seek are **perf gate deferred: host busy**. The host load was 25 on 32 cores from unrelated work for the final run, so the row in `docs/metrics.md` is labelled as not a gate. The numbers it recorded (cold 5.14 ms, warm 4.3 ms, first frame 105 ms, seek 1.07 ms) are far inside the budgets but are not evidence for them.

### Provenance

- The Linux source set (`wayland_common.c/.h`, `vulkan/context_wayland.c`, `ao_alsa.c`, `als-linux.c`, `language-posix.c`, `timer-linux.c`) comes from mpv v0.41.0 at the pin above.
- `docker/linux/Dockerfile` pins `alpine:edge` by digest. `scripts/build-deps-linux.sh` builds zlib 1.3.2, expat 2.9.0, dav1d 1.5.4, FreeType 2.15.0, FriBiDi 1.0.17, HarfBuzz 15.0.0, fontconfig 2.18.1, shaderc 2026.4, libplacebo 7.387.0, and libass 0.17.5 from SHA-256-pinned sources; FFmpeg n9.0.2 comes from `scripts/build-ffmpeg.sh` with the shared allowlist.

### Known limits

- musl systems only. A glibc system cannot load the Vulkan, Wayland, xkbcommon, and ALSA libraries. glibc is explicitly out of scope.
- Hardware decode is VA-API only and on by default (`--hwdec=auto`, Linux only); zero-copy import needs a real compositor and is owner-run. No X11, OpenGL, or DRM output.
- The headless gate uses lavapipe with `--gpu-sw=yes` and `--vulkan-swap-mode=mailbox`. FIFO pacing, fractional scale, real pointer and keyboard input, GPU output, and ALSA hardware are owner-run (`docs/manual-checklist.md`).
- Linux fixtures differ byte-for-byte from the macOS ones (`tests/fixtures/hashes-linux.json`); their structure is identical.
- `.github/workflows/linux.yml` has not run: nothing was pushed. The unprivileged user namespace the local-only gate needs may be blocked on a CI runner.
- Follow-on: aarch64 Linux and HDR output.

### Reproduction

```sh
make linux
scripts/linux-container.sh tests/run.sh suite
scripts/linux-container.sh scripts/audit-linux.sh
scripts/measure-linux.sh final
```
