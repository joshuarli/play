#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
MODE=${1:-}
LABEL=${2:-$MODE}
case "$MODE" in
    baseline|final) ;;
    *)
        printf 'usage: scripts/measure-linux.sh {baseline|final} [LABEL]\n' >&2
        exit 2
        ;;
esac
cd "$ROOT"

python3 - "$ROOT" "$MODE" "$LABEL" <<'PY'
import datetime
import json
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(sys.argv[1])
MODE = sys.argv[2]
LABEL = sys.argv[3]
RUNS = 7
MPV = ROOT / ("dist/player" if MODE == "final" else "build-linux/mpv")
MEASURE_DIR = ROOT / ".work" / "fixtures" / "measure"
TMP = ROOT / ".work" / "tmp"
METRICS = ROOT / "docs" / "metrics.md"
SECTION = "## Linux"
HEADER = [
    "Label", "Date", "Revision", "Stripped B", "NEEDED", "Cold ms", "Warm ms",
    "First frame ms", "Idle MB", "1080p MB", "1080p CPU s", "Seek ms",
    "Exit ms", "Protocols",
]

if not MPV.exists():
    raise SystemExit(f"missing player: {MPV}")
TMP.mkdir(parents=True, exist_ok=True)
MEASURE_DIR.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, str(ROOT / "tests"))
import harness  # noqa: E402
harness.MPV = MPV
from harness import Mpv  # noqa: E402


def run(command, timeout=120, **kwargs):
    return subprocess.run(command, capture_output=True, text=True,
                          timeout=timeout, **kwargs)


def median(values):
    return round(statistics.median(values), 2)


def spread(values):
    return round(max(values) - min(values), 2)


def fixture(label, width, height):
    path = MEASURE_DIR / f"{label}.mp4"
    if path.exists():
        return path
    ffmpeg = ["ffmpeg"] if run(["which", "ffmpeg"]).returncode == 0 else \
        [str(ROOT / "scripts/linux-container.sh"), "ffmpeg"]
    proc = run(ffmpeg + [
        "-hide_banner", "-loglevel", "error", "-y", "-bitexact",
        "-f", "lavfi", "-i", f"testsrc2=size={width}x{height}:rate=24:duration=3",
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "28",
        "-pix_fmt", "yuv420p", "-an",
        "-x264-params", "keyint=24:min-keyint=24:scenecut=0",
        str(path.relative_to(ROOT) if ffmpeg[0] != "ffmpeg" else path),
    ], timeout=300)
    if proc.returncode != 0:
        raise SystemExit(f"failed to generate {path}:\n{proc.stderr}")
    return path


def child_usage(command):
    start = time.monotonic()
    proc = subprocess.Popen(command, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL)
    _, status, usage = os.wait4(proc.pid, 0)
    proc.returncode = os.waitstatus_to_exitcode(status)
    if proc.returncode != 0:
        raise SystemExit(f"{command} exited {proc.returncode}")
    return {
        "real_s": time.monotonic() - start,
        "cpu_s": usage.ru_utime + usage.ru_stime,
        "rss_mb": usage.ru_maxrss / 1024,
    }


def startup(cold):
    values = []
    for _ in range(RUNS):
        if cold:
            run(["sync"])
        start = time.monotonic()
        if run([str(MPV), "--version"]).returncode != 0:
            raise SystemExit("--version failed")
        values.append((time.monotonic() - start) * 1000)
    return values


def needed():
    out = run(["readelf", "-dW", str(MPV)]).stdout
    return re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", out)


def stripped_size():
    target = TMP / "measure-linux-stripped"
    if run(["strip", "-o", str(target), str(MPV)]).returncode != 0:
        raise SystemExit("strip failed")
    size = target.stat().st_size
    target.unlink()
    return size


def first_frame(path):
    values = []
    for _ in range(RUNS):
        start = time.monotonic()
        proc = run([str(MPV), "--no-config", "--vo=null", "--ao=null",
                    "--frames=1", "--untimed", "--hwdec=no", str(path)], timeout=60)
        if proc.returncode != 0:
            raise SystemExit(f"first-frame run failed:\n{proc.stderr}")
        values.append((time.monotonic() - start) * 1000)
    return values


