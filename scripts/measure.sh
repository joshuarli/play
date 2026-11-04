#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
MODE=${1:-}
LABEL=${2:-}
if [ -z "$LABEL" ] && [ "$MODE" != cpu ] && [ "$MODE" != compare ]; then
    case "$MODE" in
        release) LABEL=release ;;
        final) LABEL=final ;;
        *) LABEL=baseline ;;
    esac
fi
cd "$ROOT"

case "$MODE" in
    baseline|size|release|final) ;;
    compare)
        [ -n "$LABEL" ] && [ -n "${3:-}" ] || {
            printf 'usage: scripts/measure.sh compare BEFORE_LABEL AFTER_LABEL\n' >&2
            exit 2
        }
        ;;
    cpu)
        [ -n "$LABEL" ] || {
            printf 'usage: scripts/measure.sh cpu BASELINE_PLAYER [CANDIDATE_PLAYER]\n' >&2
            exit 2
        }
        ;;
    *)
        printf 'usage: scripts/measure.sh {baseline|size|release|final|compare|cpu}\n' >&2
        exit 2
        ;;
esac

python3 - "$ROOT" "$MODE" "$LABEL" "${3:-}" <<'PY'
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
EXTRA = sys.argv[4]
RUNS = 7
RELEASE_PLAYER = ROOT / "dist/Player.app/Contents/MacOS/player"
REGRESSION_FRACTION = 0.10
SCHEMA = 2
TMP = ROOT / ".work" / "tmp"
if MODE == "cpu":
    MPV = Path(EXTRA) if EXTRA else RELEASE_PLAYER
elif MODE in ("release", "final"):
    MPV = RELEASE_PLAYER
else:
    MPV = ROOT / "build/mpv"
FRAMEWORKS = ROOT / "dist/Player.app/Contents/Frameworks"



MEASURE_DIR = ROOT / ".work" / "fixtures" / "measure"
TMP = ROOT / ".work" / "tmp"
METRICS = ROOT / "docs" / "metrics.md"
RESULT_PATH = TMP / f"measure-{LABEL}.json"
TMP.mkdir(parents=True, exist_ok=True)
MEASURE_DIR.mkdir(parents=True, exist_ok=True)

if MODE != "compare" and not MPV.exists():
    raise SystemExit(f"missing player: {MPV}; run scripts/build.sh")

sys.path.insert(0, str(ROOT / "tests"))
P0_UNIQUE_CLOSURE = 59_084_272

import harness  # noqa: E402
harness.MPV = MPV
from harness import Mpv, Failure  # noqa: E402


def run(command, timeout=120, **kwargs):
    return subprocess.run(command, capture_output=True, text=True,
                          timeout=timeout, **kwargs)


def median(values):
    return statistics.median(values) if values else None


def stat(values, digits=2):
    return {
        "median": round(median(values), digits),
        "spread": round(max(values) - min(values), digits),
        "samples": [round(value, digits) for value in values],
    }


def host_quiet(once=False):
    tool = shutil.which("perf-gate-ready")
    if tool is None:
        return None
    return run([tool, "-1"] if once else [tool], timeout=60).returncode == 0


def timed(action, attempts=3, wait_s=20):
    for attempt in range(attempts):
        last = attempt + 1 == attempts
        before = host_quiet()
        if before is False and not last:
            time.sleep(wait_s)
            continue
        value = action()
        if before and host_quiet(once=True):
            return value, False
        if before is None:
            return value, True
    return value, True


def regressed(before, after):
    allowed = max(before["median"] * REGRESSION_FRACTION, before["spread"], after["spread"])
    return after["median"] - before["median"] > allowed, allowed


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
        for _ in range(RUNS):
            if cold:
                run(["/bin/sync"], timeout=30)
            start = time.monotonic()
            proc = run([str(MPV), "--version"], timeout=30)
            values.append((time.monotonic() - start) * 1000)
            if proc.returncode != 0:
                raise SystemExit("player --version failed")
        return stat(values)
    return {
        "cold_ms": sample(True),
        "warm_ms": sample(False),
        "method": f"{RUNS} runs each; cold runs follow sync (macOS page cache is not dropped without root)",
    }


