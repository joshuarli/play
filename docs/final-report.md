# Release report

## Outcome

This fork is a local-only media player built on a Rust playback engine, a minimal FFmpeg n9.0.2, libplacebo, and the mpv-compatible application surface ported in this repository. The release artifact is `dist/Player.app`: a self-contained app with a 7,074,160 B executable, a 15,127,360 B framework closure in 12 dylibs, a bundled icon, an arm64-thinned MoltenVK, and no Homebrew runtime dependency. The app is 23,597,313 B on disk.

The mpv-derived C playback engine is gone. Cargo builds the player on both platforms.

## Rust ownership

- **Playback engine:** `player-engine` owns the scheduler, clock, playback state, frame pacing, subtitles, and the session worker.
- **Media:** `player-media` owns FFmpeg demuxing and video decode; `player-audio` owns Symphonia decode, mixing, resampling, the audio clock, and the CoreAudio and ALSA sinks.
- **Output:** `player-ass` renders subtitles and the OSD bitmaps; `player-output` renders through libplacebo and Vulkan; `player-display` owns the macOS Cocoa/Metal window and the Linux Wayland window.
- **Application:** `player-app` owns the executable, CLI, IPC, playlists, chapters, track switching, properties and events, command dispatch, the native OSC, GUI and TUI modes, Finder open, drag-and-drop, and screenshots.
- **Scripting:** none. The on-screen controller is a typed Rust state machine in `rust/player-app/src/osc.rs` that renders ASS events through libass.
- **Native:** the Objective-C window bridge and the pinned native dependency closure. There is no mpv playloop, libmpv, or C player process.

## Verification

| Command | Result |
|---|---|
| `cargo test --manifest-path rust/Cargo.toml` | pass |
| macOS `tests/run.sh suite` | 367 passed, 0 failed |
| Linux `tests/run.sh suite` | 361 passed, 0 failed |
| `scripts/audit.sh` | ok |
| `scripts/audit-linux.sh` | ok |
| `scripts/release.sh` | `dist/Player.app` built |
| `make linux` | tarball built |
| `scripts/measure.sh final` | final: ok |

| Budget | Limit | Actual |
|---|---:|---:|
| Distributable | 29,542,136 B | 22,201,520 B |
| Executable | 10,000,000 B | 7,074,160 B |
| First frame | 1 s | 34.65 ms |
| 1080p CPU | no fixed gate | 2.960 s |
| 1080p RSS | 300 MB | 218.22 MB |
| Seek | 250 ms | 44.45 ms |
| Network schemes | 0 | 0 |
| Lua interpreters | 0 | 0 |

The final measurement is the clean tree at `6eb7184`, after scripting was removed: cold 17.56 ms, warm 15.90 ms, idle RSS 14.25 MB, 1080p RSS 218.22 MB, 1080p CPU 2.96 s, seek 44.45 ms, exit 10.41 ms, no audio underruns. These are headless timings and match the Lua build within noise. Interleaved against the previous release, headless 1080p CPU was 2.97 s against 2.98 s. In the GUI at 2560x1440 backing pixels, paused with the OSC shown, median CPU fell from 27.2% to 0.8%; hovering the seek bar fell from 28.3% to 10.2%; playing 1080p with the OSC hidden went from 14.2% to 13.8%; RSS was about 7 MB lower paused and 26 MB lower playing. `perf-gate-ready` exited 0 before and after each run.

## Build and provenance

- mpv v0.41.0, `41f6a645068483470267271e1d09966ca3b9f413`, remains the behavioral reference for the ported application surface; the native OSC is ported from mpv-derived Lua scripts and keeps their GPL-2.0-or-later obligation.
- FFmpeg n9.0.2, `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`, minimal allowlist, `file` protocol only.
- libass 0.17.5, libplacebo 7.387.0, shaderc 2026.4 with glslang, SPIRV-Tools, and SPIRV-Headers pins, dav1d 1.5.4, FreeType 2.15.0, FriBiDi 1.0.17, HarfBuzz 15.0.0, Vulkan-Headers and Loader 1.4.363.0, MoltenVK 1.4.2 from a checksum-pinned binary, and Symphonia at `b6336209` under MPL-2.0.
- `scripts/build-deps.sh` pins source URLs and SHA-256 values. Homebrew supplies build tools, not runtime dylibs.
- `scripts/release.sh` builds `player-app` with the `dist` Cargo profile and calls `scripts/bundle.py` to copy the non-system closure into `Player.app/Contents/Frameworks`, rewrite install names to `@rpath`, embed the MoltenVK ICD, write `Info.plist` and a build stamp, and ad-hoc sign the app.
- `.github/workflows/release.yml` runs the macOS and Linux builds on a `v*` tag and publishes `Player-aarch64-apple-darwin.zip`, `player-x86_64-linux-musl.tar.gz`, and `SHA256SUMS`. The app is ad-hoc signed, not notarized.

