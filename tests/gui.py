import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import MPV, ROOT, Failure, fixture_entry, fixture_path, tail  # noqa: E402

CARGO = shutil.which("cargo") or "cargo"
PASS_LINE = re.compile(r"player-gui gate: PASS frames=(\d+) overlay_ink=(\d+)")


def gate_command(extra_args):
    fixture = str(fixture_path(fixture_entry("h264")))
    if os.environ.get("PLAYER_BIN"):
        return [
            str(MPV),
            "--no-config",
            "--gui",
            "--gate",
            "--ao=null",
            "--geometry=640x480",
            *extra_args,
            fixture,
        ]
    return [
        CARGO,
        "run",
        "--quiet",
        "--manifest-path",
        str(ROOT / "Cargo.toml"),
        "-p",
        "player-gui",
        "--",
        "--gate",
        "--ao=null",
        "--width",
        "640",
        "--height",
        "480",
        *extra_args,
        fixture,
    ]


def run(results, env=None, extra_args=(), name="gui"):
    compositor = None
    if env is None and sys.platform.startswith("linux"):
        from wayland import Compositor

        compositor = Compositor()
        env = compositor.env
    command = gate_command(extra_args)
    process_env = os.environ.copy()
    process_env.pop("PLAYER_OPTIONS", None)
    if env:
        process_env.update(env)
    if sys.platform.startswith("linux"):
        flags = process_env.get("RUSTFLAGS", "")
        process_env["RUSTFLAGS"] = f"{flags} -C target-feature=-crt-static".strip()
    try:
        proc = subprocess.run(
            command,
            cwd=str(ROOT),
            env=process_env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=900,
        )
    except Exception as exc:  # noqa: BLE001 - report the gate's own failure
        results.check(name, False, repr(exc))
        return None
    finally:
        if compositor is not None:
            compositor.close()
    lines = (proc.stdout + proc.stderr).splitlines()
    match = None
    for line in proc.stdout.splitlines():
        found = PASS_LINE.search(line)
        if found:
            match = found
    if proc.returncode != 0 or match is None:
        results.check(
            f"{name}: gate passes",
            False,
            f"exit {proc.returncode}: {tail(lines, 20)}",
        )
        return None
    frames = int(match.group(1))
    overlay_ink = int(match.group(2))
    results.check(f"{name}: gate passes", True)
    results.check(f"{name}: presents video frames", frames >= 24, f"frames {frames}")
    results.check(
        f"{name}: renders the osc overlay",
        overlay_ink >= 100,
        f"overlay ink {overlay_ink}",
    )
    return lines
