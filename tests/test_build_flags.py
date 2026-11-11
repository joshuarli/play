import os
import sys
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def supported_host():
    if sys.platform != "linux" or os.uname().machine != "x86_64" or not shutil.which("ldd"):
        return False
    result = subprocess.run(["ldd", "--version"], capture_output=True, text=True)
    return "musl" in (result.stdout + result.stderr).lower()


@unittest.skipUnless(supported_host(), "requires x86-64 musl Linux")
class BuildFlagsTest(unittest.TestCase):
    def test_encoded_flags_keep_required_linux_policy(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="build-flags-") as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            for name in ("build.sh", "build-linux.sh"):
                shutil.copy2(ROOT / "scripts" / name, scripts / name)
            shutil.copy2(ROOT / "scripts/cargo-rustflags.sh", scripts / "cargo-rustflags.sh")
            for name in ("fetch-ffmpeg-runtime.sh", "linux-check.sh", "build-rust-fontconfig-linux.sh", "install-layout.sh"):
                path = scripts / name
                path.write_text("#!/bin/sh\nexit 0\n")
                path.chmod(0o755)
            deps = root / ".work/deps/linux"
            deps.mkdir(parents=True)
            (deps / ".stamp-libplacebo").write_text("fixture\n")
            tools = root / "tools"
            tools.mkdir()
            cargo = tools / "cargo"
            cargo.write_text('#!/bin/sh\nprintf "%s" "${CARGO_ENCODED_RUSTFLAGS-}" > "$TEST_ENCODED"\nprintf "%s" "${RUSTFLAGS-}" > "$TEST_ORDINARY"\n')
            cargo.chmod(0o755)
            output = root / "encoded"
            ordinary = root / "ordinary"
            for name in ("build.sh", "build-linux.sh"):
                for original in (None, "", "-C\x1fdebuginfo=1"):
                    with self.subTest(script=name, original=original):
                        environment = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                                           RUSTFLAGS="-C debuginfo=9", TEST_ENCODED=str(output),
                                           TEST_ORDINARY=str(ordinary))
                        if original is None:
                            environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
                        else:
                            environment["CARGO_ENCODED_RUSTFLAGS"] = original
                        result = subprocess.run([str(scripts / name)], env=environment, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        flags = ordinary.read_text().split() if original is None else output.read_text().split("\x1f")
                        if original is None:
                            self.assertEqual(flags[:2], ["-C", "debuginfo=9"])
                        if original:
                            self.assertEqual(flags[:2], ["-C", "debuginfo=1"])
                        if original is not None:
                            self.assertNotIn("debuginfo=9", flags)
                        self.assertIn("target-feature=-crt-static", flags)
                        if name == "build-linux.sh":
                            self.assertIn("link-arg=-static-libgcc", flags)
                            self.assertIn(f"--remap-path-prefix={root}=.", flags)

    def test_cargo_receives_encoded_policy_and_caller_flags(self):
        kache = shutil.which("kache")
        if not kache:
            self.skipTest("requires kache")
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="cargo-flags-") as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "Cargo.toml").write_text('[package]\nname = "cargo-flags-probe"\nversion = "0.0.0"\nedition = "2024"\n[workspace]\n')
            (root / "src/lib.rs").write_text('#[cfg(not(flag_from_encoded))]\ncompile_error!("caller flag missing");\n')
            wrapper = root / "rustc-wrapper"
            wrapper.write_text('#!/bin/sh\nfor arg in "$@"; do printf "%s\\0" "$arg" >> "$TEST_ARGS"; done\nexec "$TEST_KACHE" "$@"\n')
            wrapper.chmod(0o755)
            arguments = root / "arguments"
            environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"), TMPDIR=str(scratch),
                               CARGO_ENCODED_RUSTFLAGS="--cfg\x1fflag_from_encoded", RUSTFLAGS="--cfg ignored_flag",
                               RUSTC_WRAPPER=str(wrapper), TEST_KACHE=kache, TEST_ARGS=str(arguments),
                               KACHE_DAEMON_AUTOSTART="0", CARGO_BUILD_JOBS="1")
            remap = f"--remap-path-prefix={root / 'path with space'}=."
            command = 'source "$1"; append_cargo_rustflags -C target-feature=-crt-static "$2"; exec cargo build --lib'
            result = subprocess.run(["bash", "-c", command, "flags", str(ROOT / "scripts/cargo-rustflags.sh"), remap],
                                    cwd=root, env=environment, capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            flags = arguments.read_bytes().split(b"\0")
            self.assertIn(b"flag_from_encoded", flags)
            self.assertIn(b"target-feature=-crt-static", flags)
            self.assertIn(os.fsencode(remap), flags)
            self.assertNotIn(b"ignored_flag", flags)

    def test_required_flag_with_whitespace_fails_without_encoded_flags(self):
        environment = dict(os.environ)
        environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
        command = 'source "$1"; append_cargo_rustflags "$2"'
        result = subprocess.run(["bash", "-c", command, "flags", str(ROOT / "scripts/cargo-rustflags.sh"),
                                 "--remap-path-prefix=/path with space=."], env=environment, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required Cargo flag contains whitespace", result.stderr)

    def test_required_flag_with_encoded_separator_fails(self):
        environment = dict(os.environ, CARGO_ENCODED_RUSTFLAGS="")
        command = 'source "$1"; append_cargo_rustflags "$2"'
        result = subprocess.run(["bash", "-c", command, "flags", str(ROOT / "scripts/cargo-rustflags.sh"),
                                 "--remap-path-prefix=/path\x1fother=."], env=environment, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required Cargo flag contains encoded separator", result.stderr)


if __name__ == "__main__":
    unittest.main()
