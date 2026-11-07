# Performance campaign

## Status

- **Goal:** make Play's performance evidence trustworthy, then cut the CPU and memory spent per correctly delivered frame, starting with scheduler pacing and then PLAY-31.
- **Accepted:** C0, C1 (with the C1b runner fixes), C2, C3, C4, C5, O1 (`28c99a6`).
- **In progress:** O2 (PLAY-31).
- **Next slice:** O3 only if a GUI profile on the O2 build shows OSC hover above its threshold; then close.
- **Parked:** none.

## Charter

- Decide and keep going. Record a decision here only when a later slice depends on it.
- Correct accounting before optimization. No O-item starts before C2 and C5 are accepted.
- Hard stops park the item: an action outside the repository, anything this doc forbids, or a slice whose acceptance command cannot run.
- Fix an egregious problem found on the way in its own commit when it costs under 30 changed lines. Note anything larger as a `TODO.md` ticket and leave it.
- One writer by default. Only the pairs named in **Sequence** may run in parallel, each in `.work/wt/<item>`.
- Commits are Conventional Commits whose scope is the workstream (`engine`, `app`, `bench`, `audio`, `linux`), with the item ID in the body. Push verified commits. Code comments never cite this doc.

## Decisions

- **D1, prescribed: measurement build and machine.** Measurements that decide anything use the `dist` profile (`scripts/release.sh`) on this Apple Silicon host. `build/mpv` is the dev profile (`opt-level = 2`, no fat LTO) and is used only for functional checks. Reverse only if `dist` build time makes the loop impractical; in that case use `release` and say so in every result.
- **D2, prescribed: comparison verdicts.** Baseline and candidate runs alternate, 7 pairs by default. The verdict is one of `improved`, `regressed`, `unchanged`, or `inconclusive`. The tolerance is fixed per metric at max(5% of the baseline median, an absolute floor per metric) and never grows with either run's spread. If the paired differences straddle the tolerance, the verdict is `inconclusive`, not a pass. Any failed trial makes the comparison `invalid`. The executor sets the per-metric floors after the C5 A/A run, at no less than the A/A spread.
- **D3, prescribed: unknown properties are unavailable.** A property whose value Play does not measure returns mpv's property-unavailable error, not a plausible constant. This changes a contract (see **Execution boundary**).
- **D4, prescribed: the deepest frame milestone is "submitted to the window".** libplacebo owns the swapchain through the carried patch, so presentation timestamps (Metal presented handlers, Wayland presentation feedback) are not reachable without PLAY-31. Revisit when PLAY-31 lands.
- **D5, prescribed: results.** Raw results are JSON under `.work/bench/<timestamp>-<mode>.json`, one file per run, never overwritten. Each holds every raw sample, the trial outcomes, the full argument lists, and the player's `--version` output and build profile. `docs/metrics.md` receives only reviewed summaries, under a new **Campaign** section that is kept apart from the debloat history. No hashes or manifests.
- **D6, prescribed: rejected from the source brief.** These were dropped because none of them changes a decision in this campaign: energy measurement, GPU residency tracking, Rust allocation accounting, a per-subsystem byte-ownership ledger, a hashed real-world clip corpus, TUI and terminal benchmarks, 10-bit/HDR scenarios, allocator swaps, and PGO. If an O-item's profile points at one of them, add it as its own item.
- **D7, delegated: metric names.** Rename forward-looking metrics to describe what they measure: `executable-startup` (from cold/warm startup), `headless-first-frame`, and `idle-exit`. Leave the history rows in `docs/metrics.md` as written. The executor picks the exact names.

## Baseline

- **Known red:** none. At `b3c659b` the Linux container suite had 373 passed and 5 failed: four `linux-audio` `eof-reached` timeouts and the `truncated` trace. C0 fixed both causes, and the container suite now has 386 passed, 0 failed. macOS CI only builds and packages, and runs no suite.
- **Measured inefficiency:** `build/mpv --vo=null --ao=null` on a 5 s 720p clip used 5.17 CPU-s video-only and 0.14 CPU-s with an audio track (two runs each, identical results; the host was busy, so this is not gate-qualified). The cause is `crates/player-engine/src/backend.rs:731-754`: the worker sleeps only when more than 5 ms of audio is queued, so a video-only stream spins. The explanation at `docs/metrics.md:42` is wrong.
- **Loop timings (warm):**

  | Command | Time | Result |
  | --- | --- | --- |
  | `cargo test -p player-engine` | 15 s | 69 pass |
  | `tests/run.sh seek` | 1.6 s | 6 pass |
  | `tests/run.sh traces` | 28 s | 2 pass |

