# Metrics

Automated playback is headless (`--vo=null --ao=null`), so GUI and renderer behavior is owner-run. Rows are medians of seven runs, except the earliest rows, which predate the seven-run method and are medians of three. The spread is kept in `.work/tmp/measure-LABEL.json`. RSS and CPU come from `/usr/bin/time -l`; the measurement fixtures are generated in `.work/fixtures/measure`.

## Measurement history

<!-- measure:begin -->
| Date | Revision | Label | mpv B | stripped B | closure B | files | cold ms | warm ms | first frame ms | idle MB | 1080p MB | 1080p CPU s | 4K CPU s | seek ms | exit ms | protocols | notes |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 2026-10-31 | bda16d3a5dbd | baseline | 4500432 | 4073296 | 59084272 | 34 | 79.43 | 82.71 | 177.25 | 79.38 | 214.73 | 0.21 | 0.50 | 0.64 | 2.90 | 60 | headless, 3 s clips |
| 2026-10-31 | 5c2b487e4577 | p1.3 | 15890416 | 13565152 | 21037712 | 17 | 66.42 | 67.17 | 145.92 | 78.23 | 210.88 | 0.19 | 0.47 | 0.33 | 2.57 | 16 | headless, 3 s clips |
| 2026-10-31 | 301179b792e7 | p5.5 | 16565152 | 14089312 | 21037712 | 17 | 71.44 | 68.78 | 165.64 | 76.39 | 209.33 | 0.17 | 0.44 | 0.26 | 2.66 | 1 | headless, 3 s clips |
| 2026-11-01 | ee06243e8cc4 | p6.1 | 16466080 | 13990240 | 21037712 | 17 | 66.18 | 63.17 | 148.24 | 79.03 | 210.61 | 0.18 | 0.45 | 0.68 | 3.10 | 1 | `osc.lua` 466 lines |
| 2026-11-01 | acf2fea2c172 | p8.2 | 13998112 | 13998112 | 16998336 | 12 | 60.50 | 60.25 | 143.38 | 85.33 | 217.14 | 0.16 | 0.42 | 1.73 | 3.11 | 1 | stripped and thinned |
| 2026-11-01 | 8e393464c387 | final | 13998112 | 13998112 | 16998336 | 12 | 62.94 | 63.02 | 148.39 | 85.38 | 217.41 | 0.17 | 0.43 | 1.85 | 3.00 | 1 | before the cdylib link |
| 2026-11-01 | 7a8958b9757a | final | 9640432 | 9640432 | 17166736 | 13 | 162.11 | 115.73 | 220.07 | 84.78 | 216.88 | 0.23 | 0.55 | 3.21 | 4.24 | 1 | host loaded |
| 2026-11-01 | 12b060d0108c | final | 9640096 | 9640096 | 17150384 | 13 | 70.89 | 67.77 | 141.05 | 84.17 | 215.55 | 0.18 | 0.48 | 24.41 | 2.87 | 1 | headless, 3 s clips |
| 2026-11-01 | cbb4ec27c8ea | final | 9640096 | 9640096 | 17150384 | 13 | 72.01 | 72.78 | 154.3 | 84.33 | 216.09 | 0.16 | 0.43 | 23.99 | 3.08 | 1 | headless, 3 s clips |
| 2026-11-02 | c047192252df | final | 9707520 | 9707520 | 17334096 | 13 | 70.19 | 65.2 | 152.83 | 84.34 | 215.83 | 0.16 | 0.59 | 25.0 | 4.55 | 1 | headless, 3 s clips |
| 2026-11-02 | 96331a98445a | final | 9707520 | 9707520 | 17334096 | 13 | 65.47 | 63.14 | 150.12 | 83.92 | 215.83 | 0.18 | 0.44 | 23.68 | 3.06 | 1 | headless, 3 s clips |
| 2026-11-02 | d4756faa3a87-dirty | final | 9707328 | 9707328 | 17334096 | 13 | 64.93 | 61.46 | 156.77 | 83.98 | 214.08 | 0.31 | 0.63 | 30.0 | 6.4 | 1 | headless, 3 s clips |
| 2026-11-03 | cfd4d827ecc4-dirty | final | 9707328 | 9707328 | 17334096 | 13 | 64.74 | 72.02 | 157.86 | 83.98 | 216.14 | 0.18 | 0.46 | 23.73 | 5.12 | 1 | headless, 3 s clips |
| 2026-11-03 | ee5021c5256e-dirty | final | 9440304 | 9440304 | 17334096 | 13 | 68.31 | 66.83 | 154.31 | 83.84 | 213.31 | 0.26 | 0.61 | 28.99 | 6.37 | 1 | headless, 3 s clips |
| 2026-11-03 | 7e9f4ba571d9 | final | 9440304 | 9440304 | 17334096 | 13 | 58.2 | 53.85 | 139.61 | 83.94 | 215.83 | 0.17 | 0.44 | 25.33 | 3.21 | 1 | headless, 3 s clips |
| 2026-11-03 | ab74141fcb7a-dirty | final | 7617088 | 7617088 | 15127360 | 12 | 18.2 | 17.63 | 35.11 | 14.33 | 219.75 | 2.96 | 2.98 | 42.21 | 11.4 | 1 | headless, 3 s clips |
| 2026-11-03 | e5d6ddd4c3bd | final | 7617088 | 7617088 | 15127360 | 12 | 17.17 | 14.75 | 34.25 | 14.33 | 219.62 | 2.97 | 2.98 | 36.09 | 10.69 | 1 | headless, 3 s clips |
| 2026-11-03 | cdb159a1e2b2 | final | 7600656 | 7600656 | 15127360 | 12 | 16.31 | 14.37 | 33.93 | 14.39 | 219.62 | 2.97 | 2.98 | 41.08 | 9.01 | 1 | headless, 3 s clips |
| 2026-11-03 | f41bb506e60d | final | 7600640 | 7600640 | 15127360 | 12 | 17.94 | 16.42 | 36.43 | 14.34 | 219.78 | 2.98 | 2.99 | 40.45 | 10.88 | 1 | headless, 3 s clips |
| 2026-11-03 | c12a3c3df488 | final | 7074160 | 7074160 | 15127360 | 12 | 17.56 | 15.9 | 34.65 | 14.25 | 218.22 | 2.96 | 2.97 | 44.45 | 10.41 | 1 | headless, 3 s clips |
| 2026-11-04 | da0c70dbde2b | final | 7107568 | 7107568 | 12606880 | 8 | 17.22 | 16.36 | 38.47 | 14.05 | 262.41 | 2.51 | 2.78 | 11.7 | 12.11 | 1 | headless, 3 s clips |
| 2026-11-04 | 3c2f7e61356d | final | 7881984 | 7881984 | 11796736 | 7 | 15.77 | 14.2 | 34.18 | 14.0 | 262.08 | 3.24 | 3.54 | 12.44 | 11.07 | 1 | headless, 3 s clips |
| 2026-11-05 | 81adbe76aa1e-dirty | final | 7898752 | 7898752 | 13946992 | 7 | 19.18 | 22.71 | 75.78 | 13.62 | 256.09 | 2.15 | 2.73 | 180.03 | 39.76 | 1 | headless, 3 s clips; perf gate deferred |
| 2026-11-05 | a6ccaba15479-dirty | final | 7898912 | 7898912 | 13946992 | 7 | 12.16 | 12.28 | 58.67 | 13.58 | 262.27 | 3.06 | 3.44 | 51.51 | 21.76 | 1 | headless, 3 s clips |
<!-- measure:end -->