## Known limits

- Local files only. No network protocol, TLS, CA bundle, or remote resource load.
- The scheduler currently opens video decoders in software. `player-media` carries VideoToolbox and VA-API paths and tests; wiring the scheduler and reporting `hwdec-current` is `PLAY-20`.
- GUI visual quality, HDR and 10-bit color, subtitle shaping on real fonts, Retina scaling, audible audio, hardware decode, real mouse and drag, and signing or notarization are owner-run. See `docs/manual-checklist.md`.
- The UTF-8-only path API remains.
- No known reproducible correctness or memory-safety regression remains in the supported paths.

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

The Linux build is a mostly static `x86_64-unknown-linux-musl` executable with the same Rust engine and application layer as macOS: `vo=gpu-next` over Vulkan and Wayland, ALSA audio, and libva for hardware decode support. The release is `dist/player-x86_64-linux-musl.tar.gz` (player and README) with a SHA-256 file. The stripped executable is 23,820,624 B and needs seven shared libraries: musl, `libgcc_s.so.1`, `libvulkan.so.1`, `libwayland-client.so.0`, `libasound.so.2`, `libva.so.2`, and `libva-drm.so.2`. The final container measurement records cold 13.56 ms, warm 9.03 ms, first frame 63.32 ms, 1080p RSS 214.11 MB, seek 36.63 ms, and one `file` protocol.

### Verification

| Command | Result |
|---|---|
| `scripts/linux-container.sh tests/run.sh suite` | 361 passed, 0 failed |
| `scripts/linux-container.sh scripts/audit-linux.sh` | ok |
| `cargo test --target x86_64-unknown-linux-musl` | pass |
| `make linux` | tarball built |

| Budget | Limit | Actual |
|---|---:|---:|
| Stripped binary | 50,000,000 B | 23,820,624 B |
| Direct dynamic libraries | 7 | 7 |
| Network schemes | 0 | 0 |
| Lua interpreters | 0 | 0 |
| 1080p RSS | 300 MB | 214.11 MB |

### Provenance

- `docker/linux/Dockerfile` pins the Alpine image by digest. `scripts/build-deps-linux.sh` builds zlib 1.3.2, expat 2.9.0, dav1d 1.5.4, FreeType 2.15.0, FriBiDi 1.0.17, HarfBuzz 15.0.0, fontconfig 2.18.1, shaderc 2026.4, libplacebo 7.387.0, and libass 0.17.5 from SHA-256-pinned sources; FFmpeg n9.0.2 comes from `scripts/build-ffmpeg.sh` with the shared allowlist.
- `scripts/build-linux.sh` builds the musl `dist` profile with `-C target-feature=-crt-static`, because a static-pie with Wayland in `NEEDED` dies before `main`.

### Known limits

- musl systems only. glibc is explicitly out of scope.
- The scheduler requests software video decode. Real-GPU VA-API is owner-run; `tests/run.sh linux-hwdec-default` covers the software path in the container. No X11, OpenGL, or DRM output.
- The headless GPU gate uses lavapipe with `--gpu-sw=yes` and `--vulkan-swap-mode=mailbox`. FIFO pacing, fractional scale, real pointer and keyboard input, GPU output, and ALSA hardware are owner-run (`docs/manual-checklist.md`).
- Linux fixtures match the macOS stream structure; their bytes differ from the macOS ones and between Linux hosts.

### Reproduction

```sh
make linux
scripts/linux-container.sh tests/run.sh suite
scripts/linux-container.sh scripts/audit-linux.sh
scripts/measure-linux.sh final
```
