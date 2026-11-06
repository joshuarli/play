# FFmpeg

Player links a prebuilt static FFmpeg SDK from `laputa-systems/ffmpeg-build`. Player does not build FFmpeg. The producer repository owns the sources, the component profile, the configure flags, and the full audit; Player pins one release and checks the shipped manifest against its own playback contract.

## Pin

`ffmpeg-sdk.lock.json` names the producer release:

```json
{
  "contract": 1,
  "repository": "laputa-systems/ffmpeg-build",
  "release": "<YYYYMMDD or YYYYMMDD-rN>",
  "profile": "play-sdk",
  "ffmpeg": "9.0.2",
  "targets": {
    "aarch64-apple-darwin": {"asset": "ffmpeg-9.0.2+<release>-aarch64-apple-darwin-play-sdk.tar.gz", "sha256": "..."},
    "x86_64-unknown-linux-musl": {"asset": "ffmpeg-9.0.2+<release>-x86_64-unknown-linux-musl-play-sdk.tar.gz", "sha256": "..."}
  }
}
```

The two targets are Player's two platforms. The SDK is FFmpeg n9.0.2, dav1d 1.5.4, and zlib 1.3.2, built by the producer from SHA-256-pinned sources.

## Fetch

```sh
scripts/fetch-ffmpeg-sdk.sh aarch64-apple-darwin
scripts/linux-container.sh scripts/fetch-ffmpeg-sdk.sh x86_64-unknown-linux-musl
```

The script downloads the pinned asset into `.work/tmp/ffmpeg-sdk` and verifies its SHA-256 before extracting. The archive must contain exactly one directory named after the asset. Its `share/ffmpeg-build/manifest.json` must agree with the pin on profile, target, FFmpeg version, release tag, and asset name. Only then does the script move it to `.work/deps/ffmpeg-sdk/<target>`. A prefix whose manifest already matches the pin is reused. `scripts/build.sh`, `scripts/release.sh`, and `scripts/build-linux.sh` call the script, so a normal build fetches on first use.

## Link

`crates/player-media/build.rs` uses the shared checks in `crates/player-build/src/ffmpeg_sdk.rs`:

- The prefix is `FFMPEG_PREFIX` if set, otherwise `.work/deps/ffmpeg-sdk/$TARGET`. There is no fallback to a system, Homebrew, or older local prefix.
- The prefix must pass the same manifest check as the fetch script. A missing prefix, a `tools` archive, a different release, or a Homebrew prefix fails the build with a message that names the fetch command.
- Link flags come from `pkg-config --static --libs libavformat libavcodec libswscale libavutil` with `PKG_CONFIG_LIBDIR` set to the SDK only. The tokenizer honors pkg-config's backslash-escaped spaces. A `-L` outside the SDK is an error, and every `-l` that names an SDK archive links statically. That covers the FFmpeg libraries, `libdav1d.a`, and `libz.a`.
- macOS adds the VideoToolbox, CoreFoundation, CoreMedia, CoreVideo, and CoreServices frameworks from the `.pc` files. On Linux, `-lva` and `-lva-drm` resolve through the system `libva` pkg-config libdir and are passed by absolute name, because the musl cross-linker does not search that directory for `-l`.
- The build reruns when the pin, `FFMPEG_PREFIX`, or the SDK's manifest, `include/`, or `lib/` changes.

dav1d and zlib are inside the SDK as static archives. The macOS bundle no longer carries `libdav1d.7.dylib`, and Player no longer writes an SDK-zlib `zlib.pc` or builds its own zlib.

## Manifest gate

`tests/run.sh ffmpeg-audit` (`scripts/ffmpeg-manifest.sh --check [manifest.json]`) reads the shipped manifest, by default from `.work/deps/ffmpeg-sdk/<target>`. It checks Player's playback contract, not the full producer allowlist:

- the profile is `play-sdk`, the target is the Rust target, FFmpeg is the pinned 9.0.2, and the release tag matches the pin;
- no encoders, muxers, or devices; `file` is the only protocol, no network scheme from `scripts/network-schemes.txt` is enabled, and `--disable-network` is configured;
- no FFmpeg audio decoders, because Symphonia owns audio;
- the required playback demuxers (`mov`, `matroska`, `avi`, `mp3`, `flac`, `wav`, `ogg`) and decoders (`h264`, `hevc`, `libdav1d`, `vp9`, `mpeg4`, `png`, `mjpeg`) are present. macOS also needs the VideoToolbox hwaccels. Linux needs the native `av1` decoder and the VA-API hwaccels;
- `dav1d` is the only external library, and `dav1d` and `zlib` are the only bundled dependencies.

The producer's `build.py audit` validates the complete resolved configuration against the profile in `ffmpeg-build`. Player does not duplicate that allowlist.

## Update

Move to a new producer release only after it is published with its `SHA256SUMS`:

1. Set `release` and each target's `asset` and `sha256` in `ffmpeg-sdk.lock.json` from the release's `SHA256SUMS`.
2. Run `scripts/fetch-ffmpeg-sdk.sh aarch64-apple-darwin`, `tests/run.sh ffmpeg-audit`, `scripts/audit.sh`, and `tests/run.sh suite`, and run the Linux half in the container.
3. Publish a Player patch release.

## Fixtures

`tests/fixtures/gen.sh` needs a full host FFmpeg 9.0.2: Homebrew `ffmpeg` on macOS (`scripts/dev-deps.sh` pins it), and the Alpine `ffmpeg` package in `docker/linux/Dockerfile`. That FFmpeg is a test-only dependency; the shipped SDK does not provide it, and the app never runs it.
