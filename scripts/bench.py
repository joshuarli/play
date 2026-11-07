#!/usr/bin/env python3
"""Interactive performance benchmarks: seek landing, audio and GUI CPU, frame drops, workloads.

Usage: scripts/bench.py MODE [options]; see docs/metrics.md ("Interactive benchmarks").
"""
import argparse
import os
import random
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests"))
sys.path.insert(0, str(ROOT / "scripts"))
import harness  # noqa: E402
import perfstat  # noqa: E402

FIXTURES = ROOT / ".work" / "fixtures" / "bench"
REFERENCE = Path("/opt/homebrew/bin/mpv")
HOST_BUSY = 75
SEEK_TIMEOUT = 5.0
DEFAULT_FRAME_SECONDS = 1 / 24
DEFAULT_GOP_SECONDS = 2.0
GUI_START = 1.0
WINDOW_SECONDS = 5.0
MEMORY_INTERVAL = 0.25
SCENARIOS = ("paused-hidden", "paused-shown", "hover", "playing-hidden")
SEEK_FIXTURES = ("h264-gop10", "vp9-opus")

FIXTURE_RECIPES = {
    "h264-gop10": {
        "file": "h264-gop10.mp4",
        "fps": 24.0,
        "gop_frames": 250,
        "duration": 30.0,
        "args": [
            "-f", "lavfi", "-i", "testsrc2=s=1920x1080:r=24:d=30",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=30",
            "-c:v", "libx264", "-preset", "veryfast", "-g", "250", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "128k", "-shortest",
        ],
    },
    "vp9-opus": {
        "file": "vp9-opus.webm",
        "fps": 30.0,
        "gop_frames": 150,
        "duration": 30.0,
        "args": [
            "-f", "lavfi", "-i", "testsrc2=s=1920x1080:r=30:d=30",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=30",
            "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-row-mt", "1",
            "-g", "150", "-b:v", "4M", "-pix_fmt", "yuv420p",
            "-c:a", "libopus", "-b:a", "128k", "-shortest",
        ],
    },
    "video-1080p": {
        "file": "video-1080p.mp4",
        "fps": 24.0,
        "gop_frames": 48,
        "duration": 30.0,
        "args": [
            "-f", "lavfi", "-i", "testsrc2=s=1920x1080:r=24:d=30",
            "-c:v", "libx264", "-preset", "veryfast", "-g", "48", "-pix_fmt", "yuv420p", "-an",
        ],
    },
    "av-1080p": {
        "file": "av-1080p.mp4",
        "fps": 24.0,
        "gop_frames": 48,
        "duration": 30.0,
        "args": [
            "-f", "lavfi", "-i", "testsrc2=s=1920x1080:r=24:d=30",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=30",
            "-c:v", "libx264", "-preset", "veryfast", "-g", "48", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "128k", "-shortest",
        ],
    },
    "video-4k": {
        "file": "video-4k.mp4",
        "fps": 24.0,
        "gop_frames": 24,
        "duration": 30.0,
        "args": [
            "-f", "lavfi", "-i", "testsrc2=s=3840x2160:r=24:d=30",
            "-c:v", "libx264", "-preset", "ultrafast", "-crf", "30", "-g", "24",
            "-pix_fmt", "yuv420p", "-an",
        ],
    },
}
WORKLOADS = ("video-1080p", "av-1080p", "video-4k")
WORKLOAD_SCENARIOS = ("paused-open", "ended-open", "playing")
WORKLOAD_METRICS = {
    "paused-open": ("cpu_percent", "rss_peak_mb", "rss_steady_mb"),
    "ended-open": ("cpu_percent", "rss_peak_mb", "rss_steady_mb"),
    "playing": ("cpu_percent", "cpu_per_media_second", "rss_peak_mb", "rss_steady_mb"),
}
PLAYING_WARMUP = 0.5
PLAYING_MIN_ADVANCE = 0.9


def ffmpeg_binary():
    homebrew = Path("/opt/homebrew/bin/ffmpeg")
    if homebrew.exists():
        return homebrew
    found = shutil.which("ffmpeg")
    if found is None:
        raise SystemExit("ffmpeg is required to generate benchmark fixtures")
    return Path(found)


def ffprobe_binary():
    sibling = ffmpeg_binary().with_name("ffprobe")
    if sibling.exists():
        return sibling
    found = shutil.which("ffprobe")
    return Path(found) if found is not None else None


def media_duration(path):
    probe = ffprobe_binary()
    if probe is None:
        return None
    proc = subprocess.run(
        [str(probe), "-v", "error", "-show_entries", "format=duration",
         "-of", "default=nw=1:nk=1", str(path)],
        capture_output=True, text=True, timeout=120,
    )
    if proc.returncode != 0:
        return None
    try:
        return float(proc.stdout.strip())
    except ValueError:
        return None


def generate_fixture(name, path):
    recipe = FIXTURE_RECIPES[name]
    FIXTURES.mkdir(parents=True, exist_ok=True)
    proc = subprocess.run(
        [str(ffmpeg_binary()), "-hide_banner", "-loglevel", "error", "-y",
         *recipe["args"], str(path)],
        capture_output=True, text=True, timeout=1800,
    )
    if proc.returncode != 0:
        raise SystemExit(f"failed to generate {path}:\n{proc.stderr}")


def fixture(name, minimum=None):
    recipe = FIXTURE_RECIPES[name]
    path = FIXTURES / recipe["file"]
    declared = recipe.get("duration")
    if minimum is not None and declared is not None and declared < minimum:
        raise SystemExit(
            f"fixture recipe {name} lasts {declared} s, below the {minimum:.1f} s the "
            "workload window needs; lengthen the recipe"
        )
    if not path.exists():
        generate_fixture(name, path)
        return path
    if minimum is None:
        return path
    duration = media_duration(path)
    if duration is not None and duration < minimum:
        generate_fixture(name, path)
    return path


