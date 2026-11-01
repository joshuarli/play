# Architecture

## Audio decode boundary (P1.2)

- **Demuxing:** FFmpeg (`demux_lavf`) remains the only demuxer. It produces encoded audio and video packets and the per-stream codec parameters. Keeping one demuxer preserves mpv's existing timestamp, seek, and A/V scheduling behavior.
- **Video decode:** FFmpeg decodes video through `vd_lavc`, including the VideoToolbox hardware path.
- **Audio decode:** Symphonia, vendored at `vendor/symphonia` (commit `b6336209bb40d57e4ed71079cf7f7fc6c4626aae`, MPL-2.0), decodes AAC, ALAC, FLAC, MP3, Opus, PCM, and Vorbis.
- **Bridge:** `rust/player-audio` is a Rust staticlib. `audio/decode/ad_symphonia.c` maps `mp_codec_params` to `struct player_audio_params`, and the Rust side creates a Symphonia `AudioDecoder` from `AudioCodecParameters`. The C ABI is in `rust/player-audio/include/player_audio.h`.
- **Data flow:** the demuxer packet bytes and timestamps go to `player_audio_decode`; Symphonia returns interleaved f32 PCM in a handle-owned buffer; the C filter copies it into an `mp_aframe` (`AF_FORMAT_FLOAT`) and writes it to the decoder output pin. The buffer is valid until the next decode or destroy call.
- **Selection:** `mp_decoder_entry` carries a driver id. `audio_decoder_list()` registers Symphonia first and libavcodec second. P1.2 used libavcodec as a fallback while the bridge matured; P1.3 removed the FFmpeg audio decoders, so Symphonia is the only audio decoder in the final link.
- **Extradata:** mpv copies subtitle extradata into `mp_codec_params.extradata`; for audio and video the original `lav_codecpar` holds it. `ad_symphonia.c` prefers `codec->extradata` and falls back to `codec->lav_codecpar`.
- **Lifetime and errors:** the Rust handle is owned by the C filter and destroyed on filter destroy. Every FFI entry point catches panics and writes a bounded error string. No Rust unwind crosses the C boundary. Decode errors fail the audio filter and are logged.
- **Build:** `scripts/build.sh` builds `rust/Cargo.toml -p player-audio` with `CARGO_TARGET_DIR=.work/deps/player-audio/target`, then passes `-Dplayer-audio-lib=<staticlib>` to Meson. Meson links the archive through `declare_dependency(link_args: ...)`. P4.1 replaces this temporary option with the workspace's Meson/Cargo integration.
- **Not changed:** mpv's A/V scheduler, seek pipeline, audio filters, CoreAudio output, and demuxer. Symphonia does not demux; a second demuxer would need a separate synchronization design and is follow-on work.

## License boundary

Symphonia is MPL-2.0. mpv remains GPL-2.0-or-later. Symphonia's source and license are preserved in `vendor/symphonia`. The release must carry the MPL notice and source availability; `docs/licenses.md` records the inventory and P8.3 audits it.

## Feature surface (P2.1)

`scripts/build.sh` sets all 94 Meson feature options explicitly. Enabled features are `cocoa`, `coreaudio`, `vulkan`, `videotoolbox-pl`, `swift-build`, `iconv`, `zlib`, and the four macOS SDK feature checks. Every other feature option is `disabled`, including all non-macOS audio/video outputs, GL/EGL, X11, Wayland, DRM, VAAPI, VDPAU, SDL, capture, disc, JavaScript, VapourSynth, plugins, and shader compilers that are only used on Windows.

`tests/run.sh build` parses `meson.options` and `meson configure build` and fails if any feature outside that enabled set is not `disabled`, or if an expected feature is not `enabled`. It also rejects unsupported linked libraries.

