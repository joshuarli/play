# FFmpeg

## Pin

FFmpeg **n9.0.2**, commit `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`.

`scripts/build-ffmpeg.sh` downloads the GitHub tag archive:

```text
https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n9.0.2.tar.gz
SHA-256 6e374ed621e48faa40639307dff48ba6fe574a509977956d2cce9669b7cc27e9
```

The extracted tree is `FFmpeg-n9.0.2/`. The script writes a `VERSION` file containing `9.0.2` before configuring; without it, `ffbuild/version.sh` walks up to the enclosing mpv repository and records mpv's `git describe` in `ffversion.h`.

## Build

`scripts/ffmpeg-components.txt` is the single allowlist. `scripts/build-ffmpeg.sh` maps each entry to `--enable-demuxer`, `--enable-decoder`, `--enable-parser`, `--enable-bsf`, `--enable-filter`, `--enable-hwaccel`, or `--enable-protocol`, then configures:

```sh
./configure \
  --disable-shared --enable-static \
  --disable-programs --disable-doc \
  --disable-network --disable-avdevice --disable-autodetect \
  --disable-everything \
  --enable-libdav1d \
  --enable-videotoolbox \
  ...allowlist enables...
```

`--disable-autodetect` leaves dav1d 1.5.4 plus the system VideoToolbox, CoreFoundation, CoreMedia, CoreVideo, and CoreServices frameworks. `TMPDIR` points at `.work/tmp`. No `ffmpeg` or `ffprobe` program is installed. `scripts/build.sh` prepends the development FFmpeg pkg-config path; `scripts/release.sh` links the release prefix.

A line prefixed `macos/` or `linux/` applies to that platform only. macOS carries the VideoToolbox hwaccels. Linux carries the `h264_vaapi`, `hevc_vaapi`, `vp9_vaapi`, and `av1_vaapi` hwaccels and the native `av1` decoder they attach to (software AV1 still uses `libdav1d`), drops `--enable-videotoolbox`, and adds `--enable-pic --arch=x86_64 --enable-vaapi`. Platform lines add to the common categories. `scripts/build-deps-linux.sh` puts the Linux build at `.work/deps/linux/ffmpeg`, and the manifest gate reads that tree on Linux.

Add a component only after a named fixture fails without it; remove components in groups while the corpus stays green. Do not start from a guessed allowlist, and do not respond to a failure by enabling everything.

The release build puts FFmpeg at `.work/deps/release/ffmpeg`; a standalone `scripts/build-ffmpeg.sh` defaults to `.work/deps/ffmpeg-full`. The manifest gate reads the release tree when it exists and falls back to the development tree. `scripts/build-deps.sh` stamps every component with a hash of the build script, the FFmpeg build script, and the allowlist; a changed input rebuilds the closure. Old `ok` stamps are invalid and cause one full rebuild.

## Verification

- `config_components.h` enables no AAC, MP3, FLAC, ALAC, Opus, Vorbis, or PCM FFmpeg audio decoder.
- `tests/run.sh corpus`: 104 passed, 0 failed.
- `tests/run.sh audio`: 15 passed, 0 failed.
- `tests/run.sh hwdec`: 8 passed, 0 failed.
- `tests/run.sh seek`: 5 passed, 0 failed.
- `build/mpv --version` reports `FFmpeg version: 9.0.2`.
- `build/mpv --list-protocols` reports one protocol, `file`.
- `otool -L build/mpv` contains no `libav*`, curl, or SSL library.
- Direct non-system libraries are libass, dav1d, libplacebo, and the Vulkan loader.

The configure auto-selects `h263` (pulled in by the MPEG-4 path) and `vp9_superframe_split` (pulled in by `vp9_superframe`). The manifest records both under `auto_selected`; they are not in the requested allowlist.

## Manifest gate

`scripts/ffmpeg-manifest.sh --generate` writes a manifest from the build tree (`config_components.h` and `config.mak`) to `.work/deps/ffmpeg-manifest.json`. The file is generated, not tracked.

`tests/run.sh ffmpeg-audit` regenerates the manifest in memory and checks the allowlist. It fails on any encoder, muxer, device, network protocol, decoder outside the video and subtitle allowlist, or external library outside `dav1d`.

Current result: 19 passed, 0 failed. A temporary manifest with `http` added to the protocols fails two cases (`protocols allowed`, `no network protocols`) and exits nonzero; the temporary file is removed after the check.

## Release state

The release app bundles the minimal FFmpeg 9.0.2 built from the pinned allowlist. `--list-protocols` reports only `file`; no encoder, muxer, device, network protocol, or external library beyond dav1d is enabled. The app links the pinned closure, not Homebrew, and is measured in `docs/metrics.md`.
