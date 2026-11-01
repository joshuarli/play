# Symphonia Agent Notes

## Support Matrix

| Platform | Audio output | Status |
|---|---|---|
| macOS | CoreAudio through cpal | Supported |
| Linux | ALSA through cpal | Supported |
| Windows | None | Unsupported |
| Other targets | None | Unsupported |

Linux uses ALSA only. Do not add PulseAudio, JACK, PipeWire, or another Linux backend. Do not add Windows support or Windows-specific code paths.

## Codecs and Containers

All codecs and containers supported by the workspace stay enabled in `symphonia-play`. The README codec and container tables are the source of truth. WavPack, HE-AAC, HE-AACv2, video, and subtitles are not supported.

Codecs and containers remain pure Rust. Do not add FFmpeg, libavcodec, libopus, or other C codec bindings.

## Build

- Use the toolchain pinned by `rust-toolchain.toml`.
- Build the release player with `cargo dist`. The `dist` profile uses `opt-level = 3`, fat LTO, one codegen unit, `panic = "immediate-abort"`, and stripped symbols.
- `cargo dist` rebuilds the standard library because of the immediate-abort panic strategy. A plain `cargo build --profile dist` fails.
- Keep `opt-simd`, rubato's FFT resampler, all codecs, and all containers enabled.
- Binary size is not a tracked goal. Do not add size reports, size measurements, or size-driven refactors.

## Verification

```bash
cargo test -p symphonia-play
cargo clippy -p symphonia-play --all-targets --no-deps -- -D warnings
cargo dist
target/dist/symphonia-play --decode-only /path/to/audio-directory
```

The last command must process every supported file in the directory and exit zero.