def first_frame_ms(path):
    values = []
    for _ in range(RUNS):
        start = time.monotonic()
        proc = run([str(MPV), "--no-config", "--vo=null", "--ao=null",
                    "--frames=1", "--untimed", "--hwdec=no", str(path)], timeout=60)
        if proc.returncode != 0:
            raise SystemExit(f"first-frame run failed:\n{proc.stderr}")
        values.append((time.monotonic() - start) * 1000)
    return stat(values)


def playback_command(player, path):
    return [str(player), "--no-config", "--vo=null", "--ao=null", "--hwdec=no",
            "--msg-level=all=error", str(path)]


def playback_metrics(path):
    runs = [measure_time_l(playback_command(MPV, path)) for _ in range(RUNS)]
    if any(run["exit"] != 0 for run in runs):
        raise SystemExit(f"playback of {path} exited nonzero")
    return {
        "real_s": stat([run["real_s"] for run in runs], 3),
        "cpu_s": stat([run["cpu_s"] for run in runs], 3),
        "rss_mb": stat([run["rss_mb"] for run in runs]),
    }


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
        for _ in range(RUNS):
            start = time.monotonic()
            player.command("seek", duration / 2, "absolute+exact")
            player.wait_property(
                "time-pos",
                lambda value: isinstance(value, (int, float)) and
                abs(value - duration / 2) <= 0.25,
                timeout=5)
            values.append((time.monotonic() - start) * 1000)
            player.command("seek", 0, "absolute+exact")
    return stat(values)


def exit_latency():
    values = []
    for _ in range(RUNS):
        player = Mpv(["--vo=null", "--ao=null"])
        time.sleep(0.2)
        start = time.monotonic()
        player.close()
        values.append((time.monotonic() - start) * 1000)
    return stat(values)


def audio_underruns():
    lines = []
    with Mpv(["--ao=coreaudio", "--vo=null", "--no-video", "--volume=0"]) as player:
        player.load(ROOT / ".work" / "fixtures" / "audio-wav.wav")
        player.wait_event("file-loaded", timeout=10)
        player.wait_eof(timeout=20)
        lines = player.lines
    return sum(1 for line in lines if re.search(r"(?i)underrun", line))


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
    if "shaderc" in lowered or "glslang" in lowered or "spirv" in lowered:
        return "shaderc"
    if "dav1d" in lowered:
        return "dav1d"
    if name.startswith(("libfreetype", "libfribidi", "libharfbuzz")):
        return "fonts/shaping"
    return "remaining external"


def release_closure():
    by_component = {}
    paths = []
    for path in sorted(FRAMEWORKS.glob("*.dylib")):
        component = component_for(str(path))
        entry = by_component.setdefault(component, {"bytes": 0, "files": 0})
        size = path.stat().st_size
        entry["bytes"] += size
        entry["files"] += 1
        paths.append(str(path))
    return {
        "direct": [],
        "files": len(paths),
        "bytes": sum(path.stat().st_size for path in FRAMEWORKS.glob("*.dylib")),
        "by_component": {
            key: {"bytes": value["bytes"], "files": value["files"]}
            for key, value in sorted(by_component.items())
        },
        "paths": paths,
    }


def closure():
    if MODE in ("release", "final"):
        return release_closure()
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


def osc_attribution():
    osc = ROOT / "rust" / "player-app" / "src" / "osc.rs"
    return {"osc_bytes": osc.stat().st_size, "osc_lines": len(osc.read_text().splitlines())}


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
    network_file = ROOT / "scripts" / "network-schemes.txt"
    network_set = {
        line.strip()
        for line in network_file.read_text().splitlines()
        if line.strip() and not line.startswith("#")
    }
    network = sorted(scheme for scheme in unique if scheme in network_set)
    return {
        "listed": len(listed),
        "unique": len(unique),
        "network": network,
        "all": unique,
    }


def features():
    manifest = (ROOT / "rust" / "Cargo.toml").read_text()
    profiles = [
        line.strip().strip("[]").removeprefix("profile.")
        for line in manifest.splitlines()
        if line.startswith("[profile.")
    ]
    return {
        "engine": "rust",
        "audio_decoder": "symphonia",
        "renderer": "libplacebo",
        "osc": "native",
        "profiles": ",".join(profiles),
    }


