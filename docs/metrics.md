# Metrics

Automated playback is headless (`--vo=null --ao=null`); GUI and renderer checks are owner-run. Timings are medians of 3 runs unless noted. RSS and CPU come from `/usr/bin/time -l`; the measurement fixtures are generated in `.work/fixtures/measure`. The exact commands and component attribution are in `docs/baseline.md`. Revisions before `campaign-base` refer to pre-consolidation commits removed by D16.

| Date | Revision | Label | mpv B | stripped B | closure B | closure files | cold ms | warm ms | first frame ms | idle RSS MB | 1080p RSS MB | 1080p CPU s | 4K CPU s | seek ms | exit ms | protocols | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 2026-10-31 | bda16d3a5dbd | baseline | 4500432 | 4073296 | 59084272 | 34 | 79.43 | 82.71 | 177.25 | 79.38 | 214.73 | 0.21 | 0.5 | 0.64 | 2.9 | 60 | headless; 3 s clips; GUI owner-run |
| 2026-10-31 | 5c2b487e4577 | p1.3 | 15890416 | 13565152 | 21037712 | 17 | 66.42 | 67.17 | 145.92 | 78.23 | 210.88 | 0.19 | 0.47 | 0.33 | 2.57 | 16 | headless; 3 s clips; GUI owner-run |
| 2026-10-31 | 301179b792e7 | p5.5 | 16565152 | 14089312 | 21037712 | 17 | 71.44 | 68.78 | 165.64 | 76.39 | 209.33 | 0.17 | 0.44 | 0.26 | 2.66 | 1 | headless; 3 s clips; GUI owner-run |
| 2026-11-01 | ee06243e8cc4 | p6.1 | 16466080 | 13990240 | 21037712 | 17 | 66.18 | 63.17 | 148.24 | 79.03 | 210.61 | 0.18 | 0.45 | 0.68 | 3.1 | 1 | headless; 3 s clips; GUI owner-run; osc.lua 466 lines |

OSC (P6.1): `player/lua/osc.lua` is 466 lines, down from 2977. The p6.1 row records the resulting startup and first-frame timings; the OSC redraws at 20 Hz while visible.
