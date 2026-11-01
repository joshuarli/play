#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
MODE=${1:-}
LABEL=${2:-baseline}
cd "$ROOT"

case "$MODE" in
    baseline|size|final|compare) ;;
    *)
        printf 'usage: scripts/measure.sh {baseline|size|final|compare}\n' >&2
        exit 2
        ;;
esac

if [ "$MODE" = "final" ] || [ "$MODE" = "compare" ]; then
    printf 'error: %s is implemented in P8\n' "$MODE" >&2
    exit 2
fi

python3 - "$ROOT" "$MODE" "$LABEL" <<'PY'
import datetime
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(sys.argv[1])
MODE = sys.argv[2]
LABEL = sys.argv[3]
MPV = ROOT / "build" / "mpv"
MEASURE_DIR = ROOT / ".work" / "fixtures" / "measure"
TMP = ROOT / ".work" / "tmp"
METRICS = ROOT / "docs" / "metrics.md"
RESULT_PATH = TMP / f"measure-{LABEL}.json"
TMP.mkdir(parents=True, exist_ok=True)
MEASURE_DIR.mkdir(parents=True, exist_ok=True)

if not MPV.exists():
    raise SystemExit(f"missing player: {MPV}; run scripts/build.sh")

sys.path.insert(0, str(ROOT / "tests"))
from harness import Mpv, Failure  # noqa: E402


def run(command, timeout=120, **kwargs):
    return subprocess.run(command, capture_output=True, text=True,
                          timeout=timeout, **kwargs)


def median(values):
    return statistics.median(values) if values else None


def mb(value):
    return round(value / (1024 * 1024), 2)


def kb_to_mb(value):
    return round(value / 1024, 2)


def git_revision():
    revision = run(["git", "rev-parse", "--short=12", "HEAD"]).stdout.strip()
    dirty = bool(run(["git", "status", "--porcelain"]).stdout.strip())
    return revision + ("-dirty" if dirty else "")


def measure_time_l(command):
    proc = subprocess.run(["/usr/bin/time", "-l", *command],
                          capture_output=True, text=True, timeout=180)
    text = proc.stdout + proc.stderr
    real = re.search(r"([\d.]+)\s+real\s+([\d.]+)\s+user\s+([\d.]+)\s+sys", text)
    rss = re.search(r"(\d+)\s+maximum resident set size", text)
    if not real or not rss:
        raise SystemExit(f"could not parse /usr/bin/time -l output for {command}:\n{text}")
    return {
        "exit": proc.returncode,
        "real_s": float(real.group(1)),
        "cpu_s": float(real.group(2)) + float(real.group(3)),
        "rss_mb": mb(int(rss.group(1))),
    }


def ensure_measure_fixtures():
    fixtures = {}
    for label, width, height in (
        ("720p", 1280, 720),
        ("1080p", 1920, 1080),
        ("4k", 3840, 2160),
    ):
        path = MEASURE_DIR / f"{label}.mp4"
        if not path.exists():
            proc = run([
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-bitexact",
                "-f", "lavfi", "-i",
                f"testsrc2=size={width}x{height}:rate=24:duration=3",
                "-c:v", "libx264", "-preset", "ultrafast", "-crf", "28",
                "-pix_fmt", "yuv420p", "-an",
                "-x264-params", "keyint=24:min-keyint=24:scenecut=0",
                str(path),
            ], timeout=300)
            if proc.returncode != 0:
                raise SystemExit(f"failed to generate {path}:\n{proc.stderr}")
        fixtures[label] = path
    return fixtures


def startup_times():
    def sample(cold):
        values = []
        for _ in range(3):
            if cold:
                run(["/bin/sync"], timeout=30)
            start = time.monotonic()
            proc = run([str(MPV), "--version"], timeout=30)
            values.append((time.monotonic() - start) * 1000)
            if proc.returncode != 0:
                raise SystemExit("mpv --version failed")
        return median(values)
    return {
        "cold_ms": round(sample(True), 2),
        "warm_ms": round(sample(False), 2),
        "method": "3 runs each; cold runs follow sync (macOS page cache is not dropped without root)",
    }