The `baseline` row is the original C player. The distributable budget is half of its unique closure, 29,542,136 B.

The `3c2f7e61356d` row is the first build on the `ffmpeg-build` `play-sdk` release `20261104`; its working tree differed from that commit only in docs. Against `da0c70dbde2b` the executable grows by 774,416 B because dav1d and zlib are now linked statically from the SDK, and the closure shrinks by 810,144 B and one file because `libdav1d.7.dylib` is no longer bundled. The distributable is 19,678,720 B, well within 29,542,136 B. The closure is MoltenVK, the Vulkan loader, libplacebo, and the four shaderc/glslang/SPIRV-Tools dylibs; the only protocol is `file`. Timings were taken on a busy host and are not a gate.

## Budgets

`scripts/measure.sh final` gates the release app on: distributable at most 29,542,136 B, executable at most 10 MB, 1080p RSS at most 300 MB, first frame at most 1 s, seek at most 250 ms, one `file` protocol, no network scheme, no Lua symbol, and no audio underrun. Timings are headless (`--vo=null --ao=null`). The Rust engine advances the clock at wall time there, so a 3 s clip costs about 3 s of CPU; CPU is compared, not gated.

`scripts/measure.sh cpu BASELINE_PLAYER` runs seven interleaved baseline and candidate pairs and fails only when the candidate exceeds the baseline by more than 10% and by more than the larger spread. `scripts/measure.sh compare BEFORE AFTER` applies the same rule between two recorded labels. Both print `perf gate deferred: host busy` and exit 75 when `perf-gate-ready` reports load.

## Native OSC

