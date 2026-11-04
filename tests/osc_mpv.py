import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import time
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import ROOT, TMP, Failure, tail  # noqa: E402

REFERENCE_MPV = Path(os.environ.get("REFERENCE_MPV", "/opt/homebrew/bin/mpv"))
REFERENCE_VERSION = "mpv v0.41.0"
GOLDEN = ROOT / "tests" / "osc-mpv"
WORK = TMP / "osc-mpv"
CARGO = shutil.which("cargo") or "cargo"
BACKGROUND = "0x202830"
TITLE = "Reference Title"

SIZES = [(1280, 720), (960, 540), (1920, 1080), (800, 600), (640, 640), (600, 500), (1366, 768)]

MEDIA = {
    "plain": dict(duration=205, audio=1, subs=0, rate=2),
    "tracks": dict(duration=205, audio=2, subs=1, rate=2),
    "long": dict(duration=7300, audio=1, subs=0, rate=1),
    "chapters": dict(
        duration=205, audio=1, subs=0, rate=2,
        chapters=[(0, "Intro"), (60, "Middle"), (120, "End")],
    ),
}

SCENES = {
    "default": dict(media="plain", position=62),
    "tracks": dict(media="tracks", position=62, volume=40, aid=2, sid=1),
    "muted": dict(media="plain", position=0, mute=True),
    "long": dict(media="long", position=3725, volume=0),
    "hover": dict(media="plain", position=62, mouse=(600, None)),
    "chapters": dict(media="chapters", position=62),
    "chapter-hover": dict(media="chapters", position=62, mouse=(600, None)),
    "playlist": dict(media="plain", position=62, playlist=(2, 3)),
    "milliseconds": dict(media="plain", position=62, clicks=[(180, 705)]),
    "total": dict(media="plain", position=62, clicks=[(930, 705)]),
}

MATRIX = (
    [("default", size) for size in SIZES]
    + [(scene, SIZES[0]) for scene in SCENES if scene != "default"]
)

THRESHOLD = 6


def scene_name(scene, size):
    return f"{scene}-{size[0]}x{size[1]}"


def media_path(name):
    spec = MEDIA[name]
    path = WORK / f"{name}.mp4"
    if path.is_file() and path.stat().st_size > 0:
        return path
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        raise Failure("ffmpeg is required to generate the OSC reference media")
    WORK.mkdir(parents=True, exist_ok=True)
    command = [
        ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-bitexact", "-fflags", "+bitexact",
        "-f", "lavfi", "-i", f"color=c={BACKGROUND}:s=1280x720:r={spec['rate']}:d={spec['duration']}",
    ]
    for _ in range(spec["audio"]):
        command += ["-f", "lavfi", "-t", str(spec["duration"]), "-i", "anullsrc=r=48000:cl=stereo"]
    if spec["subs"]:
        srt = WORK / "sub.srt"
        srt.write_text("1\n00:00:10,000 --> 00:00:20,000\nhello\n")
        command += ["-i", str(srt)]
    metadata_input = []
    if spec.get("chapters"):
        starts = [start for start, _ in spec["chapters"]]
        ends = starts[1:] + [spec["duration"]]
        lines = [";FFMETADATA1"]
        for (start, title), end in zip(spec["chapters"], ends):
            lines += ["[CHAPTER]", "TIMEBASE=1/1000", f"START={start * 1000}", f"END={end * 1000}", f"title={title}"]
        metadata = WORK / f"{name}.ffmetadata"
        metadata.write_text("\n".join(lines) + "\n")
        index = 1 + spec["audio"] + (1 if spec["subs"] else 0)
        command += ["-i", str(metadata)]
        metadata_input = ["-map_metadata", str(index)]
    command += [*metadata_input, "-map", "0:v"]
    for index in range(spec["audio"]):
        command += ["-map", f"{index + 1}:a"]
    if spec["subs"]:
        command += ["-map", f"{spec['audio'] + 1}:s"]
    command += [
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "20", "-pix_fmt", "yuv420p", "-g", "20",
        "-c:a", "aac", "-b:a", "16k", "-c:s", "mov_text", str(path),
    ]
    proc = subprocess.run(command, capture_output=True, text=True, timeout=300)
    if proc.returncode != 0 or not path.is_file():
        raise Failure(f"cannot generate {name} media: {tail((proc.stdout + proc.stderr).splitlines())}")
    return path


