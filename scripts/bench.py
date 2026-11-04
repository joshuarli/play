#!/usr/bin/env python3
"""Interactive performance benchmarks: seek-to-resume latency, audio and GUI CPU, frame drops.

Usage: scripts/bench.py MODE [options]; see docs/metrics.md ("Interactive benchmarks").
"""
import argparse
import json
import random
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests"))
import harness  # noqa: E402

FIXTURES = ROOT / ".work" / "fixtures" / "bench"
RESULTS = ROOT / ".work" / "tmp"
DEFAULT_PLAYER = ROOT / "build" / "mpv"
REFERENCE = Path("/opt/homebrew/bin/mpv")
HOST_BUSY = 75
SEEK_MIN_DISTANCE = 13.0

FIXTURE_RECIPES = {
    "h264-gop10": (
        "h264-gop10.mp4",
        ["-f", "lavfi", "-i", "testsrc2=s=1920x1080:r=24:d=30",
         "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=30",
         "-c:v", "libx264", "-preset", "veryfast", "-g", "250", "-pix_fmt", "yuv420p",
         "-c:a", "aac", "-b:a", "128k", "-shortest"],
    ),
    "vp9-opus": (
        "vp9-opus.webm",
        ["-f", "lavfi", "-i", "testsrc2=s=1920x1080:r=30:d=30",
         "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=30",
         "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-row-mt", "1",
         "-g", "150", "-b:v", "4M", "-pix_fmt", "yuv420p",
         "-c:a", "libopus", "-b:a", "128k", "-shortest"],
    ),
}


def fixture(name):
    filename, recipe = FIXTURE_RECIPES[name]
    path = FIXTURES / filename
    if not path.exists():
        FIXTURES.mkdir(parents=True, exist_ok=True)
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *recipe, str(path)],
                       check=True, timeout=900)
    return path


def host_quiet(once=False):
    tool = shutil.which("perf-gate-ready")
    if tool is None:
        return True
    command = [tool, "-1"] if once else [tool]
    return subprocess.run(command, capture_output=True, timeout=120).returncode == 0


def open_player(player, extra):
    harness.MPV = Path(player).resolve()
    return harness.Mpv(["--hwdec=no", *extra])


def cpu_seconds(pid):
    text = subprocess.check_output(["ps", "-o", "time=", "-p", str(pid)], text=True).strip()
    minutes, seconds = text.split(":")
    return int(minutes) * 60 + float(seconds)


def rss_mb(pid):
    return int(subprocess.check_output(["ps", "-o", "rss=", "-p", str(pid)], text=True)) / 1024


def summary(values):
    ordered = sorted(values)
    return {
        "median": round(statistics.median(ordered), 1),
        "p90": round(ordered[min(len(ordered) - 1, int(len(ordered) * 0.9))], 1),
        "max": round(ordered[-1], 1),
    }


def seek_latency(player, path, precision, runs, seed):
    """Time from the seek command until time-pos leaves its pre-seek value and then advances."""
    rng = random.Random(seed)
    latencies, offsets = [], []
    with open_player(player, ["--mute=yes"]) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=30)
        time.sleep(2)
        duration = mpv.get_property("duration")
        for _ in range(runs):
            before = mpv.get_property("time-pos")
            target = rng.uniform(5, duration - 10)
            while abs(target - before) < SEEK_MIN_DISTANCE:
                target = rng.uniform(5, duration - 10)
            started = time.monotonic()
            mpv.command("seek", f"{target:.3f}", f"absolute+{precision}")
            landed = resumed = None
            while time.monotonic() - started < 10:
                position = mpv.get_property("time-pos")
                if isinstance(position, (int, float)) and abs(position - before) > 1.0 \
                        and abs(position - target) < 12:
                    if landed is None:
                        landed = position
                    elif position > landed + 0.05:
                        resumed = time.monotonic()
                        break
                time.sleep(0.005)
            latencies.append(((resumed or time.monotonic()) - started) * 1000)
            offsets.append(landed - target if landed is not None else float("nan"))
            time.sleep(0.4)
    return {"latency_ms": summary(latencies), "land_offset_s": round(statistics.median(offsets), 3)}


