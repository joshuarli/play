import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class ReleaseArchiveTest(unittest.TestCase):
    def test_archive_is_sorted_and_reproducible(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            if sys.platform == "darwin":
                stage = root / "Player.app"
                (stage / "Contents").mkdir(parents=True)
                (stage / "Contents" / "player").write_text("player")
                (stage / "Contents" / "player").chmod(0o755)
                (stage / "Contents" / "link").symlink_to("player")
                script = "package-macos.sh"
                executable = "Player.app/Contents/player"
                link = "Player.app/Contents/link"
                expected = ["Player.app", "Player.app/Contents", link, executable]
            elif sys.platform.startswith("linux"):
                stage = root / "stage"
                stage.mkdir()
                (stage / "README").write_text("readme")
                (stage / "player").write_text("player")
                (stage / "player").chmod(0o755)
                script = "package-linux.sh"
                executable = "player"
                link = None
                expected = ["README", "player"]
            else:
                self.skipTest("requires macOS or Linux")
            archive = root / "release.tar.xz"
            command = [str(ROOT / "scripts" / script), str(stage), str(archive)]
            subprocess.run(command, check=True)
            first = archive.read_bytes()
            os.utime(stage, None)
            subprocess.run(command, check=True)
            self.assertEqual(first, archive.read_bytes())
            with tarfile.open(archive, "r:xz") as tar:
                members = tar.getmembers()
                names = [member.name for member in members]
                self.assertEqual(names, expected)
                self.assertEqual(names, sorted(names))
                self.assertTrue(all((m.uid, m.gid, m.mtime) == (0, 0, 0) for m in members))
                self.assertEqual(tar.getmember(executable).mode, 0o755)
                if link:
                    self.assertTrue(tar.getmember(link).issym())
                    self.assertEqual(tar.getmember(link).linkname, "player")


if __name__ == "__main__":
    unittest.main()