class ReferenceMpv:
    def __init__(self, args):
        if not REFERENCE_MPV.exists():
            raise Failure(f"missing reference mpv: {REFERENCE_MPV}")
        WORK.mkdir(parents=True, exist_ok=True)
        self.socket_path = WORK / f"r{os.getpid()}.sock"
        self.socket_path.unlink(missing_ok=True)
        self.command_line = [
            str(REFERENCE_MPV), "--no-config", "--idle=yes",
            f"--input-ipc-server={self.socket_path}", "--msg-level=all=warn", *args,
        ]
        self.log = open(WORK / "reference.log", "w")
        self.proc = subprocess.Popen(
            self.command_line, stdin=subprocess.DEVNULL, stdout=self.log, stderr=subprocess.STDOUT
        )
        self.sock = None
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise Failure(f"reference mpv exited {self.proc.returncode}: {self.read_log()}")
            if self.socket_path.exists():
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                try:
                    sock.connect(str(self.socket_path))
                except OSError:
                    sock.close()
                else:
                    self.sock = sock
                    self.file = sock.makefile("rw", encoding="utf-8", newline="\n")
                    break
            time.sleep(0.05)
        if self.sock is None:
            self.close()
            raise Failure(f"reference mpv IPC did not appear: {self.read_log()}")
        self.request_id = 0

    def read_log(self):
        self.log.flush()
        return tail((WORK / "reference.log").read_text().splitlines())

    def command(self, *args):
        self.request_id += 1
        self.file.write(json.dumps({"command": list(args), "request_id": self.request_id}) + "\n")
        self.file.flush()
        while True:
            line = self.file.readline()
            if not line:
                raise Failure(f"reference mpv closed IPC during {args}: {self.read_log()}")
            message = json.loads(line)
            if message.get("request_id") == self.request_id:
                if message.get("error") != "success":
                    raise Failure(f"reference command {args} failed: {message.get('error')}")
                return message.get("data")

    def wait_property(self, name, predicate, timeout=20):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                value = self.command("get_property", name)
            except Failure:
                value = None
            if value is not None and predicate(value):
                return value
            time.sleep(0.05)
        raise Failure(f"reference property {name} never settled: {self.read_log()}")

    def close(self):
        try:
            if self.sock is not None:
                self.command("quit")
        except Exception:
            pass
        try:
            self.proc.wait(5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
        self.log.close()
        self.socket_path.unlink(missing_ok=True)


def reference_args(scene, size):
    spec = SCENES[scene]
    media = media_path(spec["media"])
    args = [
        "--vo=gpu-next", "--gpu-context=macvk", "--ao=null", "--hwdec=no",
        "--force-window=yes", f"--geometry={size[0]}x{size[1]}", "--focus-on=never",
        "--pause=yes", "--keep-open=yes", "--dither-depth=no", f"--start={spec['position']}",
        f"--force-media-title={TITLE}",
        *([f"--playlist-start={spec['playlist'][0] - 1}"] if "playlist" in spec else []),
        f"--volume={spec.get('volume', 100)}", f"--mute={'yes' if spec.get('mute') else 'no'}",
        "--osd-level=0", "--osc=yes", "--script-opts=osc-visibility=always",
        "--screenshot-format=png", "--screenshot-high-bit-depth=no",
        *([str(media)] * (spec["playlist"][1] if "playlist" in spec else 1)),
    ]
    return args


def capture_reference(scene, size, directory):
    spec = SCENES[scene]
    directory.mkdir(parents=True, exist_ok=True)
    mpv = ReferenceMpv(reference_args(scene, size))
    try:
        version = mpv.command("get_property", "mpv-version")
        if not version.startswith(REFERENCE_VERSION):
            raise Failure(f"reference is {version}, expected {REFERENCE_VERSION}")
        dims = mpv.wait_property(
            "osd-dimensions", lambda value: value.get("w", 0) > 0 and value.get("h", 0) > 0
        )
        if (dims["w"], dims["h"]) != size:
            raise Failure(f"reference window is {dims['w']}x{dims['h']}, expected {size}")
        mpv.wait_property("playback-time", lambda value: abs(value - spec["position"]) < 0.01)
        mpv.wait_property("duration", lambda value: value > 0)
        if "aid" in spec:
            mpv.command("set_property", "aid", spec["aid"])
        if "sid" in spec:
            mpv.command("set_property", "sid", spec["sid"])
        mpv.command("mouse", 1, 1)
        mpv.command("script-message", "osc-visibility", "never")
        time.sleep(0.6)
        mpv.command("screenshot-to-file", str(directory / "base.png"), "window")
        mpv.command("script-message", "osc-visibility", "always")
        time.sleep(0.6)
        mpv.command("screenshot-to-file", str(directory / "idle.png"), "window")
        interacts = bool(spec.get("mouse") or spec.get("clicks"))
        for _ in range(5):
            for x, y in spec.get("clicks", []):
                mpv.command("mouse", x, y)
                time.sleep(0.3)
                mpv.command("keydown", "MBTN_LEFT")
                time.sleep(0.1)
                mpv.command("keyup", "MBTN_LEFT")
                time.sleep(0.3)
            if spec.get("mouse"):
                x, y = spec["mouse"]
                mpv.command("mouse", x, size[1] - 15 if y is None else y)
                time.sleep(0.6)
            mpv.command("screenshot-to-file", str(directory / "ref.png"), "window")
            if not interacts or (directory / "ref.png").read_bytes() != (directory / "idle.png").read_bytes():
                break
        else:
            raise Failure("reference OSC never reacted to the scripted pointer input")
        (directory / "idle.png").unlink()
        command_line = " ".join(mpv.command_line)
    finally:
        mpv.close()
    (directory / "mpv-command.txt").write_text(command_line + "\n")


def golden_paths(scene, size):
    return GOLDEN / f"base-{size[0]}x{size[1]}.png", GOLDEN / f"{scene_name(scene, size)}.png"


def decode_png(path):
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        raise Failure("ffmpeg is required to decode PNG captures")
    probe = subprocess.run(
        [ffmpeg, "-hide_banner", "-loglevel", "error", "-i", str(path), "-f", "rawvideo",
         "-pix_fmt", "rgba", "-"],
        capture_output=True, timeout=60,
    )
    if probe.returncode != 0:
        raise Failure(f"cannot decode {path}: {probe.stderr.decode(errors='replace')}")
    data = path.read_bytes()
    width, height = struct.unpack(">II", data[16:24])
    if len(probe.stdout) != width * height * 4:
        raise Failure(f"{path} decoded to {len(probe.stdout)} bytes for {width}x{height}")
    return width, height, probe.stdout


def write_png(path, width, height, rgba):
    stride = width * 4
    raw = b"".join(b"\x00" + rgba[row * stride:(row + 1) * stride] for row in range(height))

    def chunk(kind, payload):
        body = kind + payload
        return struct.pack(">I", len(payload)) + body + struct.pack(">I", zlib.crc32(body))

    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 6))
        + chunk(b"IEND", b"")
    )


