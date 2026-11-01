# Architecture

Player is one process: the retained mpv playback pipeline in C, a Rust control plane behind a C ABI, and one Lua 5.2 interpreter for the on-screen controls. It opens local files only.

## Player shape

- **C:** playback loop, demux and decode pipeline, filters, subtitles, video output (`gpu-next` over libplacebo, Vulkan, and MoltenVK), CoreAudio, Cocoa platform glue, client event delivery, track-list payloads, and execution of the supported commands.
- **Rust:** startup configuration, playlist ownership, command validation, scalar playback state and change detection, local file I/O, the Symphonia audio bridge, and the Lua host.
- **Lua:** the built-in on-screen controls and any script the user loads explicitly. The interpreter is vendored Lua 5.2.4 through `mlua`.

The release app links the Rust plane as one `libplayer_release.dylib`. The development build links `player-ffi` and `player-audio` as static libraries.

## Input boundary

`player-core::input::classify_input` is the single policy for input strings. It accepts ordinary local paths, including paths with spaces, Unicode, `:`, relative components, symlinks, and mounted volumes. It rejects every `scheme://` URL, the known protocol wrappers (`data:`, `lavfi:`, `edl:`, `memory:`, `fd:`, `av:`, `mf:`, and the rest), and network prefixes. `file://` is converted only when the caller passes `allow_file_uri`; drag-and-drop does this for an existing local path before queueing `loadfile`.

The classifier is called at startup, IPC and script `loadfile`, playlist entries, and external audio or subtitle files. Two limits are open:

- `--playlist=FILE` and `loadlist FILE` reach the stream layer directly. The stream layer accepts `file://` and fails closed on network schemes, but the classifier is not applied to the playlist path itself.
- Startup parsing rejects a non-UTF-8 argument before the classifier runs. The classifier and the local-file FFI take UTF-8, so they are narrower than upstream mpv's byte-transparent paths. macOS paths are Unicode in normal use.

The stream layer registers `file` and the internal client callback only. FFmpeg is configured with `--disable-network` and a `file`-only protocol allowlist. Network protocols, TLS, and a CA bundle are not built.

## Playback pipeline

FFmpeg demuxes every container and decodes video. VideoToolbox is the hardware path for H.264 and HEVC. Symphonia decodes AAC, ALAC, FLAC, MP3, Opus, PCM, and Vorbis from packets supplied by the FFmpeg demuxer. `libswresample` converts and resamples audio. The renderer is `vo=gpu-next` over libplacebo and Vulkan through MoltenVK; there is no usable macOS OpenGL context in mpv v0.41.0.

`audio/out/ao_coreaudio.c` skips the `kAudioOutputUnitProperty_ChannelMap` call. macOS 27 rejects the upstream channel-layout value with error -50; the audio unit's default mapping plays mono, stereo, and 5.1. Revisit this when upstream fixes the property, and keep the regression check.

## Rust control plane

- **Startup:** `player-core::config` parses the supported command line into a typed `StartupConfig`. `player/main.c` applies the fields and sends only the remaining engine options to the C parser. `--version`, `--help`, and unknown-option failures stay in C.
- **Playlist:** `player-core::playlist` owns order, IDs, the current entry, insertion, removal, and next and previous. `player-ffi` maps C `playlist_entry` pointers to Rust IDs. C owns the entry payloads and their talloc lifetime.
- **Commands:** `player-core::command` validates the supported vocabulary before the C parser builds an `mp_cmd`. The checker is narrower than the C option grammar: it rejects `set aid auto` and `seek 1:30`, and node commands do not pass through it.
- **State:** `player-core::state::PlaybackState` owns pause, fullscreen, mute, volume, speed, time-pos, duration, and EOF, with a change sequence. C property getters read the Rust store and C reports facts through the FFI. The fullscreen value written by the macOS window path through `m_config_cache_write_opt` does not reach the Rust store.
- **Local file:** `player-core::local_file::LocalFile` owns open, read, write, seek, size, and EOF with typed errors. `stream/stream_file.c` is the stream backend over that handle.

