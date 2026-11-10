import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import uuid


ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(sys.platform == "darwin" and shutil.which("kache"), "requires macOS and kache")
class NativeRpathTest(unittest.TestCase):
    def test_cargo_relinks_after_native_prefix_changes(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="native-generation-") as directory:
            workspace = Path(directory)
            crates = workspace / "crates"
            crates.mkdir()
            shutil.copytree(ROOT / "crates/player-build", crates / "player-build")
            app = crates / "generation-probe"
            (app / "src").mkdir(parents=True)
            (workspace / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/*"]\nresolver = "2"\n')
            (app / "Cargo.toml").write_text(
                '[package]\nname = "generation-probe"\nversion = "0.0.0"\nedition = "2021"\n'
                '[build-dependencies]\nplayer-build = { path = "../player-build" }\n'
            )
            (app / "src/main.rs").write_text("fn main() {}\n")
            (app / "build.rs").write_text(
                'fn main() { let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()); '
                'player_build::emit_macos_prefix_rpath("generation-probe", &dir); }\n'
            )
            deps = workspace / ".work/deps"
            first = deps / "generation-one"
            (first / "lib").mkdir(parents=True)
            (first / ".stamp-libplacebo").write_text("one\n")
            (deps / "release").symlink_to(first)
            environment = dict(os.environ, RUSTC_WRAPPER=shutil.which("kache"), KACHE_KEY_SALT="player-generation-" + uuid.uuid4().hex,
                               TMPDIR=str(scratch), CARGO_BUILD_JOBS="2")
            environment.pop("CARGO_TARGET_DIR", None)
            def build():
                result = subprocess.run(["cargo", "build", "--offline", "--quiet"], cwd=workspace,
                                        env=environment, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr)
                binary = workspace / "target/debug/generation-probe"
                output = subprocess.check_output(["otool", "-l", str(binary)], text=True)
                return "\n".join(line.strip() for line in output.splitlines() if " path " in line)
            self.assertIn(str(first / "lib"), build())
            second = deps / "generation-two"
            (second / "lib").mkdir(parents=True)
            (second / ".stamp-libplacebo").write_text("two\n")
            next_link = deps / "next"
            next_link.symlink_to(second)
            os.replace(next_link, deps / "release")
            self.assertIn(str(second / "lib"), build())

    def test_cached_build_scripts_use_the_current_checkout(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="native-rpaths-") as directory:
            root = Path(directory)
            salt = "player-rpaths-" + uuid.uuid4().hex
            target = subprocess.check_output(["rustc", "-vV"], text=True).split("host: ", 1)[1].splitlines()[0]
            outputs = []
            for name in ("first", "second"):
                workspace = root / name
                crates = workspace / "crates"
                crates.mkdir(parents=True)
                shutil.copytree(ROOT / "crates/player-build", crates / "player-build")
                app = crates / "rpath-probe"
                (app / "src").mkdir(parents=True)
                (workspace / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/*"]\nresolver = "2"\n')
                (app / "Cargo.toml").write_text(
                    '[package]\nname = "rpath-probe"\nversion = "0.0.0"\nedition = "2021"\n'
                    '[build-dependencies]\nplayer-build = { path = "../player-build" }\n'
                )
                (app / "src/main.rs").write_text("fn main() {}\n")
                (app / "build.rs").write_text(
                    'fn main() { let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()); '
                    'player_build::emit_macos_prefix_rpath("rpath-probe", &dir); '
                    'player_build::ffmpeg_runtime::emit_macos_dev_rpath("rpath-probe", &dir); }\n'
                )
                native = workspace / ".work/deps/release"
                (native / "lib").mkdir(parents=True)
                (native / ".stamp-libplacebo").write_text("same\n")
                runtime = workspace / ".work/deps/ffmpeg-runtime" / target
                (runtime / "lib").mkdir(parents=True)
                (runtime / "share/ffmpeg-build").mkdir(parents=True)
                (runtime / "share/ffmpeg-build/manifest.json").write_text("{}\n")
                environment = dict(os.environ, RUSTC_WRAPPER=shutil.which("kache"), KACHE_KEY_SALT=salt,
                                   TMPDIR=str(scratch), CARGO_BUILD_JOBS="2")
                for variable in ("CARGO_TARGET_DIR", "FFMPEG_PREFIX", "KACHE_BUILD_SCRIPT_CACHE"):
                    environment.pop(variable, None)
                result = subprocess.run(["cargo", "build", "--offline", "--quiet"], cwd=workspace,
                                        env=environment, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr)
                binary = workspace / "target/debug/rpath-probe"
                output = subprocess.check_output(["otool", "-l", str(binary)], text=True)
                paths = "\n".join(line.strip() for line in output.splitlines() if " path " in line)
                outputs.append(paths)
                self.assertIn(str(native / "lib"), paths)
                self.assertIn(str(runtime / "lib"), paths)
            self.assertNotIn(str(root / "first"), outputs[1])


if __name__ == "__main__":
    unittest.main()