def first_frame_ms(path):
    values = []
    for _ in range(3):
        start = time.monotonic()
        proc = run([str(MPV), "--no-config", "--vo=null", "--ao=null",
                    "--frames=1", "--untimed", "--hwdec=no", str(path)], timeout=60)
        if proc.returncode != 0:
            raise SystemExit(f"first-frame run failed:\n{proc.stderr}")
        values.append((time.monotonic() - start) * 1000)
    return round(median(values), 2)


def playback_metrics(path):
    return measure_time_l([
        str(MPV), "--no-config", "--vo=null", "--ao=null", "--hwdec=no",
        "--msg-level=all=error", str(path),
    ])


def idle_rss():
    try:
        with Mpv(["--vo=null", "--ao=null"]) as player:
            time.sleep(0.5)
            rss = subprocess.check_output(
                ["ps", "-o", "rss=", "-p", str(player.proc.pid)], text=True)
            return kb_to_mb(int(rss.strip()))
    except Exception as exc:
        raise SystemExit(f"idle RSS failed: {exc}")


def seek_latency(path):
    values = []
    with Mpv(["--vo=null", "--ao=null", "--pause=yes"]) as player:
        player.load(path)
        player.wait_event("file-loaded", timeout=10)
        duration = player.wait_property(
            "duration", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10)
        for _ in range(3):
            start = time.monotonic()
            player.command("seek", duration / 2, "absolute+exact")
            player.wait_property(
                "time-pos",
                lambda value: isinstance(value, (int, float)) and
                abs(value - duration / 2) <= 0.25,
                timeout=5)
            values.append((time.monotonic() - start) * 1000)
    return round(median(values), 2)


def exit_latency():
    values = []
    for _ in range(3):
        player = Mpv(["--vo=null", "--ao=null"])
        time.sleep(0.2)
        start = time.monotonic()
        player.close()
        values.append((time.monotonic() - start) * 1000)
    return round(median(values), 2)


def audio_underruns():
    lines = []
    with Mpv(["--ao=coreaudio", "--vo=null", "--no-video", "--volume=0"]) as player:
        player.load(ROOT / ".work" / "fixtures" / "audio-wav.wav")
        player.wait_event("file-loaded", timeout=10)
        player.wait_eof(timeout=20)
        lines = player.lines
    return sum(1 for line in lines if re.search(r"(?i)underrun", line))


def frame_drops(path):
    with Mpv(["--vo=null", "--ao=null"]) as player:
        player.load(path)
        player.wait_event("file-loaded", timeout=10)
        time.sleep(2.0)
        value = player.try_get_property("frame-drop-count")
        return value


def otool_deps(path):
    proc = run(["otool", "-L", str(path)], timeout=30)
    if proc.returncode != 0:
        return []
    deps = []
    for line in proc.stdout.splitlines()[1:]:
        dep = line.strip().split(" ")[0]
        if dep:
            deps.append(dep)
    return deps


def is_system(path):
    return path.startswith(("/System/", "/usr/lib/", "/usr/lib/swift/",
                            "/Applications/Xcode", "/Library/Apple/"))


def direct_libraries():
    return [dep for dep in otool_deps(MPV) if not is_system(dep)]


def resolve_dependency(parent, dep, rpaths):
    if dep.startswith("@rpath/"):
        suffix = dep[len("@rpath/"):]
        for rpath in rpaths:
            candidate = Path(rpath) / suffix
            if candidate.exists():
                return str(candidate)
        return None
    if dep.startswith("@loader_path/"):
        suffix = dep[len("@loader_path/"):]
        candidate = Path(parent).parent / suffix
        return str(candidate) if candidate.exists() else None
    return dep if os.path.exists(dep) else None


def rpaths(path):
    proc = run(["otool", "-l", str(path)], timeout=30)
    values = []
    lines = proc.stdout.splitlines()
    for index, line in enumerate(lines):
        if "cmd LC_RPATH" in line:
            for follow in lines[index + 1:index + 5]:
                match = re.search(r"path (.+?) \(offset", follow)
                if match:
                    values.append(match.group(1))
                    break
    return values


