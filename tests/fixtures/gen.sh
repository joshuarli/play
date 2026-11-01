#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
FIXTURES="$ROOT/.work/fixtures"
TMP="$ROOT/.work/tmp"
TRACKED_MANIFEST="$ROOT/tests/fixtures/manifest.json"
MANIFEST="$FIXTURES/manifest.json"
FFMPEG=${FFMPEG:-ffmpeg}
FFPROBE=${FFPROBE:-ffprobe}

version=$("$FFMPEG" -version | sed -n 1p)
case "$version" in
    "ffmpeg version 9.0.2"*) ;;
    *)
        printf 'error: ffmpeg 9.0.2 is required, found: %s\n' "$version" >&2
        exit 1
        ;;
esac

mkdir -p "$TMP"
rm -rf "$FIXTURES"
mkdir -p "$FIXTURES"

ff() {
    local output="${!#}"
    local args=("${@:1:$(($# - 1))}")
    "$FFMPEG" -hide_banner -loglevel error -y -bitexact \
        "${args[@]}" -fflags +bitexact "$output"
}

video3=(-f lavfi -i "testsrc2=size=320x240:rate=24:duration=3")
video2=(-f lavfi -i "testsrc2=size=320x240:rate=24:duration=2")
audio3=(-f lavfi -i "sine=frequency=440:sample_rate=48000:duration=3")
audio2=(-f lavfi -i "sine=frequency=440:sample_rate=48000:duration=2")

ff "${video3[@]}" "${audio3[@]}" -map 0:v -map 1:a \
    -c:v libx264 -preset ultrafast -crf 28 -pix_fmt yuv420p \
    -x264-params keyint=24:min-keyint=24:scenecut=0 \
    -c:a aac -b:a 64k -ac 2 -shortest -movflags +faststart \
    "$FIXTURES/h264.mp4"

ff "${video3[@]}" "${audio3[@]}" -map 0:v -map 1:a \
    -c:v libx265 -preset ultrafast -crf 30 -pix_fmt yuv420p \
    -x265-params keyint=24:min-keyint=24:scenecut=0:log-level=none \
    -c:a aac -b:a 64k -ac 2 -shortest \
    "$FIXTURES/hevc.mkv"

ff "${video2[@]}" "${audio2[@]}" -map 0:v -map 1:a \
    -c:v libsvtav1 -preset 12 -crf 40 -pix_fmt yuv420p -g 24 \
    -c:a libopus -b:a 64k -ac 2 -shortest \
    "$FIXTURES/av1.webm"

ff "${video2[@]}" "${audio2[@]}" -map 0:v -map 1:a \
    -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -crf 40 -b:v 0 \
    -pix_fmt yuv420p -g 24 \
    -c:a libopus -b:a 64k -ac 2 -shortest \
    "$FIXTURES/vp9.webm"

ff "${video2[@]}" "${audio2[@]}" -map 0:v -map 1:a \
    -c:v mpeg4 -q:v 5 -c:a libmp3lame -b:a 96k -ac 2 -shortest \
    "$FIXTURES/mpeg4.avi"

ff "${audio3[@]}" -ac 2 -c:a flac "$FIXTURES/audio-flac.flac"
ff "${audio3[@]}" -ac 2 -c:a libmp3lame -b:a 128k "$FIXTURES/audio-mp3.mp3"
ff "${audio3[@]}" -ac 2 -c:a pcm_s16le "$FIXTURES/audio-wav.wav"
ff "${audio3[@]}" -ac 2 -c:a libopus -b:a 96k -f ogg "$FIXTURES/audio-ogg.ogg"
ff "${audio3[@]}" -ac 2 -c:a alac -f ipod "$FIXTURES/audio-alac.m4a"

ff "${audio3[@]}" -ac 1 -c:a pcm_s16le "$FIXTURES/audio-mono.wav"
ff "${audio3[@]}" -ac 6 -channel_layout 5.1 -c:a pcm_s16le "$FIXTURES/surround51.wav"

ff "${video3[@]}" "${audio3[@]}" -f lavfi -i "sine=frequency=660:sample_rate=48000:duration=3" \
    -map 0:v -map 1:a -map 2:a \
    -c:v libx264 -preset ultrafast -crf 28 -pix_fmt yuv420p \
    -x264-params keyint=24:min-keyint=24:scenecut=0 \
    -c:a:0 aac -b:a:0 64k -ac:a:0 2 \
    -c:a:1 libopus -b:a:1 64k -ac:a:1 2 \
    -metadata:s:a:0 language=eng -metadata:s:a:0 title=English \
    -metadata:s:a:1 language=jpn -metadata:s:a:1 title=Japanese \
    -shortest "$FIXTURES/multiaudio.mkv"