def workload_fixture(name, seconds):
    return fixture(name, minimum=PLAYING_WARMUP + seconds + 2.0)


def frame_seconds(name):
    fps = FIXTURE_RECIPES[name].get("fps")
    return 1 / fps if fps else DEFAULT_FRAME_SECONDS


def gop_seconds(name):
    recipe = FIXTURE_RECIPES[name]
    if recipe.get("fps") and recipe.get("gop_frames"):
        return recipe["gop_frames"] / recipe["fps"]
    return DEFAULT_GOP_SECONDS


def seek_bound(name, precision):
    if precision == "exact":
        return 1.5 * frame_seconds(name)
    return gop_seconds(name)


def default_player():
    if not sys.platform.startswith("darwin"):
        return None
    macos = ROOT / "dist" / "Player.app" / "Contents" / "MacOS"
    if not macos.is_dir():
        return None
    preferred = macos / "player"
    if preferred.is_file():
        return preferred
    executables = [path for path in sorted(macos.iterdir())
                   if path.is_file() and os.access(path, os.X_OK)]
    return executables[0] if len(executables) == 1 else None


def open_player(player, extra):
    harness.MPV = Path(player).resolve()
    return harness.Mpv(["--hwdec=no", *extra])


def process_cpu(pid):
    return perfstat.process_cpu_seconds(pid)


def seek_targets(seed, duration, count):
    rng = random.Random(seed)
    low, high = 1.0, max(1.0, duration - 1.0)
    if high <= low:
        return [low] * count
    width = (high - low) / max(1, count)
    targets = [rng.uniform(low + index * width, low + (index + 1) * width)
               for index in range(count)]
    rng.shuffle(targets)
    return targets


def wait_for_restart(mpv, restarts, timeout):
    try:
        mpv.wait_until(
            lambda: mpv.event_count("playback-restart") > restarts,
            timeout,
            "playback-restart after seek",
        )
    except harness.Failure:
        return False
    return True


def wait_seek_complete(mpv, target, bound, timeout=SEEK_TIMEOUT, started=None,
                       restarts=0):
    began = time.monotonic() if started is None else started
    remaining = max(0.0, timeout - (time.monotonic() - began))
    if not wait_for_restart(mpv, restarts, remaining):
        return {"ok": False, "reason": "timeout",
                "position": mpv.try_get_property("time-pos"), "error_s": None}
    position = mpv.try_get_property("time-pos")
    if not isinstance(position, (int, float)):
        return {"ok": False, "reason": "timeout", "position": None, "error_s": None}
    error = abs(position - target)
    if error <= bound:
        return {
            "ok": True,
            "latency_ms": (time.monotonic() - began) * 1000,
            "position": position,
            "error_s": error,
        }
    return {"ok": False, "reason": "landed out of bound", "position": position,
            "error_s": error}


def seek_trial(mpv, target, precision, bound):
    started = time.monotonic()
    restarts = mpv.event_count("playback-restart")
    mpv.command("seek", f"{target:.3f}", f"absolute+{precision}")
    outcome = wait_seek_complete(mpv, target, bound, started=started,
                                 restarts=restarts)
    trial = {
        "target": round(target, 3),
        "precision": precision,
        "outcome": "ok" if outcome["ok"] else "failed",
    }
    if outcome["ok"]:
        trial["latency_ms"] = round(outcome["latency_ms"], 2)
        trial["error_s"] = round(outcome["error_s"], 4)
    else:
        trial["reason"] = outcome["reason"]
        if outcome["error_s"] is not None:
            trial["error_s"] = round(outcome["error_s"], 4)
    return trial


def prepare_seek_player(mpv, path):
    mpv.load(path)
    mpv.wait_event("file-loaded", timeout=30)
    mpv.set_property("pause", True)
    restarts = mpv.event_count("playback-restart")
    mpv.command("seek", 0, "absolute+exact")
    if not wait_for_restart(mpv, restarts, 10):
        raise harness.Failure("no playback-restart after the preparation seek")
    mpv.wait_property(
        "time-pos",
        lambda value: isinstance(value, (int, float)) and abs(value) <= 1.0,
        timeout=10,
    )


def seek_samples(trials):
    return {
        "latency_ms": [trial["latency_ms"] for trial in trials if trial["outcome"] == "ok"],
        "error_s": [trial["error_s"] for trial in trials if trial["outcome"] == "ok"],
    }


def seek_summary(trials):
    successful = [trial for trial in trials if trial["outcome"] == "ok"]
    return {
        "success": len(successful),
        "total": len(trials),
        "latency_ms": perfstat.stat([trial["latency_ms"] for trial in successful], 1)
        if successful else None,
        "error_s": perfstat.stat([trial["error_s"] for trial in successful], 3)
        if successful else None,
    }


def seek_run(player, path, name, precision, targets, player_args=()):
    bound = seek_bound(name, precision)
    trials = []
    with open_player(player, ["--mute=yes", "--pause=yes", *player_args]) as mpv:
        prepare_seek_player(mpv, path)
        for target in targets:
            trials.append(seek_trial(mpv, target, precision, bound))
    return {"bound_s": round(bound, 4), "trials": trials,
            "samples": seek_samples(trials), "summary": seek_summary(trials)}


