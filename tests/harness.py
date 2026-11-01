#!/usr/bin/env python3
import hashlib
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import time
import traceback
import uuid
from pathlib import Path

from local_only import URL_MATRIX

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / ".work" / "fixtures"
TMP = ROOT / ".work" / "tmp"
MANIFEST_PATH = ROOT / "tests" / "fixtures" / "manifest.json"
MPV = ROOT / "build" / "mpv"


class Failure(Exception):
    pass


def tail(values, count=12):
    return "\n".join(values[-count:])


class Results:
    def __init__(self):
        self.cases = []

    def check(self, name, ok, detail=""):
        ok = bool(ok)
        self.cases.append((name, ok, detail))
        status = "PASS" if ok else "FAIL"
        line = f"{status} {name}"
        if detail and not ok:
            line += f": {detail}"
        print(line, flush=True)

    def finish(self):
        passed = sum(1 for _, ok, _ in self.cases if ok)
        failed = len(self.cases) - passed
        print(f"TOTAL {passed} passed, {failed} failed", flush=True)
        return 0 if failed == 0 else 1


class Mpv:
    def __init__(self, args=(), cwd=None, env=None):
        if not MPV.exists():
            raise Failure(f"missing player: {MPV}; run scripts/build.sh")
        TMP.mkdir(parents=True, exist_ok=True)
        self.socket_path = TMP / f"mpv-{os.getpid()}-{uuid.uuid4().hex}.sock"
        self.socket_path.unlink(missing_ok=True)
        command = [
            str(MPV), "--no-config", "--idle=yes",
            f"--input-ipc-server={self.socket_path}",
            "--terminal=yes", "--msg-level=all=info",
            *[str(arg) for arg in args],
        ]
        process_env = os.environ.copy()
        if env:
            process_env.update(env)
        self.proc = subprocess.Popen(
            command, cwd=str(cwd or ROOT), env=process_env,
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, text=True, bufsize=1,
        )
        self.lines = []
        self.events = []
        self.responses = {}
        self.cond = threading.Condition()
        self.write_lock = threading.Lock()
        self.next_request_id = 1
        self.sock = None
        self.sock_file = None
        self._closed = False
        self._stdout_thread = threading.Thread(target=self._read_stdout, daemon=True)
        self._stdout_thread.start()
        try:
            self._connect()
        except Exception:
            self.close()
            raise
        self._socket_thread = threading.Thread(target=self._read_socket, daemon=True)
        self._socket_thread.start()

    def _read_stdout(self):
        try:
            for line in self.proc.stdout:
                with self.cond:
                    self.lines.append(line.rstrip("\n"))
                    self.cond.notify_all()
        except Exception:
            pass

    def _connect(self):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise Failure(
                    f"mpv exited {self.proc.returncode} before IPC; log:\n{tail(self.lines)}")
            if self.socket_path.exists():
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                try:
                    sock.connect(str(self.socket_path))
                except OSError:
                    sock.close()
                else:
                    self.sock = sock
                    self.sock_file = sock.makefile("r", encoding="utf-8", newline="\n")
                    return
            time.sleep(0.02)
        raise Failure(f"IPC socket did not appear; log:\n{tail(self.lines)}")

    def _read_socket(self):
        try:
            for line in self.sock_file:
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    continue
                with self.cond:
                    if "request_id" in message:
                        self.responses[message["request_id"]] = message
                    elif "event" in message:
                        self.events.append(message)
                    self.cond.notify_all()
        except Exception:
            pass
        finally:
            with self.cond:
                self.cond.notify_all()

    def _send(self, message):
        payload = json.dumps(message) + "\n"
        with self.write_lock:
            if not self.sock:
                raise Failure("IPC socket is closed")
            try:
                self.sock.sendall(payload.encode("utf-8"))
            except OSError as exc:
                raise Failure(f"IPC write failed: {exc}") from exc

    def command(self, *args, timeout=5):
        with self.write_lock:
            request_id = self.next_request_id
            self.next_request_id += 1
        self._send({"command": list(args), "request_id": request_id})
        with self.cond:
            ready = self.cond.wait_for(
                lambda: request_id in self.responses or self.proc.poll() is not None,
                timeout,
            )
            if not ready:
                raise Failure(f"timeout waiting for command {args}; log:\n{tail(self.lines)}")
            response = self.responses.pop(request_id, None)
        if response is None:
            raise Failure(f"mpv exited during command {args}; log:\n{tail(self.lines)}")
        if response.get("error") != "success":
            raise Failure(f"command {args} failed: {response.get('error')}")
        return response.get("data")

    def get_property(self, name):
        return self.command("get_property", name)

    def try_get_property(self, name, default=None):
        try:
            return self.get_property(name)
        except Failure:
            return default

    def set_property(self, name, value):
        return self.command("set_property", name, value)

    def load(self, path, mode="replace"):
        return self.command("loadfile", str(path), mode)

    def wait_until(self, predicate, timeout=10, description="condition"):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            time.sleep(0.02)
        raise Failure(f"timeout waiting for {description}; log:\n{tail(self.lines)}")

    def wait_property(self, name, predicate, timeout=10):
        def poll():
            value = self.try_get_property(name)
            if value is not None and predicate(value):
                return {"value": value}
            return None
        return self.wait_until(poll, timeout, f"property {name}")["value"]

    def wait_event(self, name, predicate=None, timeout=10):
        def matches():
            return next(
                (event for event in self.events
                 if event.get("event") == name
                 and (predicate is None or predicate(event))),
                None,
            )
        return self.wait_until(matches, timeout, f"event {name}")

    def event_count(self, name, predicate=None):
        with self.cond:
            return sum(
                1 for event in self.events
                if event.get("event") == name
                and (predicate is None or predicate(event))
            )

    def wait_log(self, pattern, timeout=10):
        regex = re.compile(pattern)

        def matches():
            return next((line for line in self.lines if regex.search(line)), None)

        return self.wait_until(matches, timeout, f"log /{pattern}/")

    def log_matches(self, pattern):
        regex = re.compile(pattern)
        return [line for line in self.lines if regex.search(line)]

    def error_lines(self):
        regex = re.compile(
            r"(?i)(\berror\b|\bfailed\b|\bfailure\b|\bcould not open\b|"
            r"\bnot found\b|\bcorrupt\b|\bunderrun\b)")
        return [line for line in self.lines
                if regex.search(line) and "No key binding found" not in line]

    def wait_eof(self, timeout=30):
        event = self.wait_event("end-file", timeout=timeout)
        if event.get("reason") != "eof":
            raise Failure(f"end-file reason {event.get('reason')!r}; log:\n{tail(self.lines)}")
        return event

    def close(self):
        if self._closed:
            return
        self._closed = True
        if self.proc.poll() is None:
            try:
                self._send({"command": ["quit"]})
            except Failure:
                pass
            try:
                self.proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.proc.terminate()
                try:
                    self.proc.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    self.proc.kill()
                    self.proc.wait()
        if self.sock_file:
            try:
                self.sock_file.close()
            except OSError:
                pass
        if self.sock:
            try:
                self.sock.close()
            except OSError:
                pass
        self.socket_path.unlink(missing_ok=True)
        with self.cond:
            self.cond.notify_all()

    @property
    def returncode(self):
        return self.proc.returncode

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.close()
        return False