def audio_cpu(player, path, seconds=10):
    proc = subprocess.Popen([str(player), "--no-config", "--no-video", "--mute=yes", "--ao=null",
                             str(path)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(3)
        start = cpu_seconds(proc.pid)
        time.sleep(seconds)
        used = cpu_seconds(proc.pid) - start
    finally:
        proc.terminate()
        proc.wait(timeout=10)
    return {"cpu_percent": round(used / seconds * 100, 2)}


def frame_drops(player, path, seconds=8):
    results = {}
    with open_player(player, ["--mute=yes", "--fs=yes"]) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=30)
        time.sleep(3)
        for visibility in ("never", "always"):
            mpv.command("set", "osc-visibility", visibility)
            time.sleep(0.5)
            dropped = mpv.get_property("frame-drop-count")
            position = mpv.get_property("time-pos")
            start = cpu_seconds(mpv.proc.pid)
            time.sleep(seconds)
            played = mpv.get_property("time-pos") - position
            results[f"osc_{visibility}"] = {
                "dropped": mpv.get_property("frame-drop-count") - dropped,
                "played_s": round(played, 2),
                "cpu_percent": round((cpu_seconds(mpv.proc.pid) - start) / seconds * 100, 1),
                "rss_mb": round(rss_mb(mpv.proc.pid), 1),
            }
    return results


def gui_scenario(player, path, name, seconds=5):
    args = ["--keep-open=yes", "--geometry=1280x720", "--mute=yes"]
    if name != "playing-hidden":
        args.append("--pause=yes")
    with open_player(player, args) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=30)
        time.sleep(2)
        if name in ("paused-shown", "hover"):
            mpv.command("set", "osc-visibility", "always")
            time.sleep(1)
        start = cpu_seconds(mpv.proc.pid)
        began = time.monotonic()
        if name == "hover":
            seek = mpv.get_property("osc-areas")["seek"]
            y = int(seek["y"] + seek["h"] / 2)
            step = 0
            while time.monotonic() - began < seconds:
                mpv.command("mouse", int(seek["x"] + (step * 7) % int(seek["w"])), y)
                step += 1
                time.sleep(1 / 60)
        else:
            time.sleep(seconds)
        elapsed = time.monotonic() - began
        return {
            "cpu_percent": round((cpu_seconds(mpv.proc.pid) - start) / elapsed * 100, 2),
            "rss_mb": round(rss_mb(mpv.proc.pid), 1),
        }


def gui_cpu(candidate, baseline, path, runs):
    results = {}
    for name in ("paused-hidden", "paused-shown", "hover", "playing-hidden"):
        samples = {"candidate": [], "baseline": []}
        for index in range(runs):
            order = [("candidate", candidate), ("baseline", baseline)]
            for side, player in order if index % 2 == 0 else order[::-1]:
                samples[side].append(gui_scenario(player, path, name))
        results[name] = {
            side: {
                "cpu_percent_median": statistics.median(s["cpu_percent"] for s in values),
                "rss_mb_median": statistics.median(s["rss_mb"] for s in values),
            }
            for side, values in samples.items()
        }
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["seek", "audio-cpu", "frames", "gui-cpu", "all"])
    parser.add_argument("--player", default=str(DEFAULT_PLAYER))
    parser.add_argument("--reference", default=str(REFERENCE) if REFERENCE.exists() else None,
                        help="player to compare against (default: Homebrew mpv when installed)")
    parser.add_argument("--baseline", help="earlier build of this player, for gui-cpu")
    parser.add_argument("--fixture", choices=sorted(FIXTURE_RECIPES), action="append")
    parser.add_argument("--precision", choices=["exact", "keyframes"], action="append")
    parser.add_argument("--runs", type=int, default=15)
    parser.add_argument("--seed", type=int, default=7)
    options = parser.parse_args()

    if not host_quiet():
        print("perf gate deferred: host busy")
        return HOST_BUSY
    names = options.fixture or sorted(FIXTURE_RECIPES)
    precisions = options.precision or ["exact", "keyframes"]
    players = {"player": options.player}
    if options.reference and options.mode in ("seek", "audio-cpu", "all"):
        players["reference"] = options.reference
    report = {"player": options.player, "reference": options.reference, "results": {}}

    for name in names:
        path = fixture(name)
        entry = report["results"].setdefault(name, {})
        if options.mode in ("seek", "all"):
            for precision in precisions:
                for label, player in players.items():
                    if label == "reference" and precision == "keyframes":
                        continue
                    result = seek_latency(player, path, precision, options.runs, options.seed)
                    entry[f"seek_{precision}_{label}"] = result
                    latency = result["latency_ms"]
                    print(f"{name:11s} seek {precision:9s} {label:9s} median {latency['median']:6.0f} ms"
                          f"  p90 {latency['p90']:6.0f}  max {latency['max']:6.0f}"
                          f"  land {result['land_offset_s']:+.2f} s", flush=True)
        if options.mode in ("audio-cpu", "all"):
            for label, player in players.items():
                result = audio_cpu(player, path)
                entry[f"audio_cpu_{label}"] = result
                print(f"{name:11s} audio-only CPU {label:9s} {result['cpu_percent']:5.2f} %", flush=True)
        if options.mode in ("frames", "all"):
            result = frame_drops(options.player, path)
            entry["frames"] = result
            for key, value in result.items():
                print(f"{name:11s} fullscreen {key:10s} dropped {value['dropped']:3d} in "
                      f"{value['played_s']:.1f} s, CPU {value['cpu_percent']:5.1f} %, "
                      f"RSS {value['rss_mb']:.0f} MB", flush=True)
        if options.mode == "gui-cpu":
            if not options.baseline:
                parser.error("gui-cpu needs --baseline")
            result = gui_cpu(options.player, options.baseline, path, max(3, options.runs // 3))
            entry["gui_cpu"] = result
            for scenario, sides in result.items():
                print(f"{name:11s} {scenario:15s} candidate {sides['candidate']['cpu_percent_median']:6.2f} %"
                      f"  baseline {sides['baseline']['cpu_percent_median']:6.2f} %", flush=True)

    if not host_quiet(once=True):
        print("perf gate deferred: host busy")
        return HOST_BUSY
    RESULTS.mkdir(parents=True, exist_ok=True)
    out = RESULTS / f"bench-{options.mode}.json"
    out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {out.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