def seek_ab(candidate, baseline, path, name, precision, targets, cand_args, base_args):
    bound = seek_bound(name, precision)
    trials = {"candidate": [], "baseline": []}
    candidate_session = open_player(candidate, ["--mute=yes", "--pause=yes", *cand_args])
    try:
        baseline_session = open_player(baseline, ["--mute=yes", "--pause=yes", *base_args])
        try:
            prepare_seek_player(candidate_session, path)
            prepare_seek_player(baseline_session, path)
            for index, target in enumerate(targets):
                order = [("candidate", candidate_session), ("baseline", baseline_session)]
                if index % 2:
                    order.reverse()
                for side, mpv in order:
                    trials[side].append(seek_trial(mpv, target, precision, bound))
        finally:
            baseline_session.close()
    finally:
        candidate_session.close()
    return {
        side: {"trials": values, "samples": seek_samples(values),
               "summary": seek_summary(values)}
        for side, values in trials.items()
    }


def success_values(run):
    return [1.0 if trial["outcome"] == "ok" else 0.0 for trial in run["trials"]]


def seek_measure(options, candidate, baseline, reference, cand_args, base_args):
    results, trials, samples = {}, [], {}
    names = options.fixture or list(SEEK_FIXTURES)
    for name in names:
        path = fixture(name)
        duration = FIXTURE_RECIPES[name].get("duration", 30.0)
        targets = seek_targets(options.seed, duration, options.runs)
        entry = {}
        for precision in options.precision or ["exact", "keyframes"]:
            if baseline is not None:
                pair = seek_ab(candidate, baseline, path, name, precision, targets,
                               cand_args, base_args)
                entry[precision] = pair
                for side in ("candidate", "baseline"):
                    trials.extend(dict(trial, side=side) for trial in pair[side]["trials"])
                    samples[f"{name}/{precision}/{side}"] = pair[side]["samples"]
                pair["comparison"] = {
                    "latency_ms": perfstat.compare_metric(
                        "latency_ms", pair["baseline"]["samples"]["latency_ms"],
                        pair["candidate"]["samples"]["latency_ms"]),
                    "error_s": perfstat.compare_metric(
                        "error_s", pair["baseline"]["samples"]["error_s"],
                        pair["candidate"]["samples"]["error_s"]),
                    "success": perfstat.compare_paired(
                        success_values(pair["baseline"]), success_values(pair["candidate"]),
                        rel_tolerance=0.0, floor=0.0, lower_is_better=False),
                }
            else:
                run = seek_run(candidate, path, name, precision, targets, cand_args)
                entry[precision] = {"candidate": run}
                trials.extend(dict(trial, side="candidate") for trial in run["trials"])
                samples[f"{name}/{precision}/candidate"] = run["samples"]
                if reference is not None and precision == "exact":
                    ref = seek_run(reference, path, name, precision, targets, [])
                    entry[precision]["reference"] = ref
                    trials.extend(dict(trial, side="reference") for trial in ref["trials"])
                    samples[f"{name}/{precision}/reference"] = ref["samples"]
        results[name] = entry
    return results, trials, samples


def audio_cpu_sample(player, path, seconds=10, player_args=()):
    command = [str(player), "--no-config", "--no-video", "--mute=yes", "--ao=null",
               *player_args, str(path)]
    proc = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(3)
        start_cpu = process_cpu(proc.pid)
        began = time.monotonic()
        time.sleep(seconds)
        elapsed = time.monotonic() - began
        used = process_cpu(proc.pid) - start_cpu
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    return {"cpu_percent": round(used / elapsed * 100, 3), "cpu_s": round(used, 4),
            "elapsed_s": round(elapsed, 3), "outcome": "ok"}


def audio_cpu_measure(options, candidate, baseline, reference, cand_args, base_args, count):
    results, trials, samples = {}, [], {}
    names = options.fixture or list(SEEK_FIXTURES)
    for name in names:
        path = fixture(name)
        sides = {"candidate": (candidate, cand_args)}
        if baseline is not None:
            sides["baseline"] = (baseline, base_args)
        elif reference is not None:
            sides["reference"] = (reference, [])
        records = {side: [] for side in sides}
        for index in range(count):
            order = list(sides)
            if index % 2:
                order.reverse()
            for side in order:
                player, args = sides[side]
                record = audio_cpu_sample(player, path, player_args=args)
                record["side"] = side
                records[side].append(record)
                trials.append(record)
        entry = {}
        for side, values in records.items():
            entry[side] = values
            samples[f"{name}/{side}"] = {"cpu_percent": [v["cpu_percent"] for v in values]}
        if baseline is not None:
            entry["comparison"] = {"cpu_percent": perfstat.compare_metric(
                "cpu_percent", [v["cpu_percent"] for v in records["baseline"]],
                [v["cpu_percent"] for v in records["candidate"]])}
        results[name] = entry
    return results, trials, samples


def pss_mb(pid):
    if not sys.platform.startswith("linux"):
        return None
    path = Path(f"/proc/{pid}/smaps_rollup")
    try:
        for line in path.read_text().splitlines():
            if line.startswith("Pss:"):
                return int(line.split()[1]) / 1024
    except OSError:
        return None
    return None


def sample_memory(pid, seconds, interval=MEMORY_INTERVAL):
    samples = []
    began = time.monotonic()
    while True:
        entry = {"t_s": round(time.monotonic() - began, 3),
                 "rss_mb": round(perfstat.rss_mb(pid), 1)}
        pss = pss_mb(pid)
        if pss is not None:
            entry["pss_mb"] = round(pss, 1)
        samples.append(entry)
        remaining = seconds - (time.monotonic() - began)
        if remaining <= 0:
            break
        time.sleep(min(interval, remaining))
    return samples


def summarize_memory(samples, settle_s=1.0):
    steady = [sample["rss_mb"] for sample in samples if sample["t_s"] >= settle_s]
    if not steady:
        steady = [sample["rss_mb"] for sample in samples]
    return {
        "rss_steady_mb": round(perfstat.median(steady), 1),
        "rss_peak_mb": round(max(sample["rss_mb"] for sample in samples), 1),
        "steady_samples": len(steady),
        "samples": len(samples),
    }