def hwdec():
    fixture = ROOT / ".work" / "fixtures" / "h264.mp4"
    proc = run(["ffmpeg", "-hide_banner", "-loglevel", "error",
                "-hwaccel", "videotoolbox", "-i", str(fixture), "-f", "null", "-"],
               timeout=60)
    ffmpeg_ok = proc.returncode == 0
    player_ok = False
    with Mpv(["--vo=null", "--ao=null", "--hwdec=videotoolbox"]) as player:
        player.load(fixture)
        player.wait_event("file-loaded", timeout=10)
        player.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10)
        player_ok = True
    return {
        "ffmpeg_videotoolbox": ffmpeg_ok,
        "player_headless_playback": player_ok,
    }


def lua_symbols():
    if not MPV.exists():
        return 0
    text = run(["nm", str(MPV)], timeout=30).stdout
    return len(re.findall(r"\b_?lua(?:L|_)[A-Za-z]\w*", text))


def budget_checks(value):
    executable = value["sizes"]["unstripped"]
    closure = value["closure"]["bytes"]
    distributable = executable + closure
    return [
        ("distributable <= 50% of P0", distributable <= P0_UNIQUE_CLOSURE * 0.5,
         f"{distributable} B <= {int(P0_UNIQUE_CLOSURE * 0.5)} B"),
        ("executable <= 10 MB", executable <= 10_000_000,
         f"{executable} B <= 10000000 B"),
        ("1080p RSS <= 300 MB", value["playback"]["1080p"]["rss_mb"]["median"] <= 300,
         f"{value['playback']['1080p']['rss_mb']['median']} MB"),
        ("no network schemes", not value["protocols"]["network"],
         ",".join(value["protocols"]["network"])),
        ("no Lua interpreter", value["lua_symbols"] == 0,
         f"{value['lua_symbols']} Lua symbols"),
    ]


def timing_budget_checks(value):
    first_frame = value["first_frame_1080p_ms"]["median"]
    seek = value["seek_ms"]["median"]
    return [
        ("first frame <= 1 s", first_frame <= 1000, f"{first_frame} ms"),
        ("seek <= 250 ms", seek <= 250, f"{seek} ms"),
    ]


def license_obligations():
    return [
        "FFmpeg 9.0.2: LGPL-2.1-or-later with GPL-enabled components; source and notice obligations.",
        "libass 0.17.5: ISC; notice.",
        "libplacebo 7.387.0: LGPL-2.1-or-later; source and notice obligations.",
        "MoltenVK 1.4.2: Apache-2.0; notice.",
        "Vulkan loader/headers 1.4.363.0: Apache-2.0; notice.",
        "shaderc 2026.4: Apache-2.0; notice.",
        "dav1d 1.5.4: BSD-2-Clause; notice.",
    ]


TIMED_METRICS = (
    ("startup cold ms", lambda v: v["startup"]["cold_ms"]),
    ("startup warm ms", lambda v: v["startup"]["warm_ms"]),
    ("first frame ms", lambda v: v["first_frame_1080p_ms"]),
    ("seek ms", lambda v: v["seek_ms"]),
    ("exit ms", lambda v: v["exit_ms"]),
    ("1080p CPU s", lambda v: v["playback"]["1080p"]["cpu_s"]),
    ("4K CPU s", lambda v: v["playback"]["4k"]["cpu_s"]),
)


def load_result(label):
    path = TMP / f"measure-{label}.json"
    if not path.exists():
        raise SystemExit(f"missing {path}; run scripts/measure.sh release {label}")
    value = json.loads(path.read_text())
    if value.get("schema") != SCHEMA:
        raise SystemExit(f"{path} predates the sampled format; measure {label} again")
    return value