cat >"$FIXTURES/external.srt" <<'EOF'
1
00:00:00,200 --> 00:00:01,500
Hello from SubRip.

2
00:00:01,600 --> 00:00:02,800
Second cue.
EOF

cat >"$FIXTURES/external.ass" <<'EOF'
[Script Info]
ScriptType: v4.00+
PlayResX: 320
PlayResY: 240

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.20,0:00:01.50,Default,,0,0,0,,Hello from ASS.
Dialogue: 0,0:00:01.60,0:00:02.80,Default,,0,0,0,,Second cue.
EOF

cat >"$FIXTURES/external.vtt" <<'EOF'
WEBVTT

00:00:00.200 --> 00:00:01.500
Hello from WebVTT.

00:00:01.600 --> 00:00:02.800
Second cue.
EOF

ff "${video3[@]}" "${audio3[@]}" -i "$FIXTURES/external.srt" \
    -map 0:v -map 1:a -map 2:s \
    -c:v libx264 -preset ultrafast -crf 28 -pix_fmt yuv420p \
    -x264-params keyint=24:min-keyint=24:scenecut=0 \
    -c:a aac -b:a 64k -ac 2 -c:s srt -shortest \
    "$FIXTURES/h264-embedded-srt.mkv"

ff "${video3[@]}" "${audio3[@]}" -i "$FIXTURES/external.srt" \
    -map 0:v -map 1:a -map 2:s \
    -c:v libx264 -preset ultrafast -crf 28 -pix_fmt yuv420p \
    -x264-params keyint=24:min-keyint=24:scenecut=0 \
    -c:a aac -b:a 64k -ac 2 -c:s mov_text -shortest \
    -movflags +faststart "$FIXTURES/h264-movtext.mp4"

ff "${video2[@]}" "${audio2[@]}" -i "$FIXTURES/external.vtt" \
    -map 0:v -map 1:a -map 2:s \
    -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -crf 40 -b:v 0 \
    -pix_fmt yuv420p -g 24 \
    -c:a libopus -b:a 64k -ac 2 -c:s webvtt -shortest \
    "$FIXTURES/vp9-webvtt.webm"

ff "${video3[@]}" -c:v ffv1 -level 3 -c:a flac "$FIXTURES/unsupported-ffv1.mkv"

seg24="$TMP/vfr-24.mkv"
seg12="$TMP/vfr-12.mkv"
concat="$TMP/vfr-concat.txt"
ff -f lavfi -i "testsrc2=size=320x240:rate=24:duration=1" \
    -c:v libx264 -preset ultrafast -crf 28 -pix_fmt yuv420p \
    -x264-params keyint=12:min-keyint=12:scenecut=0 "$seg24"
ff -f lavfi -i "testsrc2=size=320x240:rate=12:duration=1" \
    -c:v libx264 -preset ultrafast -crf 28 -pix_fmt yuv420p \
    -x264-params keyint=12:min-keyint=12:scenecut=0 "$seg12"
printf "file '%s'\nfile '%s'\n" "$seg24" "$seg12" >"$concat"
ff -f concat -safe 0 -i "$concat" -c copy -fps_mode vfr "$FIXTURES/vfr.mkv"

cp "$FIXTURES/h264.mp4" "$FIXTURES/naïve 日本語.mp4"
cp "$FIXTURES/h264.mp4" "$FIXTURES/with space.mp4"
cp "$FIXTURES/h264.mp4" "$FIXTURES/colon:name.mp4"
ln -sfn h264.mp4 "$FIXTURES/link.mp4"

