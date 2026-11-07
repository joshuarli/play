"""Shared measurement helpers for the Play benchmark and measurement scripts."""

import datetime
import itertools
import json
import re
import shutil
import statistics
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

HOST_QUIET = "quiet"
HOST_BUSY = "busy"
HOST_DEFERRED = "deferred"


def run(command, timeout=120, **kwargs):
    return subprocess.run(command, capture_output=True, text=True, timeout=timeout, **kwargs)


def median(values):
    return statistics.median(values) if values else None


def stat(values, digits=2):
    rounded = [round(value, digits) for value in values]
    if not rounded:
        return {"median": None, "min": None, "max": None, "spread": None, "samples": []}
    return {
        "median": round(statistics.median(values), digits),
        "min": round(min(values), digits),
        "max": round(max(values), digits),
        "spread": round(max(values) - min(values), digits),
        "samples": rounded,
    }


def format_range(stats, suffix=""):
    if not stats or stats.get("median") is None:
        return "n/a"
    return f"{stats['median']} [{stats['min']}, {stats['max']}]{suffix}"


def host_state(once=False, timeout=120):
    tool = shutil.which("perf-gate-ready")
    if tool is None:
        return HOST_DEFERRED
    try:
        result = subprocess.run(
            [tool, "-1"] if once else [tool], capture_output=True, timeout=timeout
        )
    except (OSError, subprocess.TimeoutExpired):
        return HOST_DEFERRED
    return HOST_QUIET if result.returncode == 0 else HOST_BUSY


def cpu_seconds(ps_time):
    """Parse `ps -o time=` output: MM:SS.ss, M:SS.ss, HH:MM:SS, or D-HH:MM:SS."""
    text = ps_time.strip()
    if not text:
        raise ValueError("empty ps time")
    days = 0
    if "-" in text:
        day_text, text = text.split("-", 1)
        days = int(day_text)
    parts = text.split(":")
    if len(parts) == 3:
        hours, minutes, seconds = parts
    elif len(parts) == 2:
        hours, minutes, seconds = "0", parts[0], parts[1]
    else:
        raise ValueError(f"unrecognized ps time: {ps_time!r}")
    return days * 86400 + int(hours) * 3600 + int(minutes) * 60 + float(seconds)


def process_cpu_seconds(pid):
    proc = run(["ps", "-o", "time=", "-p", str(pid)], timeout=10)
    if proc.returncode != 0:
        raise RuntimeError(f"ps failed for pid {pid}: {proc.stderr.strip()}")
    return cpu_seconds(proc.stdout)


def compare_paired(baseline, candidate, rel_tolerance=0.05, floor=0.0, lower_is_better=True):
    pairs = min(len(baseline), len(candidate))
    if not baseline or not candidate or len(baseline) != len(candidate):
        return {
            "verdict": "invalid",
            "reason": "paired samples differ in length or are empty",
            "median_delta": None,
            "lo": None,
            "hi": None,
            "tolerance": None,
            "pairs": pairs,
            "baseline_median": None,
            "candidate_median": None,
        }
    if any(value is None for value in baseline) or any(value is None for value in candidate):
        return {
            "verdict": "invalid",
            "reason": "a trial failed",
            "median_delta": None,
            "lo": None,
            "hi": None,
            "tolerance": None,
            "pairs": pairs,
            "baseline_median": None,
            "candidate_median": None,
        }
    baseline_median = statistics.median(baseline)
    candidate_median = statistics.median(candidate)
    tolerance = max(rel_tolerance * abs(baseline_median), floor)
    if lower_is_better:
        differences = [candidate_value - baseline_value
                       for baseline_value, candidate_value in zip(baseline, candidate)]
    else:
        differences = [baseline_value - candidate_value
                       for baseline_value, candidate_value in zip(baseline, candidate)]
    ordered = sorted(differences)
    if len(ordered) >= 7:
        lo, hi = ordered[1], ordered[-2]
    else:
        lo, hi = ordered[0], ordered[-1]
    if lo > tolerance:
        verdict = "regressed"
    elif hi < -tolerance:
        verdict = "improved"
    elif -tolerance <= lo and hi <= tolerance:
        verdict = "unchanged"
    else:
        verdict = "inconclusive"
    return {
        "verdict": verdict,
        "reason": None,
        "median_delta": statistics.median(differences),
        "lo": lo,
        "hi": hi,
        "tolerance": tolerance,
        "pairs": pairs,
        "baseline_median": baseline_median,
        "candidate_median": candidate_median,
    }


def describe_verdict(outcome):
    if outcome["verdict"] == "invalid":
        return f"INVALID ({outcome.get('reason')})"
    return (
        f"{outcome['verdict'].upper()} median delta {outcome['median_delta']:+.3f}, "
        f"lo {outcome['lo']:+.3f}, hi {outcome['hi']:+.3f}, "
        f"tolerance {outcome['tolerance']:.3f}"
    )


