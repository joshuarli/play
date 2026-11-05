Prefix: PLAY
Next: 30

# Backlog

## P2

- PLAY-20: Wire VideoToolbox and VA-API decode requests through `player-engine` and report `hwdec-current` from the decoder that actually opened. `player-media` already carries the hardware paths and their tests; the scheduler currently requests software decode.
- PLAY-21: Confirm on a real session that `build/mpv FILE` launched from a terminal takes keyboard focus. `0e7e0a2` runs AppKit's event loop on the main thread with playback on a second thread, as mpv does; Homebrew mpv takes focus on the owner's machine, and no agent session can test focus because macOS keeps it on the app being typed in.
- PLAY-23: HE-AAC plays as its AAC-LC core. Symphonia's AAC decoder skips SBR and PS, so HE-AAC loses the band above half its sample rate and HE-AAC v2 loses parametric stereo; mpv decodes both. `tests/run.sh mpv-diff` has `aac-he.m4a` and `aac-he-v2.m4a` but checks only play-to-end.
- PLAY-24: AAC layouts outside channel configurations 1-6 and 7.1 wide fail to open: configuration 12 (7.1 rear, what FFmpeg writes) and program-config-element layouts such as quad. `tests/run.sh mpv-diff` lists them in `KNOWN_GAPS`.
- PLAY-25: With `--gui` or in the bundled app, audio files with embedded cover art show a black window: the attached-picture frame never reaches it. Without `--gui` such files play audio-only and open no window. The TUI shows the cover through its own decode.

## P3

- PLAY-26: SubRip `<i>`, `<b>`, `<u>`, and `<font color>` are stripped, not styled. `player-overlay::TextBlock` carries one style; styled spans need per-run attributes in both text backends (Core Text attributed ranges; HarfBuzz runs per span on Linux).
- PLAY-27: A right-to-left paragraph with `Align::Start` starts at the left edge. Subtitles centre, so only OSD messages and the OSC title see it; base direction should pick the start edge.
- PLAY-28: Line height is the tallest font's ascent plus descent across every fallback font on the block, on both platforms, so one CJK or Thai character spreads all lines apart; on Linux Noto Sans CJK makes it visibly wider than macOS. Line spacing should come from the primary font, with tall glyphs kept inside the inset.
- PLAY-29: Re-measure `scripts/bench.py gui-cpu --baseline` (libass build `d0e94ad`) on a quiet host. A loaded run showed seek-bar hover at 14.3% CPU against 9.0% on h264 but parity on VP9. If it holds, the likely cost is re-rasterizing the whole bar and recompositing the union in floats each time the tooltip moves; cache the static layer and repaint only the tooltip and progress.

## P4