def phys_footprint(pid):
    if not sys.platform.startswith("darwin"):
        return None
    tool = shutil.which("footprint")
    if tool is None:
        return None
    try:
        proc = subprocess.run([tool, "-p", str(pid)], capture_output=True, text=True,
                              timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return None
    match = re.search(r"phys_footprint:\s*([\d.]+)\s*([KMG]?)i?B", proc.stdout + proc.stderr)
    if match is None:
        return None
    factor = {"": 1 / (1024 * 1024), "K": 1 / 1024, "M": 1.0, "G": 1024.0}[match.group(2)]
    return round(float(match.group(1)) * factor, 1)


def frame_case(player, path, visibility, seconds, player_args=()):
    arguments = ["--mute=yes", "--fs=yes", *player_args]
    with open_player(player, arguments) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=30)
        mpv.set_property("pause", True)
        mpv.command("seek", GUI_START, "absolute+exact")
        mpv.wait_property(
            "time-pos",
            lambda value: isinstance(value, (int, float))
            and abs(value - GUI_START) <= 0.5,
            timeout=10,
        )
        mpv.command("set", "osc-visibility", visibility)
        time.sleep(0.7)
        dropped_before = mpv.get_property("frame-drop-count")
        cpu_before = process_cpu(mpv.proc.pid)
        began = time.monotonic()
        mpv.set_property("pause", False)
        time.sleep(seconds)
        elapsed = time.monotonic() - began
        dropped = mpv.get_property("frame-drop-count") - dropped_before
        used = process_cpu(mpv.proc.pid) - cpu_before
        rss = perfstat.rss_mb(mpv.proc.pid)
        mpv.set_property("pause", True)
    return {"visibility": visibility, "arguments": arguments, "outcome": "ok",
            "dropped": dropped, "played_s": round(elapsed, 3),
            "cpu_percent": round(used / elapsed * 100, 3), "rss_mb": round(rss, 1)}


def frame_samples(cases):
    buckets = {}
    for case in cases:
        bucket = buckets.setdefault(
            case["visibility"], {"dropped": [], "cpu_percent": [], "rss_mb": []}
        )
        for key in bucket:
            bucket[key].append(case[key])
    return buckets


def frame_summary(cases):
    return {
        visibility: {key: perfstat.stat(values) for key, values in bucket.items()}
        for visibility, bucket in frame_samples(cases).items()
    }


def frames_measure(options, candidate, baseline, cand_args, base_args, rounds, seconds):
    results, trials, samples = {}, [], {}
    names = options.fixture or list(SEEK_FIXTURES)
    for name in names:
        path = fixture(name)
        entry = {}
        cases = {"candidate": []} if baseline is None else {"candidate": [], "baseline": []}
        for repetition in range(rounds):
            order = ("never", "always") if repetition % 2 == 0 else ("always", "never")
            for visibility in order:
                sides = [("candidate", candidate, cand_args)]
                if baseline is not None:
                    sides.append(("baseline", baseline, base_args))
                if (repetition + (visibility == "always")) % 2:
                    sides.reverse()
                for side, player, args in sides:
                    record = frame_case(player, path, visibility, seconds, args)
                    record["side"] = side
                    cases[side].append(record)
                    trials.append(record)
        for side, values in cases.items():
            entry[side] = {"cases": values, "samples": frame_samples(values),
                           "summary": frame_summary(values)}
            samples[f"{name}/{side}"] = frame_samples(values)
        if baseline is not None:
            comparison = {}
            for visibility in ("never", "always"):
                comparison[visibility] = {
                    metric: perfstat.compare_metric(
                        metric,
                        [case[metric] for case in cases["baseline"]
                         if case["visibility"] == visibility],
                        [case[metric] for case in cases["candidate"]
                         if case["visibility"] == visibility],
                    )
                    for metric in ("dropped", "cpu_percent", "rss_mb")
                }
            entry["comparison"] = comparison
        results[name] = entry
    return results, trials, samples


def gui_scenario(player, path, name, seconds=5, player_args=()):
    arguments = ["--keep-open=yes", "--geometry=1280x720", "--mute=yes", *player_args]
    with open_player(player, arguments) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=30)
        mpv.set_property("pause", True)
        mpv.command("seek", GUI_START, "absolute+exact")
        mpv.wait_property(
            "time-pos",
            lambda value: isinstance(value, (int, float))
            and abs(value - GUI_START) <= 0.5,
            timeout=10,
        )
        visibility = "always" if name in ("paused-shown", "hover") else "never"
        mpv.command("set", "osc-visibility", visibility)
        time.sleep(0.7)
        cpu_before = process_cpu(mpv.proc.pid)
        began = time.monotonic()
        events = 0
        if name == "hover":
            seek = mpv.get_property("osc-areas")["seek"]
            y = int(seek["y"] + seek["h"] / 2)
            step = 0
            while time.monotonic() - began < seconds:
                mpv.command("mouse", int(seek["x"] + (step * 7) % int(seek["w"])), y)
                events += 1
                step += 1
                time.sleep(1 / 60)
        elif name == "playing-hidden":
            mpv.set_property("pause", False)
            time.sleep(seconds)
        else:
            time.sleep(seconds)
        elapsed = time.monotonic() - began
        used = process_cpu(mpv.proc.pid) - cpu_before
        rss = perfstat.rss_mb(mpv.proc.pid)
        if name == "playing-hidden":
            mpv.set_property("pause", True)
    return {"scenario": name, "arguments": arguments, "visibility": visibility,
            "outcome": "ok", "cpu_percent": round(used / elapsed * 100, 3),
            "rss_mb": round(rss, 1), "pointer_events": events,
            "pointer_rate_hz": round(events / elapsed, 2) if name == "hover" else None,
            "elapsed_s": round(elapsed, 3)}


