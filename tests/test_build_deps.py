import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class MacosBuildDepsTest(unittest.TestCase):
    def test_clean_prefix_installs_vulkan_loader(self):
        self.check_bottle_install(reinstall=False)

    def test_reinstall_replaces_read_only_bottle_libraries(self):
        self.check_bottle_install(reinstall=True)

    def test_spirv_cross_recipe_changes_invalidate_dependencies(self):
        for name in ("meson.build", "gitversion.h.in"):
            with self.subTest(name=name):
                self.check_bottle_install(reinstall=False, changed_recipe=name)

    def test_python_patch_changes_invalidate_dependencies(self):
        self.check_bottle_install(reinstall=False, changed_patch=True)

    def check_bottle_install(self, reinstall, changed_recipe=None, changed_patch=False):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch, prefix="build-deps-test-") as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            (scripts / "patches").mkdir()
            shutil.copy2(ROOT / "scripts/build-deps.sh", scripts / "build-deps.sh")
            shutil.copy2(ROOT / "scripts/patches/libplacebo-metal.patch", scripts / "patches/libplacebo-metal.patch")
            shutil.copy2(ROOT / "scripts/patches/libplacebo-python.patch", scripts / "patches/libplacebo-python.patch")
            shutil.copytree(ROOT / "native/spirv-cross", root / "native/spirv-cross")
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
if [ "$#" -eq 2 ]; then exec /usr/bin/python3 -c 'import hashlib, sys; print(hashlib.sha256(sys.stdin.buffer.read()).hexdigest(), " -")'; fi
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
            if changed_recipe or changed_patch:
                cross_stamp = prefix / ".stamp-spirv-cross"
                cross_stamp.write_bytes((prefix / ".stamp-vulkan-loader").read_bytes())
                recipe = (root / "native/spirv-cross" / changed_recipe if changed_recipe else
                          scripts / "patches/libplacebo-python.patch")
                recipe.write_bytes(recipe.read_bytes() + b"\n")
                loader = prefix / "lib/libvulkan.1.dylib"
                loader.write_bytes(b"old")
                result = subprocess.run([str(scripts / "build-deps.sh")], env=environment,
                                        capture_output=True, text=True)
                self.assertIn("spirv-cross requested", result.stderr, result.stdout + result.stderr)
                self.assertEqual(loader.read_bytes(), b"loader")
                self.assertNotEqual(cross_stamp.read_bytes(), (prefix / ".stamp-vulkan-loader").read_bytes())

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


