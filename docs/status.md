# Status

Player is a local-only media player. The macOS release artifact is `dist/Player.app`; the Linux x86-64 musl artifact is `dist/player-x86_64-linux-musl.tar.gz`.

## Current state

- The app was rebuilt at `cbb4ec2`, and the full verification was re-run on the macOS host after the Linux campaign.
- `tests/run.sh suite`: 303 passed, 0 failed. The run includes the release gate.
- `scripts/audit.sh`: ok.
- `scripts/measure.sh final`: ok at `cbb4ec2`; the host was quiet before and after the run.
- App: 28,186,511 B; executable 9,640,096 B; framework closure 17,150,384 B in 13 files; bundled icon 1,387,652 B.
- One `file` protocol and one Lua interpreter.

## Releases

- `.github/workflows/release.yml` runs on a `v*` tag: the macOS and Linux build workflows run as reusable jobs, then one job publishes `Player-aarch64-apple-darwin.zip`, `player-x86_64-linux-musl.tar.gz`, and `SHA256SUMS`.
- `sh dl player` in `~/usr` installs the latest release on macOS and x86-64 Linux after verifying the checksum.
- Push a `v*` tag to create the first release; none exists yet.

## Limits

- Local files only. No network protocol, TLS, CA bundle, or remote resource load.
- GUI, HDR and 10-bit color, subtitle shaping, Retina scaling, audio audibility, real mouse and drag, and signing or notarization are owner-run. See `docs/manual-checklist.md`.
- Clipboard commands are compiled and reachable. Encoding, recording, and the lavfi filter graph are compiled but unreachable because the release FFmpeg has no encoders or muxers.
- The `subprocess` command and the `.run` script backend are reachable from explicit scripts and IPC. No built-in module uses them.
- Follow-on: A/V scheduler, general demux and decode orchestration, and renderer integration.
- Linux: glibc is explicitly out of scope. aarch64, X11, and a real-GPU, zero-copy VA-API, real-compositor, and real-ALSA check are outside the automated gates. The headless gate uses lavapipe with `--gpu-sw` and `mailbox` swap because headless Weston never releases FIFO buffers.

## Linux

- `scripts/linux-container.sh tests/run.sh suite`: passes on Alpine x86_64, including `linux-gpu-next` under headless Weston with lavapipe, `linux-audio` on the ALSA null PCM, `linux-local-only` under `unshare -rn`, and `linux-audit`.
- `scripts/audit-linux.sh`: ok. `dist/player` needs exactly nine libraries: musl, Vulkan, Wayland client and cursor, xkbcommon, ALSA, and libva, libva-drm, and libva-wayland.
- `--hwdec` defaults to `auto` on Linux and `no` on macOS. `suite` passes 297 checks with and without a GPU render node in the container.
- `tests/run.sh linux-vaapi` (needs a GPU render node, not part of `suite`): passes on an AMD Raphael iGPU for h264, hevc, vp9, and av1 with `vaapi-copy`.
- Fixtures match the tracked stream structure; their bytes differ from macOS and between Linux hosts because the encoders select threads and SIMD paths at run time.
- Measurements are in `docs/metrics.md`; the runtime set, headless gates, and fixtures are in `docs/linux.md`.

## Evidence

- `docs/final-report.md`: release outcome, provenance, and limits.
- `docs/metrics.md`: measurement history and budget results.
- `docs/repo-audit.md`: security, license, and complexity audit.
- `TODO.md`: open defects and their fixes.

First command: `tests/run.sh suite && scripts/audit.sh`.
