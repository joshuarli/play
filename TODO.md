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

### PLAY-6 · P2 · The release gate can pass on a stale app and the audit never inspects it

- **Area:** Build
- **Where:** `tests/harness.py:1357-1408`, `scripts/release.sh:11`, `scripts/audit.sh:6-8`
- **Problem:** `gate_release` inspects whatever is in `dist/Player.app` and never compares it with `build-release/mpv`, a revision, or a build stamp. `release.sh` removes nothing before building, so a failed build leaves the previous app and the suite still passes. `scripts/audit.sh`, which the README and status doc name next to the suite, never runs the release gate at all.
- **Recommendation:** write a build stamp into `Contents/Resources` with the revision and the hashes of the executable and bundled dylibs, make `gate_release` verify it, and add `tests/run.sh release` to `scripts/audit.sh`. A stale app then fails on the missing or mismatched stamp.
- **Fix with:** durable-state
- **Effort:** S
- **Design:** none

### PLAY-7 · P2 · The FFmpeg manifest audits the development tree and dependency stamps ignore changed inputs

- **Area:** Build
- **Where:** `scripts/ffmpeg-manifest.sh:19`, `scripts/build-deps.sh:8`, `:44-45`, `:266-269`, `scripts/build-ffmpeg.sh:14`, `:26`
- **Problem:** The audit reads `.work/deps/ffmpeg-full/build`, while `scripts/build-deps.sh` builds the shipped FFmpeg at `.work/deps/release/ffmpeg/build`. Both trees exist on this host, so the gate currently checks a different install than the release. Each release component is skipped when a bare `.stamp-*` file exists; the stamps record no version or input hash. A changed allowlist or version does not force a rebuild through `scripts/release.sh`, and `docs/ffmpeg.md` previously claimed the allowlist hash did.
- **Recommendation:** take the FFmpeg build tree as an argument and default it to the release prefix; fail when it is missing. Make every stamp carry its version and input hash, and compare both in `have`.
- **Fix with:** single-source-of-truth
- **Effort:** S
- **Design:** none

### PLAY-8 · P2 · The hostile-HOME gate cannot fail its script assertion

- **Area:** Tests
- **Where:** `tests/harness.py:759-782`, `player/scripting.c:90`, `options/options.c:520`, `:917`
- **Problem:** The gate asserts that `"failed to load script"` is absent from the output, but the loader emits `Could not load %s script %s`, and the asserted phrase appears nowhere except the test. The other half checks for `evil.lua`, so the test passes when the config is ignored and also cannot prove that a config-driven `script=` key is rejected. There is no positive control that the config was not read.
- **Recommendation:** add a benign key from the hostile `mpv.conf` (`volume=42` or `msg-level=all=no`) and assert it did not take effect; assert the loader's real error phrase; exercise `script=` and `scripts-append=`. One run settles the behavior.
- **Fix with:** test-audit
- **Effort:** S
- **Design:** none

### PLAY-9 · P2 · The macOS menu exposes removed commands and ignored config

- **Area:** Dead code
- **Where:** `osdep/mac/menu_bar.swift:128-129`, `:216-218`, `:113-124`, `:345-348`, `osdep/mac/dialog.swift:36-42`, `options/options.c:979-985`
- **Problem:** The File menu offers `Open URL…`, which the player refuses. The Playback menu offers Shuffle, Toggle Loop File, and Toggle Loop Playlist; `playlist-shuffle` is absent from the command table and the loop options were removed, so each is a silent no-op. Settings and Keyboard Shortcuts open `~/.config/mpv` and `~/.mpv`, which the shipped player does not load. The file pickers allow image, archive, and CUE extensions that have no demuxer; a `.cue` picked as a playlist is parsed as plaintext.
- **Recommendation:** remove the Open URL, Settings, Keyboard Shortcuts, Shuffle, and loop items. Restrict the picker to the built video, audio, subtitle, and playlist extensions. Leave the local-only contract unchanged.
- **Fix with:** dead-code-archaeology
- **Effort:** S
- **Design:** none

### PLAY-10 · P2 · A failed launch has no visible explanation

- **Area:** Errors
- **Where:** `player/main.c:616-648`, `osdep/mac/application.swift:89-94`, `osdep/mac/dialog.swift:84-89`, `osdep/mac/menu_bar.swift:345-348`
- **Problem:** A missing or unsupported file logs `Cannot open file` and `Failed to open` and returns 2 or 3 from `mpv_main`. The Cocoa playback thread calls `exit(exitCode)` with no alert, and a Finder launch has no terminal. The menu `Open URL…` path is different again: `cmd_loadfile` sets `cmd->success = false`, and its error is logged above the OSD gate, so the running file keeps playing and the user sees nothing.
- **Recommendation:** before exiting from the Cocoa playback thread on exit code 2 or 3, show one alert on the main thread with the log path. For a running app, make a rejected `loadfile` visible with an OSD-level message. Keep the terminal log.
- **Fix with:** error-model
- **Effort:** M
- **Design:** none

### PLAY-11 · P2 · The Rust fullscreen mirror goes stale on window-initiated changes