Removing Lua took the executable from 7,600,640 B to 7,074,160 B; headless timings did not move beyond noise (1080p CPU 2.98 s against 2.97 s over seven interleaved pairs). GUI CPU at 2560x1440 backing pixels on 1080p video, median of five interleaved runs against the Lua release:

| Scenario | Lua OSC | Native OSC |
|---|---:|---:|
| Paused, OSC hidden | 0.8% | 0.6% |
| Paused, OSC shown | 27.2% | 0.8% |
| Pointer moving over the seek bar | 28.3% | 10.2% |
| Playing, OSC hidden | 14.2% | 13.8% |

## Interactive benchmarks

`scripts/bench.py MODE` measures what a user feels in the GUI player, against Homebrew mpv where it can. Fixtures are generated into `.work/fixtures/bench`: `h264-gop10.mp4` (1080p24 x264, 250-frame GOP, AAC) and `vp9-opus.webm` (1080p VP9 realtime, 150-frame GOP, Opus). Every mode checks `perf-gate-ready` before and `perf-gate-ready -1` after, writes `.work/tmp/bench-MODE.json` (with the `--player-arg` options appended to the name, as in `bench-frames-gpu-api-metal.json`), and exits 75 with `perf gate deferred: host busy` when either check fails.

- `seek`: random absolute seeks at least 13 s from the current position; latency runs from the command to the first `time-pos` advance past the target. Medians of 15 per fixture and precision (`exact`, `keyframes`); mpv runs exact only.
- `audio-cpu`: CPU percent over 10 s of `--no-video --ao=null` playback.
- `frames`: fullscreen playback with the OSC hidden then shown: dropped frames, CPU, and RSS.
- `gui-cpu --baseline OLD_PLAYER` or `--baseline-arg OPTION`: interleaved candidate and baseline runs of four scenarios (paused with the OSC hidden or shown, the pointer sweeping the seek bar at 60 Hz, playing with the OSC hidden), medians and max-min spread of `--runs`.
- `--player-arg OPTION` (repeatable) passes an option such as `--gpu-api=metal` to the measured player; `--baseline-arg` does the same for the `gui-cpu` baseline, so one binary can be compared against itself. Every row names the options it ran with.

Results from 2026-11-03 on the native-OSC build. The host was not quiet for every run, so treat them as indicative:

| Metric | Player | mpv |
|---|---:|---:|
| Exact seek to playing, h264 | 159 ms | 107 ms |
| Exact seek to playing, VP9 | 137 ms | 107 ms |
| Keyframe seek to playing (OSC bar) | 115-118 ms | not measured |
| Audio-only CPU | 0.5-0.7% | 1.0-1.1% |
| Fullscreen, OSC hidden | 0 drops, 16-20% CPU, 263-287 MB | |
| Fullscreen, OSC shown | 0-1 drops, 29-33% CPU, 414-437 MB | |

The libass-to-native overlay change (`gui-cpu`, five interleaved runs against `d0e94ad`) measured: paused 0.4-1.0% against 0.4-1.0%, playing 20.2% against 19.2% (h264) and 24.4% against 25.8% (VP9), and seek-bar hover 14.3% against 9.0% (h264) but 12.0% against 11.6% (VP9). The host went busy before the closing check, so the run does not count as a gate; the h264 hover gap is open as PLAY-29.

## Metal backend

Candidate `--gpu-api=metal` measured against baseline `--gpu-api=vulkan` on the same release binary (`dist/Player.app/Contents/MacOS/player`, revision `a6ccaba`). The host was quiet throughout (`perf-gate-ready` passed before and after each mode with load <= 0.38/core; earlier measurements taken on a busy host were deferred). Both backends pay the two-device headless-render CPU readback and window re-upload (PLAY-31).

### GUI CPU and memory

Medians and max-min spread across five interleaved candidate and baseline runs on 1080p fixtures at 1280x720 (`scripts/bench.py gui-cpu --player-arg=--gpu-api=metal --baseline-arg=--gpu-api=vulkan`):

| Fixture | Scenario | Metal CPU | Vulkan CPU | Metal RSS | Vulkan RSS |
|---|---|---:|---:|---:|---:|
| h264-gop10 | Paused, OSC hidden | 0.60% ± 0.22% | 0.78% ± 0.20% | 208 MB | 234 MB |
| h264-gop10 | Paused, OSC shown | 0.79% ± 0.22% | 0.78% ± 0.19% | 293 MB | 334 MB |
| h264-gop10 | Seek bar hover (60 Hz) | 5.88% ± 0.65% | 5.38% ± 0.43% | 297 MB | 321 MB |
| h264-gop10 | Playing, OSC hidden | 27.29% ± 0.60% | 22.72% ± 1.11% | 270 MB | 288 MB |
| vp9-opus | Paused, OSC hidden | 0.78% ± 0.21% | 0.78% ± 0.22% | 184 MB | 210 MB |
| vp9-opus | Paused, OSC shown | 0.79% ± 0.21% | 0.78% ± 0.20% | 268 MB | 308 MB |
| vp9-opus | Seek bar hover (60 Hz) | 5.73% ± 1.10% | 4.88% ± 0.70% | 279 MB | 296 MB |
| vp9-opus | Playing, OSC hidden | 32.13% ± 1.18% | 25.78% ± 0.64% | 247 MB | 262 MB |

