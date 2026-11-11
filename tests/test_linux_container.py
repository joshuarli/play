import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class LinuxContainerTest(unittest.TestCase):
    def test_runs_built_image_when_shared_tag_changes(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="linux-container-test-") as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "tools").mkdir()
            shutil.copy2(ROOT / "scripts/linux-container.sh", root / "scripts/linux-container.sh")
            docker = root / "tools/docker"
            docker.write_text('''#!/bin/sh
if [ "$1" = build ]; then
    printf '%s\\n' "$OTHER_IMAGE" > "$TAG_FILE"
    printf '%s\\n' "$BUILT_IMAGE"
    exit 0
fi
[ "$1" = run ] || exit 1
shift
for arg in "$@"; do
    if [ "$arg" = player-linux ]; then
        cat "$TAG_FILE"
        exit 0
    fi
    case "$arg" in sha256:*) printf '%s\\n' "$arg"; exit 0;; esac
done
exit 2
''')
            docker.chmod(0o755)
            built = "sha256:" + "a" * 64
            other = "sha256:" + "b" * 64
            environment = dict(os.environ, PATH=str(root / "tools") + os.pathsep + os.environ["PATH"],
                               BUILT_IMAGE=built, OTHER_IMAGE=other, TAG_FILE=str(root / "tag"))
            result = subprocess.run([str(root / "scripts/linux-container.sh"), "echo", "hello"],
                                    env=environment, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), built)
            environment["BUILT_IMAGE"] = "player-linux"
            result = subprocess.run([str(root / "scripts/linux-container.sh"), "echo", "hello"],
                                    env=environment, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("docker build did not return an image ID", result.stderr)
            self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
