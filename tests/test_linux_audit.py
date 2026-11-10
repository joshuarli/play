import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(sys.platform.startswith("linux"), "requires Linux shell tools")
class LinuxAuditTest(unittest.TestCase):
    def test_static_ffmpeg_audit_handles_got_slots_and_readelf_errors(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="linux-audit-test-") as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            shutil.copy2(ROOT / "scripts/audit-linux.sh", scripts / "audit-linux.sh")
            (scripts / "network-schemes.txt").write_text("http\nhttps\n")
            (root / "ffmpeg-runtime.lock.json").write_text(json.dumps({"libraries": ["libavcodec"]}))
            manifest = root / ".work/deps/ffmpeg-runtime/x86_64-unknown-linux-musl/share/ffmpeg-build/manifest.json"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps({"libraries": {"libavcodec": {"soname": "libavcodec.so.61"}}}))
            player = root / "dist/player"
            player.parent.mkdir()
            self.executable(player, "#!/bin/sh\nprintf 'file://\\n'\n")
            unstripped = root / "build-linux/mpv"
            unstripped.parent.mkdir()
            unstripped.write_text("fixture")
            tools = root / "tools"
            tools.mkdir()
            self.executable(tools / "file", "#!/bin/sh\nprintf 'ELF 64-bit x86-64 stripped\\n'\n")
            self.executable(tools / "readelf", '''#!/bin/sh
case "$1" in
  -SW) [ "${AUDIT_FIXTURE_FAIL:-}" = sections ] && exit 1; printf '%s\\n' '  [35] .got PROGBITS' ;;
  -sW) [ "${AUDIT_FIXTURE_FAIL:-}" = symbols ] && exit 1; printf '%b' "$AUDIT_FIXTURE_ELF_SYMBOLS" ;;
  *) printf '%s\\n' ' 0x (NEEDED) Shared library: [libavcodec.so.61]' ' 0x (RUNPATH) Library runpath: [$ORIGIN/../lib]' ;;
esac
''')
            self.executable(tools / "nm", '''#!/bin/sh
case "$1" in
  --defined-only) printf '%b' "$AUDIT_FIXTURE_SYMBOLS" ;;
  -D) [ "$2" = --undefined-only ] && printf '                 U avformat_open_input\\n' ;;
  *) printf '00000000 T player_engine\\n' ;;
esac
exit 0
''')
            for name in ("ldd", "strings"):
                self.executable(tools / name, "#!/bin/sh\nexit 0\n")
            environment = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"])
            for name in ("PLAYER", "UNSTRIPPED", "FFMPEG_PREFIX", "PLAYER_FFMPEG_RPATH"):
                environment.pop(name, None)
            got = "00000010 d av_frame_alloc$got\\n"
            got_elf = "1535: 00000010 0 OBJECT LOCAL DEFAULT 35 av_frame_alloc$got\\n"
            environment["AUDIT_FIXTURE_SYMBOLS"] = got
            environment["AUDIT_FIXTURE_ELF_SYMBOLS"] = got_elf
            accepted = subprocess.run([str(scripts / "audit-linux.sh")], env=environment,
                                      capture_output=True, text=True)
            self.assertEqual(accepted.returncode, 0, accepted.stdout + accepted.stderr)
            for symbol, elf_symbol in (
                ("00000020 t av_frame_alloc\\n", "1536: 00000020 4 FUNC LOCAL DEFAULT 14 av_frame_alloc\\n"),
                ("00000030 T av_frame_free$got\\n", "1537: 00000030 4 FUNC GLOBAL DEFAULT 14 av_frame_free$got\\n"),
                ("00000040 d av_dict_get$got\\n", "1538: 00000040 8 OBJECT LOCAL DEFAULT 36 av_dict_get$got\\n"),
            ):
                environment["AUDIT_FIXTURE_SYMBOLS"] = got + symbol
                environment["AUDIT_FIXTURE_ELF_SYMBOLS"] = got_elf + elf_symbol
                rejected = subprocess.run([str(scripts / "audit-linux.sh")], env=environment,
                                          capture_output=True, text=True)
                self.assertNotEqual(rejected.returncode, 0, rejected.stdout + rejected.stderr)
                self.assertIn("FAIL audit-linux: no FFmpeg linked statically", rejected.stdout)
            environment["AUDIT_FIXTURE_ELF_SYMBOLS"] = got_elf
            for failure in ("sections", "symbols"):
                environment["AUDIT_FIXTURE_FAIL"] = failure
                rejected = subprocess.run([str(scripts / "audit-linux.sh")], env=environment,
                                          capture_output=True, text=True)
                self.assertNotEqual(rejected.returncode, 0, rejected.stdout + rejected.stderr)
                self.assertIn("FAIL audit-linux: no FFmpeg linked statically", rejected.stdout)

    @staticmethod
    def executable(path, contents):
        path.write_text(contents)
        path.chmod(0o755)


if __name__ == "__main__":
    unittest.main()