def load_manifest():
    if not MANIFEST_PATH.exists():
        raise Failure("fixture manifest missing; run tests/run.sh fixtures")
    return json.loads(MANIFEST_PATH.read_text())


def fixture_entry(name):
    manifest = load_manifest()
    try:
        return manifest["fixtures"][name]
    except KeyError:
        raise Failure(f"unknown fixture: {name}")


def fixture_path(entry):
    return FIXTURES / entry["path"]


def subtitle_args(entry):
    return [f"--sub-file={FIXTURES / name}" for name in entry.get("sub_files", [])]


def wait_symphonia(mpv, codec=None, timeout=10):
    pattern = r"Using Symphonia for " + (re.escape(codec) if codec else "")
    return mpv.wait_log(pattern, timeout=timeout)


def track_codecs(tracks, kind):
    return [track.get("codec") for track in tracks if track.get("type") == kind]


def normalize_codec(codec):
    return {"webvtt-webm": "webvtt"}.get(codec, codec)


def track_channels(tracks):
    return [track.get("demux-channel-count") for track in tracks
            if track.get("type") == "audio"]


def check_no_errors(results, name, mpv):
    errors = mpv.error_lines()
    results.check(f"{name}: no error lines", not errors,
                  f"first: {errors[0]}" if errors else "")


def gate_fixtures(results, args):
    generator = ROOT / "tests" / "fixtures" / "gen.sh"
    try:
        proc = subprocess.run(
            [str(generator)], cwd=str(ROOT), capture_output=True,
            text=True, timeout=180,
        )
    except subprocess.TimeoutExpired:
        results.check("fixtures: generation", False, "timed out")
        return
    results.check("fixtures: generation", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines()))

    version = subprocess.run(
        ["ffmpeg", "-version"], capture_output=True, text=True, timeout=30,
    ).stdout.splitlines()
    first = version[0] if version else ""
    results.check("fixtures: generator is ffmpeg 9.0.2", first.startswith("ffmpeg version 9.0.2"),
                  first)

    try:
        manifest = load_manifest()
    except Failure as exc:
        results.check("fixtures: manifest", False, str(exc))
        return
    results.check("fixtures: manifest generator", manifest.get("generator") == "ffmpeg 9.0.2",
                  str(manifest.get("generator")))
    entries = manifest.get("fixtures", {})
    missing = []
    hash_errors = []
    seen = set()
    for name, entry in entries.items():
        path = fixture_path(entry)
        if not path.exists():
            missing.append(name)
            continue
        seen.add(path)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != entry.get("sha256"):
            hash_errors.append(name)
    results.check("fixtures: all manifest files exist", not missing,
                  f"missing: {', '.join(missing[:5])}")
    results.check(f"fixtures: {len(seen)} file hashes", not hash_errors,
                  f"mismatch: {', '.join(hash_errors[:5])}")


