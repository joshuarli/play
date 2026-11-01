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

## Remaining technical debt

- `stream_memory` remains as an internal helper for charset conversion; it has no protocol registration.
- `demux_timeline` and the CUE parser remain for local MKV ordered chapters and embedded CUE metadata.
- `demux_lavf` keeps m3u8/HLS extension heuristics, but the FFmpeg HLS demuxer is not built.
- The P2.3 source list still contains dead platform conditionals; P2.5 simplifies it.