## Sequence

Workstreams: **linux** (C0), **bench** (`scripts/`, C1, C5), and **engine/app** (`crates/`, C2-C4, O1-O3). Allowed parallel pairs are C0 with C1, and C0 with C2. Everything inside engine/app runs serially.

### C0: Linux baseline (linux)
Run `scripts/linux-container.sh tests/run.sh suite`. Fix each of the five failures whose cause is local and the fix is small. Park the rest as `TODO.md` tickets, each with its cause.
**Accept:** the container suite is green, or every remaining red failure is listed here under **Known red** with its ticket.

### C1: Runner correctness and workloads (bench)
Owns `scripts/bench.py`, `scripts/measure.sh`, `scripts/measure-linux.sh`, and a new shared `scripts/perfstat.py`.
- Move `run`, `median`, `stat`, `startup`, `first_frame`, `idle_rss`, `seek_latency`, and `exit_latency`, now duplicated between `measure.sh` and `measure-linux.sh`, into `perfstat.py`, together with the D2 comparison and D5 result writing.
- Make `cpu_seconds` parse every `ps` time format (`MM:SS.ss`, `HH:MM:SS`, `D-HH:MM:SS`). Treat a missing `perf-gate-ready` as deferred in both scripts, as `measure.sh` already does.
- Seek benchmark: precompute an explicit target trace from the seed, independent of the current position. A seek succeeds when `playback-restart` arrives (after C3) and the position is within the precision bound: one frame for exact seeks, the GOP length for keyframe seeks. Report success count, absolute error, and latency separately. A timeout or a failed landing marks the trial failed; it is never a sample.
- Frames/GUI benchmarks: alternate the OSC hidden/shown order, and reload the clip so each case covers the same timeline segment. Record the actual pointer event rate.
- Divide CPU by the measured elapsed time. Default the binary to `dist/Player.app`'s player.
- Workloads: video-only 1080p, A/V 1080p, 4K video-only, paused-open, and ended-open. Memory: an RSS time series every 250 ms, reporting the steady-state median and the peak (on macOS also `phys_footprint` via `footprint`; on Linux `smaps_rollup` PSS). Lifecycle: 50 seeks plus 10 `loadfile` cycles.
- Tests in `scripts/test_perfstat.py`: the brief's case (baseline 100±2, candidate 115±40) gives `inconclusive`; a candidate 15% slower with a tight range gives `regressed`; `00:00:00` parses as 0; a failed trial makes the comparison `invalid`.

**Accept:** `python3 -I scripts/test_perfstat.py` passes, and `scripts/bench.py seek` against `build/mpv` writes a result JSON with raw samples and per-trial outcomes.

### C2: Frame disposition accounting (engine/app)
Owns `crates/player-engine/src/{video,sched,backend,sim}.rs` and `crates/player-app/src/app.rs`.
- Make `present()` (`video.rs:276-320`) return a typed outcome: `Delivered`, `Missing`, or `Discarded`. A missing native frame or a failed send must not count as presented.
- Engine counters: `decoded`, `dropped_late`, `evicted` (including frames evicted before `collect_new` drains them), `missing`, and `delivered`, plus an in-flight gauge. App counters: `superseded` (replaced before being shown), `submitted`, and `shown`.
- The drop count reported to users becomes the sum of real discard counters, replacing `app.rs:277-284`.
- A late drop releases its native frame immediately (`sched.rs:763-785`).

**Accept:** a new `sim.rs` test asserts after every step that `decoded == dropped_late + evicted + missing + delivered + in_flight`, including under a forced 64-frame overflow. `cargo test -p player-engine` and `tests/run.sh traces` pass.

### C3: Seek completion (engine/app)
Requires C2. Owns the same files plus `tests/harness.py` (seek gate only).
- Give every seek a generation. Emit `playback-restart` exactly once per seek generation: when the first frame that satisfies the precision is submitted, or once audio resumes on audio-only files. This replaces the two immediate events at `app.rs:521-522`.
- A newer seek supersedes an older one without emitting its event.

