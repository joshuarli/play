# Architecture

Player is one process: the retained mpv playback pipeline in C, a Rust control plane behind a C ABI, and one Lua 5.2 interpreter for the on-screen controls. It opens local files only.

## Player shape

- **C:** playback loop, demux and decode pipeline, filters, subtitles, video output (`gpu-next` over libplacebo, Vulkan, and MoltenVK), CoreAudio, Cocoa platform glue, client event delivery, track-list payloads, and execution of the supported commands.
- **Rust:** startup configuration, playlist ownership, command validation, scalar playback state and change detection, local file I/O, the Symphonia audio bridge, and the Lua host.
- **Lua:** the built-in on-screen controls and any script the user loads explicitly. The interpreter is vendored Lua 5.2.4 through `mlua`.

The release app links the Rust plane as one `libplayer_release.dylib`. The development build links `player-ffi` and `player-audio` as static libraries.

## Input boundary

`player-core::input::classify_input` is the single policy for input strings. It accepts ordinary local paths, including paths with spaces, Unicode, `:`, relative components, symlinks, and mounted volumes. It rejects every `scheme://` URL, the known protocol wrappers (`data:`, `lavfi:`, `edl:`, `memory:`, `fd:`, `av:`, `mf:`, and the rest), and network prefixes. `file://` is converted only when the caller passes `allow_file_uri`; drag-and-drop does this for an existing local path before queueing `loadfile`.

The classifier is called at startup, IPC and script `loadfile`, `--playlist` and `loadlist` paths, playlist entries, and external audio or subtitle files. One limit is open:

- Startup parsing rejects a non-UTF-8 argument before the classifier runs. The classifier and the local-file FFI take UTF-8, so they are narrower than upstream mpv's byte-transparent paths. macOS paths are Unicode in normal use.

The stream layer registers `file` and the internal client callback only. FFmpeg is configured with `--disable-network` and a `file`-only protocol allowlist. Network protocols, TLS, and a CA bundle are not built.

## Playback pipeline

FFmpeg demuxes every container and decodes video. VideoToolbox is the hardware path for H.264 and HEVC. Symphonia decodes AAC, ALAC, FLAC, MP3, Opus, PCM, and Vorbis from packets supplied by the FFmpeg demuxer. `libswresample` converts and resamples audio. A Symphonia decode error logs and skips the packet; argument, unsupported, and panic statuses fail the audio filter. The renderer is `vo=gpu-next` over libplacebo and Vulkan through MoltenVK; there is no usable macOS OpenGL context in mpv v0.41.0.

`audio/out/ao_coreaudio.c` skips the `kAudioOutputUnitProperty_ChannelMap` call. macOS 27 rejects the upstream channel-layout value with error -50; the audio unit's default mapping plays mono, stereo, and 5.1. Revisit this when upstream fixes the property, and keep the regression check.

## Rust control plane

- **Startup:** `player-core::config` parses the supported command line into a typed `StartupConfig`. `player/main.c` applies the fields and sends only the remaining engine options to the C parser. `--version`, `--help`, and unknown-option failures stay in C.
- **Playlist:** `player-core::playlist` owns order, IDs, the current entry, insertion, removal, and next and previous. `player-ffi` maps C `playlist_entry` pointers to Rust IDs. C owns the entry payloads and their talloc lifetime.
- **Commands:** `player-core::command` validates the supported vocabulary before the C parser builds an `mp_cmd`. The checker accepts `auto` track selection and `mm:ss` seek targets. Node commands do not pass through it; they use the C parser directly.
- **State:** `player-core::state::PlaybackState` owns pause, fullscreen, mute, volume, speed, time-pos, duration, and EOF, with a change sequence. C property getters read the Rust store and C reports facts through the FFI. A state update returns changed, unchanged, unsupported, or error; only a change notifies the property. The playloop refreshes fullscreen from the option each pass, so a window-initiated change reaches the Rust store and notifies the property.
- **Local file:** `player-core::local_file::LocalFile` owns open, read, write, seek, size, and EOF with typed errors. `stream/stream_file.c` is the stream backend over that handle.

## FFI contract

- Handles are opaque. The C caller owns each handle and must not use it after destruction.
- Strings and errors use caller-provided buffers. `BUFFER_TOO_SMALL` is a distinct result.
- Errors are typed where the ABI defines an enum. The command check returns an error string and `-1`; audio create and decode return a `player_audio_status` value.
- No Rust unwind may cross the ABI. `player-ffi` wraps the boundary entry points in `catch_unwind`; the state, playlist, and local-file entry points currently rely on their code not panicking. `player-audio` wraps every entry point.
- The audio decoder handle is not thread-safe. The caller serializes access. There are no shared mutable globals.
- The Lua backend and the audio bridge are the only Rust code with `unsafe`; every block states its safety argument.

## Script host