The embedded Lua modules are `defaults.lua`, `assdraw.lua`, `options.lua`, `input.lua`, `select.lua`, and `osc.lua`. P2.1 reduced the set; P5.5 removed the `.inc` generation and the C `player/lua.c` host, so the Rust host embeds these six files and `scripting.c` only routes the backend. `ytdl_hook`, `stats`, `console`, `auto_profiles`, `positioning`, `commands`, `context_menu`, and `fzy` are not embedded or loaded.

## Input surface (P2.2)

The stream list contains `stream_file` and `stream_cb` only; the demuxer list contains the local directory, Matroska, lavf, playlist, and timeline demuxers. `--list-protocols` reports `file://` only. Network, wrapper, disc, capture, and remote-playlist modules and their option tables are removed; `docs/scope.md` records the retained local utilities and remaining debt.

## Target surface (P2.5)

When `libmpv` is disabled, the sources compile directly into the `mpv` executable; no shared `libmpv` target or dylib is produced. The build gate asserts that no shared-library target exists, no `libmpv` dylib remains, and `otool -L` does not reference `libmpv`. The macOS playback path is the explicit top-level source set; inherited optional-feature conditionals are disabled and outside the built path.

## FFI boundary (P4.3)

- Opaque handles: `player-audio` decoders are created by `player_audio_create()` and destroyed by `player_audio_destroy()`; the C caller owns the handle and must not use it after destruction. No Rust-owned pointer escapes.
- Strings and errors: `player_ffi_classify_input()` writes a NUL-terminated path or error into caller-provided buffers. It never returns Rust-owned memory, and `BUFFER_TOO_SMALL` is a typed result.
- Typed errors: `PLAYER_FFI_INPUT_REJECTED` distinguishes a policy rejection from `PLAYER_FFI_INPUT_INVALID` (empty or malformed) and `PLAYER_FFI_INPUT_BUFFER_TOO_SMALL`.
- Panics: exported functions catch unwinding before returning; no Rust panic crosses the C boundary. `player_ffi_panic_for_test()` exercises that path.
- Threading: `player-audio` decoder handles are not thread-safe; the caller serializes access. There are no shared mutable globals in the FFI.
- Callbacks: none are exposed yet. Future registration must be paired with explicit unregister/cleanup and must not run after the owning handle is destroyed.

## Script host (P5.1)

- **Interpreter:** `rust/player-lua` owns the Lua state through `mlua` 0.12.2, pinned with only `lua52` and `vendored`; the vendored source is Lua 5.2.4, the same version as the C host. No async, serde, or Tokio features are enabled.
- **Libraries:** the interpreter opens mlua's safe standard-library set. That includes `io`, `os`, and `package`, which `select.lua`, `options.lua`, and `defaults.lua` use. The `debug` library and C-module loading are not enabled.
- **Link state:** P5.5 deleted `player/lua.c`; the vendored interpreter in `player-lua` is the only Lua implementation linked into `build/mpv`.

## Script host core (P5.3)

- **Thread and events:** `rust/player-lua` runs one interpreter per script on a dedicated thread. `ScriptHost::spawn` owns the thread and one ordered event queue. `raw_wait_event` blocks on that queue with the next timer deadline, so timers do not busy-wait. `stop` posts a shutdown event and waits at most 2 s.
- **`mp` table:** the host registers the native primitives: `log`, property get/set/del/observe, `command`/`commandv`/`command_native`, `request_event`/`raw_wait_event`, `get_time`, `format_time`, `find_config_file`, and `input_set_section_mouse_area`. It also creates the `mp.utils` module table. The unchanged `defaults.lua` supplies the Lua-level helpers, timers, bindings, and OSD overlay objects; `docs/lua-api.md` is the authoritative closure.
- **Values:** `NativeValue` mirrors the native node formats. Arrays and maps keep the `mp.ARRAY`/`mp.MAP` sentinels, and map entries are sorted so command nodes are deterministic.
- **Errors:** every callback catches Rust panics and turns them into Lua errors; a script error ends the thread with a typed `HostError`. `player-lua` contains no `unsafe`.
- **Trust:** the built-in OSC is trusted application code. Explicitly loaded scripts are trusted code: the host adds no network or subprocess capability and is not a sandbox. The player command set remains the capability boundary.
- **Integration:** the safe `PlayerBackend` trait is the C-boundary seam; P5.4 adapted it to the player and P5.5 removed the C host.