def gui_cpu_measure(options, candidate, baseline, cand_args, base_args, rounds):
    results, trials, samples = {}, [], {}
    names = options.fixture or list(SEEK_FIXTURES)
    for name in names:
        path = fixture(name)
        cases = {"candidate": [], "baseline": []}
        for repetition in range(rounds):
            order = SCENARIOS if repetition % 2 == 0 else tuple(reversed(SCENARIOS))
            for scenario in order:
                sides = [("candidate", candidate, cand_args),
                         ("baseline", baseline, base_args)]
                if (repetition + SCENARIOS.index(scenario)) % 2:
                    sides.reverse()
                for side, player, args in sides:
                    record = gui_scenario(player, path, scenario, player_args=args)
                    record["side"] = side
                    cases[side].append(record)
                    trials.append(record)
        entry = {}
        for side, values in cases.items():
            entry[side] = values
            for metric in ("cpu_percent", "rss_mb"):
                samples[f"{name}/{side}/{metric}"] = [value[metric] for value in values]
        comparison = {}
        for scenario in SCENARIOS:
            comparison[scenario] = {
                metric: perfstat.compare_metric(
                    metric,
                    [value[metric] for value in cases["baseline"]
                     if value["scenario"] == scenario],
                    [value[metric] for value in cases["candidate"]
                     if value["scenario"] == scenario],
                )
                for metric in ("cpu_percent", "rss_mb")
            }
        entry["comparison"] = comparison
        results[name] = entry
    return results, trials, samples


def throughput_counters(mpv):
    def read(name):
        value = mpv.try_get_property(name)
        return value if value is not None else "unavailable"
    return {"frame_drop_count": read("frame-drop-count"),
            "estimated_vf_fps": read("estimated-vf-fps")}


def workload_sample(player, path, scenario, window=WINDOW_SECONDS, player_args=()):
    arguments = ["--vo=null", "--ao=null", "--hwdec=no", "--mute=yes"]
    if scenario == "ended-open":
        arguments.append("--keep-open=yes")
    elif scenario == "paused-open":
        arguments.append("--pause=yes")
    record = {"scenario": scenario, "arguments": arguments, "outcome": "ok"}
    with open_player(player, [*arguments, *player_args]) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=30)
        if scenario == "ended-open":
            count = mpv.event_count("end-file")
            mpv.set_property("pause", False)
            mpv.wait_until(lambda: mpv.event_count("end-file") > count, timeout=180,
                           description="end-file")
        if scenario == "playing":
            mpv.wait_property(
                "time-pos",
                lambda value: isinstance(value, (int, float)) and value >= PLAYING_WARMUP,
                timeout=30,
            )
        else:
            time.sleep(1.0)
        counters_before = throughput_counters(mpv)
        media_before = mpv.try_get_property("time-pos") if scenario == "playing" else None
        cpu_before = process_cpu(mpv.proc.pid)
        began = time.monotonic()
        memory = sample_memory(mpv.proc.pid, window)
        elapsed = time.monotonic() - began
        used = process_cpu(mpv.proc.pid) - cpu_before
        counters_after = throughput_counters(mpv)
        footprint = phys_footprint(mpv.proc.pid)
        media_after = mpv.try_get_property("time-pos") if scenario == "playing" else None
        if scenario == "ended-open":
            mpv.set_property("pause", True)
    record.update({
        "cpu_percent": round(used / elapsed * 100, 3), "cpu_s": round(used, 6),
        "elapsed_s": round(elapsed, 3), "memory": summarize_memory(memory),
        "phys_footprint_mb": footprint if footprint is not None else "unavailable",
        "rss_samples": memory,
    })
    if scenario != "playing":
        return record
    advanced = None
    if isinstance(media_before, (int, float)) and isinstance(media_after, (int, float)):
        advanced = media_after - media_before
    drops = counters_after["frame_drop_count"]
    if isinstance(drops, int) and isinstance(counters_before["frame_drop_count"], int):
        drops -= counters_before["frame_drop_count"]
    record.update({
        "media_s": None if advanced is None else round(advanced, 3),
        "cpu_per_media_second": (None if not advanced or advanced <= 0
                                 else round(used / advanced, 6)),
        "frame_drop_count": counters_after["frame_drop_count"],
        "frame_drops": drops,
        "estimated_vf_fps": counters_after["estimated_vf_fps"],
    })
    if advanced is None or advanced < PLAYING_MIN_ADVANCE * elapsed:
        record["outcome"] = "failed"
        record["reason"] = (
            f"time-pos advanced {advanced:.3f} s over {elapsed:.3f} s of wall time, "
            f"below {PLAYING_MIN_ADVANCE:.0%}"
        )
        record["cpu_percent"] = None
        record["cpu_per_media_second"] = None
    return record


def workload_metric(record, metric):
    if record["outcome"] != "ok":
        return None
    if metric in ("rss_peak_mb", "rss_steady_mb"):
        return record["memory"][metric]
    return record.get(metric)


def workloads_measure(options, candidate, baseline, cand_args, base_args, rounds, seconds):
    results, trials, samples = {}, [], {}
    names = options.fixture or list(WORKLOADS)
    for name in names:
        path = workload_fixture(name, seconds)
        sides = {"candidate": (candidate, cand_args)}
        if baseline is not None:
            sides["baseline"] = (baseline, base_args)
        entry = {}
        for scenario in WORKLOAD_SCENARIOS:
            records = {side: [] for side in sides}
            for index in range(rounds):
                order = list(sides)
                if index % 2:
                    order.reverse()
                for side in order:
                    player, args = sides[side]
                    record = workload_sample(player, path, scenario, window=seconds,
                                             player_args=args)
                    record["side"] = side
                    records[side].append(record)
                    trials.append(record)
            scenario_entry = {}
            for side, values in records.items():
                scenario_entry[side] = values
                for metric in WORKLOAD_METRICS[scenario]:
                    samples[f"{name}/{scenario}/{side}/{metric}"] = [
                        workload_metric(value, metric) for value in values
                    ]
            if baseline is not None:
                scenario_entry["comparison"] = {
                    metric: perfstat.compare_metric(
                        metric,
                        [workload_metric(value, metric) for value in records["baseline"]],
                        [workload_metric(value, metric) for value in records["candidate"]])
                    for metric in WORKLOAD_METRICS[scenario]
                }
            entry[scenario] = scenario_entry
        results[name] = entry
    return results, trials, samples


