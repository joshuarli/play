# FFmpeg

## Pin

FFmpeg **n9.0.2**, commit `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`.

`scripts/build-ffmpeg.sh` downloads the GitHub tag archive:

```text
https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n9.0.2.tar.gz
SHA-256 6e374ed621e48faa40639307dff48ba6fe574a509977956d2cce9669b7cc27e9
```

The extracted tree is `FFmpeg-n9.0.2/`. The script writes a `VERSION` file containing `9.0.2` before configuring; without it, `ffbuild/version.sh` walks up to the enclosing mpv repository and records mpv's `git describe` in `ffversion.h`.

## Build (P1.3)

`scripts/ffmpeg-components.txt` is the frozen video/demux allowlist. `scripts/build-ffmpeg.sh` maps each entry to `--enable-demuxer`, `--enable-decoder`, `--enable-parser`, `--enable-bsf`, `--enable-filter`, `--enable-hwaccel`, or `--enable-protocol`, and configures:

```sh
./configure \
  --prefix=/Users/josh/d/play/.work/deps/ffmpeg-full \
  --disable-shared --enable-static \
  --disable-programs --disable-doc \
  --disable-network --disable-avdevice --disable-autodetect \
  --disable-everything \
  --enable-libdav1d \
  --enable-videotoolbox \
  ...allowlist enables...
```

| Category | Names |
|---|---|
| Demuxers | `mov`, `matroska`, `avi`, `mp3`, `flac`, `wav`, `ogg`, `srt`, `webvtt`, `ass` |
| Video decoders | `h264`, `hevc`, `libdav1d`, `vp9`, `mpeg4` |
| Subtitle decoders | `subrip`, `ass`, `webvtt`, `movtext` |
| Parsers | `h264`, `hevc`, `av1`, `vp9`, `mpeg4video`, `aac`, `mpegaudio`, `opus`, `vorbis`, `flac` |
| Bitstream filters | `h264_mp4toannexb`, `hevc_mp4toannexb`, `vp9_superframe`, `aac_adtstoasc`, `extract_extradata`, `null` |
| Filters | `format`, `scale`, `aresample`, `aformat`, `fps`, `setpts`, `crop`, `transpose`, `hflip`, `vflip`, `hwdownload`, `hwupload`, `trim`, `atrim`, `concat`, `volume`, `pan`, `channelsplit`, `null`, `anull` |
| HW accels | `h264_videotoolbox`, `hevc_videotoolbox` |
| Protocols | `file` |

Audio decoding is Symphonia; there are no FFmpeg audio decoders. Audio parsers and `aac_adtstoasc` remain because the FFmpeg demuxer uses them to produce packets. `--disable-autodetect` keeps the external library set to dav1d 1.5.4 plus the system VideoToolbox, CoreFoundation, CoreMedia, CoreVideo, and CoreServices frameworks. `TMPDIR` points at `.work/tmp`.

Installed static libraries:

```text
libavcodec.a   63.1.102   17,547,792 bytes
libavformat.a             4,388,992 bytes
libavfilter.a             1,742,832 bytes
libavutil.a               2,882,720 bytes
libswresample.a             369,240 bytes
libswscale.a              4,430,920 bytes
```

No `ffmpeg` or `ffprobe` program is installed. `scripts/build.sh` prepends `.work/deps/ffmpeg-full/lib/pkgconfig`, so the player links the static FFmpeg instead of Homebrew's dylibs. The allowlist hash is stored in the prefix; changing the allowlist forces a rebuild.

## Verification

- `config_components.h` has no `AAC`, `MP3`, `FLAC`, `ALAC`, `OPUS`, `VORBIS`, or `PCM` audio decoder enabled.
- `tests/run.sh corpus`: 104 passed, 0 failed.
- `tests/run.sh audio`: 15 passed, 0 failed.
- `tests/run.sh hwdec`: 8 passed, 0 failed.
- `tests/run.sh seek`: 5 passed, 0 failed.
- `build/mpv --version` reports `FFmpeg version: 9.0.2`.
- `build/mpv --list-protocols` reports 16 entries, 15 unique, zero network schemes.
- `otool -L build/mpv` contains no `libav*`, curl, or SSL library.
- Direct non-system libraries are libass, dav1d, libplacebo, and the Vulkan loader.
- P1.3 measurement: 15,890,416 bytes unstripped, 13,565,152 bytes stripped, 20.06 MiB in 17 closure files. The Rust/Symphonia static code is in the executable; the FFmpeg closure is no longer dynamic.

The configure auto-selects `h263` (pulled in by the MPEG-4 path) and `vp9_superframe_split` (pulled in by `vp9_superframe`). The manifest records both under `auto_selected`; they are not in the requested allowlist.

## Manifest gate (P1.4)

`scripts/ffmpeg-manifest.sh --generate` writes a manifest from the actual build tree (`config_components.h` and `config.mak`) to `.work/deps/ffmpeg-manifest.json`. The file is generated, not tracked.

`tests/run.sh ffmpeg-audit` regenerates the manifest in memory and checks the allowlist. It fails on any encoder, muxer, device, network protocol, decoder outside the video/subtitle allowlist (so every FFmpeg audio decoder), or external library outside `dav1d`.

Current result: 19 passed, 0 failed. A temporary manifest with `http` added to the protocols fails 2 cases (`protocols allowed`, `no network protocols`) and exits nonzero; the temporary file is removed after the check.

## Next

P5.4 made the Rust host the production Lua backend: the unmodified OSC loads through `player_ffi_lua_run`, a per-script pump forwards ordered events, and the external Lua library is no longer linked. P5.5 deletes the C host and completes the utility surface.
