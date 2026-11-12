# Manual checklist

Owner-run checks before a release and after a platform change. Automated gates are headless; these checks need a window, speakers, or real input devices. Run `scripts/manual-checklist.sh --fixture-dir .work/fixtures` for the dev build, or `scripts/manual-checklist.sh --fixture-dir .work/fixtures --release` for the shipped app at `dist/Player.app/Contents/MacOS/player`, or `scripts/manual-checklist.sh --fixture-dir .work/fixtures --release-linux` for `dist/player` on Linux. Do not treat these as automated gates.

| Area | Check | Result |
|---|---|---|
| Visual | H.264, HEVC, AV1, VP9, and MPEG-4 play | not run |
| Visual | Embedded and external SRT subtitles render bottom-centre, wrap, and show Arabic, Hebrew, Devanagari, Thai, CJK, and emoji | not run |
| Visual | VFR and non-ASCII paths play | not run |
| Visual | Fullscreen, resize, letterboxing, Retina 2x | not run |
| Visual | HDR/10-bit color and motion smoothness | not run |
| Visual | No dropped frames at 1080p (`frame-drop-count` stays 0 with the window renderer) | not run |
| Audio | Mono, stereo, and 5.1 are audible | not run |
| Audio | Multi-audio language switching | not run |
| Audio | No clicks, dropouts, or persistent underruns | not run |
| Interaction | OSC reveal, auto-hide (also in fullscreen), click and drag seek, volume, fullscreen, track selector with a real mouse | not run |
| Interaction | OSC hit areas line up with the drawn controls on Retina 2x; scroll wheel over volume and seek | not run |
| Appearance | OSC on a real display at 1x and Retina 2x: bar alpha, vector icons, text, seek bar, fade, and the presented overlay blend | not run |
| Interaction | Keyboard bindings match the shipped help | not run |
| Interaction | Drag-and-drop and external subtitle loading | not run |
| Interaction | Clean quit and repeated open/close | not run |
| Platform | Other Apple Silicon Macs running the supported macOS 27 or later | not run |
| Metal | `--gpu-api=metal` beside `--gpu-api=vulkan` on a real display: color, scaling, subtitle and OSC placement, resize, fullscreen | not run |
| Metal | HDR/EDR content and an external or ProMotion display under Metal: frame pacing and tone mapping (`scripts/bench.py frames` on that display) | not run |
| Metal | Keyboard focus and window behavior under `--gpu-api=metal` | not run |
| Distribution | Ad-hoc signature verifies and the documented first-launch quarantine steps work | not run |
| Distribution | A tagged release verifies against `SHA256SUMS`, and `sh dl player` installs it on macOS and x86-64 Linux | not run |
| Linux | `gpu-next` selects the GPU Vulkan device and plays with default FIFO swap on a real compositor | not run |
| Linux | Fractional scale, fullscreen, resize, and cursor shape on sway, GNOME, or KDE | not run |
| Linux | Keyboard and pointer input reach the OSC; click and drag seek | not run |
| Linux | ALSA audible on a hardware PCM; channel placement for stereo and 5.1 | not run |
| Linux | Subtitle shaping and system-font fallback for CJK, Arabic, and Greek | not run |
| Linux | VA-API decode through the scheduler, including 10-bit HEVC and AV1 and a separate decode GPU, with negotiated `hwdec-current` | not run |
| Linux | Tarball runs on a second musl host, if distributed | not run |

Remaining checks for the P1/P2 fixes:

- Listen at 0.5×, 1×, and 2× on CoreAudio and ALSA. Check pitch, artifacts, A/V alignment, paused changes, midstream changes, seeks, and track switches.
- Check delayed audio starts and midstream gaps audibly against video.
- Check fresh GUI cover-art audio and video-to-cover transitions on macOS.
- Check a High 10 or Main 10 gradient, anamorphic content, and a remuxed 90-degree display matrix on both macOS renderers. Verify screenshots as well as the window.
- Check terminal-launch keyboard focus on Metal and Vulkan. This remains unverified; the owner could not run it during the fix session.
- Direct VideoToolbox-to-Metal IOSurface presentation remains unimplemented pending a suitable device and performance comparison.