def render_ours(scene, size, base_rgba, directory):
    spec = SCENES[scene]
    media = MEDIA[spec["media"]]
    base_path = directory / "base.rgba"
    base_path.write_bytes(base_rgba)
    output = directory / "ours.rgba"
    output.unlink(missing_ok=True)
    command = [
        CARGO, "run", "--quiet", "--manifest-path", str(ROOT / "rust" / "Cargo.toml"),
        "-p", "player-app", "--example", "osc_scene", "--",
        "--base", str(base_path), "--output", str(output),
        "--width", str(size[0]), "--height", str(size[1]),
        "--title", TITLE, "--position", str(spec["position"]),
        "--duration", str(media["duration"]), "--volume", str(spec.get("volume", 100)),
        "--audio-tracks", str(media["audio"]), "--sub-tracks", str(media["subs"]),
    ]
    if spec.get("mute"):
        command += ["--muted"]
    if "aid" in spec:
        command += ["--aid", str(spec["aid"])]
    elif media["audio"]:
        command += ["--aid", "1"]
    if "sid" in spec:
        command += ["--sid", str(spec["sid"])]
    if spec.get("mouse"):
        x, y = spec["mouse"]
        command += ["--pointer", f"{x},{size[1] - 15 if y is None else y}"]
    for x, y in spec.get("clicks", []):
        command += ["--click", f"{x},{y}"]
    for start, title in media.get("chapters", []):
        command += ["--chapter", f"{start}:{title}"]
    if "playlist" in spec:
        command += ["--playlist", "{}/{}".format(*spec["playlist"])]
    proc = subprocess.run(command, cwd=str(ROOT), capture_output=True, text=True, timeout=900)
    if proc.returncode != 0 or not output.is_file():
        raise Failure("osc_scene failed: " + tail((proc.stdout + proc.stderr).splitlines(), 20))
    return output.read_bytes()


