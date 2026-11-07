#!/usr/bin/env python3
"""Golden behavioral traces.

Drives the player over the fixture corpus through the JSON IPC and
records normalized behavior: duration, tracks, chapters, metadata keys,
default properties, command results and errors, event order, seek completion,
EOF, playlist transitions, and malformed-input handling.

Goldens live in tests/traces/ and are committed. Regenerate with
`python3 tests/trace.py --regen`, then commit the result. Compare with
`python3 tests/trace.py --check` (used by the `traces` gate). Floats are
rounded; event payloads are dropped except for the fields the comparator
names. Seek completion is a range check, not an exact value, because
decode position after a seek is timing dependent.
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from harness import Failure, Mpv, fixture_path, load_manifest  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
TRACES = Path(__file__).resolve().parent / "traces"

ARGS = ["--vo=null", "--ao=null"]

METADATA_VALUE_KEYS = ("title", "artist", "album", "genre", "track", "date")


def round_floats(value, places=2):
    if isinstance(value, float):
        return round(value, places)
    if isinstance(value, list):
        return [round_floats(item, places) for item in value]
    if isinstance(value, dict):
        return {key: round_floats(item, places) for key, item in value.items()}
    return value


def norm_track(track):
    out = {key: track[key] for key in ("type", "codec") if key in track}
    for key in ("lang", "title", "default", "forced", "external"):
        if key in track:
            out[key] = track[key]
    audio = track.get("audio", {})
    if isinstance(audio, dict):
        for key in ("channels", "samplerate"):
            if key in audio:
                out["audio-" + key] = audio[key]
    demux = track.get("demux", {})
    if isinstance(demux, dict):
        for key in ("w", "h"):
            if key in demux:
                out["video-" + key] = demux[key]
    return round_floats(out)


def norm_chapters(chapters):
    out = []
    for chapter in chapters or []:
        out.append({
            "title": chapter.get("title"),
            "time": round(float(chapter.get("time", 0)), 2),
        })
    return out


def norm_metadata(meta):
    if not isinstance(meta, dict):
        return {}
    return {key: (meta[key] if key in METADATA_VALUE_KEYS else "?")
            for key in sorted(meta)}


def canon_order(events):
    """Canonicalize racy but contract-free event variation.

    The `seek` event and its `playback-restart` race on the socket, the
    engine sometimes emits a second `playback-restart` after a seek,
    teardown events after `end-file` race with the snapshot read, and macOS
    emits a startup `idle` before `start-file` that Linux does not. All four
    vary by run or platform on the reference itself, so none of them is
    contract. Event kinds, their order, and pre-EOF counts are preserved,
    so a dropped, duplicated (beyond the collapsed restart), or reordered
    event still fails comparison.
    """
    events = list(events)
    while events and events[0] == "idle":
        events.pop(0)
    if "end-file" in events:
        events = events[:events.index("end-file") + 1]
    out = []
    i = 0
    while i < len(events):
        if events[i] == "playback-restart" and i + 1 < len(events) \
                and events[i + 1] == "seek":
            out += ["seek", "playback-restart"]
            i += 2
            continue
        if events[i] == "playback-restart" and out \
                and out[-1] == "playback-restart":
            i += 1
            continue
        out.append(events[i])
        i += 1
    return out


def command_error(mpv, *args):
    try:
        mpv.command(*args, timeout=5)
    except Failure as exc:
        text = str(exc)
        marker = "failed: "
        if marker in text:
            return text.split(marker, 1)[1].strip()
        return "failure"
    return None


def transitions(initial, values):
    """Keep only genuine transitions from the initial value."""
    state = initial
    out = []
    for value in values:
        if value != state:
            state = value
            out.append(state)
    return out


def wait_params(mpv, name, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = mpv.try_get_property(name)
        if value is not None:
            return value
        time.sleep(0.05)
    return None


def trace_playable(name, entry):
    trace = {"fixture": name, "role": entry.get("role")}
    with Mpv(ARGS) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=15)
        trace["duration"] = round_floats(mpv.try_get_property("duration"))
        trace["tracks"] = [norm_track(t)
                           for t in mpv.try_get_property("track-list") or []]
        kinds = {track.get("type") for track in trace["tracks"]}
        trace["chapters"] = norm_chapters(mpv.try_get_property("chapter-list"))
        trace["metadata"] = norm_metadata(mpv.try_get_property("metadata"))
        trace["playlist-count"] = mpv.try_get_property("playlist-count")
        trace["pause-default"] = mpv.try_get_property("pause")
        trace["volume-default"] = mpv.try_get_property("volume")
        trace["speed-default"] = mpv.try_get_property("speed")
        trace["audio-params"] = round_floats(
            wait_params(mpv, "audio-params") if "audio" in kinds else None)
        trace["video-params"] = round_floats(
            wait_params(mpv, "video-params") if "video" in kinds else None)
        trace["error-bogus-property"] = command_error(
            mpv, "get_property", "bogus-oracle-prop")
        trace["error-bad-seek"] = command_error(
            mpv, "seek", "not-a-number", "absolute")
        observe_id = 424242
        mpv.command("observe_property", observe_id, "pause")
        # The observation synthesizes the current value asynchronously. Wait
        # for it so the mark below starts past it; otherwise the False probe
        # could match this stale synthesis instead of the real toggle.
        mpv.wait_event(
            "property-change",
            lambda e: e.get("id") == observe_id,
            timeout=10)
        with mpv.cond:
            mark = len(mpv.events)
        mpv.command("cycle", "pause")
        mpv.wait_until(
            lambda: any(
                event.get("id") == observe_id and event.get("data") is True
                for event in mpv.events[mark:]),
            timeout=10,
            description="observed pause True")
        mpv.command("cycle", "pause")
        mpv.wait_until(
            lambda: any(
                event.get("id") == observe_id and event.get("data") is False
                for event in mpv.events[mark:]),
            timeout=10,
            description="observed pause False")
        mpv.command("unobserve_property", observe_id)
        with mpv.cond:
            pause_events = [e for e in mpv.events[mark:]
                            if e.get("event") == "property-change"
                            and e.get("id") == observe_id]
        # Keep only genuine transitions from the pre-cycle value. The engine
        # sometimes rebroadcasts the current value or duplicates a delivery,
        # which varies run to run on the reference itself.
        trace["pause-cycle-events"] = transitions(
            trace["pause-default"],
            [event.get("data") for event in pause_events])
        duration = trace["duration"] or 0
        if duration > 1:
            target = round(duration / 2, 2)
            mpv.command("seek", str(target), "absolute")
            mpv.wait_event("seek", timeout=10)
            pos = mpv.try_get_property("time-pos") or 0
            trace["seek-target"] = target
            trace["seek-pos"] = round(float(pos), 2)
        mpv.command("seek", str((trace["duration"] or 3) + 30), "absolute")
        end = mpv.wait_eof(timeout=20)
        trace["eof-reason"] = end.get("reason")
        with mpv.cond:
            trace["event-order"] = canon_order(
                [e.get("event") for e in mpv.events
                 if e.get("event") != "property-change"])
    return trace


def trace_subtitle(name, entry, companion_name, companion_entry):
    trace = {"fixture": name, "role": entry.get("role"),
             "companion": companion_name}
    args = [*ARGS, f"--sub-file={fixture_path(entry)}"]
    with Mpv(args) as mpv:
        mpv.load(fixture_path(companion_entry))
        mpv.wait_event("file-loaded", timeout=15)
        tracks = mpv.try_get_property("track-list") or []
        trace["sub-tracks"] = [norm_track(t) for t in tracks
                               if t.get("type") == "sub"]
        # Default sid sampling races auto-resolution, so the trace drives
        # explicit selection instead: both round-trips are deterministic.
        if not trace["sub-tracks"]:
            trace["load-error"] = bool(mpv.log_matches(r"cannot load subtitle"))
            return trace
        mpv.command("set", "sid", "no")
        trace["sid-no"] = mpv.wait_until(
            lambda: mpv.try_get_property("sid") is False,
            timeout=10,
            description="sid deselected")
        trace["sub-visibility"] = mpv.try_get_property("sub-visibility")
    return trace


def trace_failure(name, entry):
    trace = {"fixture": name, "role": entry.get("role")}
    with Mpv(ARGS) as mpv:
        mpv.load(fixture_path(entry))
        try:
            end = mpv.wait_event("end-file", timeout=15)
            trace["eof-reason"] = end.get("reason")
        except Failure:
            trace["eof-reason"] = "no-eof-event"
        with mpv.cond:
            trace["event-order"] = canon_order(
                [e.get("event") for e in mpv.events])
    return trace


def trace_playlist(names, entries):
    trace = {"playlist-probe": True}
    with Mpv(ARGS) as mpv:
        mpv.load(fixture_path(entries[0]))
        mpv.wait_event("file-loaded", timeout=15)
        first = mpv.try_get_property("filename")
        mpv.command("loadfile", str(fixture_path(entries[1])), "append")
        mpv.command("playlist-next")
        with mpv.cond:
            mpv.events.clear()
        mpv.wait_event("file-loaded", timeout=15)
        trace["first"] = first
        trace["second"] = mpv.try_get_property("filename")
        trace["playlist-count"] = mpv.try_get_property("playlist-count")
        trace["playlist-pos"] = mpv.try_get_property("playlist-pos")
    trace["names"] = list(names)
    return trace


def collect():
    manifest = load_manifest()
    playable = {n: e for n, e in manifest["fixtures"].items()
                if e.get("role") in ("corpus", "audio", "cover")}
    subtitles = {n: e for n, e in manifest["fixtures"].items()
                 if e.get("role") == "subtitle"}
    failures = {n: e for n, e in manifest["fixtures"].items()
                if e.get("role") == "failure"}
    by_sub = {}
    for cname, centry in manifest["fixtures"].items():
        for sub in centry.get("sub_files", []):
            by_sub[Path(sub).name] = (cname, centry)
    traces = {}
    for name, entry in sorted(playable.items()):
        traces[name] = trace_playable(name, entry)
    for name, entry in sorted(subtitles.items()):
        found = by_sub.get(Path(entry.get("path", "")).name)
        if found is None:
            raise Failure(f"subtitle {name} has no companion corpus entry")
        traces[name] = trace_subtitle(name, entry, found[0], found[1])
    for name, entry in sorted(failures.items()):
        traces[name] = trace_failure(name, entry)
    audios = [n for n, e in sorted(playable.items())
              if e.get("role") == "audio"][:2]
    if len(audios) == 2:
        traces["_playlist"] = trace_playlist(
            audios, [manifest["fixtures"][n] for n in audios])
    return traces


def compare(traces):
    problems = []
    for name, golden in traces.items():
        path = TRACES / (name + ".json")
        if not path.exists():
            problems.append(f"{name}: missing golden")
            continue
        live = json.loads(path.read_text())
        if name == "_playlist":
            for key in ("first", "second", "playlist-count", "playlist-pos"):
                if live.get(key) != golden.get(key):
                    problems.append(f"{name}.{key}: {live.get(key)!r} != "
                                    f"{golden.get(key)!r}")
            continue
        if golden.get("role") == "subtitle":
            for key in ("companion", "sub-tracks", "sid-no",
                        "sub-visibility"):
                if live.get(key) != golden.get(key):
                    problems.append(f"{name}.{key}: {live.get(key)!r} != "
                                    f"{golden.get(key)!r}")
            continue
        for key in ("duration", "tracks", "chapters", "metadata",
                    "playlist-count", "pause-default", "volume-default",
                    "speed-default", "audio-params", "video-params",
                    "error-bogus-property", "error-bad-seek",
                    "pause-cycle-events", "eof-reason", "event-order"):
            if live.get(key) != golden.get(key):
                problems.append(f"{name}.{key}: {live.get(key)!r} != "
                                f"{golden.get(key)!r}")
        if "seek-target" in golden:
            if "seek-target" not in live:
                problems.append(f"{name}: seek did not run live")
            else:
                lo = abs((live.get("seek-pos") or 0) - golden["seek-target"])
                if lo > 1.0:
                    problems.append(
                        f"{name}.seek-pos: {live.get('seek-pos')!r} not "
                        f"within 1s of {golden['seek-target']!r}")
    return problems


def selftest():
    assert canon_order(["seek", "playback-restart"]) == [
        "seek", "playback-restart"]
    assert canon_order(["playback-restart", "seek"]) == [
        "seek", "playback-restart"]
    assert canon_order(["seek", "playback-restart", "playback-restart",
                        "end-file", "idle"]) == [
        "seek", "playback-restart", "end-file"]
    assert canon_order(["start-file", "end-file", "audio-reconfig"]) == [
        "start-file", "end-file"]
    assert canon_order(["idle", "idle", "start-file", "end-file"]) == [
        "start-file", "end-file"]
    assert canon_order(["start-file"]) == ["start-file"]
    assert transitions(False, [True, False]) == [True, False]
    assert transitions(False, [False, True, False]) == [True, False]
    assert transitions(False, [False, False]) == []
    print("selftest ok")
    return 0


def main(argv):
    if "--selftest" in argv:
        return selftest()
    regen = "--regen" in argv
    TRACES.mkdir(exist_ok=True)
    traces = collect()
    if regen:
        for name, trace in traces.items():
            (TRACES / (name + ".json")).write_text(
                json.dumps(trace, indent=1, sort_keys=True) + "\n")
        print(f"wrote {len(traces)} goldens to {TRACES}")
        return 0
    problems = compare(traces)
    for problem in problems:
        print(f"TRACE-DIFF {problem}")
    print(f"{len(traces) - len({p.split(':')[0] for p in problems})} "
          f"of {len(traces)} traces match")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
