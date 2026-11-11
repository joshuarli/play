import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from tests.test_fontconfig_writers import supported_host


ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(supported_host(), "requires x86-64 musl Linux")
class FontconfigLinkFlagsTest(unittest.TestCase):
    def test_non_utf8_pkg_config_flags_fail_without_lossy_paths(self):
        wrapper = shutil.which("kache")
        if not wrapper:
            self.skipTest("requires kache")
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="fontconfig-link-flags-") as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "Cargo.toml").write_text('[package]\nname = "fontconfig-link-probe"\nversion = "0.0.0"\nedition = "2024"\n[workspace]\n')
            (root / "src/lib.rs").write_text("pub fn probe() {}\n")
            shutil.copy2(ROOT / "crates/player-overlay/build.rs", root / "build.rs")
            tools = root / "tools"
            tools.mkdir()
            pkgconfig = tools / "pkg-config"
            pkgconfig.write_bytes(b"#!/bin/sh\nprintf '%b' '-L/tmp/invalid\\377 -lrust_fontconfig\\n'\n")
            pkgconfig.chmod(0o755)
            environment = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                               CARGO_TARGET_DIR=str(root / "target"), TMPDIR=str(scratch),
                               RUSTC_WRAPPER=wrapper, KACHE_DAEMON_AUTOSTART="0")
            environment.pop("RUST_FONTCONFIG_PREFIX", None)
            result = subprocess.run(["cargo", "build", "--lib"], cwd=root, env=environment,
                                    capture_output=True, text=True, timeout=120)
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("pkg-config returned non-UTF-8 link flags", result.stderr)


if __name__ == "__main__":
    unittest.main()
