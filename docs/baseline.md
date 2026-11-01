# Baseline

This records the P0 baseline. The current state is in `docs/status.md`.

Imported mpv **v0.41.0** at upstream commit `41f6a645068483470267271e1d09966ca3b9f413`.

FFmpeg pin: **n9.0.2** at upstream commit `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`.

Upstream mpv's `DOCS/` directory is relocated to `doc/` because macOS is case-insensitive and the repository's `docs/` cannot coexist with it. Local references were updated; inherited `.github` workflows still reference `DOCS/` and are not run.

## Upstream pins

| Component | Version | Commit |
|---|---|---|
| mpv | v0.41.0 | `41f6a645068483470267271e1d09966ca3b9f413` |
| FFmpeg | n9.0.2 | `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3` |

The mpv fork is GPL-2.0-or-later. The repository history was later consolidated to the `campaign-base` root commit; the pins above are the provenance. The checks do not push or distribute.

## Dependencies

Pinned Homebrew development dependencies:

| Package | Version |
|---|---|
| ffmpeg | 9.0.2_1 |
| libplacebo | 7.387.0 |
| libass | 0.17.5_1 |
| molten-vk | 1.4.2 |
| vulkan-loader | 1.4.363.0 |
| vulkan-headers | 1.4.363.0 |
| shaderc | 2026.4 |
| dav1d | 1.5.4 |
| meson | 1.12.1 |
| ninja | 1.13.2 |
| pkgconf | 3.0.10 |

The P0 baseline built Lua **5.2.4** from the official tarball (SHA-256 `b9e2e4aad6789b3b63a056d442f7b39f0ecfca3ae0f1fc0ae4e9614401b69f4b`) into `.work/deps/lua52` as a static `liblua.a` with `lua52.pc`. The current build uses the Lua 5.2.4 vendored by `mlua`.

## Build

Host: macOS 27.0.1, Apple Silicon arm64, Xcode 27.1, macOS SDK 27.0, Swift 6.4, Meson 1.12.1, Ninja 1.13.2, pkgconf 3.0.10, Python 3.15, Rust nightly `1.100.0` (toolchain pinned in `rust-toolchain.toml`).

`scripts/dev-deps.sh` checks and installs the pinned Homebrew packages. `scripts/build.sh` sets `PKG_CONFIG_PATH` and configures:

```sh
meson setup build . --buildtype=release \
  -Dcplayer=true -Dlibmpv=false -Dgpl=true \
  -Dcocoa=enabled -Dswift-build=enabled -Dcoreaudio=enabled \
  -Dvulkan=enabled -Dvideotoolbox-pl=enabled \
  -Dgl=disabled -Dgl-cocoa=disabled -Dvideotoolbox-gl=disabled -Dmacos-cocoa-cb=disabled \
  -Djavascript=disabled -Dlibarchive=disabled -Duchardet=disabled -Dzimg=disabled \
  -Drubberband=disabled -Dcdda=disabled -Ddvdnav=disabled -Dlibbluray=disabled \
  -Dlibavdevice=disabled -Dcplugins=disabled -Dvapoursynth=disabled \
  -Djpeg=disabled -Dlcms2=disabled -Dplain-gl=disabled \
  -Dx11=disabled -Dwayland=disabled -Ddrm=disabled -Dvaapi=disabled -Dvdpau=disabled \
  -Daudiounit=disabled -Davfoundation=disabled -Dmacos-touchbar=disabled \
  -Dmacos-media-player=disabled -Dtests=false -Dbuild-date=false
```

`build/mpv --version` reports the `v0.41.0` base with a `git describe` suffix for campaign commits. After D16, the consolidated `campaign-base` tag makes `git describe` report `campaign-base`; `MPV_VERSION` remains 0.41.0 and the source baseline is unchanged.

## Binary

`scripts/measure.sh baseline` recorded revision `bda16d3a5dbd` on 2026-10-31, before the history consolidation. The player binary is unchanged by the later documentation commit.

| Property | Value |
|---|---|
| File type | Mach-O 64-bit executable arm64 |
| Deployment target | macOS minos 27.0, SDK 27.0 |
| Executable, unstripped | 4,500,432 bytes (4.29 MiB) |
| Executable, `strip -x` | 4,073,296 bytes (3.88 MiB) |
| Direct non-system libraries | 9 |
| Unique non-system closure | 59,084,272 bytes (56.35 MiB) in 34 files |
| Closure includes | Executable excluded; MoltenVK runtime ICD included |