def component_for(path):
    lowered = path.lower()
    name = Path(path).name.lower()
    if "/ffmpeg" in lowered or name.startswith(("libav", "libsw")):
        return "FFmpeg"
    if "libplacebo" in lowered:
        return "libplacebo"
    if "libass" in lowered:
        return "libass"
    if "moltenvk" in lowered:
        return "MoltenVK"
    if "vulkan" in lowered:
        return "Vulkan"
    if "shaderc" in lowered or "glslang" in lowered:
        return "shaderc"
    if "dav1d" in lowered:
        return "dav1d"
    if "lua" in lowered:
        return "Lua"
    return "remaining external"


def closure():
    direct = direct_libraries()
    queue = list(direct)
    molten = Path("/opt/homebrew/opt/molten-vk/lib/libMoltenVK.dylib")
    if molten.exists():
        queue.append(str(molten))
    seen = {}
    while queue:
        dep = queue.pop()
        parent = Path(dep).parent
        resolved = resolve_dependency(parent, dep, rpaths(dep) if os.path.exists(dep) else [])
        if not resolved:
            continue
        real = os.path.realpath(resolved)
        if real in seen or is_system(real):
            continue
        if not os.path.isfile(real):
            continue
        seen[real] = os.path.getsize(real)
        for child in otool_deps(real):
            if not is_system(child):
                queue.append(child)
    by_component = {}
    for path, size in seen.items():
        component = component_for(path)
        entry = by_component.setdefault(component, {"bytes": 0, "files": 0})
        entry["bytes"] += size
        entry["files"] += 1
    return {
        "direct": direct,
        "files": len(seen),
        "bytes": sum(seen.values()),
        "by_component": {
            key: {"bytes": value["bytes"], "files": value["files"]}
            for key, value in sorted(by_component.items())
        },
        "paths": sorted(seen),
    }


def script_host_attribution():
    osc = ROOT / "player" / "lua" / "osc.lua"
    ffi = ROOT / ".work" / "deps" / "player-audio" / "target" / "release" / "libplayer_ffi.a"
    lua_archives = sorted((ROOT / ".work" / "deps" / "player-audio" / "target" / "release" / "build").glob(
        "mlua-sys/*/out/lib/liblua5.2.a"))
    lines = len(osc.read_text().splitlines()) if osc.exists() else 0
    return {
        "osc_bytes": osc.stat().st_size if osc.exists() else 0,
        "osc_lines": lines,
        "ffi_archive_bytes": ffi.stat().st_size if ffi.exists() else 0,
        "lua_archive_bytes": lua_archives[0].stat().st_size if lua_archives else 0,
    }


def stripped_sizes():
    target = TMP / "mpv-stripped"
    shutil.copy2(MPV, target)
    subprocess.run(["strip", "-x", str(target)], check=True, timeout=60)
    return {
        "unstripped": MPV.stat().st_size,
        "stripped": target.stat().st_size,
    }


def architecture():
    file_out = run(["file", "-b", str(MPV)], timeout=30).stdout.strip()
    vtool = run(["vtool", "-show-build", str(MPV)], timeout=30).stdout
    minos = re.search(r"minos ([\d.]+)", vtool)
    sdk = re.search(r"sdk ([\d.]+)", vtool)
    return {
        "file": file_out,
        "minos": minos.group(1) if minos else None,
        "sdk": sdk.group(1) if sdk else None,
    }


def protocols():
    proc = run([str(MPV), "--list-protocols"], timeout=30)
    listed = re.findall(r"^\s*([A-Za-z][A-Za-z0-9+.-]*)://", proc.stdout, re.MULTILINE)
    unique = sorted(set(listed))
    network = sorted(scheme for scheme in unique if scheme in {
        "http", "https", "ftp", "sftp", "rtsp", "rtmp", "hls", "dash",
        "gopher", "gophers", "dav", "davs", "webdav", "webdavs", "mms",
        "mmsh", "mmst", "rtp", "srtp", "udp", "tcp", "tls", "ipfs", "ipns",
    })
    return {
        "listed": len(listed),
        "unique": len(unique),
        "network": network,
        "all": unique,
    }