def gate_play(results, args):
    name = args[0] if args else "h264"
    entry = fixture_entry(name)
    path = fixture_path(entry)
    with Mpv(["--vo=null", "--ao=null", "--hwdec=no"]) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=10)
        results.check(f"play {name}: file loaded", True)
        if entry.get("video"):
            mpv.wait_log(r"VO: \[null\]", timeout=15)
            mpv.wait_property("vo-configured", lambda value: value is True, timeout=10)
            results.check(f"play {name}: null output", True)
        mpv.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10,
        )
        results.check(f"play {name}: clock advanced", True)
        if entry.get("audio"):
            wait_symphonia(mpv)
            results.check(f"play {name}: Symphonia audio", True)

        mpv.set_property("pause", True)
        time.sleep(0.2)
        mpv.set_property("fullscreen", True)
        mpv.wait_property("fullscreen", lambda value: value is True, timeout=10)
        time.sleep(0.2)
        mpv.set_property("fullscreen", False)
        mpv.wait_property("fullscreen", lambda value: value is False, timeout=10)
        results.check(f"play {name}: fullscreen toggles", True)

        mpv.set_property("pause", False)
        mpv.wait_eof(timeout=entry.get("duration", 3) + 15)
        results.check(f"play {name}: clean eof", True)
        check_no_errors(results, f"play {name}", mpv)
    results.check(f"play {name}: process exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def gate_cli(results, args):
    def run(argv, timeout=20):
        return subprocess.run(
            [str(MPV), "--no-config", *argv], cwd=str(ROOT),
            capture_output=True, text=True, timeout=timeout,
        )

    proc = run(["--version"])
    version_text = proc.stdout + proc.stderr
    results.check("cli: version", proc.returncode == 0 and "mpv" in version_text.lower(),
                  f"exit {proc.returncode}: {version_text.strip()}")

    proc = run(["--help"])
    help_text = proc.stdout + proc.stderr
    results.check("cli: help", proc.returncode == 0, f"exit {proc.returncode}")
    for option in ("--fs", "--start", "--sub-file"):
        results.check(f"cli: help {option}", option in help_text, "option missing from help")

    for option in ("--http-header-fields=X", "--ytdl=yes",
                   "--lavfi-complex=null", "--o=out.mp4"):
        proc = run([option])
        output = (proc.stdout + proc.stderr).lower()
        clean = proc.returncode != 0 and any(
            text in output for text in ("not found", "unknown option", "unrecognized"))
        results.check(f"cli: removed {option.split('=')[0]}", clean,
                      f"exit {proc.returncode}: {output.strip()}")

    entry = fixture_entry("h264")
    path = fixture_path(entry)

    with Mpv(["--vo=null", "--ao=null", "--fullscreen"]) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property("fullscreen", lambda value: value is True, timeout=10)
        results.check("cli: --fullscreen", True)
    results.check("cli: --fullscreen exit", mpv.returncode == 0,
                  f"exit {mpv.returncode}")

    with Mpv(["--vo=null", "--ao=null", "--start=0.5"]) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value >= 0.5,
            timeout=10,
        )
        results.check("cli: --start", True)
    results.check("cli: --start exit", mpv.returncode == 0, f"exit {mpv.returncode}")

    audio_path = fixture_path(fixture_entry("audio-mono"))
    with Mpv(["--vo=null", "--ao=null", f"--audio-file={audio_path}"]) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property(
            "track-list",
            lambda tracks: any(track.get("type") == "audio" for track in tracks),
            timeout=10,
        )
        results.check("cli: --audio-file", True)
    results.check("cli: --audio-file exit", mpv.returncode == 0,
                  f"exit {mpv.returncode}")

    sub_entry = fixture_entry("h264-external-srt")
    with Mpv(["--vo=null", "--ao=null", *subtitle_args(sub_entry)]) as mpv:
        mpv.load(fixture_path(sub_entry))
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property(
            "track-list",
            lambda tracks: any(track.get("type") == "sub" for track in tracks),
            timeout=10,
        )
        results.check("cli: --sub-file", True)
    results.check("cli: --sub-file exit", mpv.returncode == 0, f"exit {mpv.returncode}")

    second = fixture_path(fixture_entry("unicode"))
    with Mpv(["--vo=null", "--ao=null", str(path), str(second)]) as mpv:
        mpv.wait_property(
            "playlist-count", lambda value: value == 2, timeout=10)
        mpv.wait_event("file-loaded", timeout=10)
        results.check("cli: FILE...", True)
    results.check("cli: FILE... exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def corpus_case(results, name, entry):
    path = fixture_path(entry)
    with Mpv(["--vo=null", "--ao=null", "--hwdec=no", *subtitle_args(entry)]) as mpv:
        mpv.load(path)
        mpv.wait_event("file-loaded", timeout=10)
        tracks = mpv.get_property("track-list")
        got_video = track_codecs(tracks, "video")
        got_audio = track_codecs(tracks, "audio")
        got_subtitle = [normalize_codec(codec)
                        for codec in track_codecs(tracks, "sub")]
        problems = []
        if got_video != entry.get("video", []):
            problems.append(f"video {got_video} != {entry.get('video', [])}")
        if got_audio != entry.get("audio", []):
            problems.append(f"audio {got_audio} != {entry.get('audio', [])}")
        if got_subtitle != entry.get("subtitle", []):
            problems.append(f"subtitle {got_subtitle} != {entry.get('subtitle', [])}")
        if track_channels(tracks) != entry.get("channels", []):
            problems.append(
                f"channels {track_channels(tracks)} != {entry.get('channels', [])}")
        results.check(f"corpus {name}: tracks", not problems, "; ".join(problems))

        if entry.get("video"):
            mpv.wait_log(r"VO: \[null\]", timeout=15)
        if entry.get("audio"):
            mpv.wait_log(r"AO: \[null\]", timeout=10)
            wait_symphonia(mpv)
            results.check(f"corpus {name}: Symphonia audio", True)
        mpv.wait_eof(timeout=entry.get("duration", 3) + 15)
        results.check(f"corpus {name}: playback", True)
        check_no_errors(results, f"corpus {name}", mpv)
    results.check(f"corpus {name}: exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def gate_corpus(results, args):
    manifest = load_manifest()
    for name, entry in manifest["fixtures"].items():
        if entry.get("role") != "corpus":
            continue
        try:
            corpus_case(results, name, entry)
        except Failure as exc:
            results.check(f"corpus {name}", False, str(exc))


def gate_audio(results, args):
    manifest = load_manifest()
    for name, entry in manifest["fixtures"].items():
        if entry.get("role") != "audio":
            continue
        try:
            with Mpv(["--ao=coreaudio", "--vo=null", "--no-video",
                      "--volume=0"]) as mpv:
                mpv.load(fixture_path(entry))
                mpv.wait_event("file-loaded", timeout=10)
                mpv.wait_log(r"AO: \[coreaudio\]", timeout=10)
                wait_symphonia(mpv, codec="pcm_s16le")
                params = mpv.get_property("audio-params")
                channels = params.get("channel-count") if isinstance(params, dict) else None
                expected = entry.get("channels", [None])[0]
                results.check(f"audio {name}: coreaudio init", True)
                results.check(f"audio {name}: {expected} channels", channels == expected,
                              f"got {channels}")
                mpv.wait_eof(timeout=entry.get("duration", 3) + 10)
                results.check(f"audio {name}: played to eof", True)
                check_no_errors(results, f"audio {name}", mpv)
            results.check(f"audio {name}: exit", mpv.returncode == 0,
                          f"exit {mpv.returncode}")
        except Failure as exc:
            results.check(f"audio {name}", False, str(exc))


def gate_hwdec(results, args):
    for name in ("h264", "hevc"):
        entry = fixture_entry(name)
        path = fixture_path(entry)
        try:
            proc = subprocess.run(
                ["ffmpeg", "-hide_banner", "-loglevel", "error",
                 "-hwaccel", "videotoolbox", "-i", str(path), "-f", "null", "-"],
                capture_output=True, text=True, timeout=60,
            )
            results.check(f"hwdec {name}: videotoolbox decode", proc.returncode == 0,
                          tail((proc.stdout + proc.stderr).splitlines()))
        except subprocess.TimeoutExpired:
            results.check(f"hwdec {name}: videotoolbox decode", False, "timed out")
        try:
            with Mpv(["--vo=null", "--ao=null", "--hwdec=videotoolbox"]) as mpv:
                mpv.load(path)
                mpv.wait_event("file-loaded", timeout=10)
                mpv.wait_property(
                    "time-pos",
                    lambda value: isinstance(value, (int, float)) and value > 0,
                    timeout=10,
                )
                results.check(f"hwdec {name}: headless playback", True)
            results.check(f"hwdec {name}: exit", mpv.returncode == 0,
                          f"exit {mpv.returncode}")
        except Failure as exc:
            results.check(f"hwdec {name}: headless playback", False, str(exc))

    entry = fixture_entry("mpeg4")
    try:
        with Mpv(["--hwdec=videotoolbox", "--hwdec-software-fallback=yes",
                  "--vo=null", "--ao=null"]) as mpv:
            mpv.load(fixture_path(entry))
            mpv.wait_event("file-loaded", timeout=10)
            mpv.wait_property(
                "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
                timeout=10,
            )
            results.check("hwdec fallback: playback advances", True)
        results.check("hwdec fallback: exit", mpv.returncode == 0,
                      f"exit {mpv.returncode}")
    except Failure as exc:
        results.check("hwdec fallback", False, str(exc))


def gate_seek(results, args):
    entry = fixture_entry("h264")
    with Mpv(["--vo=null", "--ao=null", "--pause=yes"]) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=10)
        duration = mpv.wait_property(
            "duration", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10,
        )
        targets = [
            ("start", 0.0),
            ("mid", duration / 2),
            ("end", max(0.0, duration - 0.3)),
        ]
        for label, target in targets:
            mpv.command("seek", target, "absolute+exact")
            mpv.wait_property(
                "time-pos",
                lambda value, target=target: (
                    isinstance(value, (int, float)) and abs(value - target) <= 0.25),
                timeout=5,
            )
            results.check(f"seek {label}", True)

        for index in range(10):
            target = (index * 0.27) % max(duration - 0.4, 0.1)
            mpv.command("seek", target, "absolute+exact")
            mpv.wait_property(
                "time-pos",
                lambda value, target=target: (
                    isinstance(value, (int, float)) and abs(value - target) <= 0.3),
                timeout=5,
            )
        results.check("seek repeated 10", True)
    results.check("seek exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def gate_local_paths(results, args):
    cases = [
        ("absolute", FIXTURES / "h264.mp4", ROOT),
        ("relative", Path(".work/fixtures/h264.mp4"), ROOT),
        ("spaces", FIXTURES / "with space.mp4", ROOT),
        ("unicode", FIXTURES / "naïve 日本語.mp4", ROOT),
        ("symlink", FIXTURES / "link.mp4", ROOT),
    ]
    mounted = Path("/System/Volumes/Data") / str(FIXTURES).lstrip("/") / "h264.mp4"
    if mounted.exists():
        cases.append(("mounted", mounted, ROOT))
    for label, path, cwd in cases:
        try:
            with Mpv(["--vo=null", "--ao=null"], cwd=cwd) as mpv:
                mpv.load(path)
                mpv.wait_event("file-loaded", timeout=10)
                mpv.wait_property(
                    "time-pos",
                    lambda value: isinstance(value, (int, float)) and value > 0.1,
                    timeout=10,
                )
                results.check(f"local-paths {label}", True)
            results.check(f"local-paths {label}: exit", mpv.returncode == 0,
                          f"exit {mpv.returncode}")
        except Failure as exc:
            results.check(f"local-paths {label}", False, str(exc))


def gate_local_only(results, args):
    def reject(label, value):
        proc = subprocess.run(
            [str(MPV), "--no-config", "--vo=null", "--ao=null", value],
            cwd=str(ROOT), capture_output=True, text=True, timeout=20,
        )
        output = proc.stdout + proc.stderr
        results.check(f"local-only {label}: rejected", proc.returncode != 0,
                      f"exit {proc.returncode}: {output.strip()}")
        results.check(f"local-only {label}: message",
                      "Refusing non-local input" in output,
                      output.strip())

    for label, url in URL_MATRIX:
        reject(label, url)

    h264 = fixture_path(fixture_entry("h264"))
    reject("file-uri CLI", "file://" + str(h264))

    cases = [
        ("absolute", h264, ROOT),
        ("relative", Path(".work/fixtures/h264.mp4"), ROOT),
        ("spaces", FIXTURES / "with space.mp4", ROOT),
        ("unicode", FIXTURES / "naïve 日本語.mp4", ROOT),
        ("symlink", FIXTURES / "link.mp4", ROOT),
    ]
    colon = TMP / "with:colon.mp4"
    TMP.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(h264, colon)
    cases.append(("colon", colon, ROOT))
    mounted = Path("/System/Volumes/Data") / str(FIXTURES).lstrip("/") / "h264.mp4"
    if mounted.exists():
        cases.append(("mounted", mounted, ROOT))

    for label, path, cwd in cases:
        with Mpv(["--vo=null", "--ao=null"], cwd=cwd) as mpv:
            mpv.load(path)
            mpv.wait_event("file-loaded", timeout=10)
            mpv.wait_property(
                "time-pos",
                lambda value: isinstance(value, (int, float)) and value > 0.1,
                timeout=10,
            )
            results.check(f"local-only {label}: plays", True)
        results.check(f"local-only {label}: exit", mpv.returncode == 0,
                      f"exit {mpv.returncode}")

    second = fixture_path(fixture_entry("unicode"))
    mixed = TMP / "mixed.m3u"
    mixed.write_text(
        "#EXTM3U\n"
        f"{h264}\n"
        "http://example.com/remote.mp4\n"
        f"{second}\n"
    )
    with Mpv(["--vo=null", "--ao=null"]) as mpv:
        mpv.load(mixed)
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_log(r"Refusing non-local playlist entry", timeout=10)
        mpv.wait_log(r"Playing:.*naïve 日本語\.mp4", timeout=20)
        results.check("local-only playlist: URL entry skipped", True)
    results.check("local-only playlist: exit", mpv.returncode == 0,
                  f"exit {mpv.returncode}")

    with Mpv(["--vo=null", "--ao=null",
              "--sub-file=http://example.com/remote.srt"]) as mpv:
        mpv.load(h264)
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_log(r"Refusing non-local input", timeout=10)
        mpv.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10,
        )
        results.check("local-only remote subtitle: rejected, playback continues", True)
    results.check("local-only remote subtitle: exit", mpv.returncode == 0,
                  f"exit {mpv.returncode}")

    hostile = TMP / "hostile-home"
    hostile_conf = hostile / ".config" / "mpv"
    hostile_conf.mkdir(parents=True, exist_ok=True)
    (hostile_conf / "mpv.conf").write_text(
        "script=/nonexistent/evil.lua\n"
        "load-scripts=yes\n"
        "vo=x11\n"
    )
    (hostile_conf / "input.conf").write_text("x quit\n")
    env = os.environ.copy()
    env["HOME"] = str(hostile)
    env["MPV_HOME"] = str(hostile_conf)
    env["XDG_CONFIG_HOME"] = str(hostile / ".config")
    env["XDG_STATE_HOME"] = str(hostile / ".state")
    env["XDG_CACHE_HOME"] = str(hostile / ".cache")
    proc = subprocess.run(
        [str(MPV), "--vo=null", "--ao=null", "--frames=1", str(h264)],
        cwd=str(ROOT), env=env, capture_output=True, text=True, timeout=30,
    )
    hostile_output = (proc.stdout + proc.stderr).lower()
    results.check("local-only hostile config: no script",
                  proc.returncode == 0 and "evil.lua" not in hostile_output and
                  "failed to load script" not in hostile_output,
                  f"exit {proc.returncode}: {hostile_output.strip()}")

    sandbox = shutil.which("sandbox-exec")
    if sandbox:
        proc = subprocess.run(
            [sandbox, "-p", "(version 1)(allow default)(deny network*)", str(MPV),
             "--no-config", "--vo=null", "--ao=null", "--frames=1", str(h264)],
            cwd=str(ROOT), capture_output=True, text=True, timeout=30,
        )
        results.check("local-only sandbox playback", proc.returncode == 0,
                      f"exit {proc.returncode}: {(proc.stdout + proc.stderr).strip()}")
    else:
        results.check("local-only sandbox playback", False, "sandbox-exec missing")

    proc = subprocess.run(
        [str(MPV), "--no-config", "--list-protocols"],
        cwd=str(ROOT), capture_output=True, text=True, timeout=20,
    )
    protocols = proc.stdout + proc.stderr
    network = [word for word in ("http", "https", "ftp", "rtsp", "rtmp", "smb")
               if word in protocols.lower()]
    results.check("local-only protocol list",
                  proc.returncode == 0 and "file://" in protocols and not network,
                  protocols.strip())


def gate_osc(results, args):
    entry = fixture_entry("h264")
    with Mpv(["--vo=null", "--ao=null"]) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10,
        )
        mpv.set_property("pause", True)
        margins = mpv.get_property("user-data/osc/margins")
        results.check("osc: timer/property path",
                      isinstance(margins, dict) and {"l", "t", "r", "b"} <= set(margins),
                      str(margins))
        before = mpv.get_property("user-data/osc/visibility")
        mpv.command("script-message-to", "osc", "osc-visibility", "cycle")
        time.sleep(0.2)
        after = mpv.get_property("user-data/osc/visibility")
        results.check("osc: script message", after != before and after is not None,
                      f"{before} -> {after}")
        bindings = mpv.get_property("input-bindings")
        osc = [binding for binding in bindings if binding.get("owner") == "osc"]
        results.check("osc: bindings registered", bool(osc), f"count {len(osc)}")
        mouse = [binding for binding in osc if "MOUSE" in binding.get("key", "")]
        keys = [binding for binding in osc if "MOUSE" not in binding.get("key", "")]
        results.check("osc: mouse bindings", bool(mouse), f"count {len(mouse)}")
        results.check("osc: key bindings", bool(keys), f"count {len(keys)}")
        mpv.command("mouse", 80, 80)
        mpv.command("keypress", "MBTN_LEFT")
        time.sleep(0.2)
        results.check("osc: mouse and key dispatch",
                      not mpv.log_matches(r"(?i)\[osc\].*(error|failed)"),
                      "; ".join(mpv.log_matches(r"(?i)\[osc\].*(error|failed)")[:1]))
    results.check("osc: exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def gate_osc_input(results, args):
    from osc_input import gate_osc_input as run_osc_input
    run_osc_input(results, Mpv, fixture_entry, fixture_path)


def gate_input(results, args):
    entry = fixture_entry("h264")
    with Mpv(["--vo=null", "--ao=null"]) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10,
        )
        mpv.set_property("pause", True)
        mpv.wait_property("pause", lambda value: value is True, timeout=5)
        bindings = mpv.get_property("input-bindings")
        keys = {binding.get("key") for binding in bindings}
        required = {"SPACE", "f", "9", "0", "m", "[", "]", ".", "q"}
        missing = sorted(required - keys)
        results.check("input: expected bindings", not missing,
                      f"missing {missing}")

        mpv.command("keypress", "SPACE")
        mpv.wait_property("pause", lambda value: value is False, timeout=5)
        results.check("input: space toggles pause", True)
        mpv.command("keypress", "SPACE")
        mpv.wait_property("pause", lambda value: value is True, timeout=5)

        mpv.command("keypress", "f")
        mpv.wait_property("fullscreen", lambda value: value is True, timeout=5)
        mpv.command("keypress", "f")
        mpv.wait_property("fullscreen", lambda value: value is False, timeout=5)
        results.check("input: f toggles fullscreen", True)

        volume = mpv.get_property("volume")
        mpv.command("keypress", "9")
        mpv.wait_property("volume", lambda value: value != volume, timeout=5)
        results.check("input: volume key", True)
        mpv.command("keypress", "0")
        mpv.wait_property("volume", lambda value: value == volume, timeout=5)

        mpv.command("keypress", "m")
        mpv.wait_property("mute", lambda value: value is True, timeout=5)
        mpv.command("keypress", "m")
        mpv.wait_property("mute", lambda value: value is False, timeout=5)
        results.check("input: mute key", True)

        mpv.command("keypress", "]")
        mpv.wait_property("speed", lambda value: value > 1.0, timeout=5)
        mpv.command("keypress", "[")
        mpv.wait_property("speed", lambda value: value == 1.0, timeout=5)
        results.check("input: speed keys", True)

        before = mpv.get_property("time-pos")
        mpv.command("keypress", ".")
        mpv.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > before,
            timeout=5,
        )
        results.check("input: frame step", True)

        mpv.command("keypress", "q", timeout=1)
        deadline = time.monotonic() + 5
        while mpv.proc.poll() is None and time.monotonic() < deadline:
            time.sleep(0.02)
        results.check("input: q quits", mpv.proc.poll() is not None)
    results.check("input: exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def gate_playlist(results, args):
    first = fixture_entry("h264")
    second = fixture_entry("unicode")
    first_path = fixture_path(first)
    second_path = fixture_path(second)
    with Mpv(["--vo=null", "--ao=null"]) as mpv:
        mpv.load(first_path)
        mpv.wait_event("file-loaded", timeout=10)
        mpv.load(second_path, "append")
        mpv.wait_property("playlist-count", lambda value: value == 2, timeout=10)
        results.check("playlist: two entries", True)
        playlist = mpv.get_property("playlist")
        results.check("playlist: unicode entry",
                      any(Path(item["filename"]).name == second_path.name for item in playlist),
                      str(playlist))
        mpv.command("playlist-next")
        mpv.wait_property("playlist-pos", lambda value: value == 1, timeout=10)
        results.check("playlist: next", True)
        mpv.command("playlist-prev")
        mpv.wait_property("playlist-pos", lambda value: value == 0, timeout=10)
        results.check("playlist: previous", True)
        mpv.command("playlist-remove", 0)
        mpv.wait_property("playlist-count", lambda value: value == 1, timeout=10)
        playlist = mpv.get_property("playlist")
        results.check("playlist: remove", len(playlist) == 1 and
                      Path(playlist[0]["filename"]).name == second_path.name,
                      str(playlist))
    results.check("playlist: exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def gate_state(results, args):
    entry = fixture_entry("multiaudio")
    with Mpv(["--vo=null", "--ao=null"]) as mpv:
        mpv.load(fixture_path(entry))
        observed = {
            1: "pause", 2: "time-pos", 3: "duration", 4: "fullscreen",
            5: "volume", 6: "speed", 7: "mute", 8: "track-list", 9: "eof-reached",
        }
        for request_id, name in observed.items():
            mpv.command("observe_property", request_id, name)
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property("duration", lambda value: isinstance(value, (int, float)) and
                          value > 0, timeout=10)
        results.check("state: duration", True)
        results.check("state: pause false", mpv.get_property("pause") is False)
        results.check("state: volume 100", mpv.get_property("volume") == 100.0)
        results.check("state: speed 1", mpv.get_property("speed") == 1.0)
        results.check("state: mute false", mpv.get_property("mute") is False)
        control = mpv.get_property("control-state")
        results.check("state: rust control state",
                      "pause=0" in control and "volume=100.000" in control and
                      "speed=1.000" in control, control)
        tracks = mpv.get_property("track-list")
        results.check("state: track list", len(tracks) >= 3, f"count {len(tracks)}")

        before = mpv.event_count(
            "property-change", lambda event: event.get("name") == "pause")
        for index in range(100):
            current = mpv.event_count(
                "property-change", lambda event: event.get("name") == "pause")
            mpv.set_property("pause", index % 2 == 0)
            mpv.wait_until(
                lambda current=current: mpv.event_count(
                    "property-change",
                    lambda event: event.get("name") == "pause") >= current + 1,
                timeout=2, description="next pause property event",
            )
        after = mpv.event_count(
            "property-change", lambda event: event.get("name") == "pause")
        results.check("state: 100-event burst", after >= before + 100,
                      f"events {after - before}")

        duration = mpv.get_property("duration")
        mpv.command("seek", duration / 2, "absolute+exact")
        mpv.wait_property(
            "time-pos",
            lambda value: isinstance(value, (int, float)) and
            abs(value - duration / 2) <= 0.25,
            timeout=5,
        )
        results.check("state: seek", True)

        mpv.set_property("pause", True)
        mpv.wait_property("pause", lambda value: value is True, timeout=5)
        mpv.set_property("pause", False)
        mpv.wait_property("pause", lambda value: value is False, timeout=5)
        results.check("state: pause round trip", True)

        audio_ids = [track["id"] for track in tracks if track.get("type") == "audio"]
        if len(audio_ids) >= 2:
            mpv.set_property("aid", audio_ids[1])
            mpv.wait_property("aid", lambda value: value == audio_ids[1], timeout=5)
            mpv.wait_log(r"Using Symphonia for opus", timeout=10)
            results.check("state: audio track switch", True)

        mpv.set_property("pause", False)
        mpv.wait_eof(timeout=entry.get("duration", 3) + 15)
        results.check("state: eof", True)
    results.check("state: exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def gate_build(results, args):
    try:
        proc = subprocess.run(
            [str(ROOT / "scripts" / "build.sh")], cwd=str(ROOT),
            capture_output=True, text=True, timeout=240,
        )
    except subprocess.TimeoutExpired:
        results.check("build: compile", False, "timed out")
        return
    results.check("build: compile", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))
    if not MPV.exists():
        results.check("build: player exists", False, str(MPV))
        return
    version = subprocess.run(
        [str(MPV), "--version"], capture_output=True, text=True, timeout=30,
    ).stdout.splitlines()
    results.check("build: version",
                  bool(version) and ("0.41.0" in version[0] or
                                     "campaign-base" in version[0]),
                  version[0] if version else "no output")
    configured = subprocess.run(
        ["meson", "configure", "build"], cwd=str(ROOT),
        capture_output=True, text=True, timeout=60,
    ).stdout
    basic = {
        "cplayer": "true", "libmpv": "false", "gpl": "true",
        "tests": "false", "build-date": "false",
    }
    missing = []
    for option, value in basic.items():
        if not re.search(rf"^\s*{re.escape(option)}\s+{re.escape(value)}\s",
                         configured, re.MULTILINE):
            missing.append(f"{option}={value}")
    results.check("build: core options", not missing,
                  f"not found: {', '.join(missing)}")

    feature_text = (ROOT / "meson.options").read_text()
    feature_names = re.findall(
        r"option\('([^']+)',\s*type\s*:\s*'feature'", feature_text)
    values = {}
    for line in configured.splitlines():
        fields = line.split()
        if len(fields) >= 2 and fields[0] in feature_names:
            values[fields[0]] = fields[1]
    allowed_enabled = {
        "cocoa", "coreaudio", "vulkan", "videotoolbox-pl", "swift-build",
        "iconv", "zlib", "macos-10-15-4-features", "macos-11-features",
        "macos-11-3-features", "macos-12-features",
    }
    missing_features = [name for name in feature_names if name not in values]
    wrong = [f"{name}={values.get(name)}" for name in feature_names
             if name not in allowed_enabled and values.get(name) != "disabled"]
    not_enabled = [f"{name}={values.get(name)}" for name in allowed_enabled
                   if values.get(name) != "enabled"]
    results.check("build: feature surface",
                  not missing_features and not wrong and not not_enabled,
                  f"missing={missing_features[:3]} wrong={wrong[:5]} "
                  f"not-enabled={not_enabled[:5]}")

    otool = subprocess.run(
        ["otool", "-L", str(MPV)], capture_output=True, text=True, timeout=30,
    ).stdout
    bad_needles = ("curl", "bluray", "dvdnav", "cdio", "pipewire",
                   "libpulse", "libjack", "avfoundation", "libx11",
                   "libwayland", "libsdl", "libssl", "libcrypto", "libmpv",
                   "liblua", "mlua")
    bad_libraries = [line.strip() for line in otool.splitlines()
                     if any(needle in line.lower() for needle in bad_needles)]
    results.check("build: no unsupported libraries", not bad_libraries,
                  "; ".join(bad_libraries[:3]))

    nm = subprocess.run(
        ["nm", str(MPV)], capture_output=True, text=True, timeout=60,
    ).stdout
    interpreters = [line for line in nm.splitlines() if "_luaL_newstate" in line]
    results.check("build: single Lua interpreter", len(interpreters) == 1,
                  f"count {len(interpreters)}")

    targets_proc = subprocess.run(
        ["meson", "introspect", "build", "--targets"], cwd=str(ROOT),
        capture_output=True, text=True, timeout=60,
    )
    try:
        targets = json.loads(targets_proc.stdout) if targets_proc.returncode == 0 else []
    except json.JSONDecodeError:
        targets = []
    shared = [target.get("name") for target in targets
              if target.get("type") == "shared library"]
    results.check("build: no shared library target", not shared,
                  ", ".join(str(name) for name in shared))
    stray = sorted((ROOT / "build").glob("libmpv*.dylib"))
    results.check("build: no stray libmpv dylib", not stray,
                  ", ".join(path.name for path in stray))


def gate_rust_link(results, args):
    proc = subprocess.run(
        [str(ROOT / "scripts" / "build.sh")], cwd=str(ROOT),
        capture_output=True, text=True, timeout=300,
    )
    results.check("rust-link: build", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))
    if not MPV.exists():
        results.check("rust-link: player exists", False, str(MPV))
        return

    nm = subprocess.run(["nm", "-g", str(MPV)], capture_output=True, text=True,
                        timeout=30).stdout
    results.check("rust-link: symbol", "player_ffi_abi_version" in nm, "symbol missing")

    proc = subprocess.run(
        ["cargo", "test", "--manifest-path", str(ROOT / "rust" / "Cargo.toml"),
         "-p", "player-core", "-p", "player-ffi"],
        cwd=str(ROOT), capture_output=True, text=True, timeout=300,
    )
    results.check("rust-link: cargo test", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))

    entry = fixture_entry("h264")
    with Mpv(["--vo=null", "--ao=null", "--hwdec=no"]) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10,
        )
        results.check("rust-link: playback", True)
    results.check("rust-link: exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def gate_ffi(results, args):
    proc = subprocess.run(
        [str(ROOT / "scripts" / "build.sh")], cwd=str(ROOT),
        capture_output=True, text=True, timeout=300,
    )
    results.check("ffi: build", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))
    ffi_lib = ROOT / ".work" / "deps" / "player-audio" / "target" / "release" / "libplayer_ffi.a"
    binary = ROOT / "build" / "ffi_test"
    proc = subprocess.run(
        ["cc", "-std=c11", "-Wall", "-Wextra",
         "-I", str(ROOT / "rust" / "player-ffi" / "include"),
         str(ROOT / "tests" / "ffi_test.c"), str(ffi_lib),
         "-Wl,-undefined,dynamic_lookup", "-o", str(binary)],
        cwd=str(ROOT), capture_output=True, text=True, timeout=120,
    )
    results.check("ffi: compile", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))
    if proc.returncode != 0:
        return
    proc = subprocess.run([str(binary)], cwd=str(ROOT), capture_output=True,
                          text=True, timeout=30)
    results.check("ffi: run", proc.returncode == 0 and "ffi ok" in proc.stdout,
                  f"exit {proc.returncode}: {(proc.stdout + proc.stderr).strip()}")