class LibplaceboPythonTest(unittest.TestCase):
    def setUp(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=scratch, prefix="libplacebo-python-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_libplacebo_uses_pinned_python_packages(self):
        real_meson = "/opt/homebrew/bin/meson" if Path("/opt/homebrew/bin/meson").is_file() else shutil.which("meson")
        real_ninja = shutil.which("ninja") or shutil.which("samu")
        if not real_meson or not real_ninja:
            self.skipTest("requires Meson and a Ninja-compatible backend")
        external = self.root / "external"
        external.mkdir()
        (external / "jinja2.py").write_text('value = "external"\n')
        user_base = self.root / "python-user"
        site_environment = dict(os.environ, PYTHONUSERBASE=str(user_base), PYTHONNOUSERSITE="1")
        user_site = Path(subprocess.check_output([sys.executable, "-c", "import site; print(site.getusersitepackages())"],
                                                 env=site_environment, text=True).strip())
        user_site.mkdir(parents=True)
        (user_site / "frontend_runtime.py").write_text('value = "available"\n')
        (user_site / "sitecustomize.py").write_text('import os, sys\nfrom pathlib import Path\nif sys.argv[0].endswith(("generator.py", "version.py")):\n    Path(os.environ["TEST_USER_SITE_MARKER"]).touch()\n')
        patches = self.root / "scripts/patches"
        patches.mkdir(parents=True)
        shutil.copy2(ROOT / "scripts/patches/libplacebo-python.patch", patches / "libplacebo-python.patch")
        for platform in ("macos", "linux"):
            with self.subTest(platform=platform):
                script = ROOT / "scripts" / ("build-deps.sh" if platform == "macos" else "build-deps-linux.sh")
                recipe = script.read_text()
                start = recipe.index("if ! have libplacebo; then")
                body = recipe[start:recipe.index("\nfi", start) + len("\nfi")]
                source = self.root / platform
                for directory in ("libplacebo-v1/3rdparty", "libplacebo-v1/src", "fast_float-1", "glad2-1/glad", "jinja2-1/src/jinja2", "markupsafe-1/src/markupsafe"):
                    (source / directory).mkdir(parents=True)
                (source / "jinja2-1/src/jinja2/__init__.py").write_text('value = "pinned"\n')
                project = source / "libplacebo-v1"
                library = "libplacebo.dylib" if platform == "macos" else "libplacebo.a"
                (project / "meson.build").write_text("project('fixture')\n" + "\n" * 490 + '''thirdparty = meson.project_source_root()/'3rdparty'
python = find_program(meson.project_source_root()/'python-wrapper')
python_env = environment()
python_env.append('PYTHONPATH', thirdparty/'jinja/src')
python_env.append('PYTHONPATH', thirdparty/'markupsafe/src')
python_env.append('PYTHONPATH', thirdparty/'glad')

if host_machine.system() == 'windows'
  threads = []
endif
version_pretty = '1'
custom_target('library', command: [python, files('generator.py'), '@OUTPUT@'],
  output: 'LIBRARY', env: python_env, install: true, install_dir: get_option('libdir'))
subdir('src')
'''.replace('LIBRARY', library))
                (project / "generator.py").write_text('import jinja2, sys\nfrom pathlib import Path\nPath(sys.argv[1]).write_text(jinja2.value)\n')
                wrapper = project / "python-wrapper"
                wrapper.write_text('#!/bin/sh\nexec "$TEST_PYTHON" "$@"\n')
                wrapper.chmod(0o755)
                (project / "src/meson.build").write_text("\n" * 234 + '''version_h = custom_target(
  command: [ python, join_paths(meson.current_source_dir(), 'version.py'),
             '@INPUT@', '@OUTPUT@', '@CURRENT_SOURCE_DIR@', version_pretty ],
  input: 'version.h.in',
  output: 'version.h',
  build_by_default: true,
  build_always_stale: true,
)
''')
                (project / "src/version.h.in").write_text("version\n")
                (project / "src/version.py").write_text('import sys\nfrom pathlib import Path\nPath(sys.argv[2]).write_text("version")\n')
                prefix = self.root / (platform + "-prefix")
                meson = self.root / (platform + "-meson")
                meson.write_text('#!/bin/sh\n"$TEST_PYTHON" -c "import frontend_runtime" || exit 1\nexec "$TEST_REAL_MESON" "$1" "$2" "$3" --prefix="$PREFIX" --libdir=lib\n')
                meson.chmod(0o755)
                driver = self.root / (platform + "-recipe")
                driver.write_text('''#!/bin/bash
set -eu
have() { return 1; }
fetch() { :; }
extract() { :; }
stamp() { :; }
run_log() {
    name=$1
    shift
    case "$name" in libplacebo-patch|libplacebo-id) return ;; esac
    "$@"
}
MESON_SETUP_LIBPLACEBO=("$TEST_MESON" setup)
NINJA=$TEST_NINJA
SRC=$TEST_SOURCE
ROOT=$TEST_ROOT
LIBPLACEBO_VERSION=1
FAST_FLOAT_VERSION=1
GLAD_VERSION=1
JINJA_VERSION=1
MARKUPSAFE_VERSION=1
export PREFIX
''' + body + '\n')
                driver.chmod(0o755)
                environment = dict(os.environ, PYTHONPATH=str(external), TEST_SOURCE=str(source), TEST_ROOT=str(self.root),
                                   TEST_MESON=str(meson), TEST_REAL_MESON=real_meson, TEST_NINJA=real_ninja,
                                   TEST_PYTHON=sys.executable, PREFIX=str(prefix), PYTHONUSERBASE=str(user_base),
                                   PYTHONNOUSERSITE="", TEST_USER_SITE_MARKER=str(self.root / (platform + "-site-loaded")))
                run = subprocess.run([str(driver)], env=environment, capture_output=True, text=True)
                self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
                self.assertEqual((prefix / "lib" / library).read_text(), "pinned")
                self.assertFalse(Path(environment["TEST_USER_SITE_MARKER"]).exists())


if __name__ == "__main__":
    unittest.main()