python3 - "$FIXTURES/h264.mp4" "$FIXTURES/truncated.mp4" <<'PY'
import pathlib, sys
source, target = map(pathlib.Path, sys.argv[1:])
data = source.read_bytes()
target.write_bytes(data[:max(4096, len(data) // 20)])
PY

python3 - "$FIXTURES/hevc.mkv" "$FIXTURES/corrupt.mkv" <<'PY'
import pathlib, sys
source, target = map(pathlib.Path, sys.argv[1:])
data = bytearray(source.read_bytes())
data[:512] = b"\0" * 512
target.write_bytes(bytes(data))
PY

python3 - "$FIXTURES" "$MANIFEST" <<'PY'
import hashlib, json, pathlib, subprocess, sys

fixtures = pathlib.Path(sys.argv[1])
manifest_path = pathlib.Path(sys.argv[2])

entries = {
    "h264": {"path": "h264.mp4", "role": "corpus", "duration": 3,
             "video": ["h264"], "audio": ["aac"], "subtitle": [], "channels": [2]},
    "hevc": {"path": "hevc.mkv", "role": "corpus", "duration": 3,
             "video": ["hevc"], "audio": ["aac"], "subtitle": [], "channels": [2]},
    "av1": {"path": "av1.webm", "role": "corpus", "duration": 2,
            "video": ["av1"], "audio": ["opus"], "subtitle": [], "channels": [2]},
    "vp9": {"path": "vp9.webm", "role": "corpus", "duration": 2,
            "video": ["vp9"], "audio": ["opus"], "subtitle": [], "channels": [2]},
    "mpeg4": {"path": "mpeg4.avi", "role": "corpus", "duration": 2,
              "video": ["mpeg4"], "audio": ["mp3"], "subtitle": [], "channels": [2]},
    "audio-flac": {"path": "audio-flac.flac", "role": "corpus", "duration": 3,
                   "video": [], "audio": ["flac"], "subtitle": [], "channels": [2]},
    "audio-mp3": {"path": "audio-mp3.mp3", "role": "corpus", "duration": 3,
                  "video": [], "audio": ["mp3"], "subtitle": [], "channels": [2]},
    "audio-wav": {"path": "audio-wav.wav", "role": "corpus", "duration": 3,
                  "video": [], "audio": ["pcm_s16le"], "subtitle": [], "channels": [2]},
    "audio-ogg": {"path": "audio-ogg.ogg", "role": "corpus", "duration": 3,
                  "video": [], "audio": ["opus"], "subtitle": [], "channels": [2]},
    "audio-alac": {"path": "audio-alac.m4a", "role": "corpus", "duration": 3,
                   "video": [], "audio": ["alac"], "subtitle": [], "channels": [2]},
    "multiaudio": {"path": "multiaudio.mkv", "role": "corpus", "duration": 3,
                   "video": ["h264"], "audio": ["aac", "opus"], "subtitle": [],
                   "channels": [2, 2]},
    "h264-embedded-srt": {"path": "h264-embedded-srt.mkv", "role": "corpus",
                          "duration": 3, "video": ["h264"], "audio": ["aac"],
                          "subtitle": ["subrip"], "channels": [2]},
    "h264-movtext": {"path": "h264-movtext.mp4", "role": "corpus", "duration": 3,
                     "video": ["h264"], "audio": ["aac"], "subtitle": ["mov_text"],
                     "channels": [2]},
    "vp9-webvtt": {"path": "vp9-webvtt.webm", "role": "corpus", "duration": 2,
                   "video": ["vp9"], "audio": ["opus"], "subtitle": ["webvtt"],
                   "channels": [2]},
    "h264-external-srt": {"path": "h264.mp4", "role": "corpus", "duration": 3,
                          "video": ["h264"], "audio": ["aac"],
                          "subtitle": ["subrip"], "channels": [2],
                          "sub_files": ["external.srt"]},
    "h264-external-ass": {"path": "h264.mp4", "role": "corpus", "duration": 3,
                          "video": ["h264"], "audio": ["aac"],
                          "subtitle": ["ass"], "channels": [2],
                          "sub_files": ["external.ass"]},
    "h264-external-vtt": {"path": "h264.mp4", "role": "corpus", "duration": 3,
                          "video": ["h264"], "audio": ["aac"],
                          "subtitle": ["webvtt"], "channels": [2],
                          "sub_files": ["external.vtt"]},
    "surround51": {"path": "surround51.wav", "role": "corpus", "duration": 3,
                   "video": [], "audio": ["pcm_s16le"], "subtitle": [],
                   "channels": [6]},
    "vfr": {"path": "vfr.mkv", "role": "corpus", "duration": 2,
            "video": ["h264"], "audio": [], "subtitle": [], "channels": []},
    "unicode": {"path": "naïve 日本語.mp4", "role": "corpus", "duration": 3,
                "video": ["h264"], "audio": ["aac"], "subtitle": [], "channels": [2]},
    "space": {"path": "with space.mp4", "role": "corpus", "duration": 3,
              "video": ["h264"], "audio": ["aac"], "subtitle": [], "channels": [2]},
    "colon": {"path": "colon:name.mp4", "role": "local-path", "duration": 3,
              "video": ["h264"], "audio": ["aac"], "subtitle": [], "channels": [2]},
    "link": {"path": "link.mp4", "role": "local-path", "duration": 3,
             "video": ["h264"], "audio": ["aac"], "subtitle": [], "channels": [2]},
    "audio-mono": {"path": "audio-mono.wav", "role": "audio", "duration": 3,
                   "video": [], "audio": ["pcm_s16le"], "subtitle": [], "channels": [1]},
    "audio-stereo": {"path": "audio-wav.wav", "role": "audio", "duration": 3,
                     "video": [], "audio": ["pcm_s16le"], "subtitle": [], "channels": [2]},
    "audio-surround51": {"path": "surround51.wav", "role": "audio", "duration": 3,
                         "video": [], "audio": ["pcm_s16le"], "subtitle": [], "channels": [6]},
    "external-srt": {"path": "external.srt", "role": "subtitle", "duration": 3,
                     "video": [], "audio": [], "subtitle": ["subrip"], "channels": []},
    "external-ass": {"path": "external.ass", "role": "subtitle", "duration": 3,
                     "video": [], "audio": [], "subtitle": ["ass"], "channels": []},
    "external-vtt": {"path": "external.vtt", "role": "subtitle", "duration": 3,
                     "video": [], "audio": [], "subtitle": ["webvtt"], "channels": []},
    "truncated": {"path": "truncated.mp4", "role": "failure", "kind": "truncated"},
    "corrupt": {"path": "corrupt.mkv", "role": "failure", "kind": "corrupt"},
    "unsupported": {"path": "unsupported-ffv1.mkv", "role": "failure",
                    "kind": "unsupported", "video": ["ffv1"], "audio": ["flac"]},
}

def probe(path):
    out = subprocess.check_output(
        ["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json",
         str(path)], text=True)
    return json.loads(out)["streams"]

for name, entry in entries.items():
    path = fixtures / entry["path"]
    if not path.exists():
        raise SystemExit(f"missing fixture: {path}")
    data = path.read_bytes()
    entry["sha256"] = hashlib.sha256(data).hexdigest()
    entry["bytes"] = len(data)
    if entry["role"] not in ("failure",):
        streams = probe(path)
        got_video = [s.get("codec_name") for s in streams if s.get("codec_type") == "video"]
        got_audio = [s.get("codec_name") for s in streams if s.get("codec_type") == "audio"]
        got_sub = [s.get("codec_name") for s in streams if s.get("codec_type") == "subtitle"]
        got_channels = [s.get("channels") for s in streams if s.get("codec_type") == "audio"]
        if entry.get("sub_files"):
            got_sub = []
        if got_video != entry.get("video", []):
            raise SystemExit(f"{name}: video {got_video} != {entry.get('video', [])}")
        if got_audio != entry.get("audio", []):
            raise SystemExit(f"{name}: audio {got_audio} != {entry.get('audio', [])}")
        expected_sub = [] if entry.get("sub_files") else entry.get("subtitle", [])
        if got_sub != expected_sub:
            raise SystemExit(f"{name}: subtitle {got_sub} != {expected_sub}")
        if got_channels != entry.get("channels", []):
            raise SystemExit(f"{name}: channels {got_channels} != {entry.get('channels', [])}")
        for sub_file in entry.get("sub_files", []):
            sub_streams = probe(fixtures / sub_file)
            sub_got = [s.get("codec_name") for s in sub_streams
                       if s.get("codec_type") == "subtitle"]
            if sub_got != entry.get("subtitle", []):
                raise SystemExit(
                    f"{name}: {sub_file} subtitle {sub_got} != {entry.get('subtitle', [])}")

manifest = {
    "generator": "ffmpeg 9.0.2",
    "fixtures": entries,
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
print(f"wrote {len(entries)} fixtures to {manifest_path}")
PY

if [ "${1:-}" = "--write-manifest" ]; then
    cp "$MANIFEST" "$TRACKED_MANIFEST"
    printf 'wrote %s\n' "$TRACKED_MANIFEST"
fi

if [ "${1:-}" = "--write-linux-hashes" ]; then
    python3 - "$MANIFEST" "$ROOT/tests/fixtures/hashes-linux.json" <<'PY'
import json
import sys

fixtures = json.load(open(sys.argv[1]))["fixtures"]
hashes = {name: {"sha256": entry["sha256"], "bytes": entry["bytes"]}
          for name, entry in sorted(fixtures.items())}
open(sys.argv[2], "w").write(json.dumps(hashes, indent=2, sort_keys=True) + "\n")
PY
    printf 'wrote %s\n' "$ROOT/tests/fixtures/hashes-linux.json"
fi

printf 'fixtures generated in %s\n' "$FIXTURES"