def idle_rss():
    with Mpv(["--vo=null", "--ao=null"]) as player:
        time.sleep(0.5)
        for line in Path(f"/proc/{player.proc.pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) / 1024
    raise SystemExit("VmRSS missing")


def seek_latency(path):
    values = []
    with Mpv(["--vo=null", "--ao=null", "--pause=yes"]) as player:
        player.load(path)
        player.wait_event("file-loaded", timeout=10)
        duration = player.wait_property(
            "duration", lambda v: isinstance(v, (int, float)) and v > 0, timeout=10)
        for _ in range(RUNS):
            start = time.monotonic()
            player.command("seek", duration / 2, "absolute+exact")
            player.wait_property(
                "time-pos",
                lambda v: isinstance(v, (int, float)) and abs(v - duration / 2) <= 0.25,
                timeout=5)
            values.append((time.monotonic() - start) * 1000)
            player.command("seek", 0, "absolute+exact")
    return values


def exit_latency():
    values = []
    for _ in range(RUNS):
        player = Mpv(["--vo=null", "--ao=null"])
        time.sleep(0.2)
        start = time.monotonic()
        player.close()
        values.append((time.monotonic() - start) * 1000)
    return values


def protocols():
    out = run([str(MPV), "--list-protocols"]).stdout
    return sorted(set(re.findall(r"^\s*([A-Za-z][A-Za-z0-9+.-]*)://", out, re.MULTILINE)))


def revision():
    head = run(["git", "rev-parse", "--short", "HEAD"], cwd=str(ROOT)).stdout.strip()
    dirty = run(["git", "status", "--porcelain", "--untracked-files=no"],
                cwd=str(ROOT)).stdout.strip()
    return head + ("-dirty" if dirty else "")


p1080 = fixture("1080p", 1920, 1080)
cold, warm = startup(True), startup(False)
frame = first_frame(p1080)
usage = [child_usage([str(MPV), "--no-config", "--vo=null", "--ao=null",
                      "--hwdec=no", "--msg-level=all=error", str(p1080)])
         for _ in range(RUNS)]
seek = seek_latency(p1080)
exits = exit_latency()
libs = needed()
schemes = protocols()
result = {
    "label": LABEL,
    "date": datetime.date.today().isoformat(),
    "revision": revision(),
    "stripped_bytes": stripped_size(),
    "needed": libs,
    "cold_ms": median(cold), "cold_spread": spread(cold),
    "warm_ms": median(warm), "warm_spread": spread(warm),
    "first_frame_ms": median(frame), "first_frame_spread": spread(frame),
    "idle_rss_mb": round(idle_rss(), 1),
    "rss_1080p_mb": median([u["rss_mb"] for u in usage]),
    "cpu_1080p_s": median([u["cpu_s"] for u in usage]),
    "seek_ms": median(seek), "seek_spread": spread(seek),
    "exit_ms": median(exits), "exit_spread": spread(exits),
    "protocols": schemes,
}
(TMP / f"measure-linux-{LABEL}.json").write_text(json.dumps(result, indent=2) + "\n")

row = [
    LABEL, result["date"], result["revision"], f"{result['stripped_bytes']:,}",
    str(len(libs)), f"{result['cold_ms']} ± {result['cold_spread']}",
    f"{result['warm_ms']} ± {result['warm_spread']}",
    f"{result['first_frame_ms']} ± {result['first_frame_spread']}",
    str(result["idle_rss_mb"]), str(result["rss_1080p_mb"]), str(result["cpu_1080p_s"]),
    f"{result['seek_ms']} ± {result['seek_spread']}",
    f"{result['exit_ms']} ± {result['exit_spread']}", str(len(schemes)),
]
line = "| " + " | ".join(row) + " |"

text = METRICS.read_text()
if SECTION not in text:
    text = text.rstrip("\n") + (
        f"\n\n{SECTION}\n\n"
        "Medians of 7 runs with the spread (max − min). `scripts/measure-linux.sh`; "
        "cold starts follow `sync` because the page cache is not dropped without root.\n\n"
        "| " + " | ".join(HEADER) + " |\n"
        "|" + "|".join("---" for _ in HEADER) + "|\n")
head, _, tail = text.partition(SECTION)
rows = [l for l in tail.splitlines()]
rows = [l for l in rows if not l.startswith(f"| {LABEL} |")]
end = max((i for i, l in enumerate(rows) if l.startswith("|")), default=len(rows) - 1)
rows.insert(end + 1, line)
METRICS.write_text(head + SECTION + "\n".join(rows).rstrip("\n") + "\n")
print(line)
print("needed:", ", ".join(libs))
print("protocols:", ", ".join(schemes))
PY
