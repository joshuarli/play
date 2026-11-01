# Manual checklist

Owner-run checks at each milestone and before release. Automated gates are headless; these checks need a window, speakers, or real input devices. Run `scripts/manual-checklist.sh --fixture-dir .work/fixtures` to print the fixture paths and commands. Do not treat these as automated gates.

| Area | Check | Result |
|---|---|---|
| Visual | H.264, HEVC, AV1, VP9, and MPEG-4 play | not run |
| Visual | Embedded and external SRT/ASS/WebVTT subtitles render and position | not run |
| Visual | VFR and non-ASCII paths play | not run |
| Visual | Fullscreen, resize, letterboxing, Retina 2x | not run |
| Visual | HDR/10-bit color and motion smoothness | not run |
| Audio | Mono, stereo, and 5.1 are audible | not run |
| Audio | Multi-audio language switching | not run |
| Audio | No clicks, dropouts, or persistent underruns | not run |
| Interaction | OSC reveal, auto-hide, seek, volume, fullscreen, track selector | not run |
| Interaction | Keyboard bindings match the shipped help | not run |
| Interaction | Drag-and-drop and external subtitle loading | not run |
| Interaction | Clean quit and repeated open/close | not run |
| Platform | Other Apple Silicon Macs and older macOS versions, if distributed | not run |
| Distribution | Signing and notarization, if distributed | not run |
