import os
import shutil
import struct
import subprocess
import sys
import time
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import ROOT, TMP, Failure, Mpv, tail  # noqa: E402

WIDTH = 320
HEIGHT = 240
CARGO = shutil.which("cargo") or "cargo"

OSC_TEXT = "osc"
PLAIN_TEXT = "probe text"


def solid_fixture():
    directory = TMP / "osd-fidelity"
    path = directory / "solid.mp4"
    if path.is_file() and path.stat().st_size > 0:
        return path
    directory.mkdir(parents=True, exist_ok=True)
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        raise Failure("ffmpeg is required to generate the OSD fidelity fixture")
    proc = subprocess.run(
        [
            ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-bitexact",
            "-f", "lavfi", "-i", f"color=c=black:s={WIDTH}x{HEIGHT}:r=24:d=3",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=3",
            "-map", "0:v", "-map", "1:a",
            "-c:v", "libx264", "-preset", "ultrafast", "-crf", "10",
            "-pix_fmt", "yuv420p",
            "-x264-params", "keyint=24:min-keyint=24:scenecut=0",
            "-c:a", "aac", "-b:a", "64k", "-ac", "2",
            "-shortest", "-movflags", "+faststart", "-fflags", "+bitexact",
            str(path),
        ],
        capture_output=True, text=True, timeout=120,
    )
    if proc.returncode != 0 or not path.is_file():
        raise Failure(f"cannot generate the OSD fixture: {tail((proc.stdout + proc.stderr).splitlines())}")
    return path