Direct non-system `otool -L` entries:

```text
/opt/homebrew/opt/libass/lib/libass.9.dylib
/opt/homebrew/opt/ffmpeg/lib/libavcodec.63.dylib
/opt/homebrew/opt/ffmpeg/lib/libavfilter.12.dylib
/opt/homebrew/opt/ffmpeg/lib/libavformat.63.dylib
/opt/homebrew/opt/ffmpeg/lib/libavutil.61.dylib
/opt/homebrew/opt/libplacebo/lib/libplacebo.387.dylib
/opt/homebrew/opt/ffmpeg/lib/libswresample.7.dylib
/opt/homebrew/opt/ffmpeg/lib/libswscale.10.dylib
/opt/homebrew/opt/vulkan-loader/lib/libvulkan.1.dylib
```

The remaining direct entries are system frameworks and libraries (AppKit, CoreAudio, AudioToolbox, CoreFoundation, CoreVideo, Metal, QuartzCore, IOKit, Swift runtime, libSystem, libiconv, libz).

Closure attribution (unique files, symlinks resolved):

| Component | Size | Files |
|---|---|---|
| FFmpeg | 15.74 MiB | 6 |
| shaderc | 7.26 MiB | 1 |
| MoltenVK | 5.10 MiB | 1 |
| remaining external | 25.59 MiB | 22 |
| libplacebo | 0.99 MiB | 1 |
| dav1d | 0.81 MiB | 1 |
| Vulkan loader | 0.65 MiB | 1 |
| libass | 0.22 MiB | 1 |

The P0 baseline linked Lua 5.2.4 from the source build and no Rust code. The current build uses the vendored Lua 5.2.4 inside `mlua` and the Rust host; the version is unchanged. The build still emits `build/libmpv.2.dylib` even with `-Dlibmpv=false`; the `mpv` executable does not link it. The release closure excludes it.

## Features

Enabled: `cplayer`, GPL build, Cocoa, Swift build tools, CoreAudio, Vulkan, VideoToolbox through libplacebo, Lua 5.2, libass, and libplacebo.

Disabled: external libmpv, GL and its Cocoa/VideoToolbox backends, JavaScript, libarchive, uchardet, zimg, rubberband, CDDA, DVD/Blu-ray, libavdevice, C plugins, VapourSynth, JPEG, LCMS2, plain GL, X11, Wayland, DRM, VAAPI, VDPAU, AudioUnit, AVFoundation, Touch Bar, Media Player, tests, and build date.

## Protocols

`build/mpv --list-protocols` reports 60 entries and 58 unique schemes. Twenty-one network schemes remained: `dav`, `davs`, `ftp`, `gopher`, `gophers`, `http`, `https`, `ipfs`, `ipns`, `mms`, `mmsh`, `mmst`, `rtmp`, `rtp`, `rtsp`, `srtp`, `tcp`, `tls`, `udp`, `webdav`, `webdavs`. The release build removes them and enforces the local-only boundary; see `docs/scope.md`.

## Performance

All automated playback is headless (`--vo=null --ao=null`) and silent. Values are medians of 3 runs unless noted. The clips are 3 s, 24 fps, H.264, 320×240 for the corpus and 1280×720, 1920×1080, and 3840×2160 for these measurements. RSS and CPU come from `/usr/bin/time -l`.

| Metric | Value |
|---|---|
| Cold `--version` | 79.43 ms (`sync` before each run; macOS page cache is not dropped without root) |
| Warm `--version` | 82.71 ms |
| First frame, 1080p | 177.25 ms (`--frames=1 --untimed`) |
| Idle RSS | 79.38 MB |
| 720p RSS / CPU | 155.12 MB / 0.17 s |
| 1080p RSS / CPU | 214.73 MB / 0.21 s |
| 4K RSS / CPU | 514.88 MB / 0.50 s |
| Seek latency | 0.64 ms (IPC seek to `time-pos` within 0.25 s) |
| Exit latency | 2.90 ms (IPC quit to process exit) |
| Audio underruns | 0 (CoreAudio, muted, 3 s stereo) |
| Frame drops, 1080p | 0 (`frame-drop-count` after 2 s) |
| Decoder-to-renderer copies | not observable with a null video output |