def gate_baseline_docs(results, args):
    path = ROOT / "docs" / "baseline.md"
    if not path.exists():
        results.check("baseline-docs: file", False, str(path))
        return
    text = path.read_text()
    required = {
        "title": "# Baseline",
        "imported commit": "41f6a645068483470267271e1d09966ca3b9f413",
        "ffmpeg pin": "n9.0.2",
        "upstream pins": "## Upstream pins",
        "dependencies": "## Dependencies",
        "build section": "## Build",
        "binary": "## Binary",
        "features": "## Features",
        "protocols": "## Protocols",
        "performance": "## Performance",
        "videotoolbox": "## VideoToolbox",
        "licenses": "## Licenses",
        "limitations": "## Known limitations",
        "d7": "## D7 CoreAudio deviation",
        "headless": "## Headless test mode",
        "lua version": "Lua **5.2.4**",
        "configure command": "meson setup build",
        "architecture": "arm64",
        "deployment target": "minos 27.0",
        "unstripped size": "4,500,432",
        "stripped size": "4,073,296",
        "closure size": "59,084,272",
        "closure files": "34 files",
        "direct libraries": "Direct non-system libraries",
        "cold label": "Cold `--version`",
        "warm label": "Warm `--version`",
        "first frame label": "First frame, 1080p",
        "1080p playback label": "1080p RSS / CPU",
        "4k playback label": "4K RSS / CPU",
        "seek label": "Seek latency",
        "exit label": "Exit latency",
        "protocols listed": "60 entries",
        "network protocol": "http",
        "license": "GPL-2.0-or-later",
        "coreaudio result": "AO: [coreaudio]",
        "null result": "AO: [null]",
        "headless flag": "--vo=null",
        "manual checklist": "scripts/manual-checklist.sh",
    }
    missing = [name for name, needle in required.items() if needle not in text]
    results.check("baseline-docs: required content", not missing,
                  f"missing: {', '.join(missing)}")

    metrics = ROOT / "docs" / "metrics.md"
    metrics_text = read_text(metrics)
    results.check("baseline-docs: metrics file", metrics.exists(), str(metrics))
    results.check("baseline-docs: baseline row", "| baseline |" in metrics_text,
                  "no baseline row")
    results.check("baseline-docs: manual checklist",
                  (ROOT / "docs" / "manual-checklist.md").exists(),
                  "missing docs/manual-checklist.md")