if MODE == "compare":
    before, after = load_result(LABEL), load_result(EXTRA)
    print(f"regression: later median exceeds earlier by more than the larger of "
          f"{REGRESSION_FRACTION:.0%} and the observed spread of either side")
    print(f"{'metric':18} {LABEL:>20} {EXTRA:>20}  verdict")
    failed = []
    deferred = before["deferred"] or after["deferred"]
    for name, getter in TIMED_METRICS:
        old, new = getter(before), getter(after)
        line = (f"{name:18} {old['median']:>10} ±{old['spread']:<8} "
                f"{new['median']:>10} ±{new['spread']:<8}")
        if deferred:
            print(line + "  DEFERRED")
            continue
        bad, allowed = regressed(old, new)
        print(line + (f"  FAIL (+{new['median'] - old['median']:.3f} > {allowed:.3f})"
                      if bad else "  ok"))
        if bad:
            failed.append(name)
    for name, getter in (
        ("idle RSS MB", lambda v: v["idle_rss_mb"]),
        ("1080p RSS MB", lambda v: v["playback"]["1080p"]["rss_mb"]["median"]),
        ("closure B", lambda v: v["closure"]["bytes"]),
        ("executable B", lambda v: v["sizes"]["unstripped"]),
    ):
        old, new = getter(before), getter(after)
        print(f"{name:18} {old:>20} {new:>20}  {new - old:+.2f}")
    checks = budget_checks(after) + ([] if deferred else timing_budget_checks(after))
    for name, ok, detail in checks:
        print(("PASS" if ok else "FAIL") + " budget: " + name + " (" + detail + ")")
        if not ok:
            failed.append(name)
    if failed:
        raise SystemExit("compare: failed " + ", ".join(failed))
    if deferred:
        print("perf gate deferred: host busy")
        raise SystemExit(75)
    print("compare: ok")
    raise SystemExit(0)

if MODE == "cpu":
    baseline_player = Path(LABEL)
    if not baseline_player.exists():
        raise SystemExit(f"missing baseline player: {baseline_player}")
    path = ensure_measure_fixtures()["1080p"]

    def interleaved():
        samples = {"baseline": [], "candidate": []}
        for index in range(RUNS):
            order = (("baseline", baseline_player), ("candidate", MPV))
            for side, player in order if index % 2 == 0 else order[::-1]:
                result = measure_time_l(playback_command(player, path))
                if result["exit"] != 0:
                    raise SystemExit(f"{side} playback exited {result['exit']}")
                samples[side].append(result["cpu_s"])
        return samples

    samples, deferred = timed(interleaved)
    base, cand = stat(samples["baseline"], 3), stat(samples["candidate"], 3)
    bad, allowed = regressed(base, cand)
    delta = cand["median"] - base["median"]
    print(f"1080p CPU s over {RUNS} interleaved pairs")
    print(f"baseline  {baseline_player}: median {base['median']} spread {base['spread']} "
          f"{base['samples']}")
    print(f"candidate {MPV}: median {cand['median']} spread {cand['spread']} "
          f"{cand['samples']}")
    print(f"delta {delta:+.3f} s; a regression must exceed {allowed:.3f} s")
    (TMP / "measure-cpu.json").write_text(json.dumps(
        {"baseline": base, "candidate": cand, "deferred": deferred}, indent=2) + "\n")
    if bad:
        raise SystemExit("cpu: regression")
    if deferred:
        print("perf gate deferred: host busy")
        raise SystemExit(75)
    print("cpu: ok")
    raise SystemExit(0)

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
    print("osc: osc.rs {osc_bytes} B / {osc_lines} lines".format(**osc_attribution()))
    raise SystemExit(0)


def timed_measurements():
    return {
        "startup": startup_times(),
        "first_frame_1080p_ms": first_frame_ms(fixtures["1080p"]),
        "playback": {
            "720p": playback_metrics(fixtures["720p"]),
            "1080p": playback_metrics(fixtures["1080p"]),
            "4k": playback_metrics(fixtures["4k"]),
        },
        "seek_ms": seek_latency(fixtures["1080p"]),
        "exit_ms": exit_latency(),
    }


timings, deferred = timed(timed_measurements)
idle = idle_rss()
underruns = audio_underruns()
proto = protocols()
feat = features()
hw = hwdec()

result = {
    "schema": SCHEMA,
    "date": date,
    "revision": revision,
    "label": LABEL,
    "player": str(MPV),
    "architecture": arch,
    "sizes": sizes,
    "closure": closure_data,
    **timings,
    "deferred": deferred,
    "idle_rss_mb": idle,
    "audio_underruns": underruns,
    "protocols": proto,
    "features": feat,
    "hwdec": hw,
    "licenses": license_obligations(),
    "mode": MODE,
    "distributable_bytes": sizes["unstripped"] + closure_data["bytes"],
    "lua_symbols": lua_symbols(),
}
RESULT_PATH.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


