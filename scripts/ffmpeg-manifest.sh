#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
MODE=${1:---check}
MANIFEST=${2:-}

python3 - "$ROOT" "$MODE" "$MANIFEST" <<'PY'
import json
import os
import pathlib
import re
import sys

ROOT = pathlib.Path(sys.argv[1])
MODE = sys.argv[2]
MANIFEST_ARG = sys.argv[3]
MANIFEST = pathlib.Path(MANIFEST_ARG) if MANIFEST_ARG else (
    ROOT / ".work" / "deps" / "ffmpeg-manifest.json")
PLATFORM = "linux" if sys.platform.startswith("linux") else "macos"
RELEASE_BUILD = ROOT / ".work" / "deps" / (
    "linux" if PLATFORM == "linux" else "release") / "ffmpeg" / "build"
DEV_BUILD = ROOT / ".work" / "deps" / "ffmpeg-full" / "build"
BUILD = pathlib.Path(os.environ["FFMPEG_BUILD"]) if os.environ.get("FFMPEG_BUILD") else (
    RELEASE_BUILD if RELEASE_BUILD.exists() else DEV_BUILD)
ALLOWLIST = ROOT / "scripts" / "ffmpeg-components.txt"
NETWORK_SCHEMES = ROOT / "scripts" / "network-schemes.txt"
COMPONENTS = BUILD / "config_components.h"
CONFIG_MAK = BUILD / "ffbuild" / "config.mak"

AUTO_DECODERS = {"h263"}
AUTO_BSFS = {"vp9_superframe_split"}

KIND_MAP = {
    "DECODER": "decoders",
    "DEMUXER": "demuxers",
    "PARSER": "parsers",
    "BSF": "bsfs",
    "FILTER": "filters",
    "HWACCEL": "hwaccels",
    "PROTOCOL": "protocols",
    "ENCODER": "encoders",
    "MUXER": "muxers",
    "DEVICE": "devices",
    "INDEV": "devices",
    "OUTDEV": "devices",
}


def parse_allowlist():
    allowed = {}
    for line in ALLOWLIST.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        category, names = line.split(":", 1)
        category = category.strip()
        if "/" in category:
            platform, category = category.split("/", 1)
            if platform != PLATFORM:
                continue
        allowed[category] = set(names.split())
    return allowed


def parse_network_schemes():
    return {
        line.strip()
        for line in NETWORK_SCHEMES.read_text().splitlines()
        if line.strip() and not line.startswith("#")
    }


def parse_components():
    if not COMPONENTS.exists():
        raise SystemExit(
            f"missing {COMPONENTS}; run scripts/build-deps.sh or scripts/build-ffmpeg.sh")
    components = {}
    pattern = re.compile(r"^#define CONFIG_(\w+)_(DECODER|DEMUXER|PARSER|BSF|FILTER|HWACCEL|PROTOCOL|ENCODER|MUXER|DEVICE|INDEV|OUTDEV) 1$")
    for line in COMPONENTS.read_text().splitlines():
        match = pattern.match(line)
        if not match:
            continue
        name, kind = match.groups()
        components.setdefault(KIND_MAP[kind], set()).add(name.lower())
    return {key: sorted(value) for key, value in components.items()}


def parse_external_libraries():
    libraries = set()
    if not CONFIG_MAK.exists():
        return []
    for line in CONFIG_MAK.read_text().splitlines():
        match = re.match(r"^CONFIG_LIB([A-Z0-9]+)=yes$", line)
        if not match:
            continue
        name = match.group(1).lower()
        if name.endswith(("_decoder", "_encoder", "_parser", "_bsf", "_filter",
                          "_demuxer", "_muxer", "_hwaccel", "_protocol")):
            continue
        libraries.add(name[3:] if name.startswith("lib") else name)
    return sorted(libraries)


def configure_args():
    if not CONFIG_MAK.exists():
        return ""
    for line in CONFIG_MAK.read_text().splitlines():
        if line.startswith("FFMPEG_CONFIGURATION="):
            return line.split("=", 1)[1].replace(str(ROOT), "$ROOT")
    return ""