def gate_meta(results, args):
    required_paths = [
        "PLAN.md", "docs/baseline.md", "docs/metrics.md", "docs/manual-checklist.md",
        "docs/ffmpeg.md", "docs/architecture.md", "docs/licenses.md", "docs/scope.md",
        "scripts/build.sh", "scripts/dev-deps.sh",
        "scripts/build-ffmpeg.sh", "scripts/measure.sh",
        "scripts/ffmpeg-components.txt", "scripts/ffmpeg-manifest.sh",
        "scripts/audit.sh",
        "scripts/manual-checklist.sh", "tests/run.sh", "tests/harness.py",
        "tests/fixtures/gen.sh", "tests/fixtures/manifest.json",
        "rust/Cargo.toml", "rust/Cargo.lock",
        "rust/player-audio/Cargo.toml", "rust/player-audio/include/player_audio.h",
        "vendor/symphonia/LICENSE", "vendor/symphonia/symphonia/Cargo.toml",
    ]
    missing = [name for name in required_paths if not (ROOT / name).exists()]
    results.check("meta: required files", not missing, f"missing: {', '.join(missing)}")
    dirs = ["rust", "scripts", "tests", "docs"]
    missing_dirs = [name for name in dirs if not (ROOT / name).is_dir()]
    results.check("meta: campaign directories", not missing_dirs,
                  f"missing: {', '.join(missing_dirs)}")

    ignore = read_text(ROOT / ".gitignore") if (ROOT / ".gitignore").exists() else ""
    ignore_missing = [entry for entry in (".work/", "build/", "dist/", "target/")
                      if entry not in ignore]
    results.check("meta: .gitignore entries", not ignore_missing,
                  f"missing: {', '.join(ignore_missing)}")

    exclude_path = ROOT / ".git" / "info" / "exclude"
    exclude = read_text(exclude_path) if exclude_path.exists() else ""
    results.check("meta: .git/info/exclude", ".work/" in exclude, exclude.strip())

    tracked_work = subprocess.run(
        ["git", "ls-files", ".work"], cwd=str(ROOT),
        capture_output=True, text=True, timeout=30,
    ).stdout.strip()
    results.check("meta: fixtures untracked", not tracked_work, tracked_work)
    tracked_manifest = subprocess.run(
        ["git", "ls-files", "tests/fixtures/manifest.json"], cwd=str(ROOT),
        capture_output=True, text=True, timeout=30,
    ).stdout.strip()
    results.check("meta: manifest tracked", bool(tracked_manifest), tracked_manifest)

    bad_temp = []
    temp_pattern = re.compile(
        r"(?<![A-Za-z0-9_.])/" + "tmp" + r"(?:/|\b)|/var/" + "folders" +
        r"|\$TMP" + "DIR")
    for directory in (ROOT / "scripts", ROOT / "tests"):
        for path in directory.rglob("*"):
            if path.is_file() and path.suffix in (".sh", ".py"):
                text = read_text(path)
                if temp_pattern.search(text):
                    bad_temp.append(str(path.relative_to(ROOT)))
    results.check("meta: no system temp paths", not bad_temp,
                  f"found: {', '.join(bad_temp)}")

    executable = [name for name in ("scripts/build.sh", "scripts/dev-deps.sh",
                                    "scripts/build-ffmpeg.sh", "scripts/measure.sh",
                                    "scripts/ffmpeg-manifest.sh",
                                    "scripts/manual-checklist.sh",
                                    "tests/run.sh", "tests/fixtures/gen.sh")
                  if (ROOT / name).exists() and not os.access(ROOT / name, os.X_OK)]
    results.check("meta: scripts executable", not executable,
                  f"not executable: {', '.join(executable)}")


