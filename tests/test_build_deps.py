import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class MacosBuildDepsTest(unittest.TestCase):
    def test_clean_prefix_installs_vulkan_loader(self):
        self.check_bottle_install(reinstall=False)

    def test_reinstall_replaces_read_only_bottle_libraries(self):
        self.check_bottle_install(reinstall=True)

    def check_bottle_install(self, reinstall):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="build-deps-test-") as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            (scripts / "patches").mkdir()
            shutil.copy2(ROOT / "scripts/build-deps.sh", scripts / "build-deps.sh")
            shutil.copy2(ROOT / "scripts/patches/libplacebo-metal.patch", scripts / "patches/libplacebo-metal.patch")
            archives = root / "archives"
            archives.mkdir()
            self.archive(archives / "headers.tar.gz", {
                "Vulkan-Headers-vulkan-sdk-1.4.363.0/include/vulkan/vulkan.h": b"vulkan",
                "Vulkan-Headers-vulkan-sdk-1.4.363.0/include/vk_video/video.h": b"video",
                "Vulkan-Headers-vulkan-sdk-1.4.363.0/registry/vk.xml": b"registry",
            })
            self.archive(archives / "loader.tar.gz", {
                "vulkan-loader/1.4.363.0/lib/libvulkan.1.dylib": b"loader",
                "vulkan-loader/1.4.363.0/lib/pkgconfig/vulkan.pc": b"prefix=@@HOMEBREW_CELLAR@@/vulkan-loader/1.4.363.0\n",
            })
            self.archive(archives / "shaderc.tar.gz", {
                "shaderc/2026.4/include/shaderc/shaderc.h": b"shaderc header",
                "shaderc/2026.4/lib/libshaderc_shared.1.dylib": b"shaderc",
                "shaderc/2026.4/lib/pkgconfig/shaderc.pc": b"prefix=@@HOMEBREW_CELLAR@@/shaderc/2026.4\n",
            })
            tools = root / "tools"
            tools.mkdir()
            self.executable(tools / "curl", """#!/usr/bin/python3
import os, shutil, sys
args = sys.argv[1:]
out = args[args.index('-o') + 1]
url = next(x for x in args if x.startswith('https://'))
if 'Vulkan-Headers' in url:
    archive = 'headers.tar.gz'
elif 'vulkan-loader' in url:
    archive = 'loader.tar.gz'
elif 'shaderc' in url:
    archive = 'shaderc.tar.gz'
else:
    print('spirv-cross requested', file=sys.stderr)
    sys.exit(42)
shutil.copyfile(os.path.join(os.environ['FIXTURE_ARCHIVES'], archive), out)
""")
            self.executable(tools / "shasum", """#!/bin/sh
if [ "$#" -eq 2 ]; then cat >/dev/null; fi
case "$3" in
  *Vulkan-Headers*) digest=4a078be12bef21cfebc09d878b77a63cff9d68f899254a0b00d0e37ef73e7f7e ;;
  *vulkan-loader*) digest=6d5ee00d4104774f3ae56e5e9441a59a6a6f5d800f937f4e926a8f9e01ed046f ;;
  *shaderc*) digest=12ba0c59a98fb8f671f9473573002b2a812f26d2e60ea4f577f0c8c750c9838a ;;
  *) digest=fixture ;;
esac
printf '%s  %s\\n' "$digest" "${3:--}"
""")
            for tool in ("install_name_tool", "codesign", "xcrun"):
                self.executable(tools / tool, "#!/bin/sh\nexit 0\n")
            environment = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                               FIXTURE_ARCHIVES=str(archives), MESON="/usr/bin/true", NINJA="/usr/bin/true")
            prefix = root / ".work/deps/release"
            if reinstall:
                library = prefix / "lib"
                library.mkdir(parents=True)
                for name in ("libvulkan.1.dylib", "libshaderc_shared.1.dylib"):
                    path = library / name
                    path.write_bytes(b"old")
                    path.chmod(0o444)
                for name in ("vulkan-loader", "shaderc"):
                    (prefix / f".stamp-{name}").write_text("old")
            result = subprocess.run([str(scripts / "build-deps.sh")], env=environment,
                                    capture_output=True, text=True)
            self.assertIn("spirv-cross requested", result.stderr, result.stdout + result.stderr)
            self.assertEqual((prefix / "lib/libvulkan.1.dylib").read_bytes(), b"loader")
            self.assertTrue((prefix / ".stamp-vulkan-loader").is_file())
            self.assertEqual((prefix / "lib/libshaderc_shared.1.dylib").read_bytes(), b"shaderc")
            self.assertTrue((prefix / ".stamp-shaderc").is_file())

    @staticmethod
    def archive(path, entries):
        with tarfile.open(path, "w:gz") as archive:
            for name, data in entries.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))

    @staticmethod
    def executable(path, contents):
        path.write_text(contents)
        path.chmod(0o755)


if __name__ == "__main__":
    unittest.main()