**Accept:** `tests/run.sh seek` asserts one `playback-restart` per seek, a position within the precision bound when it arrives, and no event for a superseded seek.

### C4: Truthful properties and audio counters (engine/app, audio)
Requires C3. Owns `crates/player-app/src/properties.rs` and `crates/player-audio/src/{backend_coreaudio,backend_alsa,sink}.rs`.
- `container-fps` comes from the stream. `estimated-vf-fps` comes from the PTS deltas of delivered frames. `current-vo` names the backend actually in use. `decoder-frame-drop-count` and `demuxer-cache-duration` become measured values or unavailable (D3).
- CoreAudio and ALSA expose live underrun counters, kept apart from the startup and seek/flush drain events. Publish them through engine status and an `audio-underrun-count` property.

**Accept:** a `tests/run.sh cli` property check shows `container-fps` matching ffprobe on a 30 fps fixture, the unavailable properties return an error, and a forced starvation in a `player-audio` test increments the counter.

### C5: Native baseline (bench)
Requires C1-C4.
- Build `dist`. Run an A/A comparison of the same binary on every C1 workload, then set the D2 floors from the result.
- Record the baseline in `docs/metrics.md` under **Campaign**, and correct the `docs/metrics.md:42` explanation.

**Accept:** with `perf-gate-ready` exiting 0 before and `-1` after, the A/A run gives `unchanged` on every metric. If it is `inconclusive`, raise the pair count until it is not.

### O1: Scheduler pacing (engine)
Requires C5.
- The worker blocks on `recv_timeout` until the earliest of: the next frame due time, the audio low watermark, or a command. It never spins.
- `audio_wanted` (`sched.rs:630`) requires an active audio stream.

**Accept:** on the video-only 1080p workload, CPU-s per media-s falls to the A/V workload's decode-bound cost (0.26 at baseline; the earlier 0.1 target assumed decode was cheaper than it is), with no frame drops added.

**Result:** video-1080p 0.99 to 0.27 CPU-s per media-s and 189 MB less RSS; video-4k 0.54 lower and 760 MB less RSS; both went from about 76 drops per 3 s (every frame evicted) to 0; av-1080p unchanged. A/V workloads are `unchanged` or `improved`. `cargo test -p player-engine`, `tests/run.sh traces`, and `tests/run.sh seek` pass.

### O2: PLAY-31 single-device presentation (engine, output, display)
Requires O1. Needs an `architect` pass before any code, because it changes the boundary between `player-engine`, `player-output`, and `player-display`.

**Accept:** PLAY-31's own criteria (`--vo=null` and screenshots still work). On 1080p and 4K GUI playback, C5 comparison verdicts are `improved` for CPU and steady RSS, `submitted` frame counts are equal, and drops do not increase.

### O3: Overlay recomposition (overlay)
Only if the C5 profile shows OSC hover or tooltip cost above 2 CPU percentage points over hidden-OSC playback. This absorbs PLAY-29.

**Accept:** the hover workload is `improved`; `tests/run.sh osc` passes.

## Acceptance

The campaign closes when C0-C5 and O1 are accepted and O2 has either landed or been parked with an `architect` decision. O3 closes either by landing or by C5 showing it under threshold.

## Needs environment

- Linux x86-64 musl on real Wayland and ALSA hardware: the owner runs `scripts/bench.py all --player <tarball player>` before and after O1 and O2. The container's Weston with software Vulkan does not qualify presentation or audio timing.
- The visual and audio checks in `docs/manual-checklist.md` after O2.

## Execution boundary and non-goals

- **Allowed:** edits under `crates/`, `scripts/`, `tests/`, `docs/`, and `TODO.md`; local builds; the Linux container; commits and pushes to `main`.
- **Forbidden:** CI workflow changes, new runtime dependencies, notarization, and network-facing scope.
- **Non-goals:** hardware decode (PLAY-20, PLAY-32), HDR (PLAY-33), the D6 list, X11, and glibc.
- **Contract changes for the owner:**
  - `playback-restart` fires once, on completion (C3).
  - Placeholder properties return unavailable (C4).
  - A new `audio-underrun-count` property (C4).
  - The frame-drop count's meaning changes (C2).