def read_text(path):
    try:
        return path.read_text(errors="replace")
    except OSError:
        return ""


def gate_failures(results, args):
    for name in ("truncated", "corrupt"):
        entry = fixture_entry(name)
        try:
            with Mpv(["--vo=null", "--ao=null"]) as mpv:
                mpv.load(fixture_path(entry))
                mpv.wait_event("end-file", timeout=10)
                results.check(f"failures {name}: handled", True)
                results.check(f"failures {name}: clear message",
                              bool(mpv.log_matches(
                                  r"(?i)(failed to recognize|invalid data|could not open|"
                                  r"error opening|failed to open|not found|"
                                  r"packet corrupt|partial file|invalid nal|"
                                  r"error while decoding|error splitting)")),
                              tail(mpv.lines))
            results.check(f"failures {name}: exit", mpv.returncode == 0,
                          f"exit {mpv.returncode}")
        except Failure as exc:
            results.check(f"failures {name}", False, str(exc))

    entry = fixture_entry("unsupported")
    try:
        with Mpv(["--vo=null", "--ao=null"]) as mpv:
            mpv.load(fixture_path(entry))
            event = mpv.wait_event("end-file", timeout=10)
            results.check("failures unsupported: no hang",
                          event.get("reason") in ("eof", "error"), str(event.get("reason")))
        results.check("failures unsupported: exit", mpv.returncode == 0,
                      f"exit {mpv.returncode}")
    except Failure as exc:
        results.check("failures unsupported", False, str(exc))