def read_png_rgba(path):
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise Failure(f"{path} is not a PNG")
    offset = 8
    header = None
    compressed = b""
    while offset < len(data):
        length, kind = struct.unpack(">I4s", data[offset:offset + 8])
        chunk = data[offset + 8:offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", chunk)
        elif kind == b"IDAT":
            compressed += chunk
    if header is None or header[2:5] != (8, 6, 0):
        raise Failure(f"{path} is not 8-bit RGBA: {header}")
    width, height = header[0], header[1]
    raw = zlib.decompress(compressed)
    stride = width * 4
    pixels = bytearray()
    previous = bytearray(stride)
    for row in range(height):
        start = row * (stride + 1)
        kind = raw[start]
        line = bytearray(raw[start + 1:start + 1 + stride])
        for index in range(stride):
            left = line[index - 4] if index >= 4 else 0
            up = previous[index]
            corner = previous[index - 4] if index >= 4 else 0
            if kind == 1:
                line[index] = (line[index] + left) & 0xff
            elif kind == 2:
                line[index] = (line[index] + up) & 0xff
            elif kind == 3:
                line[index] = (line[index] + (left + up) // 2) & 0xff
            elif kind == 4:
                estimate = left + up - corner
                distances = (abs(estimate - left), abs(estimate - up), abs(estimate - corner))
                predictor = (left, up, corner)[distances.index(min(distances))]
                line[index] = (line[index] + predictor) & 0xff
            elif kind != 0:
                raise Failure(f"{path} uses unknown PNG filter {kind}")
        pixels += line
        previous = line
    return width, height, bytes(pixels)


def gpu_args():
    if sys.platform.startswith("linux"):
        return [
            "--vo=gpu-next",
            "--gpu-context=waylandvk",
            "--gpu-sw=yes",
        ]
    return [
        "--vo=gpu-next",
        "--gpu-context=macvk",
    ]


def capture_reference(results, label, fixture, env, env_args):
    directory = TMP / "osd-fidelity" / label
    directory.mkdir(parents=True, exist_ok=True)
    for path in directory.glob("*"):
        path.unlink()
    capture_env = os.environ.copy()
    if env:
        capture_env.update(env)
    with Mpv(
        [
            *gpu_args(),
            *env_args,
            "--ao=null",
            "--hwdec=no",
            "--screenshot-sw=yes",
            "--pause=yes",
            "--start=1.0",
            f"--geometry={WIDTH}x{HEIGHT}",
            "--osd-level=0",
        ],
        env=capture_env,
    ) as player:
        player.load(fixture)
        player.wait_event("file-loaded", timeout=30)
        dims = player.wait_property(
            "osd-dimensions",
            lambda value: value.get("w", 0) > 0 and value.get("h", 0) > 0,
            timeout=15,
        )
        scale = dims["w"] / WIDTH
        results.check(
            f"osd-fidelity {label}: window size",
            scale in (1, 2) and dims["h"] == HEIGHT * scale,
            str(dims),
        )
        (directory / "window").write_text(f"{dims['w']} {dims['h']}\n")
        if label == OSC_TEXT:
            time.sleep(0.8)
            player.command("set", "osc-visibility", "always")
            player.wait_property("osc-visibility", lambda value: value == "always", timeout=10)
            time.sleep(1.2)
        else:
            player.command("set", "osc-visibility", "never")
            time.sleep(1.2)
            player.command("show-text", PLAIN_TEXT, 5000, 0)
            time.sleep(0.3)
        player.command("screenshot-to-file", str(directory / "base.png"), "video")
        player.command("screenshot-to-file", str(directory / "overlay.png"), "window")
        player.command("quit")
    for name in ("base", "overlay"):
        width, height, pixels = read_png_rgba(directory / f"{name}.png")
        if (width, height) != (WIDTH, HEIGHT):
            raise Failure(f"{name} capture is {width}x{height}, expected {WIDTH}x{HEIGHT}")
        (directory / f"{name}.rgba").write_bytes(pixels)
    return directory


def render_rust(results, label, fixture, directory):
    output = directory / "rust.rgba"
    output.unlink(missing_ok=True)
    command = [
        CARGO, "run", "--quiet",
        "--manifest-path", str(ROOT / "rust" / "Cargo.toml"),
        "-p", "player-app", "--example", "osd_frame", "--",
        "--fixture", str(fixture),
        "--base", str(directory / "base.rgba"),
        "--output", str(output),
        "--width", str(WIDTH),
        "--height", str(HEIGHT),
        "--position", "1.0",
        "--window", (directory / "window").read_text().strip().replace(" ", "x"),
    ]
    if label == OSC_TEXT:
        command += ["--osc", "yes"]
    else:
        command += ["--osc", "no", "--text", PLAIN_TEXT]
    env = os.environ.copy()
    if sys.platform.startswith("linux"):
        flags = env.get("RUSTFLAGS", "")
        env["RUSTFLAGS"] = f"{flags} -C target-feature=-crt-static".strip()
    proc = subprocess.run(
        command, cwd=str(ROOT), env=env, capture_output=True, text=True, timeout=900
    )
    if proc.returncode != 0 or not output.is_file():
        raise Failure(
            "Rust OSD frame failed: "
            + tail((proc.stdout + proc.stderr).splitlines(), 20)
        )
    return output.read_bytes()


def compare(results, label, base, reference, rendered):
    if len(reference) != len(base) or len(rendered) != len(base):
        results.check(f"osd-fidelity {label}: buffer sizes", False,
                      f"{len(base)} {len(reference)} {len(rendered)}")
        return
    ink_reference = 0
    ink_rendered = 0
    union = 0
    differing = 0
    strong = 0
    total = 0
    maximum = 0
    for offset in range(0, len(base), 4):
        base_pixel = base[offset:offset + 3]
        reference_pixel = reference[offset:offset + 3]
        rendered_pixel = rendered[offset:offset + 3]
        reference_delta = max(
            abs(reference_pixel[index] - base_pixel[index]) for index in range(3)
        )
        rendered_delta = max(
            abs(rendered_pixel[index] - base_pixel[index]) for index in range(3)
        )
        difference = max(
            abs(reference_pixel[index] - rendered_pixel[index]) for index in range(3)
        )
        total += difference
        maximum = max(maximum, difference)
        reference_ink = reference_delta > 6
        rendered_ink = rendered_delta > 6
        if reference_ink:
            ink_reference += 1
        if rendered_ink:
            ink_rendered += 1
        if reference_ink or rendered_ink:
            union += 1
            if difference > 2:
                differing += 1
            if difference > 4:
                strong += 1
    pixels = len(base) // 4
    mean = total / pixels
    expected_ink = 500 if label == OSC_TEXT else 80
    results.check(
        f"osd-fidelity {label}: overlay coverage",
        union >= expected_ink,
        f"union {union}, reference {ink_reference}, rendered {ink_rendered}",
    )
    results.check(
        f"osd-fidelity {label}: coverage agrees",
        abs(ink_reference - ink_rendered) <= max(4, union // 50),
        f"reference {ink_reference}, rendered {ink_rendered}, union {union}",
    )
    results.check(
        f"osd-fidelity {label}: pixel tolerance",
        maximum <= 8 and strong <= max(8, union // 100) and differing <= max(12, union // 50),
        f"max {maximum}, strong {strong}, differing {differing}, union {union}",
    )
    results.check(
        f"osd-fidelity {label}: mean difference",
        mean <= 0.5,
        f"{mean:.4f} over {pixels} pixels",
    )


def run(results, env=None, env_args=()):
    fixture = solid_fixture()
    compositor = None
    if env is None and sys.platform.startswith("linux"):
        from wayland import Compositor
        compositor = Compositor()
        env = compositor.env
    try:
        for label in (OSC_TEXT, PLAIN_TEXT):
            try:
                directory = capture_reference(results, label, str(fixture), env, env_args)
                rendered = render_rust(results, label, fixture, directory)
                base = (directory / "base.rgba").read_bytes()
                reference = (directory / "overlay.rgba").read_bytes()
                compare(results, label, base, reference, rendered)
            except Failure as exc:
                results.check(f"osd-fidelity {label}", False, str(exc))
            except Exception as exc:  # noqa: BLE001 - report unexpected fixture failures
                results.check(f"osd-fidelity {label}", False, repr(exc))
    finally:
        if compositor is not None:
            compositor.close()
