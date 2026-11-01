# Metrics

Automated playback is headless (`--vo=null --ao=null`), so GUI and renderer behavior is owner-run. Timings are medians of three runs unless noted. RSS and CPU come from `/usr/bin/time -l`; the measurement fixtures are generated in `.work/fixtures/measure`. Revisions before `campaign-base` are pre-consolidation commits that no longer exist.

## Measurement history

| Date | Revision | Label | mpv B | stripped B | closure B | files | cold ms | warm ms | first frame ms | idle MB | 1080p MB | 1080p CPU s | 4K CPU s | seek ms | exit ms | protocols | notes |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 2026-10-31 | bda16d3a5dbd | baseline | 4500432 | 4073296 | 59084272 | 34 | 79.43 | 82.71 | 177.25 | 79.38 | 214.73 | 0.21 | 0.50 | 0.64 | 2.90 | 60 | headless, 3 s clips |
| 2026-10-31 | 5c2b487e4577 | p1.3 | 15890416 | 13565152 | 21037712 | 17 | 66.42 | 67.17 | 145.92 | 78.23 | 210.88 | 0.19 | 0.47 | 0.33 | 2.57 | 16 | headless, 3 s clips |
| 2026-10-31 | 301179b792e7 | p5.5 | 16565152 | 14089312 | 21037712 | 17 | 71.44 | 68.78 | 165.64 | 76.39 | 209.33 | 0.17 | 0.44 | 0.26 | 2.66 | 1 | headless, 3 s clips |
| 2026-11-01 | ee06243e8cc4 | p6.1 | 16466080 | 13990240 | 21037712 | 17 | 66.18 | 63.17 | 148.24 | 79.03 | 210.61 | 0.18 | 0.45 | 0.68 | 3.10 | 1 | `osc.lua` 466 lines |
| 2026-11-01 | acf2fea2c172 | p8.2 | 13998112 | 13998112 | 16998336 | 12 | 60.50 | 60.25 | 143.38 | 85.33 | 217.14 | 0.16 | 0.42 | 1.73 | 3.11 | 1 | stripped and thinned |
| 2026-11-01 | 8e393464c387 | final | 13998112 | 13998112 | 16998336 | 12 | 62.94 | 63.02 | 148.39 | 85.38 | 217.41 | 0.17 | 0.43 | 1.85 | 3.00 | 1 | before the cdylib link |
| 2026-11-01 | 7a8958b9757a | final | 9640432 | 9640432 | 17166736 | 13 | 162.11 | 115.73 | 220.07 | 84.78 | 216.88 | 0.23 | 0.55 | 3.21 | 4.24 | 1 | host loaded |

The `baseline` row is the P0 comparison point. The P8 budget is half of its unique closure size, 29,542,136 B.

## Final release

Measured at `7a8958b` on 2026-11-01 with `scripts/measure.sh final`. The host was loaded during the run; the absolute timings are evidence, not a product promise.

| Metric | Value |
|---|---:|
| `Player.app` total | 26,807,168 B |
| executable | 9,640,432 B |
| framework closure | 17,166,736 B in 13 files |
| cold startup | 162.11 ms |
| warm startup | 115.73 ms |
| first frame, 1080p | 220.07 ms |
| idle RSS | 84.78 MB |
| 1080p RSS | 216.88 MB |
| 1080p CPU | 0.23 s |
| 4K CPU | 0.55 s |
| seek | 3.21 ms |
| exit | 4.24 ms |
| audio underruns | 0 |
| frame drops | 0 |
| protocols | 1 (`file`) |
| Lua interpreters | 1 |

| Budget | Limit | Actual | Result |
|---|---:|---:|---|
| Distributable | 29,542,136 B | 26,807,168 B | pass |
| Executable | 10,000,000 B | 9,640,432 B | pass |
| First frame | 1 s | 220.07 ms | pass |
| 1080p CPU | 0.231 s | 0.23 s | pass within noise |
| 1080p RSS | 300 MB | 216.88 MB | pass |
| Seek | 250 ms | 3.21 ms | pass |
| Network schemes | 0 | 0 | pass |
| Duplicate Lua interpreters | 0 | 0 | pass |

The 1080p CPU result is inside the run-to-run spread. The 3-run median cannot distinguish it from the 0.231 s limit, and the same metric ranged from 0.16 s to 0.23 s across the runs above. `scripts/measure.sh compare` checks the fixed budgets, not a regression between two labels. The frame-drop count is read under `--vo=null`, where there is no renderer clock, so it is zero by construction and is not a renderer claim.

## Optimization history

The reduction came from stripping, thinning MoltenVK to arm64, linking shaderc as MinSizeRel, and moving the Rust plane to `libplayer_release.dylib`.

| Item | Before | After |
|---|---:|---:|
| `Player.app` on disk | 42 MiB | 30 MiB |
| executable | 16,485,056 B | 13,998,112 B |
| framework closure | 27,755,936 B in 12 files | 16,998,336 B in 12 files |
| `libMoltenVK.dylib` | 10,891,168 B (universal) | 3,864,768 B (arm64) |
| closure in `measure.sh` | 26.47 MiB | 16.21 MiB |

Plain LTO was rejected because the two Rust staticlibs duplicate `std` symbols at link time. `opt-level = "s"` increased the executable and was reverted.

## Script host attribution

Measured with `scripts/measure.sh size` and nine interleaved first-frame runs.

| Item | Value |
|---|---:|
| `osc.lua` | 466 lines, 14,479 B |
| `libplayer_ffi.a` archive | 20,951,920 B |
| vendored Lua 5.2.4 archive | 319,008 B |
| `build/mpv` stripped | 13,990,240 B |
| closure | 20.06 MiB in 17 files |

First-frame medians: default 169.23 ms; `--osc=no` 164.91 ms; a third run with the then-current `--load-select=no` 169.60 ms, an option removed with `select.lua`. The spread across nine runs was 158.74 to 215.92 ms, so no isolated script-host cost above noise was measurable. Symbol-level attribution is not available: macOS `nm -S` reports zero sizes, and archive sizes include Rust `std` rather than the linked contribution.
