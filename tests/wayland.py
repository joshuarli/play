import os
import subprocess
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import harness  # noqa: E402
from harness import (FIXTURES, Failure, Mpv, Results, ROOT, TMP, fixture_entry,  # noqa: E402
                     fixture_path, subtitle_args, tail)

ICD_DIR = Path("/usr/share/vulkan/icd.d")
GPU_ARGS = ["--vo=gpu-next", "--gpu-context=waylandvk", "--gpu-sw=yes", "--ao=null",
            "--hwdec=no", "--vulkan-swap-mode=mailbox", "--keep-open=yes", "--msg-level=all=info,vo=v"]


class Compositor:
    def __init__(self):
        self.runtime_dir = TMP / f"wayland-{os.getpid()}-{uuid.uuid4().hex[:8]}"
        self.runtime_dir.mkdir(parents=True, mode=0o700)
        self.socket = "wl-player"
        self.log_path = self.runtime_dir / "weston.log"
        env = {**os.environ, "XDG_RUNTIME_DIR": str(self.runtime_dir)}
        self.proc = subprocess.Popen(
            ["weston", "--backend=headless", f"--socket={self.socket}", "--no-config",
             "--idle-time=0"],
            env=env, stdout=self.log_path.open("w"), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 15
        while not (self.runtime_dir / self.socket).exists():
            if self.proc.poll() is not None or time.monotonic() > deadline:
                raise Failure(f"weston did not start:\n{self.log_path.read_text()[-800:]}")
            time.sleep(0.05)

    @property
    def env(self):
        icds = sorted(ICD_DIR.glob("lvp_icd*.json"))
        if not icds:
            raise Failure(f"lavapipe ICD not found in {ICD_DIR}")
        return {
            "XDG_RUNTIME_DIR": str(self.runtime_dir),
            "WAYLAND_DISPLAY": self.socket,
            "VK_DRIVER_FILES": str(icds[0]),
        }

    def close(self):
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
        for path in self.runtime_dir.iterdir():
            path.unlink(missing_ok=True)
        self.runtime_dir.rmdir()


def fixture_1080p():
    path = FIXTURES / "measure" / "1080p.mp4"
    if not path.exists():
        raise Failure(f"missing {path}; run scripts/measure-linux.sh baseline")
    return path


def gate_linux_gpu_next(results, args):
    compositor = Compositor()
    try:
        play(results, compositor)
        window(results, compositor)
        subtitles(results, compositor)
        shutdown(results, compositor)
    finally:
        compositor.close()


def play(results, compositor):
    with Mpv(GPU_ARGS, env=compositor.env) as player:
        player.load(fixture_1080p())
        player.wait_event("file-loaded", timeout=30)
        player.wait_property("eof-reached", lambda value: value is True, timeout=60)
        results.check("gpu-next: VO selected",
                      player.get_property("current-vo") == "gpu-next")
        results.check("gpu-next: Vulkan context",
                      bool(player.log_matches(r"libplacebo.*GPU 0:")),
                      tail(player.lines))
        results.check("gpu-next: Wayland output registered",
                      bool(player.log_matches(r"vo/gpu-next/wayland.*Registered output")))
        errors = [line for line in player.error_lines() if "Compositor doesn't support" not in line]
        results.check("gpu-next: no errors", not errors, tail(errors))
        player.command("quit")
        player.proc.wait(timeout=10)
        results.check("gpu-next: clean exit", player.returncode == 0,
                      f"exit {player.returncode}")


def window(results, compositor):
    with Mpv(GPU_ARGS, env=compositor.env) as player:
        player.load(fixture_1080p())
        player.wait_event("file-loaded", timeout=30)
        player.wait_property("osd-width", lambda value: value > 0, timeout=15)
        player.command("show-text", "gpu-next osd", 2000)
        dims = player.get_property("osd-dimensions")
        results.check("gpu-next: OSD surface", dims["w"] > 0 and dims["h"] > 0, str(dims))
        windowed = player.get_property("osd-dimensions")["w"]
        player.set_property("fullscreen", True)
        fullscreen = player.wait_property(
            "osd-dimensions", lambda value: value["w"] != windowed, timeout=10)["w"]
        results.check("gpu-next: fullscreen resizes to the output", fullscreen == 1024,
                      f"{windowed} -> {fullscreen}")
        player.set_property("fullscreen", False)
        restored = player.wait_property(
            "osd-dimensions", lambda value: value["w"] == windowed, timeout=10)["w"]
        results.check("gpu-next: leaving fullscreen restores the window", restored == windowed,
                      f"{fullscreen} -> {restored}")
        before = player.get_property("osd-dimensions")["w"]
        player.set_property("window-scale", 0.5)
        resized = player.wait_property("osd-dimensions", lambda value: value["w"] != before,
                                       timeout=10)
        results.check("gpu-next: resize", resized["w"] != before, f"{before} -> {resized['w']}")


SUBTITLE_FIXTURES = (
    "h264-embedded-srt", "h264-external-srt", "h264-external-ass",
    "h264-external-vtt", "h264-movtext", "vp9-webvtt",
)
MIXED_SCRIPT_SRT = """1
00:00:00,000 --> 00:00:09,000
Latin éèü, 日本語の字幕, مرحبا بالعالم, Ελληνικά
"""


def render_subtitle(results, compositor, label, path, extra=()):
    with Mpv([*GPU_ARGS, "--pause=yes", "--sid=1", *extra], env=compositor.env) as player:
        player.load(path)
        player.wait_event("file-loaded", timeout=30)
        player.command("seek", 1, "absolute+exact")
        text = player.wait_property("sub-text", lambda value: bool(value.strip()), timeout=15)
        failures = player.log_matches(r"Failed to load fontconfig|fontconfig.*(error|failed)")
        results.check(f"subtitles {label}: cue decoded", bool(text), tail(player.lines))
        results.check(f"subtitles {label}: libass fonts", not failures, tail(failures))
        return text


def subtitles(results, compositor):
    for name in SUBTITLE_FIXTURES:
        entry = fixture_entry(name)
        render_subtitle(results, compositor, name, fixture_path(entry), subtitle_args(entry))
    directory = TMP / f"subtitles-{os.getpid()}"
    unicode_dir = directory / "字幕 vidéo"
    unicode_dir.mkdir(parents=True)
    try:
        source = fixture_path(fixture_entry("h264"))
        media = unicode_dir / "映画 clip.mp4"
        media.symlink_to(source)
        mixed = unicode_dir / "混合 subs.srt"
        mixed.write_text(MIXED_SCRIPT_SRT, encoding="utf-8")
        text = render_subtitle(results, compositor, "non-ASCII path and mixed scripts", media,
                               [f"--sub-file={mixed}"])
        results.check("subtitles: mixed-script text preserved", "日本語" in text and "مرحبا" in text,
                      repr(text))
    finally:
        for path in sorted(directory.rglob("*"), reverse=True):
            path.unlink() if not path.is_dir() or path.is_symlink() else path.rmdir()
        directory.rmdir()


def shutdown(results, compositor):
    with Mpv(GPU_ARGS, env=compositor.env) as player:
        player.load(fixture_1080p())
        player.wait_event("file-loaded", timeout=30)
        start = time.monotonic()
        player.command("quit")
        try:
            player.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            results.check("gpu-next: quit while playing", False, "no exit within 5 s")
            return
        results.check("gpu-next: quit while playing",
                      player.returncode == 0 and time.monotonic() - start < 5,
                      f"exit {player.returncode}")


def main():
    results = Results()
    try:
        gate_linux_gpu_next(results, [])
    except Failure as exc:
        results.check("linux-gpu-next", False, str(exc))
    return results.finish()


if __name__ == "__main__":
    sys.exit(main())
