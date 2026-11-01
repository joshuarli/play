# Metrics

Automated playback is headless (`--vo=null --ao=null`); GUI and renderer checks are owner-run. Timings are medians of 3 runs unless noted. RSS and CPU come from `/usr/bin/time -l`; the measurement fixtures are generated in `.work/fixtures/measure`. The exact commands and component attribution are in `docs/baseline.md`. Revisions before `campaign-base` refer to pre-consolidation commits removed by D16.


OSC (P6.1): `player/lua/osc.lua` is 466 lines, down from 2977. The p6.1 row records the resulting startup and first-frame timings; the OSC redraws at 20 Hz while visible.

## Release footprint (P8.2)

Measured at `acf2fea` on 2026-11-01 with `scripts/measure.sh release p8.2`. The "before" column is the same release bundle before stripping and before thinning MoltenVK; the "after" column is the shipped bundle.


The release measurement: cold 60.50 ms, warm 60.25 ms, first frame 143.38 ms, idle RSS 85.33 MB, 1080p RSS 217.14 MB, 1080p CPU 0.16 s, 4K CPU 0.42 s, seek 1.73 ms, exit 3.11 ms. No audio underruns and no frame drops. The timings are within the spread of the P6.1 dev row; the only evidence-named optimization was disk footprint. `strip -x` and `lipo -thin arm64` are applied by `scripts/bundle.py` before signing; no source-level optimization was attempted.

## Script host attribution (P6.2)

Measured at `ee06243` on 2026-11-01 with `scripts/measure.sh size` and nine interleaved first-frame runs.


First-frame medians (9 interleaved runs, `--frames=1 --untimed --hwdec=no` on the 1080p measure fixture): default 169.23 ms; `--osc=no` 164.91 ms; `--osc=no --load-select=no` 169.60 ms. The differences are within the run-to-run spread (158.74-215.92 ms), so no isolated host cost above noise was measurable; the 50 ms threshold is not implicated.

Per-symbol linked attribution is unavailable on this host: macOS `nm -S` reports zero symbol sizes, and archive sizes include Rust `std` and are not linked-size contributions. Do not treat the archive numbers as the host's package footprint.

| Item | Before | After |
|---|---:|---:|
| `Player.app` on disk | 42 MiB | 30 MiB |
| executable | 16,485,056 B | 13,998,112 B |
| framework closure | 27,755,936 B / 12 files | 16,998,336 B / 12 files |
| `libMoltenVK.dylib` | 10,891,168 B (universal) | 3,864,768 B (arm64) |
| closure in `measure.sh` | 26.47 MiB | 16.21 MiB |
| Date | Revision | Label | mpv B | stripped B | closure B | closure files | cold ms | warm ms | first frame ms | idle RSS MB | 1080p RSS MB | 1080p CPU s | 4K CPU s | seek ms | exit ms | protocols | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 2026-10-31 | bda16d3a5dbd | baseline | 4500432 | 4073296 | 59084272 | 34 | 79.43 | 82.71 | 177.25 | 79.38 | 214.73 | 0.21 | 0.5 | 0.64 | 2.9 | 60 | headless; 3 s clips; GUI owner-run |
| 2026-10-31 | 5c2b487e4577 | p1.3 | 15890416 | 13565152 | 21037712 | 17 | 66.42 | 67.17 | 145.92 | 78.23 | 210.88 | 0.19 | 0.47 | 0.33 | 2.57 | 16 | headless; 3 s clips; GUI owner-run |
| 2026-10-31 | 301179b792e7 | p5.5 | 16565152 | 14089312 | 21037712 | 17 | 71.44 | 68.78 | 165.64 | 76.39 | 209.33 | 0.17 | 0.44 | 0.26 | 2.66 | 1 | headless; 3 s clips; GUI owner-run |
| 2026-11-01 | ee06243e8cc4 | p6.1 | 16466080 | 13990240 | 21037712 | 17 | 66.18 | 63.17 | 148.24 | 79.03 | 210.61 | 0.18 | 0.45 | 0.68 | 3.1 | 1 | headless; 3 s clips; GUI owner-run; osc.lua 466 lines |
| Item | Value |
|---|---|
| `osc.lua` | 466 lines, 14,479 B |
| `libplayer_ffi.a` archive | 20,951,920 B |
| vendored Lua 5.2.4 archive | 319,008 B |
| `build/mpv` stripped | 13,990,240 B |
| closure | 20.06 MiB in 17 files |
| 2026-11-01 | acf2fea2c172 | p8.2 | 13998112 | 13998112 | 16998336 | 12 | 60.5 | 60.25 | 143.38 | 85.33 | 217.14 | 0.16 | 0.42 | 1.73 | 3.11 | 1 | headless; 3 s clips; GUI owner-run |
| 2026-11-01 | 8e393464c387 | final | 13998112 | 13998112 | 16998336 | 12 | 62.94 | 63.02 | 148.39 | 85.38 | 217.41 | 0.17 | 0.43 | 1.85 | 3.0 | 1 | headless; 3 s clips; GUI owner-run |
| 2026-11-01 | e93cf47ccc37 | final | 9640432 | 9640432 | 17166736 | 13 | 103.38 | 102.44 | 207.01 | 84.72 | 213.45 | 0.25 | 0.64 | 2.78 | 5.28 | 1 | headless; 3 s clips; GUI owner-run |
