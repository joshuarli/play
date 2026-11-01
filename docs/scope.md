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

## Remaining technical debt

- `stream_memory` remains as an internal helper for charset conversion; it has no protocol registration.
- `demux_timeline` and the CUE parser remain for local MKV ordered chapters and embedded CUE metadata.
- `demux_lavf` keeps m3u8/HLS extension heuristics, but the FFmpeg HLS demuxer is not built.
- P2.3 removes encoding, recording, JavaScript, plugins, and unused UI/platform integrations.