def fresh_manifest():
    return {
        "generated_from": "config_components.h + config.mak",
        "configure_args": configure_args(),
        "components": parse_components(),
        "external_libraries": parse_external_libraries(),
        "auto_selected": {
            "decoders": sorted(AUTO_DECODERS),
            "bsfs": sorted(AUTO_BSFS),
        },
    }


def generate():
    manifest = fresh_manifest()
    MANIFEST.parent.mkdir(parents=True, exist_ok=True)
    MANIFEST.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(f"wrote {MANIFEST}")


def check():
    results = []
    try:
        tree = BUILD.relative_to(ROOT)
    except ValueError:
        tree = BUILD
    print(f"ffmpeg-audit: build tree {tree}")

    def case(name, ok, detail=""):
        results.append(ok)
        status = "PASS" if ok else "FAIL"
        line = f"{status} ffmpeg-audit: {name}"
        if detail and not ok:
            line += f": {detail}"
        print(line)

    if MANIFEST_ARG:
        try:
            manifest = json.loads(MANIFEST.read_text())
        except Exception as exc:
            case("manifest readable", False, str(exc))
            print(f"TOTAL 0 passed, {len(results)} failed")
            return 1
        case("manifest readable", True)
    else:
        manifest = fresh_manifest()
        case("manifest generated from build tree", True)

    allowed = parse_allowlist()
    components = manifest.get("components", {})
    enabled = {key: set(value) for key, value in components.items()}
    allowed_decoders = allowed.get("video_decoders", set()) | allowed.get("subtitle_decoders", set())

    case("no encoders", not enabled.get("encoders", set()),
         f"encoders: {sorted(enabled.get('encoders', set()))}")
    case("no muxers", not enabled.get("muxers", set()),
         f"muxers: {sorted(enabled.get('muxers', set()))}")
    case("no devices", not (enabled.get("devices", set())),
         f"devices: {sorted(enabled.get('devices', set()))}")

    protocols = enabled.get("protocols", set())
    network = parse_network_schemes()
    case("protocols allowed", protocols <= allowed.get("protocols", set()),
         f"protocols: {sorted(protocols - allowed.get('protocols', set()))}")
    case("no network protocols", not (protocols & network),
         f"network: {sorted(protocols & network)}")

    decoders = enabled.get("decoders", set())
    decoders_allowed = allowed_decoders | AUTO_DECODERS
    case("decoders allowed", decoders <= decoders_allowed,
         f"extra: {sorted(decoders - decoders_allowed)}")
    case("allowlist decoders present", allowed_decoders <= decoders,
         f"missing: {sorted(allowed_decoders - decoders)}")

    for category, allowed_key, autos in (
        ("demuxers", "demuxers", set()),
        ("parsers", "parsers", set()),
        ("bsfs", "bsfs", AUTO_BSFS),
        ("filters", "filters", set()),
        ("hwaccels", "hwaccels", set()),
    ):
        got = enabled.get(category, set())
        want = allowed.get(allowed_key, set())
        case(f"{category} allowed", got <= want | autos,
             f"extra: {sorted(got - (want | autos))}")
        case(f"allowlist {category} present", want <= got,
             f"missing: {sorted(want - got)}")

    libraries = set(manifest.get("external_libraries", []))
    case("external libraries allowed", libraries <= {"dav1d"},
         f"extra: {sorted(libraries - {'dav1d'})}")

    failed = sum(1 for ok in results if not ok)
    print(f"TOTAL {len(results) - failed} passed, {failed} failed")
    return 0 if failed == 0 else 1


if MODE == "--generate":
    generate()
    sys.exit(0)
elif MODE == "--check":
    sys.exit(check())
else:
    print("usage: scripts/ffmpeg-manifest.sh --generate|--check [manifest]",
          file=sys.stderr)
    sys.exit(2)
PY