Metal holds 15–41 MB less resident memory across all GUI scenarios. Paused CPU and seek-bar hover are at parity within spread. Active playback draws 4.5–6.4% more CPU on Metal under the two-device readback pipeline.

### Fullscreen frame drops

Medians and spread across three interleaved rounds of 8 s fullscreen playback (`scripts/bench.py frames`):

| Fixture | OSC | Metal drops | Vulkan drops | Metal CPU | Vulkan CPU | Metal RSS | Vulkan RSS |
|---|---|---:|---:|---:|---:|---:|---:|
| h264-gop10 | hidden | 12 (11–15) | 13 (11–16) | 30.0% ± 0.5% | 26.1% ± 0.8% | 269 MB ± 5 MB | 289 MB ± 8 MB |
| h264-gop10 | shown | 18 (15–19) | 21 (12–21) | 41.4% ± 1.2% | 37.2% ± 1.2% | 358 MB ± 14 MB | 378 MB ± 14 MB |
| vp9-opus | hidden | 47 (45–56) | 43 (42–49) | 35.4% ± 6.4% | 30.2% ± 1.2% | 254 MB ± 15 MB | 274 MB ± 13 MB |
| vp9-opus | shown | 51 (51–62) | 54 (53–55) | 46.8% ± 14.3% | 40.0% ± 1.0% | 341 MB ± 5 MB | 352 MB ± 22 MB |

Frame drop counts are essentially identical between backends (within 1–4 frames over 8 s). Metal maintains a consistent ~20 MB RSS reduction fullscreen.

### Seek latency

Medians and spread across three interleaved rounds of 15 seeks per precision and fixture (`scripts/bench.py seek`):

| Fixture | Precision | Metal median ms | Vulkan median ms | Metal p90 ms | Vulkan p90 ms | Metal max ms | Vulkan max ms |
|---|---|---:|---:|---:|---:|---:|---:|
| h264-gop10 | exact | 242.7 ± 23.7 | 243.6 ± 34.3 | 321.5 | 304.0 | 341.5 | 355.3 |
| h264-gop10 | keyframes | 200.5 ± 5.2 | 193.3 ± 14.1 | 235.1 | 236.6 | 245.3 | 240.7 |
| vp9-opus | exact | 275.9 ± 20.4 | 263.4 ± 3.6 | 321.7 | 314.9 | 358.0 | 348.7 |
| vp9-opus | keyframes | 205.3 ± 11.9 | 202.4 ± 13.3 | 227.6 | 237.5 | 235.3 | 241.0 |

Seek-to-playing latency is at parity across backends (h264 exact medians differ by under 1 ms; all other comparisons are within the run-to-run spread).

## Linux

Medians of 7 runs with the spread (max − min). `scripts/measure-linux.sh`; cold starts follow `sync` because the page cache is not dropped without root.

| Label | Date | Revision | Stripped B | NEEDED | Cold ms | Warm ms | First frame ms | Idle MB | 1080p MB | 1080p CPU s | Seek ms | Exit ms | Protocols |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline | 2026-11-01 | 2681555-dirty | 6,643,680 | 18 | 62.3 ± 60.95 | 62.5 ± 6.16 | 187.43 ± 45.29 | 39.6 | 156.33 | 0.57 | 1.65 ± 35.14 | 11.42 ± 1.37 | 1 |
| final (host busy: timings are not a gate) | 2026-11-01 | 646198c | 26,355,208 | 6 | 5.14 ± 1.53 | 4.3 ± 1.53 | 105.41 ± 48.74 | 15.7 | 128.94 | 0.36 | 1.07 ± 30.4 | 2.6 ± 1.67 | 1 |
| final | 2026-11-04 | 6eb7184 | 23,820,624 | 7 | 13.56 ± 6.48 | 9.03 ± 1.74 | 63.32 ± 3.55 | 9.5 | 214.11 | 3.01 | 36.63 ± 6.88 | 13.77 ± 10.13 | 1 |

Container timings are not a gate.