def gate_lua_host(results, args):
    proc = subprocess.run(
        ["cargo", "test", "--manifest-path", str(ROOT / "rust" / "Cargo.toml"),
         "-p", "player-lua", "lua_host", "--", "--test-threads=1"],
        cwd=str(ROOT), capture_output=True, text=True, timeout=300,
    )
    results.check("lua-host", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))


def gate_lua_api_audit(results, args):
    proc = subprocess.run(
        [str(ROOT / "tests" / "lua-api-audit.sh")], cwd=str(ROOT),
        capture_output=True, text=True, timeout=60,
    )
    results.check("lua-api-audit", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))



def gate_release(results, args):
    app = ROOT / "dist/Player.app"
    binary = app / "Contents/MacOS/player"
    frameworks = app / "Contents/Frameworks"
    results.check("release: app", app.is_dir() and binary.is_file())
    if not binary.is_file():
        return

    bad = []
    targets = [binary] + sorted(frameworks.glob("*.dylib"))
    for target in targets:
        linked = subprocess.check_output(["otool", "-L", str(target)], text=True)
        if "/opt/homebrew" in linked or ".work/deps" in linked:
            bad.append(str(target))
    results.check("release: no Homebrew or build paths", not bad, ", ".join(bad))

    expected = {
        "libass.9.dylib", "libplacebo.387.dylib", "libvulkan.1.dylib",
        "libMoltenVK.dylib", "libshaderc_shared.1.dylib",
        "libdav1d.7.dylib", "libfreetype.6.dylib", "libfribidi.0.dylib",
        "libharfbuzz.0.dylib", "libplayer_release.dylib",
    }
    present = {path.name for path in frameworks.glob("*.dylib")}
    results.check("release: bundled closure", expected <= present,
                  ",".join(sorted(expected - present)))

    icd_path = app / "Contents/Resources/vulkan/icd.d/MoltenVK_icd.json"
    try:
        icd = json.loads(icd_path.read_text())
        library = Path(icd["ICD"]["library_path"])
        library = (icd_path.parent / library).resolve()
        results.check("release: ICD", library.is_file(), str(library))
    except Exception as exc:
        results.check("release: ICD", False, f"{type(exc).__name__}: {exc}")

    linked = subprocess.check_output(["otool", "-l", str(binary)], text=True)
    results.check("release: rpath", "@executable_path/../Frameworks" in linked)

    proc = subprocess.run(
        [str(binary), "--no-config", "--vo=null", "--ao=null", "--frames=1",
         str(fixture_path(fixture_entry("h264")))],
        capture_output=True, text=True, timeout=60,
    )
    results.check("release: playback", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))

    proc = subprocess.run(
        ["codesign", "--verify", "--deep", "--strict", str(app)],
        capture_output=True, text=True, timeout=60,
    )
    results.check("release: signature", proc.returncode == 0,
                  tail((proc.stdout + proc.stderr).splitlines(), 20))