- **Area:** Data
- **Where:** `rust/player-core/src/state.rs:4`, `:40-56`, `:113-121`, `player/command.c:4339-4350`, `:7839`, `osdep/mac/option_helper.swift:56-61`, `video/out/mac/window.swift:193`, `:202`, `:250`, `:269`
- **Problem:** The Rust store is updated only from `control_state_sync_option`, which the core option callback runs. The macOS window writes the `fullscreen` option through `m_config_cache_write_opt`, which updates the shadow and wakes cache listeners but never invokes the core callback. Entering or leaving fullscreen from the green button or the window menu therefore leaves `mpv_get_property("fullscreen")`, the `control-state` snapshot, and the OSC's fullscreen state permanently wrong. The property SET path still works, so only the window-originated direction is stale.
- **Recommendation:** notify the core after the VO cache write, or remove `fullscreen` from the Rust mirror and read the live option. Add a test that changes the VO option and reads `control-state`.
- **Fix with:** make-invalid-states-unrepresentable
- **Effort:** S
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

### PLAY-13 · P3 · Network scheme lists diverge across scripts, tests, and docs

- **Area:** Security
- **Where:** `scripts/ffmpeg-manifest.sh:149-153`, `scripts/measure.sh:418-422`, `tests/harness.py:801-802`, `scripts/audit.sh:15`, `tests/local_only.py:1-18`, `docs/baseline.md:116`
- **Problem:** The local-only boundary has six lists. The manifest and measure sets have 24 schemes, the harness and audit check 6, the URL matrix tests 16 inputs, and the baseline row records 21. `sftp`, `hls`, `dash`, `ipfs`, and `ipns` are gated in one place and not another; `smb` is missing from the manifest set. A scheme added to one list is silently ungated elsewhere.
- **Recommendation:** put one declarative list in `scripts/network-schemes.txt`, consume it from the manifest gate, measure, audit, harness, and URL matrix, and fail a test when a consumer's set differs.
- **Fix with:** single-source-of-truth
- **Effort:** M
- **Design:** none

### PLAY-14 · P3 · Startup playlist handling bypasses the classifier and reverses option order

- **Area:** Types
- **Where:** `rust/player-ffi/src/startup.rs:60-72`, `:86-100`, `player/main.c:406-443`, `options/parse_commandline.c:192-206`, `player/command.c:6280-6293`, `common/playlist.c:316-336`, `stream/stream_file.c:156-159`
- **Problem:** The Rust startup parser applies every positional file first and the remaining C arguments last, so `player --playlist=list.m3u extra.mp4` plays `extra.mp4` before the playlist entries. `--playlist` and `loadlist` also hand the playlist path to the stream layer without `classify_input`, so `file:///tmp/x.m3u` is accepted while the CLI and IPC reject `file://` for ordinary files. The docs state both behaviors as limits.
- **Recommendation:** emit one ordered operation stream from the startup parser (`AddFile`, `AddPlaylist`, option operations) and apply it in order; classify the playlist path at the same boundary as every other input. Add a mixed `--playlist` and positional test plus a `file://` playlist case.
- **Fix with:** api-surface-and-module-boundaries
- **Effort:** M
- **Design:** none

### PLAY-15 · P3 · `state_change != 0` treats unsupported options as changed

- **Area:** Errors
- **Where:** `player/command.c:4339-4361`, `:7839-7841`, `rust/player-ffi/src/state.rs:84-105`, `rust/player-core/src/state.rs:58-91`
- **Problem:** `control_state_sync_option` returns `-1` for an option it does not own, for a null handle, and for an FFI failure. The caller treats every nonzero value as a change and notifies the property. Every unrelated option change therefore emits a property-change event, and a real FFI failure is indistinguishable from a value change. `set_number` also returns `-1` for a supported property with a non-finite or unknown value, collapsing three outcomes.
- **Recommendation:** define a tri-state or four-state result (`0` unchanged, `1` changed, `-1` unsupported, `-2` error) across the FFI, notify only on `> 0`, and return unsupported only for an unknown name. Add a test for an unrelated option and for a rejected value.
- **Fix with:** error-model
- **Effort:** S
- **Design:** none

### PLAY-16 · P3 · The Lua FFI drops event, panic, and join failures without diagnostics

- **Area:** Errors
- **Where:** `rust/player-ffi/src/lua.rs:736-747`, `:765`, `:837`
- **Problem:** A node-conversion error or a non-map event is discarded, so the script silently stops receiving that event. `let _ = self.thread.join()` ignores a panic in the pump thread, after which the script runs with no events. The `Err(_)` arm returns `-1` without `callbacks.log`, so a panic inside `player_ffi_lua_run` looks like a load failure with no cause.
- **Recommendation:** log the dropped conversion error, the join error, and the panic payload at `MSGL_ERR`, naming the event and the script.
- **Fix with:** error-model
- **Effort:** S
- **Design:** none

### PLAY-17 · P3 · The audio bridge does not contain panics on reset/destroy and loses error categories

- **Area:** Errors
- **Where:** `rust/player-audio/src/lib.rs:154-160`, `:226-243`, `audio/decode/ad_symphonia.c:228-232`, `:242-253`, `audio/decode/ad_lavc.c:188-209`
- **Problem:** `player_audio_create` and `player_audio_decode` catch panics; `player_audio_reset` and `player_audio_destroy` do not, so a panic there aborts at the ABI. `player_audio_create` discards its `ERR_ARG`/`ERR_UNSUPPORTED` category and returns only `NULL`. `ad_symphonia.c` fails the whole audio chain on any decode error, while `ad_lavc.c` logs and continues. The output frame setup ignores `mp_aframe_set_rate` and a null `mp_aframe_get_data_rw`, then copies into `data[0]`.
- **Recommendation:** wrap reset and destroy in `catch_unwind`; return the create error category through the ABI; treat a decode error as a skipped frame with a log unless the state is corrupt; check the frame rate and data pointer before the copy.
- **Fix with:** error-model
- **Effort:** S
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
