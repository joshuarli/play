import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


def supported_host():
    if sys.platform != "linux" or os.uname().machine != "x86_64" or not shutil.which("ldd"):
        return False
    result = subprocess.run(["ldd", "--version"], capture_output=True, text=True)
    return "musl" in (result.stdout + result.stderr).lower()


@unittest.skipUnless(supported_host(), "requires x86-64 musl Linux")
class FontconfigWritersTest(unittest.TestCase):
    def test_concurrent_builds_do_not_share_the_source_tree(self):
        self.check_concurrent_builds(kill_first=False)

    def test_killed_writer_releases_the_source_tree(self):
        self.check_concurrent_builds(kill_first=True)

    def test_concurrent_same_prefix_reuses_the_first_build(self):
        self.check_concurrent_builds(kill_first=False, same_prefix=True)

    def check_concurrent_builds(self, kill_first, same_prefix=False):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="fontconfig-writers-") as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "docker/linux").mkdir(parents=True)
            shutil.copy2(ROOT / "scripts/build-rust-fontconfig-linux.sh", root / "scripts/build-rust-fontconfig-linux.sh")
            shutil.copy2(ROOT / "docker/linux/rust-fontconfig.Cargo.lock", root / "docker/linux/rust-fontconfig.Cargo.lock")
            source = root / "source"
            (source / "ffi").mkdir(parents=True)
            (source / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.0.0"\n')
            (source / "ffi/rust_fontconfig.h").write_text("fixture header\n")
            tools = root / "tools"
            tools.mkdir()
            cargo = tools / "cargo"
            cargo.write_text('''#!/bin/sh
set -eu
printf ready > "$TEST_READY"
read answer < "$TEST_RELEASE"
mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release"
printf '%s' "$TEST_ID" > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
''')
            cargo.chmod(0o755)
            descriptors = []
            fifos = {}
            for name in ("first-ready", "first-release", "second-ready", "second-release"):
                fifo = root / name
                os.mkfifo(fifo)
                fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
                descriptors.append(fd)
                fifos[name] = (fifo, fd)
            self.addCleanup(lambda: [os.close(fd) for fd in descriptors])
            processes = []
            try:
                for name in ("first", "second"):
                    environment = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                                       RUST_FONTCONFIG_SOURCE=str(source),
                                       RUST_FONTCONFIG_PREFIX=str(root / ("shared" if same_prefix else name)),
                                       TEST_READY=str(fifos[name + "-ready"][0]),
                                       TEST_RELEASE=str(fifos[name + "-release"][0]), TEST_ID=name)
                    process = subprocess.Popen([str(root / "scripts/build-rust-fontconfig-linux.sh")], env=environment,
                                               start_new_session=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                    processes.append(process)
                    if name == "first":
                        self.assertTrue(select.select([fifos["first-ready"][1]], [], [], 20)[0])
                        self.assertEqual(os.read(fifos["first-ready"][1], 5), b"ready")
                self.assertFalse(select.select([fifos["second-ready"][1]], [], [], 2)[0],
                                 "second build entered the shared source tree")
                if kill_first:
                    os.killpg(processes[0].pid, signal.SIGKILL)
                    processes[0].communicate(timeout=10)
                    self.assertFalse((root / "first/.input").exists())
                else:
                    os.write(fifos["first-release"][1], b"continue\n")
                if same_prefix:
                    first_output = processes[0].communicate(timeout=20)
                    second_output = processes[1].communicate(timeout=20)
                    self.assertEqual(processes[0].returncode, 0, first_output)
                    self.assertEqual(processes[1].returncode, 0, second_output)
                    self.assertIn(b"already in", second_output[0])
                    self.assertFalse(select.select([fifos["second-ready"][1]], [], [], 0)[0])
                    self.assertEqual((root / "shared/lib/librust_fontconfig.so.5.0.2").read_text(), "first")
                    return
                self.assertTrue(select.select([fifos["second-ready"][1]], [], [], 20)[0])
                self.assertEqual(os.read(fifos["second-ready"][1], 5), b"ready")
                os.write(fifos["second-release"][1], b"continue\n")
                for process in processes[1:] if kill_first else processes:
                    output = process.communicate(timeout=20)
                    self.assertEqual(process.returncode, 0, output)
                for name in ("second",) if kill_first else ("first", "second"):
                    self.assertEqual((root / name / "lib/librust_fontconfig.so.5.0.2").read_text(), name)
            finally:
                for process in processes:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.communicate(timeout=10)


if __name__ == "__main__":
    unittest.main()