## FFI contract

- Handles are opaque. The C caller owns each handle and must not use it after destruction.
- Strings and errors use caller-provided buffers. `BUFFER_TOO_SMALL` is a distinct result.
- Errors are typed where the ABI defines an enum. The command check returns an error string and `-1`; the audio create call returns `NULL` and loses its error category.
- No Rust unwind may cross the ABI. `player-ffi` wraps the boundary entry points in `catch_unwind`; the state, playlist, and local-file entry points currently rely on their code not panicking. `player-audio` wraps create and decode but not reset and destroy.
- The audio decoder handle is not thread-safe. The caller serializes access. There are no shared mutable globals.
- The Lua backend and the audio bridge are the only Rust code with `unsafe`; every block states its safety argument.

## Script host

- **Interpreter:** `rust/player-lua` owns one Lua state per script through `mlua` 0.12.2 with `lua52` and `vendored`. The only interpreter in the process is the vendored Lua 5.2.4.
- **Libraries:** the interpreter opens `io`, `os`, and `package`, which the built-in modules use. It does not open `debug`. The host is not a sandbox: scripts can open files, run processes with `os.execute`, and send the full mpv command table, including `subprocess` and `run`. Explicitly loaded scripts are trusted code.
- **Built-in modules:** `defaults.lua`, `assdraw.lua`, `options.lua`, `input.lua`, `select.lua`, and `osc.lua`. `select.lua` is loaded by default and contains process execution and URL-opening bindings; the OSC does not call it.
- **Threading:** each script gets a thread and an event pump. `raw_wait_event` blocks on an ordered queue until the next timer deadline. The queue is unbounded, and the production wait on script completion has no timeout; both are open defects.
- **OSC:** one bottom bar with play and pause, seek, elapsed and duration, volume and mute, fullscreen, and optional track selectors. It publishes `user-data/osc/areas`, `user-data/osc/margins`, and `user-data/osc/visibility`, and registers the `osc-message`, `osc-visibility`, `osc-show`, and `osc-hide` messages.

## Release and dependencies

`scripts/build-deps.sh` builds Vulkan-Headers and Loader, dav1d, FreeType, FriBiDi, HarfBuzz, shaderc, libplacebo, libass, and FFmpeg from pinned sources, and fetches the checksum-pinned MoltenVK binary. `scripts/release.sh` builds the Rust cdylib, configures mpv against the release prefix, and calls `scripts/bundle.py` to copy the non-system closure into `Player.app/Contents/Frameworks`, rewrite install names to `@rpath`, add the executable rpath, bundle MoltenVK and its ICD manifest, write `Info.plist`, and ad-hoc sign the app.

`player/main.c` sets `VK_DRIVER_FILES` and `VK_ICD_FILENAMES` to the bundled ICD before renderer initialization. No Homebrew path is linked into the release.

The bundle must set `MPVBUNDLE=true` in `Info.plist`; the Cocoa layer uses that value to choose the regular activation policy, the Dock icon, and the menu bar. The current bundler omits the key.

## Provenance

- mpv v0.41.0 at `41f6a645068483470267271e1d09966ca3b9f413`.
- FFmpeg n9.0.2 at `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`.
- Symphonia at `b6336209bb40d57e4ed71079cf7f7fc6c4626aae`, MPL-2.0, vendored under `vendor/symphonia`.
- The repository history was consolidated to the `campaign-base` root commit. Upstream mpv and Symphonia histories are not retained; the pins above are the provenance.

## Follow-on

The A/V scheduler, general demux and decode orchestration beyond the Symphonia audio bridge, renderer integration, and Linux remain outside this scope. A network capability, a second Lua interpreter, or automatic user config and scripts would need an explicit decision.
