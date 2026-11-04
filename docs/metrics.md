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
<!-- measure:end -->

The `baseline` row is the original C player. The distributable budget is half of its unique closure, 29,542,136 B.

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

## Linux

Medians of 7 runs with the spread (max − min). `scripts/measure-linux.sh`; cold starts follow `sync` because the page cache is not dropped without root.

| Label | Date | Revision | Stripped B | NEEDED | Cold ms | Warm ms | First frame ms | Idle MB | 1080p MB | 1080p CPU s | Seek ms | Exit ms | Protocols |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline | 2026-11-01 | 2681555-dirty | 6,643,680 | 18 | 62.3 ± 60.95 | 62.5 ± 6.16 | 187.43 ± 45.29 | 39.6 | 156.33 | 0.57 | 1.65 ± 35.14 | 11.42 ± 1.37 | 1 |
| final (host busy: timings are not a gate) | 2026-11-01 | 646198c | 26,355,208 | 6 | 5.14 ± 1.53 | 4.3 ± 1.53 | 105.41 ± 48.74 | 15.7 | 128.94 | 0.36 | 1.07 ± 30.4 | 2.6 ± 1.67 | 1 |
| final | 2026-11-04 | 6eb7184 | 23,820,624 | 7 | 13.56 ± 6.48 | 9.03 ± 1.74 | 63.32 ± 3.55 | 9.5 | 214.11 | 3.01 | 36.63 ± 6.88 | 13.77 ± 10.13 | 1 |

Container timings are not a gate.
