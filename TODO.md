Prefix: PLAY
Next: 20

# Backlog

## P2

### PLAY-5 · P2 · The Lua host has no bounded lifecycle for its event queue or shutdown

- **Area:** Concurrency
- **Where:** `rust/player-lua/src/host.rs:106`, `:136-148`, `:170-191`, `rust/player-ffi/src/lua.rs:814-825`, `player/scripting.c:89-94`
- **Problem:** `ScriptHost` uses an unbounded `mpsc::channel`, so a script blocked in a callback accumulates every event the player posts. The production path calls `host.wait()`, which has no timeout; the documented 2 s bound exists only in `stop()`, and `stop()` detaches the thread on timeout. If the wait ever returns on that path, C frees the script client and `mp_args` while the detached thread still holds both. A non-returning script can therefore hang process exit, and the timeout branch can use freed memory.
- **Recommendation:** use a bounded queue with an explicit drop policy (never drop shutdown or client messages), make the production wait bounded, and give one owner the job of destroying the client. Log a timeout as a script error. Add a fixture script that never returns and assert that `quit` still exits.
- **Fix with:** lifecycle-concurrency
- **Effort:** M
- **Design:** none

### PLAY-12 · P2 · The 1080p CPU budget passes inside run noise and compare cannot fail a regression

- **Area:** Performance
- **Where:** `scripts/measure.sh:59`, `:482-486`, `:639-652`, `docs/metrics.md`, `PLAN.md:466`
- **Problem:** The 1080p limit is a hardcoded copy of the baseline (`0.21`), so the limit is `0.231 s`, and the final measurement is `0.23 s`. The same metric ranged from `0.16 s` to `0.23 s` across recorded runs; a three-run median cannot resolve a 1 ms difference. `scripts/measure.sh compare` checks only the fixed budgets, so a final label that is worse than the previous label on cold startup, warm startup, first frame, seek, and exit still prints `compare: ok`. The frame-drop budget is read under `--vo=null`, where it is zero by construction.
- **Recommendation:** use at least five interleaved runs per side, report the spread, and fail `compare` on a stated regression between labels. Re-measure the baseline instead of hardcoding it. Move frame drops to the owner-run renderer checklist.
- **Fix with:** benchmark-checklist
- **Effort:** M
- **Design:** none

## P3

### PLAY-14 · P3 · Startup playlist handling bypasses the classifier and reverses option order

- **Area:** Types
- **Where:** `rust/player-ffi/src/startup.rs:60-72`, `:86-100`, `player/main.c:406-443`, `options/parse_commandline.c:192-206`, `player/command.c:6280-6293`, `common/playlist.c:316-336`, `stream/stream_file.c:156-159`
- **Problem:** The Rust startup parser applies every positional file first and the remaining C arguments last, so `player --playlist=list.m3u extra.mp4` plays `extra.mp4` before the playlist entries. `--playlist` and `loadlist` also hand the playlist path to the stream layer without `classify_input`, so `file:///tmp/x.m3u` is accepted while the CLI and IPC reject `file://` for ordinary files. The docs state both behaviors as limits.
- **Recommendation:** emit one ordered operation stream from the startup parser (`AddFile`, `AddPlaylist`, option operations) and apply it in order; classify the playlist path at the same boundary as every other input. Add a mixed `--playlist` and positional test plus a `file://` playlist case.
- **Fix with:** api-surface-and-module-boundaries
- **Effort:** M
- **Design:** none

### PLAY-18 · P3 · Generated docs and fixtures heal their own oracles instead of failing on drift

- **Area:** Tests
- **Where:** `tests/lua-api-audit.sh:233-242`, `tests/harness.py:334-377`, `:1272`, `tests/fixtures/gen.sh:21`, `:297`, `scripts/measure.sh:639-670`
- **Problem:** `lua-api-audit` regenerates `docs/lua-api.md`, writes it, and then records the check as passed unconditionally. The fixture gate runs `gen.sh`, which rewrites the tracked `tests/fixtures/manifest.json`, and then hashes the files against that fresh manifest, so the integrity check cannot fail. `measure.sh update_metrics` extracts every table line from `docs/metrics.md` and appends it after the preamble, which is how the published table lost its column boundaries. The suite also silently skips the `mounted` path cases when the path is absent, so the recorded 292 count is host-dependent.
- **Recommendation:** make regeneration an explicit `--write` action and diff in check mode; generate fixtures into `.work` or compare a separately generated manifest with the tracked one; give `measure.sh` a marked block and leave the rest of the file alone; make the mounted case unconditional with a symlink fallback.
- **Fix with:** durable-state
- **Effort:** M
- **Design:** none

## P4

### PLAY-19 · P4 · Release bookkeeping has unchecked copies and a test-only exported panic hook

- **Area:** Build
- **Where:** `rust/player-ffi/src/lib.rs:92-95`, `rust/player-release/Cargo.toml:9-11`, `scripts/bundle.py:150-151`, `scripts/build-deps.sh:118-131`, `:23-29`, `scripts/measure.sh:446-464`
- **Problem:** `player_ffi_panic_for_test` is exported by `player-ffi`, is declared in the installed header, and is linked into the release `libplayer_release.dylib`; only `tests/ffi_test.c` calls it. `bundle.py` hardcodes `0.41.0` and build `41` instead of reading `MPV_VERSION`, and no gate reads the plist version. `build-deps.sh` writes a `zlib.pc` claiming version `1.2.12` with a system libdir purely to satisfy FreeType. It checks that `cmake`, `meson`, `ninja`, `pkg-config`, and `python3` exist but not which versions run. `measure.sh` writes hardcoded `True` values into its results JSON.
- **Recommendation:** put the panic hook behind a `test-support` feature that `player-release` does not enable; read `MPV_VERSION` in the bundler and assert it in `gate_release`; generate the zlib descriptor from the SDK or state the shim and its removal condition; verify the pinned tool versions or record the resolved versions in the release stamp; remove constants from the measurement result.
- **Fix with:** none
- **Effort:** S
- **Design:** none