def measure(base, reference, ours, width, height):
    stride = width * 4
    ink_reference = ink_ours = union = 0
    differing = strong = 0
    total = 0
    maximum = 0
    rows = []
    for row in range(height):
        start = row * stride
        end = start + stride
        if reference[start:end] == ours[start:end] and reference[start:end] == base[start:end]:
            continue
        row_changed = False
        for offset in range(start, end, 4):
            base_pixel = base[offset:offset + 3]
            reference_pixel = reference[offset:offset + 3]
            ours_pixel = ours[offset:offset + 3]
            reference_delta = max(abs(a - b) for a, b in zip(reference_pixel, base_pixel))
            ours_delta = max(abs(a - b) for a, b in zip(ours_pixel, base_pixel))
            difference = max(abs(a - b) for a, b in zip(reference_pixel, ours_pixel))
            reference_ink = reference_delta > THRESHOLD
            ours_ink = ours_delta > THRESHOLD
            if reference_ink:
                ink_reference += 1
            if ours_ink:
                ink_ours += 1
            if reference_ink or ours_ink:
                union += 1
            total += difference
            maximum = max(maximum, difference)
            if difference > 2:
                differing += 1
            if difference > 8:
                strong += 1
            if reference_ink or ours_ink or difference:
                row_changed = True
        if row_changed:
            rows.append(row)
    return dict(
        ink_reference=ink_reference,
        ink_ours=ink_ours,
        union=union,
        differing=differing,
        strong=strong,
        maximum=maximum,
        mean=total / (width * height),
        rows=(rows[0], rows[-1]) if rows else (0, 0),
    )


def write_comparison(path, width, total_height, rows, reference, ours):
    top = max(0, rows[0] - 4)
    bottom = min(total_height, rows[1] + 5)
    strip = []
    for source in (reference, ours):
        strip.append(source[top * width * 4:bottom * width * 4])
    diff = bytearray()
    for index in range(0, len(strip[0]), 4):
        pair = [max(abs(strip[0][index + channel] - strip[1][index + channel]) for channel in range(3))]
        value = min(255, pair[0] * 16)
        diff += bytes((value, value, value, 255))
    height = bottom - top
    stacked = strip[0] + strip[1] + bytes(diff)
    write_png(path, width, height * 3, stacked)


def check_scene(results, scene, size, mode):
    name = scene_name(scene, size)
    directory = WORK / name
    directory.mkdir(parents=True, exist_ok=True)
    if mode == "live":
        capture_reference(scene, size, directory)
        golden_base, golden_reference = golden_paths(scene, size)
        if golden_reference.is_file() and golden_base.is_file():
            fresh = decode_png(directory / "ref.png")[2]
            fresh_base = decode_png(directory / "base.png")[2]
            results.check(
                f"osc-mpv {name}: golden capture is current",
                fresh == decode_png(golden_reference)[2] and fresh_base == decode_png(golden_base)[2],
                "real mpv no longer renders the committed reference; rerun with capture",
            )
    if mode == "golden":
        base_path, reference_path = golden_paths(scene, size)
        for path in (base_path, reference_path):
            if not path.is_file():
                raise Failure(f"missing golden capture {path}; run tests/run.sh osc-mpv capture")
    else:
        base_path, reference_path = directory / "base.png", directory / "ref.png"
    width, height, base = decode_png(base_path)
    ref_width, ref_height, reference = decode_png(reference_path)
    if (width, height) != size or (ref_width, ref_height) != size:
        raise Failure(f"{name}: capture is {width}x{height}, expected {size}")
    ours = render_ours(scene, size, base, directory)
    stats = measure(base, reference, ours, width, height)
    write_comparison(directory / "compare.png", width, height, stats["rows"], reference, ours)
    write_png(directory / "ours.png", width, height, ours)
    write_png(directory / "reference.png", width, height, reference)
    return name, stats


def tolerance_check(results, name, stats):
    union = max(stats["union"], 1)
    results.check(
        f"osc-mpv {name}: overlay present",
        stats["ink_reference"] >= 500 and stats["ink_ours"] >= 500,
        f"reference {stats['ink_reference']}, ours {stats['ink_ours']}",
    )
    results.check(
        f"osc-mpv {name}: pixels match",
        stats["maximum"] <= 1,
        f"max {stats['maximum']}, differing {stats['differing']}, strong {stats['strong']}, "
        f"union {union}, mean {stats['mean']:.5f}",
    )


def run(results, args):
    mode = "live" if "live" in args else "cached" if "cached" in args else "golden"
    capture = "capture" in args
    only = [arg for arg in args if arg not in ("live", "capture", "cached")]
    if capture:
        for scene, size in MATRIX:
            if only and scene_name(scene, size) not in only:
                continue
            name = scene_name(scene, size)
            capture_reference(scene, size, WORK / name)
            base_path, reference_path = golden_paths(scene, size)
            GOLDEN.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(WORK / name / "base.png", base_path)
            shutil.copyfile(WORK / name / "ref.png", reference_path)
            print(f"captured {name}", flush=True)
        return
    for scene, size in MATRIX:
        if only and scene_name(scene, size) not in only:
            continue
        try:
            name, stats = check_scene(results, scene, size, mode)
        except Failure as exc:
            results.check(f"osc-mpv {scene_name(scene, size)}", False, str(exc))
            continue
        print(f"{name}: {json.dumps(stats)}", flush=True)
        tolerance_check(results, name, stats)