Thirty-second steady-state playback, GPU rendering cost, and visual motion/fullscreen behavior are owner-run and are not part of the headless baseline.

## VideoToolbox

`ffmpeg -hwaccel videotoolbox` decodes the H.264 and HEVC fixtures without error. `mpv --vo=null --hwdec=videotoolbox` accepts the option and plays both fixtures headlessly. mpv's `Using hardware decoding (videotoolbox)` log requires a video output and is therefore an owner-run check; the headless gate does not claim it.

## Licenses

- mpv: GPL-2.0-or-later; source and notice obligations on distribution.
- FFmpeg 9.0.2: LGPL-2.1-or-later with GPL-enabled components; source and notice obligations.
- libass 0.17.5: ISC; notice.
- libplacebo 7.387.0: LGPL-2.1-or-later; source and notice obligations.
- MoltenVK 1.4.2: Apache-2.0; notice.
- Vulkan loader and headers 1.4.363.0: Apache-2.0; notice.
- shaderc 2026.4: Apache-2.0; notice.
- dav1d 1.5.4: BSD-2-Clause; notice.
- Lua 5.2.4: MIT; notice.

The release audit checks the final closure and preserves the required notices and source obligations.

## Known limitations

- CoreAudio fails upstream with error -50 at the ChannelMap property; D7 is applied until upstream fixes it.
- There is no usable macOS OpenGL renderer in v0.41.0; `gpu-next`/Vulkan/MoltenVK is the only path. `vo=gpu` hangs.
- Hardware-decoded frames cannot be screenshotted; diagnostics use `--hwdec=no` or `videotoolbox-copy`.
- `-Dlibmpv=false` still produces `libmpv.2.dylib`; it is not linked and must be excluded from the release.
- No `timeout` binary is available; the harness uses IPC and process timeouts, terminating at 2 s and killing at 5 s.
- There is no CI in this repo; gates are local commands in `tests/run.sh` and `scripts/audit.sh`.
- The stock build had network protocols; the release build does not.
- FFmpeg n9.0.2's native AV1 decoder is hardware-only; the release uses libdav1d for software AV1.
- External ASS subtitles require the `ass` demuxer; the MP4 decoder name is `movtext`, not `mov_text`.
- The baseline is measured headless; `gpu-next` rendering, 30 s steady state, real audio audibility, HDR/10-bit color, subtitle shaping, Retina scaling, and motion smoothness are owner-run.

## D7 CoreAudio deviation

`audio/out/ao_coreaudio.c` no longer sets `kAudioOutputUnitProperty_ChannelMap`. macOS 27 rejects the channel-description layout with error -50; the audio unit's default mapping initializes and plays mono, stereo, and 5.1. Revisit when upstream fixes the property; keep the regression test.

## Headless test mode

Automated gates run the player with `--vo=null --ao=null` so they do not open a window. `play` and `corpus` assert `VO: [null]`; fullscreen still toggles through the property; OSC and input checks are binding-level. VideoToolbox is checked headlessly with `ffmpeg -hwaccel videotoolbox`; mpv's `Using hardware decoding (videotoolbox)` log requires a video output and is an owner-run check. `gpu-next` rendering and visual behavior remain owner-run through `scripts/manual-checklist.sh`.

## Acceptance

P0.2:

- `scripts/build.sh` exits 0.
- `build/mpv --version` reports the v0.41.0 base.
- A 1 s 48 kHz stereo sine plays through `--ao=coreaudio` and logs `AO: [coreaudio] 48000Hz stereo 2ch s16`.
- The same file plays to EOF with `--ao=null` (`AO: [null] 48000Hz stereo 2ch s16`, exit 0).

P0.3:

- `tests/run.sh fixtures` regenerates the corpus and verifies 27 file hashes.
- `tests/run.sh play h264` passes headless.
- `tests/run.sh audio` passes with D7.
- `tests/run.sh suite` reported 169 passed, 0 failed at the P0.3 baseline.

P0.4:

- `scripts/measure.sh baseline` writes the baseline row in `docs/metrics.md`.
- `tests/run.sh baseline-docs` checks this document.