## Lua host integration (P5.4)

- **Backend:** `player/lua_host.c` defines the `mp_scripting_lua` backend. It passes the script's `mpv_handle`, name, filename, path, and C helpers (log, `find_config_file`/free, input mouse area) to `player_ffi_lua_run`.
- **Client bridge:** `player-ffi` implements `PlayerBackend` over the public client API for commands, properties, observation, event requests, time formatting, and native node conversion. A per-script pump thread is the only caller of `mpv_wait_event`; commands and properties run on the Lua thread. The pump forwards ordered `ScriptEvent`s to the host queue.
- **Module order:** the host loads `defaults.lua`, then `mp.assdraw`, `mp.input`, and `mp.options`, then the script source. `@osc.lua` and `@select.lua` resolve to the embedded sources; the upstream OSC is unchanged.
- **Link:** the external Lua library is no longer linked. The vendored Lua 5.2.4 in `mlua` is the only interpreter in `build/mpv`, and `nm` shows one `luaL_newstate`.

## Single Lua host (P5.5)

- `player/lua.c`, `player/lua/meson.build`, the external Lua dependency, and the Meson `lua` option are gone. `player/lua_host.c`, `player-ffi`, and `player-lua` are the only Lua path.
- The six `.lua` files remain as the embedded source of the built-in modules; there is no `.inc` generation and no second interpreter.
- `mp.utils` host functions are implemented in `player-lua` (`split_path`, `readdir`, `file_info`, `parse_json`, `format_json`), closing the audit's host and built-in scopes. The Lua-level `mp.utils` helpers in `defaults.lua` are unchanged.
- The interpreter is vendored Lua 5.2.4 through `mlua`; no external `liblua` is linked. `nm` reports one `luaL_newstate`, and `otool -L` has no Lua dylib.
- `tests/run.sh build` checks for one interpreter; `scripts/audit.sh` checks for C host source remnants, no linked Lua dylib, and one interpreter symbol.

## Simplified OSC (P6.1)

- `player/lua/osc.lua` is one bottom bar: play/pause, seek bar with elapsed/duration, volume/mute, fullscreen, and optional audio/subtitle selectors. It has one layout and no context menus, window controls, touch handling, chapter/playlist/title elements, or compatibility branches.
- Auto-hide and pointer-move reveal remain. The OSC ticks at 20 Hz while visible, scales from the OSD dimensions for Retina, and falls back to a 1280x720 canvas when headless.
- It publishes `user-data/osc/areas` (hitboxes), `user-data/osc/margins`, and `user-data/osc/visibility`. Messages are `osc-message`, `osc-visibility`, `osc-show`, and `osc-hide`; named bindings are `play-pause`, `seek-forward`, `seek-backward`, `toggle-fullscreen`, and `toggle-osc` (key `o`).
- Size: 466 lines, down from 2977. The P6.1 metrics row records startup and first-frame timings; `tests/osc-input` drives the hitboxes and named bindings.

## OSC decision (P6.2)

D4 remains: the simplified Lua OSC on `mlua` is the permanent default. Rust owns the player core, not the overlay UI, and there is no second UI implementation.