def metrics_row(value):
    closure_value = value["closure"]
    playback = value["playback"]
    notes = "headless, 3 s clips" + ("; perf gate deferred" if value["deferred"] else "")
    cells = [
        value["date"], value["revision"], value["label"],
        value["sizes"]["unstripped"], value["sizes"]["stripped"],
        closure_value["bytes"], closure_value["files"],
        value["startup"]["cold_ms"]["median"], value["startup"]["warm_ms"]["median"],
        value["first_frame_1080p_ms"]["median"], value["idle_rss_mb"],
        playback["1080p"]["rss_mb"]["median"],
        round(playback["1080p"]["cpu_s"]["median"], 2),
        round(playback["4k"]["cpu_s"]["median"], 2),
        value["seek_ms"]["median"], value["exit_ms"]["median"],
        value["protocols"]["listed"], notes,
    ]
    return "| " + " | ".join(str(cell) for cell in cells) + " |"


METRICS_BEGIN = "<!-- measure:begin -->"
METRICS_END = "<!-- measure:end -->"


def update_metrics(value):
    text = METRICS.read_text()
    if text.count(METRICS_BEGIN) != 1 or text.count(METRICS_END) != 1:
        raise SystemExit(f"{METRICS} needs exactly one {METRICS_BEGIN} / {METRICS_END} block")
    head, rest = text.split(METRICS_BEGIN)
    block, tail = rest.split(METRICS_END)
    lines = [line for line in block.strip().splitlines()]
    if len(lines) < 2 or not all(line.startswith("|") for line in lines):
        raise SystemExit(f"the measure block in {METRICS} must hold only the table")
    header, rows = lines[:2], lines[2:]
    columns = len(header[0].strip("|").split("|"))
    row = metrics_row(value)
    if len(row.strip("|").split("|")) != columns:
        raise SystemExit(f"measured row does not match the {columns} columns in {METRICS}")
    key = [cell.strip() for cell in row.strip("|").split("|")[:3]]
    rows = [line for line in rows
            if [cell.strip() for cell in line.strip("|").split("|")[:3]] != key]
    rows.append(row)
    METRICS.write_text(head + METRICS_BEGIN + "\n" + "\n".join(header + rows) + "\n"
                       + METRICS_END + tail)


update_metrics(result)

print(f"{LABEL} {revision} {date}")
print(f"player: {sizes['unstripped']} B unstripped, {sizes['stripped']} B stripped")
print(f"architecture: {arch['file']}, minos {arch['minos']}, sdk {arch['sdk']}")
print(f"closure: {mb(closure_data['bytes'])} MiB in {closure_data['files']} files")
for component, value in closure_data["by_component"].items():
    print(f"  {component}: {mb(value['bytes'])} MiB / {value['files']} files")
startup = result["startup"]
print(f"startup: cold {startup['cold_ms']['median']} ms (spread {startup['cold_ms']['spread']}), "
      f"warm {startup['warm_ms']['median']} ms (spread {startup['warm_ms']['spread']})")
first_frame = result["first_frame_1080p_ms"]
print(f"first frame 1080p: {first_frame['median']} ms (spread {first_frame['spread']})")
for label, value in result["playback"].items():
    print(f"playback {label}: RSS {value['rss_mb']['median']} MB, "
          f"CPU {value['cpu_s']['median']:.3f} s (spread {value['cpu_s']['spread']:.3f}), "
          f"real {value['real_s']['median']:.2f} s")
print(f"idle RSS: {idle} MB; seek {result['seek_ms']['median']} ms; "
      f"exit {result['exit_ms']['median']} ms")
print(f"audio underruns: {underruns}")
print(f"protocols: {proto['listed']} listed, {proto['unique']} unique; "
      f"network {len(proto['network'])}")
print(f"hwdec ffmpeg videotoolbox: {hw['ffmpeg_videotoolbox']}; "
      f"player headless playback: {hw['player_headless_playback']}")

if MODE == "final":
    checks = budget_checks(result)
    checks += [
        ("one local protocol", proto["listed"] == 1 and not proto["network"],
         f"{proto['listed']} listed"),
        ("no audio underruns", underruns == 0, str(underruns)),
    ]
    if not deferred:
        checks += timing_budget_checks(result)
    failed = [name for name, ok, _ in checks if not ok]
    for name, ok, detail in checks:
        print(("PASS" if ok else "FAIL") + " budget: " + name + " (" + detail + ")")
    if failed:
        raise SystemExit("final: failed " + ", ".join(failed))
    if deferred:
        print("perf gate deferred: host busy")
        raise SystemExit(75)
    print("final: ok")
PY
