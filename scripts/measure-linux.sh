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
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(sys.argv[1])
MODE = sys.argv[2]
LABEL = sys.argv[3]
RUNS = 7
MPV = ROOT / ("dist/player" if MODE == "final" else "build-linux/mpv")
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
sys.path.insert(0, str(ROOT / "tests"))
sys.path.insert(0, str(ROOT / "scripts"))
import harness  # noqa: E402
import perfstat  # noqa: E402
harness.MPV = MPV
from harness import Mpv  # noqa: E402

run = perfstat.run


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


def protocols():
    out = run([str(MPV), "--list-protocols"]).stdout
    return sorted(set(re.findall(r"^\s*([A-Za-z][A-Za-z0-9+.-]*)://", out, re.MULTILINE)))


def revision():
    head = run(["git", "rev-parse", "--short", "HEAD"], cwd=str(ROOT)).stdout.strip()
    dirty = run(["git", "status", "--porcelain", "--untracked-files=no"],
                cwd=str(ROOT)).stdout.strip()
    return head + ("-dirty" if dirty else "")


p1080 = harness.measure_fixture("1080p", 1920, 1080)
host_before = perfstat.host_state()
startup = perfstat.startup(MPV, runs=RUNS)
frame = perfstat.first_frame(MPV, p1080, runs=RUNS)
usage = [child_usage([str(MPV), "--no-config", "--vo=null", "--ao=null",
                      "--hwdec=no", "--msg-level=all=error", str(p1080)])
         for _ in range(RUNS)]
seek = perfstat.seek_latency(Mpv, p1080, runs=RUNS)
exits = perfstat.exit_latency(Mpv, runs=RUNS)
host_after = perfstat.host_state(once=True)
deferred = host_before != perfstat.HOST_QUIET or host_after != perfstat.HOST_QUIET
libs = needed()
schemes = protocols()
result = {
    "label": LABEL,
    "date": datetime.date.today().isoformat(),
    "revision": revision(),
    "stripped_bytes": stripped_size(),
    "needed": libs,
    "executable_startup": startup,
    "headless_first_frame_ms": frame,
    "idle_rss_mb": round(perfstat.idle_rss(Mpv), 1),
    "rss_1080p_mb": round(perfstat.median([u["rss_mb"] for u in usage]), 2),
    "cpu_1080p_s": round(perfstat.median([u["cpu_s"] for u in usage]), 2),
    "seek_ms": seek,
    "idle_exit_ms": exits,
    "deferred": deferred,
    "host": {"before": host_before, "after": host_after},
    "protocols": schemes,
}
(TMP / f"measure-linux-{LABEL}.json").write_text(json.dumps(result, indent=2) + "\n")


def cell(stats):
    return f"{stats['median']} [{stats['min']}, {stats['max']}]"


row = [
    LABEL, result["date"], result["revision"], f"{result['stripped_bytes']:,}",
    str(len(libs)), cell(startup["after_sync"]), cell(startup["warm"]),
    cell(frame),
    str(result["idle_rss_mb"]), str(result["rss_1080p_mb"]), str(result["cpu_1080p_s"]),
    cell(seek),
    cell(exits), str(len(schemes)),
]
line = "| " + " | ".join(row) + " |"

text = METRICS.read_text()
if SECTION not in text:
    text = text.rstrip("\n") + (
        f"\n\n{SECTION}\n\n"
        "Medians of 7 runs. `scripts/measure-linux.sh`; the synced start follows `sync` "
        "because the page cache is not dropped without root.\n\n"
        "| " + " | ".join(HEADER) + " |\n"
        "|" + "|".join("---" for _ in HEADER) + "|\n")
head, _, tail = text.partition(SECTION)
rows = [l for l in tail.splitlines()]
rows = [l for l in rows if not l.startswith(f"| {LABEL} |")]
end = max((i for i, l in enumerate(rows) if l.startswith("|")), default=len(rows) - 1)
rows.insert(end + 1, line)
METRICS.write_text(head + SECTION + "\n".join(rows).rstrip("\n") + "\n")
print(line)
print("executable-startup:", cell(startup["after_sync"]), "ms after sync,",
      cell(startup["warm"]), "ms warm")
print("headless-first-frame:", cell(frame), "ms")
print("idle-exit:", cell(exits), "ms")
print("needed:", ", ".join(libs))
print("protocols:", ", ".join(schemes))
if deferred:
    print("perf gate deferred: host busy")
PY