- **Attribution:** `osc.lua` is 466 lines and 14,479 B. `libplayer_ffi.a` is a 20,951,920 B archive that includes Rust `std`, so it is not a linked-size figure; the vendored Lua 5.2.4 archive is 319,008 B. The stripped executable is 13,990,240 B. Per-symbol attribution is unreliable because macOS `nm -S` reports zero symbol sizes.
- **Startup:** nine interleaved first-frame runs gave medians of 169.23 ms default, 164.91 ms with the OSC disabled, and 169.60 ms with both built-in scripts disabled. The differences are within the observed spread.
- **Follow-on threshold:** Lua + mlua + OSC above 10% of the final distributable footprint (2.01 MiB at the current 20.06 MiB closure) or above 50 ms of cold startup. P8 evaluates this on the final closure; a breach opens a follow-on, not an in-campaign rewrite.

## Typed startup configuration (P7.1)

- `player-core::config` parses the supported startup surface into `StartupConfig`: positional files, `--fullscreen`/`--fs`, `--start` (absolute, relative, percent, chapter, or none), and `--audio-file(s)`/`--sub-file(s)` path operations. Unknown options pass through.
- `player-ffi::startup` exposes `player_ffi_startup_parse` with a callback sink. `player/main.c` applies the typed fields directly to `MPOpts`/`MPContext` (fullscreen, `play_start`, path lists, playlist entries) and sends only the remaining engine/test flags to `m_config_parse_mp_command_line`.
- `--version`, `--help`, engine options, and removed-option failures stay in the C option frontend; there is no second dynamic-option framework. The option table remains for help and runtime properties.
- The local-only boundary still classifies paths at load; the startup parser only classifies argv shape.

## Rust playlist ownership (P7.2)

- `player-core::playlist` owns the entry order, IDs, current entry, completed/started flags, insertion, removal, clear, move, and next/previous. Entries are identified by a `u64`; the pure list is unit-tested.
- `player-ffi` exposes an opaque `player_playlist` handle that maps Rust IDs to the C `playlist_entry` pointers. `common/playlist.c` is now a thin C shim: entry payloads (filename, title, params, flags) and their talloc lifetime stay at the C boundary, while the C list arrays and current/index fields are gone. Callers use `playlist_get_current`, `playlist_entry_count`, `playlist_entry_from_index`, and the other accessors.
- `playlist_transfer_entries`/`playlist_transfer_entries_to` move entries into the destination, assign new IDs, and steal the talloc ownership and playlist-path strings; `playlist_parse_file` uses the same path.
- Shuffle/unshuffle and the `--shuffle`, `--loop-playlist`, `--loop-file`, `playlist-shuffle`, and `playlist-unshuffle` surfaces are removed. A-B loop remains.
- Local-only rejection is unchanged; URL entries are rejected at the boundary.

## Open work

- P1.4 added the FFmpeg manifest gate over the frozen allowlist.
- P2.1 locked the Meson feature surface and reduced the embedded Lua modules.
- P2.2 restricted the input surface to local files.
- P2.3 removed the unsupported platform, UI, scripting, plugin, and filter sources; encoding/recording/lavfi and clipboard remain compiled as documented debt because the minimal FFmpeg has no encoders or muxers.
- P2.4 reduced the user-visible CLI and config surface; the encode/lavfi/recording debt has no remaining CLI entry point.
- P2.5 removed the shared `libmpv` target from the locked build and verified the executable-only target surface.
- P3.1 enforces the local input boundary at CLI, IPC, playlists, external files, and drag-and-drop; automatic user config/scripts are disabled by default.
- P3.2 locked the boundary with the URL/playlist/hostile-config tests and `scripts/audit.sh`.
- P4.1 added `player-core`/`player-ffi`, built both static libraries, and linked/called `player_ffi_abi_version` from the player.
- P4.2 moved the input classifier to `player-core::input`; the C boundary delegates through `player_ffi_classify_input` with typed results.
- P4.3 documented the FFI invariants and added the C bridge test.
- P7.2 moved local playlist ownership into Rust and removed shuffle/repeat; P7.3 types the command and input dispatch.
- P7 moves config, playlist, input, state, and local file handling into Rust; the audio bridge stays at the decode boundary.