def features():
    proc = run(["meson", "configure", "build"], cwd=str(ROOT), timeout=60)
    wanted = {
        "cplayer", "libmpv", "gpl", "lua", "cocoa", "swift-build",
        "coreaudio", "vulkan", "videotoolbox-pl", "gl", "gl-cocoa",
        "videotoolbox-gl", "macos-cocoa-cb", "x11", "wayland", "drm",
        "vaapi", "vdpau", "javascript", "libavdevice", "tests",
    }
    values = {}
    for line in proc.stdout.splitlines():
        fields = line.split()
        if len(fields) >= 2 and fields[0] in wanted:
            values[fields[0]] = fields[1]
    return values


def hwdec():
    fixture = ROOT / ".work" / "fixtures" / "h264.mp4"
    proc = run(["ffmpeg", "-hide_banner", "-loglevel", "error",
                "-hwaccel", "videotoolbox", "-i", str(fixture), "-f", "null", "-"],
               timeout=60)
    ffmpeg_ok = proc.returncode == 0
    mpv_ok = False
    with Mpv(["--vo=null", "--ao=null", "--hwdec=videotoolbox"]) as player:
        player.load(fixture)
        player.wait_event("file-loaded", timeout=10)
        player.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10)
        mpv_ok = True
    return {
        "ffmpeg_videotoolbox": ffmpeg_ok,
        "mpv_headless_playback": mpv_ok,
        "mpv_log_requires_window": True,
    }


def license_obligations():
    return [
        "mpv: GPL-2.0-or-later; source and notice obligations on distribution.",
        "FFmpeg 9.0.2: LGPL-2.1-or-later with GPL-enabled components; source and notice obligations.",
        "libass 0.17.5: ISC; notice.",
        "libplacebo 7.387.0: LGPL-2.1-or-later; source and notice obligations.",
        "MoltenVK 1.4.2: Apache-2.0; notice.",
        "Vulkan loader/headers 1.4.363.0: Apache-2.0; notice.",
        "shaderc 2026.4: Apache-2.0; notice.",
        "dav1d 1.5.4: BSD-2-Clause; notice.",
        "Lua 5.2.4: MIT; notice.",
    ]


fixtures = ensure_measure_fixtures()
revision = git_revision()
date = datetime.date.today().isoformat()
arch = architecture()
sizes = stripped_sizes()
closure_data = closure()
if MODE == "size":
    print(f"{LABEL} {revision} {date}")
    print(f"player: {sizes['unstripped']} B unstripped, {sizes['stripped']} B stripped")
    print(f"closure: {mb(closure_data['bytes'])} MiB in {closure_data['files']} files")
    for component, value in closure_data["by_component"].items():
        print(f"  {component}: {mb(value['bytes'])} MiB / {value['files']} files")
    host = script_host_attribution()
    print("script host: osc.lua {osc_bytes} B / {osc_lines} lines; "
          "player-ffi archive {ffi_archive_bytes} B; vendored Lua archive "
          "{lua_archive_bytes} B (archives, not linked size)".format(**host))
    raise SystemExit(0)
startup = startup_times()
first_frame = first_frame_ms(fixtures["1080p"])
playback = {
    "720p": playback_metrics(fixtures["720p"]),
    "1080p": playback_metrics(fixtures["1080p"]),
    "4k": playback_metrics(fixtures["4k"]),
}
idle = idle_rss()
seek = seek_latency(fixtures["1080p"])
exit_ms = exit_latency()
underruns = audio_underruns()
drops = frame_drops(fixtures["1080p"])
proto = protocols()
feat = features()
hw = hwdec()

result = {
    "date": date,
    "revision": revision,
    "label": LABEL,
    "player": str(MPV),
    "architecture": arch,
    "sizes": sizes,
    "closure": closure_data,
    "startup": startup,
    "first_frame_1080p_ms": first_frame,
    "playback": playback,
    "idle_rss_mb": idle,
    "seek_ms": seek,
    "exit_ms": exit_ms,
    "audio_underruns": underruns,
    "frame_drop_count_1080p": drops,
    "protocols": proto,
    "features": feat,
    "hwdec": hw,
    "licenses": license_obligations(),
    "headless": True,
}
RESULT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


