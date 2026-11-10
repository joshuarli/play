import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class BundleTest(unittest.TestCase):
    def test_read_only_dependency_is_writable_in_bundle(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="bundle-test-") as directory:
            root = Path(directory)
            tools = root / "tools"
            tools.mkdir()
            self.executable(tools / "otool", '''#!/bin/sh
printf '%s:\\n' "$2"
case "$1:$2" in
  -L:*/Contents/MacOS/player) printf '\\t@rpath/libvulkan.1.dylib (compatibility version 1.0.0, current version 1.0.0)\\n' ;;
esac
''')
            self.executable(tools / "strip", '''#!/usr/bin/python3
import os, stat, sys
path = sys.argv[-1]
if not os.stat(path).st_mode & stat.S_IWUSR:
    sys.exit('strip: output is not writable')
with open(path, 'ab') as output:
    output.write(b'stripped')
''')
            self.executable(tools / "lipo", '''#!/usr/bin/python3
import shutil, sys
shutil.copyfile(sys.argv[3], sys.argv[5])
''')
            for name in ("install_name_tool", "codesign"):
                self.executable(tools / name, "#!/bin/sh\nexit 0\n")
            self.executable(tools / "git", "#!/bin/sh\nprintf 'fixture\\n'\n")
            (root / "ffmpeg-runtime.lock.json").write_text(json.dumps({
                "ffmpeg": "9.0.2", "release": "fixture", "libraries": [],
            }))
            app = root / "crates/player-app"
            app.mkdir(parents=True)
            (app / "Cargo.toml").write_text('[package]\nversion = "1.0.0"\n')
            binary = root / "target/dist/player"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"player")
            binary.chmod(0o755)
            prefix = root / ".work/deps/release"
            library = prefix / "lib/libvulkan.1.dylib"
            library.parent.mkdir(parents=True)
            library.write_bytes(b"vulkan")
            library.chmod(0o444)
            (library.parent / "libMoltenVK.dylib").write_bytes(b"moltenvk")
            icd = prefix / "share/vulkan/icd.d/MoltenVK_icd.json"
            icd.parent.mkdir(parents=True)
            icd.write_text(json.dumps({"ICD": {"library_path": "original"}}))
            manifest = root / ".work/deps/ffmpeg-runtime/aarch64-apple-darwin/share/ffmpeg-build/manifest.json"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps({"libraries": {}}))
            environment = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"])
            environment.pop("FFMPEG_PREFIX", None)
            result = subprocess.run(["python3", str(ROOT / "scripts/bundle.py"), str(root)],
                                    env=environment, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            bundled = root / "dist/Player.app/Contents/Frameworks/libvulkan.1.dylib"
            self.assertEqual(bundled.read_bytes(), b"vulkanstripped")
            self.assertEqual(bundled.stat().st_mode & 0o777, 0o444)
            self.assertEqual(library.read_bytes(), b"vulkan")
            self.assertEqual(library.stat().st_mode & 0o777, 0o444)

    @staticmethod
    def executable(path, contents):
        path.write_text(contents)
        path.chmod(0o755)


if __name__ == "__main__":
    unittest.main()