def prepare_lifecycle_player(mpv, path):
    mpv.load(path)
    mpv.wait_event("file-loaded", timeout=30)
    mpv.set_property("pause", True)
    mpv.command("seek", 0, "absolute+exact")
    mpv.wait_property(
        "time-pos",
        lambda value: isinstance(value, (int, float)) and abs(value) <= 1.0,
        timeout=10,
    )


def lifecycle_cycle(mpv, path, targets, index, per_cycle, bound):
    peak = perfstat.rss_mb(mpv.proc.pid)
    for _ in range(per_cycle):
        target = targets[index % len(targets)]
        index += 1
        seek_trial(mpv, target, "exact", bound)
        peak = max(peak, perfstat.rss_mb(mpv.proc.pid))
    count = mpv.event_count("file-loaded")
    mpv.command("loadfile", str(path), "replace")
    mpv.wait_until(lambda: mpv.event_count("file-loaded") > count, timeout=30,
                   description="reload")
    mpv.set_property("pause", True)
    peak = max(peak, perfstat.rss_mb(mpv.proc.pid))
    return peak, index


def lifecycle_plan(name, seeks, loads, seed):
    duration = FIXTURE_RECIPES[name].get("duration", 30.0)
    targets = seek_targets(seed, duration, seeks)
    return targets, seek_bound(name, "exact"), max(1, seeks // loads)


def lifecycle_run(player, path, name, seeks=50, loads=10, seed=7, player_args=()):
    targets, bound, per_cycle = lifecycle_plan(name, seeks, loads, seed)
    cycles = []
    with open_player(player, ["--mute=yes", "--pause=yes", "--vo=null", "--ao=null",
                              "--hwdec=no", *player_args]) as mpv:
        prepare_lifecycle_player(mpv, path)
        index = 0
        for cycle in range(loads):
            peak, index = lifecycle_cycle(mpv, path, targets, index, per_cycle, bound)
            cycles.append({"cycle": cycle, "rss_peak_mb": round(peak, 1), "outcome": "ok"})
    peak_samples = [cycle["rss_peak_mb"] for cycle in cycles]
    return {"cycles": cycles, "samples": {"rss_peak_mb": peak_samples},
            "summary": {"rss_peak_mb": perfstat.stat(peak_samples, 1)}}


def lifecycle_ab(candidate, baseline, path, name, cand_args, base_args,
                 seeks=50, loads=10, seed=7):
    targets, bound, per_cycle = lifecycle_plan(name, seeks, loads, seed)
    sessions = {
        "candidate": open_player(candidate, ["--mute=yes", "--pause=yes", "--vo=null",
                                             "--ao=null", "--hwdec=no", *cand_args]),
        "baseline": open_player(baseline, ["--mute=yes", "--pause=yes", "--vo=null",
                                           "--ao=null", "--hwdec=no", *base_args]),
    }
    cycles = {"candidate": [], "baseline": []}
    try:
        for mpv in sessions.values():
            prepare_lifecycle_player(mpv, path)
        indexes = {"candidate": 0, "baseline": 0}
        for cycle in range(loads):
            order = ["candidate", "baseline"]
            if cycle % 2:
                order.reverse()
            for side in order:
                peak, indexes[side] = lifecycle_cycle(
                    sessions[side], path, targets, indexes[side], per_cycle, bound)
                cycles[side].append({"cycle": cycle, "rss_peak_mb": round(peak, 1),
                                     "outcome": "ok"})
    finally:
        for mpv in sessions.values():
            mpv.close()
    result = {}
    for side, values in cycles.items():
        peak_samples = [cycle["rss_peak_mb"] for cycle in values]
        result[side] = {"cycles": values, "samples": {"rss_peak_mb": peak_samples},
                        "summary": {"rss_peak_mb": perfstat.stat(peak_samples, 1)}}
    return result


def lifecycle_measure(options, candidate, baseline, cand_args, base_args):
    name = (options.fixture or ["h264-gop10"])[0]
    path = fixture(name)
    if baseline is None:
        run = lifecycle_run(candidate, path, name, player_args=cand_args)
        trials = [dict(cycle, side="candidate") for cycle in run["cycles"]]
        return {name: {"candidate": run}}, trials, {f"{name}/candidate": run["samples"]}
    pair = lifecycle_ab(candidate, baseline, path, name, cand_args, base_args)
    pair["comparison"] = {"rss_peak_mb": perfstat.compare_metric(
        "rss_peak_mb", pair["baseline"]["samples"]["rss_peak_mb"],
        pair["candidate"]["samples"]["rss_peak_mb"])}
    trials = []
    for side, run in pair.items():
        if side == "comparison":
            continue
        trials.extend(dict(cycle, side=side) for cycle in run["cycles"])
    samples = {f"{name}/{side}": run["samples"] for side, run in pair.items()
               if side != "comparison"}
    return {name: pair}, trials, samples


def mode_fixed(mode):
    if mode == "seek":
        return ["--mute=yes", "--pause=yes"]
    if mode == "audio-cpu":
        return ["--no-config", "--no-video", "--mute=yes", "--ao=null"]
    if mode == "frames":
        return ["--mute=yes", "--fs=yes"]
    if mode == "gui-cpu":
        return ["--keep-open=yes", "--geometry=1280x720", "--mute=yes"]
    if mode == "workloads":
        return ["--vo=null", "--ao=null", "--hwdec=no", "--mute=yes"]
    if mode == "lifecycle":
        return ["--mute=yes", "--pause=yes", "--vo=null", "--ao=null", "--hwdec=no"]
    if mode == "all":
        collected = []
        for name in ("seek", "audio-cpu", "frames", "gui-cpu", "workloads", "lifecycle"):
            for argument in mode_fixed(name):
                if argument not in collected:
                    collected.append(argument)
        return collected
    return []


def workload_line(record):
    memory = record["memory"]
    fields = [f"CPU {record['cpu_percent']:6.3f} %" if record["cpu_percent"] is not None
              else "CPU n/a"]
    if "cpu_per_media_second" in record:
        rate = record["cpu_per_media_second"]
        fields.append(f"{rate:7.4f} CPU-s/media-s" if rate is not None
                      else "CPU-s/media-s n/a")
        fields.append(f"media {record['media_s']} s")
        fields.append(f"drops {record['frame_drops']}")
        fields.append(f"vf-fps {record['estimated_vf_fps']}")
    fields.append(f"RSS steady {memory['rss_steady_mb']} MB")
    fields.append(f"peak {memory['rss_peak_mb']} MB")
    fields.append(f"footprint {record['phys_footprint_mb']}")
    if record["outcome"] != "ok":
        fields.append(f"FAILED ({record.get('reason')})")
    return "  ".join(fields)


def print_results(results):
    for section, payload in results.items():
        if section == "seek":
            for name, entry in payload.items():
                for precision, sides in entry.items():
                    for side, run in sides.items():
                        if side == "comparison":
                            continue
                        summary = run["summary"]
                        latency = perfstat.format_range(summary["latency_ms"], " ms")
                        error = perfstat.format_range(summary["error_s"], " s")
                        print(f"{name:12s} seek {precision:9s} [{side:9s}] "
                              f"success {summary['success']}/{summary['total']}  "
                              f"latency {latency}  error {error}", flush=True)
                    if "comparison" in sides:
                        for metric, outcome in sides["comparison"].items():
                            print(f"{name:12s} seek {precision:9s} {metric:10s} "
                                  f"{perfstat.describe_verdict(outcome)}", flush=True)
        elif section == "audio-cpu":
            for name, entry in payload.items():
                for side, records in entry.items():
                    if side == "comparison":
                        continue
                    stats = perfstat.stat([record["cpu_percent"] for record in records], 3)
                    print(f"{name:12s} audio-only CPU [{side:9s}] "
                          f"{perfstat.format_range(stats, ' %')} over {len(records)}",
                          flush=True)
                if "comparison" in entry:
                    outcome = entry["comparison"]["cpu_percent"]
                    print(f"{name:12s} audio-only CPU {perfstat.describe_verdict(outcome)}",
                          flush=True)
        elif section == "frames":
            for name, entry in payload.items():
                for side, run in entry.items():
                    if side == "comparison":
                        continue
                    for visibility, summary in run["summary"].items():
                        dropped = summary["dropped"]
                        cpu = perfstat.format_range(summary["cpu_percent"], " %")
                        rss = perfstat.format_range(summary["rss_mb"], " MB")
                        print(f"{name:12s} fullscreen {visibility:6s} [{side:9s}] "
                              f"dropped {dropped['median']} "
                              f"[{dropped['min']}, {dropped['max']}]  "
                              f"CPU {cpu}  RSS {rss}", flush=True)
                if "comparison" in entry:
                    for visibility, metrics in entry["comparison"].items():
                        for metric, outcome in metrics.items():
                            print(f"{name:12s} fullscreen {visibility:6s} {metric:11s} "
                                  f"{perfstat.describe_verdict(outcome)}", flush=True)
        elif section == "gui-cpu":
            for name, entry in payload.items():
                for side, records in entry.items():
                    if side == "comparison":
                        continue
                    for scenario in SCENARIOS:
                        values = [record for record in records
                                  if record["scenario"] == scenario]
                        if not values:
                            continue
                        cpu = perfstat.stat([value["cpu_percent"] for value in values], 2)
                        rss = perfstat.stat([value["rss_mb"] for value in values], 1)
                        pointer = values[0].get("pointer_rate_hz")
                        extra = f"  pointer {pointer:.2f} Hz" if pointer else ""
                        print(f"{name:12s} {scenario:15s} [{side:9s}] "
                              f"CPU {perfstat.format_range(cpu, ' %')}  "
                              f"RSS {perfstat.format_range(rss, ' MB')}{extra}",
                              flush=True)
                if "comparison" in entry:
                    for scenario, metrics in entry["comparison"].items():
                        for metric, outcome in metrics.items():
                            print(f"{name:12s} {scenario:15s} {metric:11s} "
                                  f"{perfstat.describe_verdict(outcome)}", flush=True)
        elif section == "workloads":
            for name, entry in payload.items():
                for scenario, sides in entry.items():
                    for side, records in sides.items():
                        if side == "comparison":
                            continue
                        for record in records:
                            print(f"{name:12s} {scenario:12s} [{side:9s}] "
                                  f"{workload_line(record)}", flush=True)
                    if "comparison" in sides:
                        for metric, outcome in sides["comparison"].items():
                            print(f"{name:12s} {scenario:12s} {metric:11s} "
                                  f"{perfstat.describe_verdict(outcome)}", flush=True)
        elif section == "lifecycle":
            for name, entry in payload.items():
                for side, run in entry.items():
                    if side == "comparison":
                        continue
                    summary = perfstat.format_range(run["summary"]["rss_peak_mb"], " MB")
                    print(f"{name:12s} lifecycle [{side:9s}] RSS peak {summary} over "
                          f"{len(run['cycles'])} cycles", flush=True)
                if "comparison" in entry:
                    outcome = entry["comparison"]["rss_peak_mb"]
                    print(f"{name:12s} lifecycle RSS peak "
                          f"{perfstat.describe_verdict(outcome)}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["seek", "audio-cpu", "frames", "gui-cpu",
                                         "workloads", "lifecycle", "all"])
    parser.add_argument("--player", default=None,
                        help="player to measure (default: dist/Player.app on macOS)")
    parser.add_argument("--reference", default=str(REFERENCE) if REFERENCE.exists() else None,
                        help="player to compare against (default: Homebrew mpv when installed)")
    parser.add_argument("--baseline", help="player to compare against, interleaved")
    parser.add_argument("--player-arg", action="append", default=[],
                        help="option passed to the player, such as --gpu-api=metal; repeatable")
    parser.add_argument("--baseline-arg", action="append", default=[],
                        help="option passed to the baseline player; repeatable")
    parser.add_argument("--fixture", choices=sorted(FIXTURE_RECIPES), action="append")
    parser.add_argument("--precision", choices=["exact", "keyframes"], action="append")
    parser.add_argument("--runs", type=int, default=None,
                        help="passes when --baseline is given, samples per side otherwise; an "
                             "explicit count is used exactly for every mode, including "
                             "workloads (default: 15 samples, which keeps the documented 3 "
                             "pairs for audio-cpu, frames and workloads and 5 for gui-cpu)")
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--seconds", type=float, default=None,
                        help="measurement window for frames, GUI and workload modes; workloads "
                             "default to 5 s of steady playback (default: mode-specific)")
    options = parser.parse_args()
    runs_given = options.runs is not None
    options.runs = 15 if not runs_given else options.runs

    player = Path(options.player) if options.player else default_player()
    if player is None:
        parser.error("--player is required: no player inside dist/Player.app")
    if not player.exists():
        parser.error(f"missing player: {player}")
    baseline = Path(options.baseline) if options.baseline else None
    if baseline is not None and not baseline.exists():
        parser.error(f"missing baseline player: {baseline}")
    if options.mode == "gui-cpu" and baseline is None and not options.baseline_arg:
        parser.error("gui-cpu needs --baseline or --baseline-arg")
    if options.mode == "gui-cpu" and baseline is None:
        baseline = player
    reference = Path(options.reference) if options.reference else None
    if reference is not None and not reference.exists():
        reference = None
    if options.mode not in ("seek", "audio-cpu", "all"):
        reference = None

    fixed = mode_fixed(options.mode)
    cand_args = [*fixed, *options.player_arg]
    base_args = [*fixed, *(options.baseline_arg or options.player_arg)]

    host_before = perfstat.host_state()
    if host_before != perfstat.HOST_QUIET:
        print("perf gate deferred: host busy", flush=True)

    results, trials, samples = {}, [], {}

    if options.mode in ("seek", "all"):
        found = seek_measure(options, player, baseline, reference, cand_args, base_args)
        results["seek"] = found[0]
        trials.extend(found[1])
        samples.update(found[2])
    if options.mode in ("audio-cpu", "all"):
        count = (1 if baseline is None
                 else options.runs if runs_given else max(3, options.runs // 5))
        found = audio_cpu_measure(options, player, baseline, reference, cand_args,
                                  base_args, count)
        results["audio-cpu"] = found[0]
        trials.extend(found[1])
        samples.update(found[2])
    if options.mode in ("frames", "all"):
        rounds = options.runs if runs_given else max(2, options.runs // 5)
        seconds = options.seconds or 8.0
        found = frames_measure(options, player, baseline, cand_args, base_args,
                               rounds, seconds)
        results["frames"] = found[0]
        trials.extend(found[1])
        samples.update(found[2])
    if options.mode == "gui-cpu" or (options.mode == "all" and baseline is not None):
        rounds = options.runs if runs_given else max(3, options.runs // 3)
        found = gui_cpu_measure(options, player, baseline, cand_args, base_args, rounds)
        results["gui-cpu"] = found[0]
        trials.extend(found[1])
        samples.update(found[2])
    if options.mode in ("workloads", "all"):
        rounds = 1 if baseline is None else (options.runs if runs_given
                                            else max(3, options.runs // 5))
        seconds = options.seconds or WINDOW_SECONDS
        found = workloads_measure(options, player, baseline, cand_args, base_args,
                                  rounds, seconds)
        results["workloads"] = found[0]
        trials.extend(found[1])
        samples.update(found[2])
    if options.mode in ("lifecycle", "all"):
        found = lifecycle_measure(options, player, baseline, cand_args, base_args)
        results["lifecycle"] = found[0]
        trials.extend(found[1])
        samples.update(found[2])

    print_results(results)

    host_after = perfstat.host_state(once=True)
    deferred = (host_before != perfstat.HOST_QUIET
                or host_after != perfstat.HOST_QUIET)
    players = {"candidate": perfstat.player_record(player, cand_args)}
    if baseline is not None:
        players["baseline"] = perfstat.player_record(baseline, base_args)
    if reference is not None:
        players["reference"] = perfstat.player_record(reference, [])
    payload = {
        "deferred": deferred,
        "host": {"before": host_before, "after": host_after},
        "players": players,
        "results": results,
        "samples": samples,
        "trials": trials,
    }
    path = perfstat.write_result(options.mode, payload)
    print(f"wrote {path.relative_to(ROOT)}", flush=True)
    if deferred:
        print("perf gate deferred: host busy", flush=True)
        if baseline is not None:
            return HOST_BUSY
    return 0


if __name__ == "__main__":
    sys.exit(main())