GATES = {
    "fixtures": gate_fixtures,
    "play": gate_play,
    "cli": gate_cli,
    "corpus": gate_corpus,
    "audio": gate_audio,
    "hwdec": gate_hwdec,
    "seek": gate_seek,
    "local-paths": gate_local_paths,
    "local-only": gate_local_only,
    "osc": gate_osc,
    "osc-input": gate_osc_input,
    "input": gate_input,
    "playlist": gate_playlist,
    "state": gate_state,
    "build": gate_build,
    "rust-link": gate_rust_link,
    "ffi": gate_ffi,
    "lua-host": gate_lua_host,
    "lua-api-audit": gate_lua_api_audit,
    "baseline-docs": gate_baseline_docs,
    "meta": gate_meta,
    "release": gate_release,
    "failures": gate_failures,
}

SUITE = [
    "fixtures", "corpus", "audio", "hwdec", "seek", "local-paths",
    "local-only", "osc", "osc-input", "input", "playlist", "cli", "state", "failures",
    "rust-link", "ffi",
    "lua-host",
    "lua-api-audit", "release",
]


def run_gate(name, rest, results):
    handler = GATES.get(name)
    if handler is None:
        results.check(name, False, "unknown gate")
        return
    try:
        handler(results, rest)
    except Failure as exc:
        results.check(name, False, str(exc))
    except Exception as exc:
        results.check(name, False, f"{type(exc).__name__}: {exc}")
        traceback.print_exc()


def main(argv):
    if len(argv) < 2:
        print("usage: tests/run.sh <gate> [args]", file=sys.stderr)
        print("gates: " + ", ".join(sorted(GATES)) + ", suite", file=sys.stderr)
        return 2
    gate = argv[1]
    rest = argv[2:]
    results = Results()
    if gate == "suite":
        for name in SUITE:
            run_gate(name, rest, results)
    else:
        run_gate(gate, rest, results)
    return results.finish()


if __name__ == "__main__":
    sys.exit(main(sys.argv))
