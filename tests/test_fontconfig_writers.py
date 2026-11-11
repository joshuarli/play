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

    def test_separate_checkouts_serialize_a_shared_prefix(self):
        self.check_concurrent_builds(kill_first=False, same_prefix=True, separate_checkouts=True)

    def test_source_directory_can_be_the_prefix(self):
        self.check_concurrent_builds(kill_first=False, same_prefix=True, prefix_is_source=True)

    def test_crossed_source_and_prefix_directories_do_not_deadlock(self):
        self.check_concurrent_builds(kill_first=False, separate_checkouts=True, crossed_prefixes=True)

    def check_concurrent_builds(self, kill_first, same_prefix=False, separate_checkouts=False,
                                prefix_is_source=False, crossed_prefixes=False):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="fontconfig-writers-") as directory:
            root = Path(directory)
            checkouts = {name: root / name if separate_checkouts else root for name in ("first", "second")}
            for checkout in set(checkouts.values()):
                (checkout / "scripts").mkdir(parents=True)
                (checkout / "docker/linux").mkdir(parents=True)
                shutil.copy2(ROOT / "scripts/build-rust-fontconfig-linux.sh", checkout / "scripts/build-rust-fontconfig-linux.sh")
                shutil.copy2(ROOT / "scripts/rust_fontconfig_install.py", checkout / "scripts/rust_fontconfig_install.py")
                shutil.copy2(ROOT / "scripts/cargo-rustflags.sh", checkout / "scripts/cargo-rustflags.sh")
                shutil.copy2(ROOT / "docker/linux/rust-fontconfig.Cargo.lock", checkout / "docker/linux/rust-fontconfig.Cargo.lock")
            sources = {name: root / (name + "-source") if separate_checkouts else root / "source"
                       for name in ("first", "second")}
            for source in set(sources.values()):
                (source / "ffi").mkdir(parents=True)
                (source / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.0.0"\n')
                (source / "ffi/rust_fontconfig.h").write_text(f"fixture header {source.name}\n")
            if crossed_prefixes:
                prefixes = {"first": checkouts["second"] / ".work/tmp/rust-fontconfig",
                            "second": checkouts["first"] / ".work/tmp/rust-fontconfig"}
            elif prefix_is_source:
                prefixes = {name: checkouts[name] / ".work/tmp/rust-fontconfig" for name in checkouts}
            else:
                prefixes = {name: root / ("shared" if same_prefix else name) for name in checkouts}
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
            if crossed_prefixes:
                barrier = root / "barrier"
                barrier.mkdir()
                flock = tools / "flock"
                flock.write_text('''#!/bin/sh
set -eu
"$TEST_REAL_FLOCK" "$@"
if mkdir "$TEST_BARRIER/$TEST_ID" 2>/dev/null; then
    count=0
    while [ "$count" -lt 20 ] && [ ! -d "$TEST_BARRIER/$TEST_OTHER" ]; do
        sleep 0.1
        count=$((count + 1))
    done
fi
''')
                flock.chmod(0o755)
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
                                       RUST_FONTCONFIG_SOURCE=str(sources[name]), RUST_FONTCONFIG_PREFIX=str(prefixes[name]),
                                       TEST_READY=str(fifos[name + "-ready"][0]),
                                       TEST_RELEASE=str(fifos[name + "-release"][0]), TEST_ID=name)
                    if crossed_prefixes:
                        environment.update(TEST_REAL_FLOCK=shutil.which("flock"), TEST_BARRIER=str(barrier),
                                           TEST_OTHER="second" if name == "first" else "first")
                    process = subprocess.Popen([str(checkouts[name] / "scripts/build-rust-fontconfig-linux.sh")], env=environment,
                                               start_new_session=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                    processes.append(process)
                    if name == "first" and not crossed_prefixes:
                        self.assertTrue(select.select([fifos["first-ready"][1]], [], [], 20)[0])
                        self.assertEqual(os.read(fifos["first-ready"][1], 5), b"ready")
                if crossed_prefixes:
                    pending = {fifos[name + "-ready"][1]: name for name in ("first", "second")}
                    while pending:
                        ready = select.select(list(pending), [], [], 20)[0]
                        self.assertTrue(ready, "writers deadlocked on crossed source and prefix directories")
                        for fd in ready:
                            name = pending.pop(fd)
                            self.assertEqual(os.read(fd, 5), b"ready")
                            os.write(fifos[name + "-release"][1], b"continue\n")
                    for process in processes:
                        output = process.communicate(timeout=20)
                        self.assertEqual(process.returncode, 0, output)
                    for name in ("first", "second"):
                        self.assertEqual((prefixes[name] / "lib/librust_fontconfig.so.5.0.2").read_text(), name)
                    return
                self.assertFalse(select.select([fifos["second-ready"][1]], [], [], 2)[0],
                                 "second build entered the shared source tree")
                if kill_first:
                    os.killpg(processes[0].pid, signal.SIGKILL)
                    processes[0].communicate(timeout=10)
                    self.assertFalse((prefixes["first"] / ".input").exists())
                else:
                    os.write(fifos["first-release"][1], b"continue\n")
                if same_prefix and not separate_checkouts:
                    first_output = processes[0].communicate(timeout=20)
                    second_output = processes[1].communicate(timeout=20)
                    self.assertEqual(processes[0].returncode, 0, first_output)
                    self.assertEqual(processes[1].returncode, 0, second_output)
                    self.assertIn(b"already in", second_output[0])
                    self.assertFalse(select.select([fifos["second-ready"][1]], [], [], 0)[0])
                    self.assertEqual((prefixes["first"] / "lib/librust_fontconfig.so.5.0.2").read_text(), "first")
                    return
                self.assertTrue(select.select([fifos["second-ready"][1]], [], [], 20)[0])
                self.assertEqual(os.read(fifos["second-ready"][1], 5), b"ready")
                os.write(fifos["second-release"][1], b"continue\n")
                for process in processes[1:] if kill_first else processes:
                    output = process.communicate(timeout=20)
                    self.assertEqual(process.returncode, 0, output)
                for name in ("second",) if kill_first or same_prefix else ("first", "second"):
                    self.assertEqual((prefixes[name] / "lib/librust_fontconfig.so.5.0.2").read_text(), name)
            finally:
                for process in processes:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.communicate(timeout=10)


if __name__ == "__main__":
    unittest.main()