- **Interpreter:** `rust/player-lua` owns one Lua state per script through `mlua` 0.12.2 with `lua52` and `vendored`. The only interpreter in the process is the vendored Lua 5.2.4. Lua 5.2.4 is the newest non-JIT interpreter mpv v0.41.0 supports; LuaJIT and Lua 5.3 are not options.
- **Libraries:** the interpreter opens `io`, `os`, and `package`, which the built-in modules use. It does not open `debug`. The host is not a sandbox: scripts can open files, run processes with `os.execute`, and send the full mpv command table, including `subprocess` and `run`. Explicitly loaded scripts are trusted code.
- **Built-in modules:** `defaults.lua`, `assdraw.lua`, `options.lua`, `input.lua`, and `osc.lua`.
- **Values:** Lua-to-native table conversion is bounded at 64 levels. Arrays and maps keep the `mp.ARRAY`/`mp.MAP` sentinels, and map entries are sorted so command nodes are deterministic.
- **Threading:** each script runs on its own thread; the C script thread pumps mpv events into a bounded queue (1024) that `raw_wait_event` drains. A full queue drops only `property-change`, `tick`, and `log-message` events and the script is told how many; shutdown, client messages, and replies are always queued. After the mpv shutdown event the script has 2 s to return. If it does not, the C thread logs an error, closes the client gate so every later `mp.*` call from the abandoned thread fails instead of touching the freed client, and returns so the player can exit. The spinning thread is not killed.
- **OSC:** one bottom bar with play and pause, seek, elapsed and duration, volume and mute, fullscreen, and optional track selectors. It publishes `user-data/osc/areas`, `user-data/osc/margins`, and `user-data/osc/visibility`, and registers the `osc-message`, `osc-visibility`, `osc-show`, and `osc-hide` messages. The OSC stays in Lua; revisit a Rust rewrite only if the script host exceeds 10% of the distributable footprint or 50 ms of cold startup.

## Release and dependencies

`scripts/build-deps.sh` builds Vulkan-Headers and Loader, dav1d, FreeType, FriBiDi, HarfBuzz, shaderc, libplacebo, libass, and FFmpeg from pinned sources, and fetches the checksum-pinned MoltenVK binary. `scripts/release.sh` removes the old app, builds the Rust cdylib, configures mpv against the release prefix, and calls `scripts/bundle.py` to copy the non-system closure into `Player.app/Contents/Frameworks`, rewrite install names to `@rpath`, add the executable rpath, bundle MoltenVK and its ICD manifest, write `Info.plist`, write a build stamp with the revision and release binary hash, and ad-hoc sign the app.

`player/main.c` sets `VK_DRIVER_FILES` and `VK_ICD_FILENAMES` to the bundled ICD before renderer initialization. No Homebrew path is linked into the release.

The bundler sets `MPVBUNDLE=true` in `Info.plist`; the Cocoa layer uses that value to choose the regular activation policy, the Dock icon, and the menu bar. It also declares the movie, audio, and playlist types and bundles the icon. A bundle launch with no file stays idle so a Finder open event can play.

A bundle launch that exits with a load error shows one alert with the log path. A rejected `loadfile` command shows an OSD message and leaves the current file playing.

## Linux

Linux x86-64 musl uses the same pipeline, Rust plane, and Lua host as macOS. The platform split is explicit in Meson and `#if`:

| Area | macOS | Linux |
|---|---|---|
| Video | `gpu-next`, Vulkan over MoltenVK, Cocoa | `gpu-next`, Vulkan, Wayland (`--gpu-context=waylandvk`) |
| Audio | CoreAudio | ALSA |
| Rust link | `libplayer_release.dylib` | `libplayer_ffi.a` and `libplayer_audio.a` |
| Build | `scripts/build.sh`, `scripts/release.sh` | `scripts/build-linux.sh` |

Files restored from the mpv v0.41.0 pin (`41f6a645068483470267271e1d09966ca3b9f413`) with headers intact: `video/out/wayland_common.c`, `video/out/wayland_common.h`, `video/out/vulkan/context_wayland.c`, `audio/out/ao_alsa.c`, `osdep/als-linux.c`, `osdep/language-posix.c`, and `osdep/timer-linux.c`, `video/out/hwdec/hwdec_vaapi.c`, and `video/filter/vf_vavpp.c`. `audio/out/ao.c` registers `alsa` under `HAVE_ALSA`.

`scripts/build-linux.sh` links the static prefix from `scripts/build-deps-linux.sh` and disables `libavdevice`; the local-only player has no capture devices. The Rust bridges link as one `libplayer_release.a` built with `--crate-type staticlib`, so the binary has one Rust runtime. The executable needs only musl, `libvulkan.so.1`, `libwayland-client.so.0`, `libwayland-cursor.so.0`, `libxkbcommon.so.0`, `libasound.so.2`, `libva.so.2`, `libva-drm.so.2`, and `libva-wayland.so.2`, and `scripts/audit-linux.sh` enforces that set. VA-API hardware decode is opt-in: `hwdec_vaapi.c` exports a surface as a dmabuf and `dmabuf_interop_pl.c` imports it into Vulkan; the VA display comes from the Wayland connection, or from a DRM render node for `vaapi-copy`. `meson.build` no longer requires mpv's `drm` feature for `vaapi-drm`; that feature, with its VO, stays off. Remaining Linux gaps: fractional scale, FIFO pacing, real input, and audible output are owner-run (`docs/manual-checklist.md`); there is no hardware decode other than VA-API, no glibc build, and no system-font fallback beyond fontconfig and the installed fonts.

## Provenance

- mpv v0.41.0 at `41f6a645068483470267271e1d09966ca3b9f413`.
- FFmpeg n9.0.2 at `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`.
- Symphonia at `b6336209bb40d57e4ed71079cf7f7fc6c4626aae`, MPL-2.0, vendored under `vendor/symphonia`.
- The repository history was consolidated to the `campaign-base` root commit. Upstream mpv and Symphonia histories are not retained; the pins above are the provenance.

## Follow-on

The A/V scheduler, general demux and decode orchestration beyond the Symphonia audio bridge, and renderer integration remain outside this scope. The Linux x86-64 musl gpu-next build is the active campaign (`PLAN.md`). A network capability, a second Lua interpreter, or automatic user config and scripts would need an explicit decision.
