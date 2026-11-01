# Scope

The player is local-only. It opens filesystem paths and local playlist files; it does not implement network protocols, remote playlists, capture, or disc access.

## Retained

- `stream_file`: `file` and the empty protocol, plus stdin/stdout.
- `stream_cb`: the internal client API stream callback.
- `stream_memory`: internal use only for local subtitle charset conversion; it is not registered as a protocol.
- `demux_lavf`: FFmpeg demuxing for local containers.
- `demux_mkv`, `demux_mkv_timeline`: local Matroska and ordered chapters.
- `demux_playlist`: local M3U/M3U8/PLS files. URL entries are not rejected until P3.1.
- `demux_timeline`: the generic timeline demuxer used by MKV ordered chapters.
- `demux/cue.c`: embedded CUE metadata parsing.
- `demux/demux_lavf.c` retains its local file/subtitle charset conversion path.

## Removed in P2.2

- Stream modules: cookies, AVDevice, Blu-ray, CDDA, concat, DVB, DVDNAV, EDL, libavformat/lavf, libarchive, image-file (`mf`), `mpv`, null, and slice.
- Stream wrappers/protocols: `av://`, `avdevice://`, `memory://`, `hex://`, `null://`, `mf://`, `edl://`, `appending://`, `slice://`, `fd://`, `fdclose://`, `mpv://`, `ffmpeg://`, and `lavf://`.
- Demuxers: disc, external CUE (`demux_cue`), EDL, libarchive, image-file, `mpv`, null, and raw audio/video.
- `player/lua/ytdl_hook.lua` and the stream-lavf, raw-demuxer, disc, and capture option tables.

## Boundary status

`build/mpv --list-protocols` contains only `file://`. The protocol handlers for network and wrapper inputs are gone; P3.1 adds the path-shaped rejection boundary and the tests that exercise it. `demux_playlist` still parses local playlist entries, and a URL entry is rejected when P3.1 lands.

## Removed in P2.3

- Platform backends: Windows, Linux, and Android audio outputs, video outputs, window systems, GL/OpenGL, Vulkan display/X11/Wayland/Windows contexts, and their supporting sources.
- Video outputs: image, tct, kitty, sixel, caca, X11, XV, Wayland, DRM, SDL, VA-API, VDPAU, Direct3D, MediaCodec, and the GL `vo_gpu`.
- Scripting/plugins: MuJS JavaScript, VapourSynth, C plugins, Touch Bar, and Media Player integration.
- Unused built-in Lua modules: `auto_profiles`, `commands`, `console`, `context_menu`, `fzy`, `positioning`, `stats` (the six retained modules are unchanged).
- Filter entry points for lavfi/VapourSynth/VA-API/VDPAU/D3D11/fingerprint/AC3 encode; the built-in filter registry now contains only the local filters.
- macOS GL/libmpv-GL sources that were only used by removed paths; the used clipboard property/command bridge remains.

## Retained as technical debt

- Encoding, stream recording, and the `f_lavfi` graph remain compiled but are unreachable in the shipped configuration: the minimal FFmpeg build has no encoders or muxers. P2.4 removes their CLI surface, and P8 audits the remaining source.
- Clipboard remains because the clipboard property/commands and the macOS app bridge still reference it.

## CLI surface (P2.4)

Supported: `player FILE...`, `--fullscreen`/`--fs`, `--start`, `--audio-file`/`--audio-files`, `--sub-file`/`--sub-files`, `--version`, `--help`, and the playback/track/OSD controls used by the OSC and local playback.

Removed: `--ytdl*`, `--hls-bitrate`, `--lavfi-complex`, `--stream-dump`, `--stretch-dvd-subs`, the encoding group (`--o`, `--of`, `--ovc`, `--oac`, their `*opts`, `--orawts`, and metadata flags), the Windows window/capture group, and the options for deleted Lua overlay modules. `--http-header-fields` and the other network groups were already absent after P2.2. The encode suboption table is now empty, so the retained encode implementation has no CLI entry point.

## Local-only boundary (P3.1)

`mp_input_is_local_path` is the single input classifier. It accepts ordinary local paths, including paths with spaces, Unicode, `:`, relative components, symlinks, and mounted network volumes (the player itself makes no network connection). It rejects every `scheme://` URL and the `data:`, `lavfi:`, `edl:`, `memory:`, `fd:`, `fdclose:`, `av:`, `avdevice:`, `mf:`, `null:`, `slice:`, `appending:`, `mpv:`, `ffmpeg:`, `lavf:`, and known network prefixes. `file://` is converted only from drag-and-drop when it resolves to an existing path; the CLI, IPC loadfile, playlists, and external files reject it.

The boundary is applied at startup and CLI playlist insertion, IPC `loadfile`, M3U/PLS/TXT playlist entries, external audio/subtitle files, and drag-and-drop. A remote playlist entry is skipped with an error while the local entries continue; a remote subtitle is rejected while the main file plays.

Automatic user config and scripts are disabled by default (`load_config=false`, `auto_load_scripts=false`); the built-in OSC remains. The sandbox gate uses `(version 1)(allow default)(deny network*)`; this host requires the explicit default allow to execute the player under `sandbox-exec`.

## Boundary audit (P3.2)

`tests/local_only.py` carries the URL matrix. The local-only gate also creates a hostile temporary `HOME` with `mpv.conf`, `input.conf`, and a nonexistent script; playback succeeds without loading them. `scripts/audit.sh` runs the FFmpeg audit, build gate, and local-only gate, then rechecks `--list-protocols` and `otool -L` for network protocols and stray linked libraries.

## Remaining technical debt

- `stream_memory` remains as an internal helper for charset conversion; it has no protocol registration.
- `demux_timeline` and the CUE parser remain for local MKV ordered chapters and embedded CUE metadata.
- `demux_lavf` keeps m3u8/HLS extension heuristics, but the FFmpeg HLS demuxer is not built.
- The P2.3 source list still contains dead platform conditionals; P2.5 simplifies it.


## Removed in P7.2

- Playlist shuffle/unshuffle commands and state.
- `--shuffle`, `--loop-playlist`, `--loop-file`, and `--loop` options and the remaining-file-loops property. A-B loop remains.
