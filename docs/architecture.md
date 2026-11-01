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

The embedded Lua modules are reduced to `defaults.lua`, `assdraw.lua`, `options.lua`, `input.lua`, `select.lua`, and `osc.lua`. The `.inc` build targets and the `player/lua.c` / `player/scripting.c` loading tables match that set; `ytdl_hook`, `stats`, `console`, `auto_profiles`, `positioning`, `commands`, `context_menu`, and `fzy` are no longer embedded or loaded.

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
- P5.1 audits and ports the Lua API surface to `mlua`.
- P7 moves config, playlist, input, state, and local file handling into Rust; the audio bridge stays at the decode boundary.
