#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
FIXTURE_DIR="$ROOT/.work/fixtures"
PLAYER="$ROOT/build/mpv"
RUN=0
LINUX=0

while [ $# -gt 0 ]; do
    case "$1" in
        --fixture-dir)
            FIXTURE_DIR=$2
            shift 2
            ;;
        --release)
            PLAYER="$ROOT/dist/Player.app/Contents/MacOS/player"
            shift
            ;;
        --release-linux)
            PLAYER="$ROOT/dist/player"
            LINUX=1
            shift
            ;;
        --run)
            RUN=1
            shift
            ;;
        *)
            printf 'usage: scripts/manual-checklist.sh [--fixture-dir DIR] [--release | --release-linux] [--run]\n' >&2
            exit 2
            ;;
    esac
done

if [ ! -x "$PLAYER" ]; then
    printf 'error: player not found: %s\n' "$PLAYER" >&2
    exit 1
fi

cat <<EOF
Manual checklist for $(basename "$PLAYER")
Fixture directory: $FIXTURE_DIR

These checks need a window, speakers, or real input devices. They are not
automated gates. Record pass/fail in docs/manual-checklist.md.

Visual:
  [ ] H.264, HEVC, AV1, VP9, and MPEG-4 files open and render correctly.
  [ ] Embedded and external SRT/ASS/WebVTT subtitles appear and are positioned.
  [ ] VFR and non-ASCII-path files play without stutter or path errors.
  [ ] Fullscreen, window resize, letterboxing, and Retina 2x scaling look right.
  [ ] HDR/10-bit color and motion smoothness are acceptable.
  [ ] No dropped frames at 1080p (frame-drop-count stays 0 with the window renderer).

Audio:
  [ ] Mono, stereo, and 5.1 initialize CoreAudio and are audible.
  [ ] Multi-audio track switching selects the expected language.
  [ ] No clicks, dropouts, or persistent underruns.

Interaction:
  [ ] OSC appears on pointer move and auto-hides.
  [ ] OSC play/pause, seek, volume/mute, fullscreen, and track selector work.
  [ ] Keyboard bindings match the shipped help.
  [ ] Drag-and-drop and external subtitle loading work.
  [ ] Quit and repeated open/close are clean.

EOF

if [ "$LINUX" = 1 ]; then
    cat <<'LINUX_CHECKS'
Linux (real compositor, GPU, and sound card):
  [ ] vo=gpu-next picks the GPU Vulkan device, not llvmpipe, and plays with the default FIFO swap mode.
  [ ] Fractional scale, fullscreen, resize, and cursor shape look right on sway, GNOME, or KDE.
  [ ] Keyboard and pointer input reach the OSC; click and drag seek.
  [ ] ALSA plays audibly on a hardware PCM; channel placement is right for stereo and 5.1.
  [ ] Subtitle shaping and system-font fallback look right for CJK, Arabic, and Greek text.
  [ ] The default --hwdec=auto plays zero-copy on a real compositor and hwdec-current reports vaapi.
  [ ] 10-bit HEVC and AV1 decode with --hwdec=vaapi, and a decode GPU that differs from the display GPU.
  [ ] The tarball extracts and runs on a second musl host, if distributed.
LINUX_CHECKS
fi

echo
echo "Commands:"

python3 - "$ROOT/tests/fixtures/manifest.json" "$FIXTURE_DIR" "$PLAYER" "$LINUX" <<'PY'
import json
import pathlib
import shlex
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
fixture_dir = pathlib.Path(sys.argv[2])
player = pathlib.Path(sys.argv[3])
audio = "alsa" if sys.argv[4] == "1" else "coreaudio"
for name, entry in sorted(manifest["fixtures"].items()):
    if entry.get("role") not in ("corpus", "audio", "hwdec-fallback"):
        continue
    path = fixture_dir / entry["path"]
    if not path.exists():
        continue
    print(f"  {name}: {path}")
    print(f"    {shlex.quote(str(player))} --no-config --vo=gpu-next --ao={audio} "
          f"{shlex.quote(str(path))}")
PY

if [ "$RUN" = 1 ]; then
    printf '\n--run is intentionally not automatic; start the commands above one at a time.\n'
fi
