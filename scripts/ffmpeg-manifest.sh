#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
MODE=${1:---check}
MANIFEST=${2:-}

python3 - "$ROOT" "$MODE" "$MANIFEST" <<'PY'
import json
import os
import pathlib
import platform
import sys

ROOT = pathlib.Path(sys.argv[1])
MODE = sys.argv[2]
MANIFEST_ARG = sys.argv[3]
PIN = ROOT / "ffmpeg-runtime.lock.json"
NETWORK_SCHEMES = ROOT / "scripts" / "network-schemes.txt"
LOCAL_PROTOCOLS = {"file", "pipe", "fd"}
TARGETS = {"Darwin": "aarch64-apple-darwin", "Linux": "x86_64-unknown-linux-musl"}

REQUIRED = {
    "demuxers": {"mov", "matroska", "avi", "mp3", "flac", "wav", "ogg"},
    "decoders": {"h264", "hevc", "libdav1d", "vp9", "mpeg4", "png", "mjpeg"},
}
REQUIRED_BY_TARGET = {
    "aarch64-apple-darwin": {"hwaccels": {"h264_videotoolbox", "hevc_videotoolbox"}},
    "x86_64-unknown-linux-musl": {
        "decoders": {"av1"},
        "hwaccels": {"h264_vaapi", "hevc_vaapi", "vp9_vaapi", "av1_vaapi"},
    },
}


def usage():
    print("usage: scripts/ffmpeg-manifest.sh --check [manifest.json]", file=sys.stderr)
    sys.exit(2)


def manifest_path(target):
    if MANIFEST_ARG:
        return pathlib.Path(MANIFEST_ARG)
    prefix = os.environ.get("FFMPEG_PREFIX")
    prefix = pathlib.Path(prefix) if prefix else ROOT / ".work" / "deps" / "ffmpeg-runtime" / target
    return prefix / "share" / "ffmpeg-build" / "manifest.json"


def check():
    target = TARGETS.get(platform.system())
    if target is None:
        raise SystemExit(f"ffmpeg-audit: unsupported platform {platform.system()}")
    pin = json.loads(PIN.read_text())
    path = manifest_path(target)
    results = []

    def case(name, ok, detail=""):
        results.append(bool(ok))
        line = f"{'PASS' if ok else 'FAIL'} ffmpeg-audit: {name}"
        if detail and not ok:
            line += f": {detail}"
        print(line)

    try:
        shown = path.relative_to(ROOT)
    except ValueError:
        shown = path
    print(f"ffmpeg-audit: runtime manifest {shown}")
    try:
        manifest = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        case("manifest readable", False, f"{exc}; run scripts/fetch-ffmpeg-runtime.sh {target}")
        print(f"TOTAL 0 passed, {len(results)} failed")
        return 1
    case("manifest readable", True)

    case("profile is runtime", manifest.get("profile") == "runtime", manifest.get("profile"))
    case(f"target is {target}", manifest.get("target") == target, manifest.get("target"))
    version = manifest.get("sources", {}).get("ffmpeg", {}).get("version")
    case(f"FFmpeg {pin['ffmpeg']}", version == pin["ffmpeg"], version)
    tag = manifest.get("release", {}).get("tag")
    case(f"release {pin['release']} matches the pin", tag == pin["release"], tag)

    enabled = {key: set(value) for key, value in manifest.get("components", {}).items()}
    case("no devices", not enabled.get("devices"), sorted(enabled.get("devices", ())))

    protocols = enabled.get("protocols", set())
    network = {
        line.strip()
        for line in NETWORK_SCHEMES.read_text().splitlines()
        if line.strip() and not line.startswith("#")
    }
    case("no network protocols", not (protocols & network), sorted(protocols & network))
    case("protocols are local only", protocols <= LOCAL_PROTOCOLS, sorted(protocols - LOCAL_PROTOCOLS))
    case("--disable-network configured", "--disable-network" in manifest.get("configure", ""))

    libraries = manifest.get("libraries", {})
    identity = "install_name" if target.endswith("-apple-darwin") else "soname"
    missing = [name for name in pin["libraries"] if not libraries.get(name, {}).get(identity)]
    case(f"pinned libraries carry a {identity}", not missing, f"missing: {missing}")

    required = {key: set(value) for key, value in REQUIRED.items()}
    for key, value in REQUIRED_BY_TARGET[target].items():
        required.setdefault(key, set()).update(value)
    for category, names in sorted(required.items()):
        missing = names - enabled.get(category, set())
        case(f"required playback {category} present", not missing, f"missing: {sorted(missing)}")

    external = set(manifest.get("external_libraries", []))
    case("dav1d is the only external library", external == {"dav1d"}, sorted(external))
    bundled = set(manifest.get("bundled_dependencies", []))
    case("dav1d and zlib are the only bundled dependencies", bundled == {"dav1d", "zlib"}, sorted(bundled))

    failed = sum(1 for ok in results if not ok)
    print(f"TOTAL {len(results) - failed} passed, {failed} failed")
    return 0 if failed == 0 else 1


if MODE == "--check":
    sys.exit(check())
usage()
PY
