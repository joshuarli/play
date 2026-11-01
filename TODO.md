Prefix: PLAY
Next: 20

# Backlog

## P2

## P3

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
