# Metrics

Automated playback is headless (`--vo=null --ao=null`), so GUI and renderer behavior is owner-run. Rows are medians of seven runs, except the earliest rows, which predate the seven-run method and are medians of three. The spread is kept in `.work/tmp/measure-LABEL.json`. RSS and CPU come from `/usr/bin/time -l`; the measurement fixtures are generated in `.work/fixtures/measure`. Revisions before `campaign-base` are pre-consolidation commits that no longer exist.

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
<!-- measure:end -->

The `baseline` row is the P0 comparison point. The P8 budget is half of its unique closure size, 29,542,136 B.

## Final release

Measured on the S9b working tree at `ab74141` on 2026-11-03 with `scripts/measure.sh final`. The host was quiet before and after the run (`perf-gate-ready` exited 0).

| Metric | Value |
|---|---:|
| `Player.app` total | 24,123,809 B |
| executable | 7,600,656 B |
| framework closure | 15,127,360 B in 12 files |
| cold startup | 18.20 ms |
| warm startup | 17.63 ms |
| first frame, 1080p | 35.11 ms |
| idle RSS | 14.33 MB |
| 1080p RSS | 219.75 MB |
| 1080p CPU | 2.96 s |
| 4K CPU | 2.98 s |
| seek | 42.21 ms |
| exit | 11.40 ms |
| audio underruns | 0 |
| protocols | 1 (`file`) |
| Lua interpreters | 1 |

| Budget | Limit | Actual | Result |
|---|---:|---:|---|
| Distributable | 29,542,136 B | 22,744,448 B | pass |
| Executable | 10,000,000 B | 7,600,656 B | pass |
| First frame | 1 s | 35.11 ms | pass |
| 1080p RSS | 300 MB | 219.75 MB | pass |
| Seek | 250 ms | 42.21 ms | pass |
| Network schemes | 0 | 0 | pass |
| Duplicate Lua interpreters | 0 | 0 | pass |

The 1080p CPU result is not a fixed budget. The S9 row is real-time headless playback: with `--vo=null --ao=null` the Rust engine still advances the clock at wall time, so a 3 s clip costs about 3 s of CPU. It is not comparable to the older C rows, which finished null output ahead of wall time. The gated budgets are size, RSS, first frame, seek, protocols, and Lua count. `scripts/measure.sh cpu BASELINE_PLAYER` runs seven interleaved baseline and candidate pairs, reports both medians and spreads, and fails only when the candidate exceeds the baseline by more than 10% and by more than the larger spread. `scripts/measure.sh compare BEFORE AFTER` applies the same rule to startup, first frame, seek, exit, and CPU between two labels. Both print `perf gate deferred: host busy` and exit 75 when `perf-gate-ready` reports load, which is never a pass. Frame drops are no longer measured: under `--vo=null` there is no renderer clock, so the count is zero by construction. The renderer check is in `docs/manual-checklist.md`.

The S9 build removes the C engine and the Rust cdylib. The app is 24,123,809 B on disk with a 7,600,656 B executable and a 15,127,360 B closure, under the 29,542,136 B distributable budget.

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

First-frame medians: default 169.23 ms; `--osc=no` 164.91 ms; a third run with the then-current `--load-select=no` 169.60 ms, an option removed with `select.lua`. The spread across nine runs was 158.74 to 215.92 ms, so no isolated script-host cost above noise was measurable. Symbol-level attribution is not available: macOS `nm -S` reports zero sizes, and archive sizes include Rust `std` rather than the linked contribution. S9 removed the Rust cdylib and its `libplayer_ffi.a` archive; `scripts/measure.sh size` now reports `osc.lua` and the vendored Lua archive only.

## Linux

Medians of 7 runs with the spread (max − min). `scripts/measure-linux.sh`; cold starts follow `sync` because the page cache is not dropped without root.

| Label | Date | Revision | Stripped B | NEEDED | Cold ms | Warm ms | First frame ms | Idle MB | 1080p MB | 1080p CPU s | Seek ms | Exit ms | Protocols |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline | 2026-11-01 | 2681555-dirty | 6,643,680 | 18 | 62.3 ± 60.95 | 62.5 ± 6.16 | 187.43 ± 45.29 | 39.6 | 156.33 | 0.57 | 1.65 ± 35.14 | 11.42 ± 1.37 | 1 |
| final (host busy: timings are not a gate) | 2026-11-01 | 646198c | 26,355,208 | 6 | 5.14 ± 1.53 | 4.3 ± 1.53 | 105.41 ± 48.74 | 15.7 | 128.94 | 0.36 | 1.07 ± 30.4 | 2.6 ± 1.67 | 1 |
| final | 2026-11-04 | ab74141-dirty | 24,475,984 | 7 | 12.14 ± 8.98 | 12.62 ± 4.06 | 75.34 ± 13.98 | 9.7 | 216.09 | 3.05 | 36.05 ± 1.85 | 20.11 ± 10.41 | 1 |

The S9 `final` row is the Rust player: 24,475,984 B stripped, seven `NEEDED` entries, and 216.09 MB at 1080p. Its timings come from the container and are not a gate. The published v0.41.0 tarball predates S9: it adds VA-API (three libraries and the `--hwdec=auto` default) to the `646198c` build, with a 26,465,800 B stripped binary and nine `NEEDED` entries.