def profile_label(player):
    try:
        resolved = Path(player).resolve()
    except OSError:
        resolved = Path(player)
    try:
        resolved.relative_to((ROOT / "dist").resolve())
        return "dist"
    except ValueError:
        pass
    for dev in (ROOT / "build" / "mpv", ROOT / "build-linux" / "mpv"):
        if resolved == dev.resolve():
            return "dev"
    return "unknown"


def version(player):
    proc = run([str(player), "--version"], timeout=30)
    if proc.returncode != 0:
        raise RuntimeError(f"{player} --version exited {proc.returncode}: {proc.stderr.strip()}")
    return (proc.stdout + proc.stderr).strip()


def player_record(player, arguments):
    return {
        "path": str(player),
        "profile": profile_label(player),
        "version": version(player),
        "arguments": [str(player), *[str(argument) for argument in arguments]],
    }


def write_result(mode, payload, root=ROOT):
    for key in ("players", "samples", "trials"):
        if key not in payload:
            raise ValueError(f"result for {mode} is missing {key}")
    candidate = payload["players"].get("candidate")
    if not candidate or not all(key in candidate for key in ("arguments", "version", "profile")):
        raise ValueError("result needs a candidate record with arguments, version, and profile")
    payload = dict(payload)
    payload.setdefault("mode", mode)
    payload.setdefault("timestamp", datetime.datetime.now(datetime.timezone.utc).isoformat())
    directory = Path(root) / ".work" / "bench"
    directory.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    safe_mode = re.sub(r"[^A-Za-z0-9._-]+", "-", mode).strip("-") or "run"
    stem = f"{stamp}-{safe_mode}"
    for suffix in itertools.count():
        path = directory / (f"{stem}.json" if suffix == 0 else f"{stem}-{suffix}.json")
        try:
            handle = open(path, "x", encoding="utf-8")
        except FileExistsError:
            continue
        with handle:
            handle.write(json.dumps(payload, indent=2) + "\n")
        return path


def startup(player, runs=7):
    def series(synced):
        values = []
        for _ in range(runs):
            if synced:
                run(["sync"], timeout=30)
            started = time.monotonic()
            proc = run([str(player), "--version"], timeout=30)
            values.append((time.monotonic() - started) * 1000)
            if proc.returncode != 0:
                raise RuntimeError("player --version failed")
        return stat(values)

    return {"after_sync": series(True), "warm": series(False)}


def first_frame(player, path, runs=7):
    values = []
    for _ in range(runs):
        started = time.monotonic()
        proc = run(
            [str(player), "--no-config", "--vo=null", "--ao=null", "--frames=1",
             "--untimed", "--hwdec=no", str(path)],
            timeout=60,
        )
        if proc.returncode != 0:
            raise RuntimeError(f"first-frame run failed:\n{proc.stderr}")
        values.append((time.monotonic() - started) * 1000)
    return stat(values)


def rss_mb(pid):
    proc = run(["ps", "-o", "rss=", "-p", str(pid)], timeout=10)
    text = proc.stdout.strip()
    if proc.returncode != 0 or not text:
        raise RuntimeError(f"could not read RSS for pid {pid}")
    return int(text) / 1024


def idle_rss(mpv, runs=1):
    values = []
    for _ in range(runs):
        with mpv(["--vo=null", "--ao=null"]) as player:
            time.sleep(0.5)
            values.append(rss_mb(player.proc.pid))
    return values[0] if runs == 1 else stat(values)


def seek_latency(mpv, path, runs=7, tolerance=0.25, timeout=5.0):
    values = []
    with mpv(["--vo=null", "--ao=null", "--pause=yes"]) as player:
        player.load(path)
        player.wait_event("file-loaded", timeout=10)
        duration = player.wait_property(
            "duration", lambda value: isinstance(value, (int, float)) and value > 0, timeout=10
        )
        for _ in range(runs):
            started = time.monotonic()
            player.command("seek", duration / 2, "absolute+exact")
            player.wait_property(
                "time-pos",
                lambda value: isinstance(value, (int, float))
                and abs(value - duration / 2) <= tolerance,
                timeout=timeout,
            )
            values.append((time.monotonic() - started) * 1000)
            player.command("seek", 0, "absolute+exact")
    return stat(values)


def exit_latency(mpv, runs=7):
    values = []
    for _ in range(runs):
        player = mpv(["--vo=null", "--ao=null"])
        time.sleep(0.2)
        started = time.monotonic()
        player.close()
        values.append((time.monotonic() - started) * 1000)
    return stat(values)