def metrics_row(value):
    closure_value = value["closure"]
    return "| {date} | {revision} | {label} | {mpv} | {stripped} | {closure} | {files} | {cold} | {warm} | {first} | {idle} | {rss} | {cpu} | {cpu4k} | {seek} | {exit_ms} | {protos} | {notes} |".format(
        date=value["date"],
        revision=value["revision"],
        label=value["label"],
        mpv=value["sizes"]["unstripped"],
        stripped=value["sizes"]["stripped"],
        closure=closure_value["bytes"],
        files=closure_value["files"],
        cold=value["startup"]["cold_ms"],
        warm=value["startup"]["warm_ms"],
        first=value["first_frame_1080p_ms"],
        idle=value["idle_rss_mb"],
        rss=value["playback"]["1080p"]["rss_mb"],
        cpu=round(value["playback"]["1080p"]["cpu_s"], 2),
        cpu4k=round(value["playback"]["4k"]["cpu_s"], 2),
        seek=value["seek_ms"],
        exit_ms=value["exit_ms"],
        protos=value["protocols"]["listed"],
        notes="headless; 3 s clips; GUI owner-run",
    )


def update_metrics(value):
    row = metrics_row(value)
    header = (
        "# Metrics\n\n"
        "Automated playback is headless (`--vo=null --ao=null`); GUI and renderer checks are "
        "owner-run. Timings are medians of 3 runs unless noted. RSS and CPU come from "
        "`/usr/bin/time -l`; the measurement fixtures are generated in "
        "`.work/fixtures/measure`. The exact commands and component attribution are in "
        "`docs/baseline.md`.\n\n"
        "| Date | Revision | Label | mpv B | stripped B | closure B | closure files | "
        "cold ms | warm ms | first frame ms | idle RSS MB | 1080p RSS MB | 1080p CPU s | "
        "4K CPU s | seek ms | exit ms | protocols | notes |\n"
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n"
    )
    existing = METRICS.read_text() if METRICS.exists() else header
    lines = existing.splitlines()
    table_lines = [line for line in lines if line.startswith("|")]
    kept = []
    for line in table_lines:
        fields = [field.strip() for field in line.strip("|").split("|")]
        if len(fields) >= 3 and fields[0] == value["date"] and \
                fields[1] == value["revision"] and fields[2] == value["label"]:
            continue
        kept.append(line)
    if not any(line.startswith("| Date |") for line in kept):
        kept.insert(0, "| Date | Revision | Label | mpv B | stripped B | closure B | "
                       "closure files | cold ms | warm ms | first frame ms | idle RSS MB | "
                       "1080p RSS MB | 1080p CPU s | 4K CPU s | seek ms | exit ms | "
                       "protocols | notes |")
        kept.insert(1, "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    kept.append(row)
    preamble = [line for line in lines if not line.startswith("|")]
    while preamble and preamble[-1] == "":
        preamble.pop()
    METRICS.write_text("\n".join(preamble) + "\n\n" + "\n".join(kept) + "\n")


update_metrics(result)

print(f"{LABEL} {revision} {date}")
print(f"player: {sizes['unstripped']} B unstripped, {sizes['stripped']} B stripped")
print(f"architecture: {arch['file']}, minos {arch['minos']}, sdk {arch['sdk']}")
print(f"closure: {mb(closure_data['bytes'])} MiB in {closure_data['files']} files")
for component, value in closure_data["by_component"].items():
    print(f"  {component}: {mb(value['bytes'])} MiB / {value['files']} files")
print(f"startup: cold {startup['cold_ms']} ms, warm {startup['warm_ms']} ms")
print(f"first frame 1080p: {first_frame} ms")
for label, value in playback.items():
    print(f"playback {label}: RSS {value['rss_mb']} MB, CPU {value['cpu_s']:.2f} s, "
          f"real {value['real_s']:.2f} s")
print(f"idle RSS: {idle} MB; seek {seek} ms; exit {exit_ms} ms")
print(f"audio underruns: {underruns}; frame drops 1080p: {drops}")
print(f"protocols: {proto['listed']} listed, {proto['unique']} unique; "
      f"network {len(proto['network'])}")
print(f"hwdec ffmpeg videotoolbox: {hw['ffmpeg_videotoolbox']}; "
      f"mpv headless playback: {hw['mpv_headless_playback']}")
PY
