import os
import fcntl
import json
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

from tests.test_fontconfig_writers import supported_host


ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(supported_host(), "requires x86-64 musl Linux")
class FontconfigIntegrityTest(unittest.TestCase):
    def setUp(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=scratch, prefix="fontconfig-integrity-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "scripts").mkdir()
        (self.root / "docker/linux").mkdir(parents=True)
        shutil.copy2(ROOT / "scripts/build-rust-fontconfig-linux.sh", self.root / "scripts/build-rust-fontconfig-linux.sh")
        shutil.copy2(ROOT / "scripts/rust_fontconfig_install.py", self.root / "scripts/rust_fontconfig_install.py")
        shutil.copy2(ROOT / "scripts/cargo-rustflags.sh", self.root / "scripts/cargo-rustflags.sh")
        shutil.copy2(ROOT / "docker/linux/rust-fontconfig.Cargo.lock", self.root / "docker/linux/rust-fontconfig.Cargo.lock")
        self.source = self.root / "source"
        (self.source / "ffi").mkdir(parents=True)
        (self.source / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.0.0"\n')
        (self.source / "ffi/rust_fontconfig.h").write_text("fixture header\n")
        self.prefix = self.root / "prefix"
        self.record = self.root / "builds"
        tools = self.root / "tools"
        tools.mkdir()
        cargo = tools / "cargo"
        cargo.write_text('''#!/bin/sh
set -eu
[ "$1" != clean ] || exit 0
printf 'build\\n' >> "$TEST_RECORD"
printf '%s' "${CARGO_ENCODED_RUSTFLAGS-}" > "$TEST_FLAGS"
[ "${TEST_FAIL:-}" != 1 ] || exit 37
[ "${TEST_REMOVE_HEADER:-}" != 1 ] || rm -f ffi/rust_fontconfig.h
mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release"
printf library > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
''')
        cargo.chmod(0o755)
        self.env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                        RUST_FONTCONFIG_SOURCE=str(self.source), RUST_FONTCONFIG_PREFIX=str(self.prefix),
                        RUSTC=shutil.which("rustc"), TEST_RECORD=str(self.record), TEST_FLAGS=str(self.root / "flags"))

    def build(self, **environment):
        return subprocess.run([str(self.root / "scripts/build-rust-fontconfig-linux.sh")],
                              env=dict(self.env, **environment), capture_output=True, text=True, errors="surrogateescape")

    def test_replaced_prefix_is_not_accepted_after_waiting_for_its_lock(self):
        self.check_replaced_lock_path(self.prefix)

    def test_replaced_source_is_not_accepted_after_waiting_for_its_lock(self):
        self.check_replaced_lock_path(self.root / ".work/tmp/rust-fontconfig")

    def test_source_symlink_does_not_modify_its_target(self):
        outside = self.root / "outside"
        tree = outside / "rust-fontconfig-5.0.2-laputa.2"
        tree.mkdir(parents=True)
        sentinel = tree / "sentinel"
        sentinel.write_text("leave alone\n")
        source = self.root / ".work/tmp/rust-fontconfig"
        source.parent.mkdir(parents=True)
        source.symlink_to(outside, target_is_directory=True)
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(sentinel.read_text(), "leave alone\n")

    def test_source_manifest_symlinks_do_not_modify_their_targets(self):
        for name in ("Cargo.toml", "Cargo.lock"):
            with self.subTest(name=name):
                original = self.source / name
                if original.exists():
                    original.unlink()
                outside = self.root / ("outside-" + name)
                outside.write_text("leave alone\n")
                original.symlink_to(outside)
                try:
                    result = self.build()
                    self.assertEqual(outside.read_text(), "leave alone\n")
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    self.assertFalse((self.prefix / ".input").exists())
                finally:
                    original.unlink(missing_ok=True)
                    if name == "Cargo.toml":
                        original.write_text('[package]\nname = "fixture"\nversion = "0.0.0"\n')


    def test_source_tree_symlink_does_not_modify_its_target(self):
        outside = self.root / "outside-tree"
        outside.mkdir()
        sentinel = outside / "sentinel"
        sentinel.write_text("leave alone\n")
        tree = self.root / ".work/tmp/rust-fontconfig/rust-fontconfig-5.0.2-laputa.2"
        tree.parent.mkdir(parents=True)
        tree.symlink_to(outside, target_is_directory=True)
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertTrue(tree.is_symlink())
        self.assertEqual(sentinel.read_text(), "leave alone\n")


    def test_source_tree_special_file_is_not_removed(self):
        tree = self.root / ".work/tmp/rust-fontconfig/rust-fontconfig-5.0.2-laputa.2"
        tree.parent.mkdir(parents=True)
        os.mkfifo(tree)
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertTrue(tree.exists())


    def test_source_modified_during_build_publishes_no_success(self):
        cargo = self.root / "tools/cargo"
        cargo.write_text('''#!/bin/sh
set -eu
[ "$1" != clean ] || exit 0
printf changed > "$RUST_FONTCONFIG_SOURCE/ffi/rust_fontconfig.h"
mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release"
printf library > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
''')
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("source changed during build", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())


    def test_source_digest_failure_during_validation_is_not_reused(self):
        first = self.build()
        self.assertEqual(first.returncode, 0, first.stderr)
        real_tar = shutil.which("tar")
        tar = self.root / "tools/tar"
        tar.write_text('''#!/bin/sh
case " $* " in
    *" --sort=name "*)
        if [ -e "$TEST_DIGEST_MARKER" ]; then
            "$TEST_REAL_TAR" "$@"
            exit 1
        fi
        : > "$TEST_DIGEST_MARKER"
        ;;
esac
exec "$TEST_REAL_TAR" "$@"
''')
        tar.chmod(0o755)
        result = self.build(TEST_REAL_TAR=real_tar, TEST_DIGEST_MARKER=str(self.root / "digest-marker"))
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("source changed during validation", result.stderr)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])

    def test_source_modified_during_validation_is_not_reused(self):
        first = self.build()
        self.assertEqual(first.returncode, 0, first.stderr)
        real_tar = shutil.which("tar")
        tar = self.root / "tools/tar"
        tar.write_text('''#!/bin/sh
case " $* " in
    *" --sort=name "*)
        if [ -e "$TEST_DIGEST_MARKER" ]; then
            printf changed > "$RUST_FONTCONFIG_SOURCE/ffi/rust_fontconfig.h"
        else
            : > "$TEST_DIGEST_MARKER"
        fi
        ;;
esac
exec "$TEST_REAL_TAR" "$@"
''')
        tar.chmod(0o755)
        result = self.build(TEST_REAL_TAR=real_tar, TEST_DIGEST_MARKER=str(self.root / "digest-marker"))
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("source changed during validation", result.stderr)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])
        tar.unlink()
        retry = self.build()
        self.assertEqual(retry.returncode, 0, retry.stderr)
        self.assertEqual(self.record.read_text().splitlines(), ["build", "build"])

    def test_source_modified_during_copy_publishes_no_success(self):
        real_tar = shutil.which("tar")
        tar = self.root / "tools/tar"
        tar.write_text('''#!/bin/sh
if [ "$1" = -x ]; then
    printf changed > "$RUST_FONTCONFIG_SOURCE/ffi/rust_fontconfig.h"
fi
exec "$TEST_REAL_TAR" "$@"
''')
        tar.chmod(0o755)
        result = self.build(TEST_REAL_TAR=real_tar)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("source changed during copying", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def test_source_parent_symlink_does_not_modify_its_target(self):
        outside = self.root / "outside"
        tree = outside / "rust-fontconfig/rust-fontconfig-5.0.2-laputa.2"
        tree.mkdir(parents=True)
        sentinel = tree / "sentinel"
        sentinel.write_text("leave alone\n")
        parent = self.root / ".work/tmp"
        parent.parent.mkdir(parents=True)
        parent.symlink_to(outside, target_is_directory=True)
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(sentinel.read_text(), "leave alone\n")

    def test_source_target_symlink_does_not_write_outside_the_cache(self):
        outside = self.root / "outside"
        outside.mkdir()
        source = self.root / ".work/tmp/rust-fontconfig"
        source.mkdir(parents=True)
        (source / "target").symlink_to(outside, target_is_directory=True)
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(list(outside.iterdir()), [])

    def test_source_target_subdirectory_symlink_does_not_write_outside_the_cache(self):
        outside = self.root / "outside"
        outside.mkdir()
        target = self.root / ".work/tmp/rust-fontconfig/target/x86_64-unknown-linux-musl"
        target.mkdir(parents=True)
        (target / "release").symlink_to(outside, target_is_directory=True)
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(list(outside.iterdir()), [])

    def test_source_target_special_file_fails_without_opening_it(self):
        target = self.root / ".work/tmp/rust-fontconfig/target"
        target.mkdir(parents=True)
        fifo = target / "fifo"
        os.mkfifo(fifo)
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertTrue(fifo.exists())

    def test_valid_installed_product_does_not_require_a_safe_target_tree(self):
        self.assertEqual(self.build().returncode, 0)
        target = self.root / ".work/tmp/rust-fontconfig/target"
        fifo = target / "fifo"
        os.mkfifo(fifo)
        result = self.build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already in", result.stdout)
        self.assertTrue(fifo.exists())
        self.assertEqual(self.record.read_text().splitlines(), ["build"])

    def check_replaced_lock_path(self, path):
        self.assertEqual(self.build().returncode, 0)
        outside = self.root / "outside"
        outside.mkdir()
        marker = self.root / "waiting"
        flock = self.root / "tools/flock"
        flock.write_text('''#!/bin/sh
set -eu
if [ "$(readlink "/proc/$$/fd/$2")" = "$TEST_LOCK_PATH" ]; then
    : > "$TEST_WAITING"
fi
exec "$TEST_FLOCK" "$@"
''')
        flock.chmod(0o755)
        descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
        fcntl.flock(descriptor, fcntl.LOCK_EX)
        environment = dict(self.env, TEST_LOCK_PATH=str(path), TEST_WAITING=str(marker), TEST_FLOCK=shutil.which("flock"))
        process = subprocess.Popen([str(self.root / "scripts/build-rust-fontconfig-linux.sh")], env=environment,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            deadline = time.monotonic() + 10
            while not marker.exists() and process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(marker.exists(), "writer did not wait for the expected lock")
            path.rename(self.root / "moved")
            path.symlink_to(outside, target_is_directory=True)
            fcntl.flock(descriptor, fcntl.LOCK_UN)
            output = process.communicate(timeout=10)
            self.assertNotEqual(process.returncode, 0, output)
            self.assertIn(b"rust-fontconfig lock path changed", output[1])
            self.assertEqual(list(outside.iterdir()), [])
        finally:
            os.close(descriptor)
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate(timeout=10)

    def test_reuse_validates_all_owned_outputs(self):
        self.assertEqual(self.build().returncode, 0)
        owned = (".input", "lib/librust_fontconfig.so", "lib/librust_fontconfig.so.5.0.2",
                 "lib/pkgconfig/rust-fontconfig.pc", "include/rust-fontconfig/rust_fontconfig.h")
        before = {name: (self.prefix / name).lstat().st_mtime_ns for name in owned}
        self.assertEqual(self.build().returncode, 0)
        self.assertEqual({name: (self.prefix / name).lstat().st_mtime_ns for name in owned}, before)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])
        for index, relative in enumerate(("include/rust-fontconfig/rust_fontconfig.h",
                                          "lib/librust_fontconfig.so.5.0.2",
                                          "lib/pkgconfig/rust-fontconfig.pc"), 2):
            with self.subTest(product=relative):
                product = self.prefix / relative
                product.write_text("corrupted")
                result = self.build()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotEqual(product.read_text(), "corrupted")
                self.assertEqual(len(self.record.read_text().splitlines()), index)

    def test_failed_rebuild_removes_the_success_marker(self):
        self.assertEqual(self.build().returncode, 0)
        (self.source / "ffi/rust_fontconfig.h").write_text("changed source\n")
        result = self.build(TEST_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.prefix / ".input").exists())

    def test_rebuild_does_not_republish_a_corrupted_cargo_product(self):
        cargo = self.root / "tools/cargo"
        cargo.write_text('''#!/bin/sh
set -eu
product="$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
if [ "$1" = clean ]; then
    rm -f "$product"
    exit 0
fi
printf 'build\\n' >> "$TEST_RECORD"
mkdir -p "$(dirname "$product")"
if [ ! -e "$product" ]; then printf library > "$product"; fi
''')
        self.assertEqual(self.build().returncode, 0)
        target = self.root / ".work/tmp/rust-fontconfig/target/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
        target.write_text("corrupt cargo output")
        installed = self.prefix / "lib/librust_fontconfig.so.5.0.2"
        installed.write_text("corrupt installed output")
        result = self.build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(installed.read_text(), "library")

    def real_cargo_fixture(self):
        kache = shutil.which("kache")
        if not kache:
            self.skipTest("requires kache")
        (self.source / "src").mkdir()
        (self.source / "Cargo.toml").write_text('[package]\nname="rust-fontconfig"\nversion="5.0.2"\nedition="2024"\n[features]\nffi=[]\n')
        (self.source / "src/lib.rs").write_text('#[unsafe(no_mangle)]\npub extern "C" fn fontconfig_probe() -> u32 { 42 }\n')
        (self.root / "docker/linux/rust-fontconfig.Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "rust-fontconfig"\nversion = "5.0.2"\n')
        environment = dict(self.env, PATH=os.environ["PATH"], RUSTC_WRAPPER=kache,
                           KACHE_CACHE_DIR=str(self.root / "kache-cache"), KACHE_DAEMON_AUTOSTART="0")
        command = [str(self.root / "scripts/build-rust-fontconfig-linux.sh")]
        return environment, command

    def test_real_cargo_repairs_a_corrupted_product_on_a_rebuild(self):
        environment, command = self.real_cargo_fixture()
        first = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
        self.assertEqual(first.returncode, 0, first.stderr)
        target = self.root / ".work/tmp/rust-fontconfig/target/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
        installed = self.prefix / "lib/librust_fontconfig.so.5.0.2"
        self.assertEqual(installed.read_bytes()[:4], b"\x7fELF")
        target.write_bytes(b"corrupt cargo output")
        installed.write_bytes(b"corrupt installed output")
        repaired = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
        self.assertEqual(repaired.returncode, 0, repaired.stderr)
        self.assertEqual(installed.read_bytes()[:4], b"\x7fELF")

    def test_invalid_cargo_compiler_configuration_does_not_publish_success(self):
        cargo = self.root / "tools/cargo"
        cargo.write_text('#!/bin/sh\n[ "$1" = -Z ] || exit 22\nprintf invalid\n')
        environment = dict(self.env)
        environment.pop("RUSTC", None)
        environment.pop("CARGO_BUILD_RUSTC", None)
        result = subprocess.run([str(self.root / "scripts/build-rust-fontconfig-linux.sh")], env=environment, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("compiler cannot be resolved", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def test_unknown_relative_compiler_origin_does_not_publish_success(self):
        cargo = self.root / "tools/cargo"
        cargo.write_text('''#!/bin/sh
set -eu
[ "$1" = -Z ] || exit 22
case " $* " in
    *" --show-origin "*) printf 'unrecognized origin\n' ;;
    *) printf '{"build":{"rustc":"tools/rustc"}}\n' ;;
esac
''')
        environment = dict(self.env)
        environment.pop("RUSTC", None)
        environment.pop("CARGO_BUILD_RUSTC", None)
        result = subprocess.run([str(self.root / "scripts/build-rust-fontconfig-linux.sh")], env=environment, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("compiler cannot be resolved", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def test_cargo_configuration_probe_does_not_change_build_environment(self):
        cargo = self.root / "tools/cargo"
        cargo.write_text('''#!/bin/sh
set -eu
if [ "$1" = -Z ]; then
    [ "$KACHE_DAEMON_AUTOSTART" = 0 ] || exit 23
    [ "$RUSTC_BOOTSTRAP" = 1 ] || exit 24
    printf '{}\n'
    exit 0
fi
[ "$KACHE_DAEMON_AUTOSTART" = 1 ] || exit 25
[ "$RUSTC_BOOTSTRAP" = original ] || exit 26
[ "$1" != clean ] || exit 0
mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release"
printf library > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
''')
        environment = dict(self.env, KACHE_DAEMON_AUTOSTART="1", RUSTC_BOOTSTRAP="original")
        environment.pop("RUSTC", None)
        environment.pop("CARGO_BUILD_RUSTC", None)
        result = subprocess.run([str(self.root / "scripts/build-rust-fontconfig-linux.sh")], env=environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_cargo_compiler_configuration_changed_during_build_does_not_publish_success(self):
        first = self.root / "tools/first-rustc"
        second = self.root / "tools/second-rustc"
        for compiler in (first, second):
            compiler.write_text("#!/bin/sh\nprintf 'rustc fixture\\n'\n")
            compiler.chmod(0o755)
        config = self.root / "cargo-config.json"
        config.write_text('{"build":{"rustc":' + json.dumps(str(first)) + '}}')
        cargo = self.root / "tools/cargo"
        cargo.write_text('''#!/bin/sh
set -eu
if [ "$1" = -Z ]; then cat "$TEST_CONFIG"; exit 0; fi
[ "$1" != clean ] || exit 0
printf '%s' "$TEST_CHANGED_CONFIG" > "$TEST_CONFIG"
mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release"
printf library > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
''')
        environment = dict(self.env, TEST_CONFIG=str(config), TEST_CHANGED_CONFIG='{"build":{"rustc":' + json.dumps(str(second)) + '}}')
        environment.pop("RUSTC", None)
        environment.pop("CARGO_BUILD_RUSTC", None)
        result = subprocess.run([str(self.root / "scripts/build-rust-fontconfig-linux.sh")], env=environment, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("compiler changed", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def test_missing_selected_compiler_does_not_publish_success(self):
        result = self.build(RUSTC=str(self.root / "missing-rustc"))
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("compiler cannot be resolved", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def test_compiler_without_version_does_not_publish_success(self):
        compiler = self.root / "tools/rustc-fixture"
        compiler.write_text("#!/bin/sh\nexit 0\n")
        compiler.chmod(0o755)
        result = self.build(RUSTC=str(compiler))
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("compiler cannot be resolved", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def test_compiler_version_is_selected_from_copied_source(self):
        compiler = self.root / "tools/rustc-fixture"
        compiler.write_text('#!/bin/sh\npwd > "$TEST_COMPILER_CWD"\nprintf "rustc fixture\\n"\n')
        compiler.chmod(0o755)
        recorded = self.root / "compiler-cwd"
        result = self.build(RUSTC=str(compiler), TEST_COMPILER_CWD=str(recorded))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(recorded.read_text().strip(), str(self.root / ".work/tmp/rust-fontconfig/rust-fontconfig-5.0.2-laputa.2"))

    def test_missing_compiler_working_directory_rebuilds(self):
        first = self.build()
        self.assertEqual(first.returncode, 0, first.stderr)
        tree = self.root / ".work/tmp/rust-fontconfig/rust-fontconfig-5.0.2-laputa.2"
        shutil.rmtree(tree)
        second = self.build()
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(self.record.read_text().splitlines(), ["build", "build"])

    def test_selected_compiler_changed_during_build_does_not_publish_success(self):
        compiler = self.root / "tools/rustc-fixture"
        compiler.write_text("#!/bin/sh\nprintf 'rustc fixture\\n'\n")
        compiler.chmod(0o755)
        cargo = self.root / "tools/cargo"
        cargo.write_text('''#!/bin/sh
set -eu
[ "$1" != clean ] || exit 0
printf '#!/bin/sh\nexit 1\n' > "$RUSTC"
mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release"
printf library > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
''')
        result = self.build(RUSTC=str(compiler))
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("compiler changed", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def check_real_rustc_version_change(self, *, selected):
        environment, command = self.real_cargo_fixture()
        (self.source / "src/lib.rs").write_text('#[unsafe(no_mangle)]\npub extern "C" fn fontconfig_probe() -> u32 { if cfg!(compiler_one) { 1 } else { 2 } }\n')
        directory = self.root / "compiler-tools"
        directory.mkdir()
        compiler = directory / "rustc"
        compiler.write_text('#!/bin/sh\nif [ "$1" = -vV ]; then "$TEST_REAL_RUSTC" "$@"; cat "$TEST_VERSION"; else exec "$TEST_REAL_RUSTC" --cfg "$(cat "$TEST_VERSION")" "$@"; fi\n')
        compiler.chmod(0o755)
        selector = self.root / "compiler-version"
        environment.update(TEST_REAL_RUSTC=shutil.which("rustc"), TEST_VERSION=str(selector))
        environment.pop("RUSTC", None)
        environment.pop("CARGO_BUILD_RUSTC", None)
        if selected:
            environment["RUSTC"] = str(compiler)
        else:
            environment["PATH"] = str(directory) + os.pathsep + environment["PATH"]
        installed = self.prefix / "lib/librust_fontconfig.so.5.0.2"
        probe = 'import ctypes, sys; lib = ctypes.CDLL(sys.argv[1]); print(lib.fontconfig_probe())'
        for flag, expected in (("compiler_one", "1"), ("compiler_two", "2")):
            with self.subTest(flag=flag):
                selector.write_text(flag + "\n")
                result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr)
                value = subprocess.run([sys.executable, "-c", probe, str(installed)],
                                       capture_output=True, text=True, timeout=30)
                self.assertEqual(value.returncode, 0, value.stderr)
                self.assertEqual(value.stdout.strip(), expected, result.stdout + result.stderr)

    def test_real_cargo_rebuilds_when_default_rustc_version_changes(self):
        self.check_real_rustc_version_change(selected=False)

    def test_real_cargo_rebuilds_when_selected_rustc_version_changes(self):
        self.check_real_rustc_version_change(selected=True)

    def test_real_cargo_rebuilds_when_cargo_selected_rustc_changes(self):
        environment, command = self.real_cargo_fixture()
        (self.source / "src/lib.rs").write_text('#[unsafe(no_mangle)]\npub extern "C" fn fontconfig_probe() -> u32 { if cfg!(compiler_one) { 1 } else { 2 } }\n')
        compiler = self.root / "rustc"
        compiler.write_text("")
        compiler.chmod(0o755)
        environment.update(CARGO_BUILD_RUSTC=str(compiler), TEST_REAL_RUSTC=shutil.which("rustc"))
        environment.pop("RUSTC", None)
        installed = self.prefix / "lib/librust_fontconfig.so.5.0.2"
        probe = 'import ctypes, sys; lib = ctypes.CDLL(sys.argv[1]); print(lib.fontconfig_probe())'
        for flag, expected in (("compiler_one", "1"), ("compiler_two", "2")):
            with self.subTest(flag=flag):
                compiler.write_text(f'#!/bin/sh\nexec "$TEST_REAL_RUSTC" --cfg {flag} "$@"\n')
                result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr)
                value = subprocess.run([sys.executable, "-c", probe, str(installed)],
                                       capture_output=True, text=True, timeout=30)
                self.assertEqual(value.returncode, 0, value.stderr)
                self.assertEqual(value.stdout.strip(), expected, result.stdout + result.stderr)

    def check_real_cargo_configured_rustc_changes(self, *, relative):
        environment, command = self.real_cargo_fixture()
        (self.source / "src/lib.rs").write_text('#[unsafe(no_mangle)]\npub extern "C" fn fontconfig_probe() -> u32 { if cfg!(compiler_one) { 1 } else { 2 } }\n')
        compiler = self.root / "configured # tools/rustc"
        compiler.parent.mkdir()
        compiler.write_text('#!/bin/sh\nexec "$TEST_REAL_RUSTC" --cfg compiler_one "$@"\n')
        compiler.chmod(0o755)
        configuration = self.root / ".cargo/config.toml"
        configuration.parent.mkdir()
        selected = "configured # tools/rustc" if relative else str(compiler)
        configuration.write_text(f'[build]\nrustc = "{selected}"\n')
        environment.update(TEST_REAL_RUSTC=shutil.which("rustc"))
        environment.pop("RUSTC", None)
        environment.pop("CARGO_BUILD_RUSTC", None)
        installed = self.prefix / "lib/librust_fontconfig.so.5.0.2"
        probe = 'import ctypes, sys; lib = ctypes.CDLL(sys.argv[1]); print(lib.fontconfig_probe())'
        for flag, expected in (("compiler_one", "1"), ("compiler_two", "2")):
            with self.subTest(flag=flag):
                compiler.write_text(f'#!/bin/sh\nexec "$TEST_REAL_RUSTC" --cfg {flag} "$@"\n')
                result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr)
                value = subprocess.run([sys.executable, "-c", probe, str(installed)], capture_output=True, text=True, timeout=30)
                self.assertEqual(value.returncode, 0, value.stderr)
                self.assertEqual(value.stdout.strip(), expected, result.stdout + result.stderr)

    def test_real_cargo_rebuilds_when_configured_rustc_changes(self):
        self.check_real_cargo_configured_rustc_changes(relative=False)

    def test_real_cargo_rebuilds_when_relative_configured_rustc_changes(self):
        self.check_real_cargo_configured_rustc_changes(relative=True)

    def test_real_cargo_rustc_overrides_cargo_build_rustc(self):
        environment, command = self.real_cargo_fixture()
        compiler = self.root / "rustc"
        compiler.write_text('#!/bin/sh\nexec "$TEST_REAL_RUSTC" "$@"\n')
        compiler.chmod(0o755)
        environment.update(RUSTC=str(compiler), CARGO_BUILD_RUSTC="/missing/first-compiler",
                           TEST_REAL_RUSTC=shutil.which("rustc"))
        first = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
        self.assertEqual(first.returncode, 0, first.stderr)
        environment["CARGO_BUILD_RUSTC"] = "/missing/second-compiler"
        second = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertIn("already in", second.stdout)

    def test_real_cargo_rebuilds_when_the_selected_linker_changes(self):
        environment, command = self.real_cargo_fixture()
        linker = self.root / "linker"
        linker.write_text('#!/bin/sh\nprintf "one\\n" >> "$TEST_LINK_LOG"\nexec "$TEST_REAL_LINKER" "$@"\n')
        linker.chmod(0o755)
        log = self.root / "link-log"
        environment.update(CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=str(linker),
                           TEST_REAL_LINKER=str(ROOT / "docker/linux/musl-cc-wrap.sh"), TEST_LINK_LOG=str(log))
        first = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertIn("one", log.read_text().splitlines())
        linker.write_text('#!/bin/sh\nprintf "two\\n" >> "$TEST_LINK_LOG"\nexec "$TEST_REAL_LINKER" "$@"\n')
        second = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertIn("two", log.read_text().splitlines())
        before = log.read_bytes()
        third = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
        self.assertEqual(third.returncode, 0, third.stderr)
        self.assertIn("already in", third.stdout)
        self.assertEqual(log.read_bytes(), before)

    def test_real_cargo_honors_encoded_linker_override(self):
        environment, command = self.real_cargo_fixture()
        environment.update(CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="/missing/configured",
                           CARGO_ENCODED_RUSTFLAGS=f"-C\x1flinker={ROOT / 'docker/linux/musl-cc-wrap.sh'}")
        result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.prefix / "lib/librust_fontconfig.so.5.0.2").read_bytes()[:4], b"\x7fELF")

    def test_real_cargo_rebuilds_when_release_panic_policy_changes(self):
        environment, command = self.real_cargo_fixture()
        (self.source / "src/lib.rs").write_text('#[unsafe(no_mangle)]\npub extern "C" fn fontconfig_probe() -> u32 { if cfg!(panic = "abort") { 1 } else { 2 } }\n')
        installed = self.prefix / "lib/librust_fontconfig.so.5.0.2"
        probe = 'import ctypes, sys; lib = ctypes.CDLL(sys.argv[1]); print(lib.fontconfig_probe())'
        for policy, expected in (("abort", "1"), ("unwind", "2")):
            with self.subTest(policy=policy):
                environment["CARGO_PROFILE_RELEASE_PANIC"] = policy
                result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr)
                value = subprocess.run([sys.executable, "-c", probe, str(installed)],
                                       capture_output=True, text=True, timeout=30)
                self.assertEqual(value.returncode, 0, value.stderr)
                self.assertEqual(value.stdout.strip(), expected)

    def test_overridden_profile_environment_does_not_invalidate_reuse(self):
        first = {"CARGO_PROFILE_RELEASE_LTO": "off", "CARGO_PROFILE_RELEASE_OPT_LEVEL": "0",
                 "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": "16"}
        second = {"CARGO_PROFILE_RELEASE_LTO": "thin", "CARGO_PROFILE_RELEASE_OPT_LEVEL": "2",
                  "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": "8"}
        self.assertEqual(self.build(**first).returncode, 0)
        result = self.build(**second)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already in", result.stdout)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])

    def test_profile_environment_order_does_not_invalidate_reuse(self):
        first = dict(self.env, CARGO_PROFILE_RELEASE_PANIC="abort", CARGO_PROFILE_RELEASE_DEBUG="0")
        second = dict(reversed(list(first.items())))
        command = [str(self.root / "scripts/build-rust-fontconfig-linux.sh")]
        result = subprocess.run(command, env=first, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run(command, env=second, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already in", result.stdout)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])

    def test_encoded_flags_keep_required_policy_and_invalidate_reuse(self):
        first = "-C\x1fdebuginfo=1"
        result = self.build(CARGO_ENCODED_RUSTFLAGS=first)
        self.assertEqual(result.returncode, 0, result.stderr)
        flags = (self.root / "flags").read_text().split("\x1f")
        self.assertEqual(flags[:2], ["-C", "debuginfo=1"])
        self.assertIn("target-feature=-crt-static", flags)
        self.assertIn("link-arg=-static-libgcc", flags)
        self.assertIn("link-arg=-Wl,--as-needed", flags)
        self.assertIn("link-arg=-Wl,-soname,librust_fontconfig.so.5.0.2", flags)
        self.assertEqual(self.build(CARGO_ENCODED_RUSTFLAGS=first).returncode, 0)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])
        self.assertEqual(self.build(CARGO_ENCODED_RUSTFLAGS="-C\x1fdebuginfo=2").returncode, 0)
        self.assertEqual(self.record.read_text().splitlines(), ["build", "build"])

    def test_configured_linker_path_and_contents_invalidate_reuse(self):
        linker = self.root / "tools/linker"
        linker.write_text("#!/bin/sh\nexit 0\n")
        linker.chmod(0o755)
        environment = {"CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER": str(linker)}
        result = self.build(**environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.build(**environment).returncode, 0)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])
        linker.write_text("#!/bin/sh\nexit 1\n")
        result = self.build(**environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.record.read_text().splitlines(), ["build", "build"])
        second = self.root / "tools/second-linker"
        shutil.copy2(linker, second)
        result = self.build(CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=str(second))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.record.read_text().splitlines(), ["build", "build", "build"])

    def test_changed_linker_during_build_publishes_no_success(self):
        linker = self.root / "tools/linker"
        linker.write_text("#!/bin/sh\nexit 0\n")
        linker.chmod(0o755)
        cargo = self.root / "tools/cargo"
        cargo.write_text('''#!/bin/sh
set -eu
[ "$1" != clean ] || exit 0
printf '#!/bin/sh\\nexit 1\\n' > "$TEST_LINKER"
mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release"
printf library > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so"
''')
        result = self.build(CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=str(linker), TEST_LINKER=str(linker))
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("linker changed during build", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def test_relative_linker_in_local_source_can_be_reused(self):
        linker = self.source / "fixture-linker"
        linker.write_text("#!/bin/sh\nexit 0\n")
        linker.chmod(0o755)
        environment = {"CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER": "./fixture-linker"}
        first = self.build(**environment)
        self.assertEqual(first.returncode, 0, first.stderr)
        second = self.build(**environment)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertIn("already in", second.stdout)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])

    def test_path_selected_linker_changes_invalidate_reuse(self):
        first_directory = self.root / "tools"
        second_directory = self.root / "other-tools"
        second_directory.mkdir()
        for directory, value in ((first_directory, "first"), (second_directory, "second")):
            linker = directory / "fixture-linker"
            linker.write_text(f"#!/bin/sh\nprintf {value}\n")
            linker.chmod(0o755)
        environment = {"CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER": "fixture-linker"}
        self.assertEqual(self.build(**environment).returncode, 0)
        self.assertEqual(self.build(**environment).returncode, 0)
        environment["PATH"] = str(second_directory) + os.pathsep + self.env["PATH"]
        result = self.build(**environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.record.read_text().splitlines(), ["build", "build"])

    def test_missing_configured_linker_does_not_publish_success(self):
        result = self.build(CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=str(self.root / "missing-linker"))
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("linker cannot be resolved", result.stderr)
        self.assertFalse((self.prefix / ".input").exists())

    def test_encoded_linker_overrides_the_configured_linker(self):
        linker = self.root / "tools/linker"
        linker.write_text("#!/bin/sh\nexit 0\n")
        linker.chmod(0o755)
        encoded = f"-Clinker=/missing/first\x1f-C\x1flinker={linker}"
        environment = {"CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER": "/missing/configured",
                       "CARGO_ENCODED_RUSTFLAGS": encoded}
        first = self.build(**environment)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(self.build(**environment).returncode, 0)
        linker.write_text("#!/bin/sh\nexit 1\n")
        changed = self.build(**environment)
        self.assertEqual(changed.returncode, 0, changed.stderr)
        self.assertEqual(self.record.read_text().splitlines(), ["build", "build"])

    def test_empty_encoded_flags_keep_required_policy(self):
        result = self.build(CARGO_ENCODED_RUSTFLAGS="")
        self.assertEqual(result.returncode, 0, result.stderr)
        flags = (self.root / "flags").read_text().split("\x1f")
        self.assertIn("target-feature=-crt-static", flags)
        self.assertIn("link-arg=-static-libgcc", flags)
        self.assertIn("link-arg=-Wl,-soname,librust_fontconfig.so.5.0.2", flags)

    def test_absent_and_empty_encoded_flags_reuse_the_same_build(self):
        environment = dict(self.env)
        environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
        first = subprocess.run([str(self.root / "scripts/build-rust-fontconfig-linux.sh")],
                               env=environment, capture_output=True, text=True)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(self.build(CARGO_ENCODED_RUSTFLAGS="").returncode, 0)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])

    def test_failed_install_does_not_publish_a_success_marker(self):
        self.assertEqual(self.build().returncode, 0)
        (self.source / "ffi/rust_fontconfig.h").write_text("changed source\n")
        result = self.build(TEST_REMOVE_HEADER="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.prefix / ".input").exists())
        self.assertEqual(self.build().returncode, 0)
        self.assertEqual((self.prefix / "include/rust-fontconfig/rust_fontconfig.h").read_text(), "changed source\n")

    def test_missing_output_and_changed_mode_trigger_rebuilds(self):
        self.assertEqual(self.build().returncode, 0)
        header = self.prefix / "include/rust-fontconfig/rust_fontconfig.h"
        header.unlink()
        self.assertEqual(self.build().returncode, 0)
        header.chmod(0o600)
        self.assertEqual(self.build().returncode, 0)
        self.assertEqual(header.stat().st_mode & 0o777, 0o644)
        self.assertEqual(len(self.record.read_text().splitlines()), 3)

    def test_unowned_stale_products_fail_without_modification(self):
        self.assertEqual(self.build().returncode, 0)
        for relative in ("lib/librust_fontconfig.so.4.0.0", "lib/librust_fontconfig.a",
                         "include/rust-fontconfig/old.h", "lib/pkgconfig/rust-fontconfig-uninstalled.pc"):
            with self.subTest(product=relative):
                stale = self.prefix / relative
                stale.write_text("leave alone\n")
                try:
                    result = self.build()
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    self.assertIn("unowned rust-fontconfig", result.stderr)
                    self.assertEqual(stale.read_text(), "leave alone\n")
                finally:
                    stale.unlink(missing_ok=True)
                self.assertEqual(self.build().returncode, 0)

    def test_symlinked_owned_product_does_not_modify_its_target(self):
        self.assertEqual(self.build().returncode, 0)
        product = self.prefix / "lib/librust_fontconfig.so.5.0.2"
        outside = self.root / "outside"
        outside.write_text("leave alone\n")
        product.unlink()
        product.symlink_to(outside)
        result = self.build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(outside.read_text(), "leave alone\n")
        self.assertFalse(product.is_symlink())

    def test_symlinked_output_directory_fails_without_external_writes(self):
        self.assertEqual(self.build().returncode, 0)
        directory = self.prefix / "lib/pkgconfig"
        shutil.rmtree(directory)
        outside = self.root / "outside-directory"
        outside.mkdir()
        product = outside / "rust-fontconfig.pc"
        product.write_text("leave alone\n")
        directory.symlink_to(outside, target_is_directory=True)
        result = self.build()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(product.read_text(), "leave alone\n")
        self.assertFalse((self.prefix / ".input").exists())

    def test_unowned_custom_prefix_is_not_claimed(self):
        (self.prefix / "lib").mkdir(parents=True)
        product = self.prefix / "lib/librust_fontconfig.so.5.0.2"
        product.write_text("another installation\n")
        marker = self.prefix / ".input"
        marker.write_text("0" * 64 + "\n")
        result = self.build()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(product.read_text(), "another installation\n")
        self.assertEqual(marker.read_text(), "0" * 64 + "\n")
        self.assertFalse((self.prefix / ".rust-fontconfig-managed").exists())
        self.assertEqual({path.name for path in self.prefix.iterdir()}, {"lib", ".input"})
        self.assertEqual({path.name for path in (self.prefix / "lib").iterdir()}, {product.name})

    def test_legacy_default_prefix_is_rebuilt(self):
        prefix = self.root / ".work/deps/rust-fontconfig/x86_64-unknown-linux-musl"
        (prefix / "lib").mkdir(parents=True)
        product = prefix / "lib/librust_fontconfig.so.5.0.2"
        product.write_text("legacy library\n")
        (prefix / ".input").write_text("0" * 64 + "\n")
        result = self.build(RUST_FONTCONFIG_PREFIX="")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(product.read_text(), "library")
        self.assertTrue((prefix / ".rust-fontconfig-managed").is_file())

    def test_symlinked_default_does_not_claim_legacy_products(self):
        outside = self.root / "outside-prefix"
        (outside / "lib").mkdir(parents=True)
        product = outside / "lib/librust_fontconfig.so.5.0.2"
        product.write_text("another installation\n")
        marker = outside / ".input"
        marker.write_text("0" * 64 + "\n")
        default = self.root / ".work/deps/rust-fontconfig/x86_64-unknown-linux-musl"
        default.parent.mkdir(parents=True)
        default.symlink_to(outside, target_is_directory=True)
        result = self.build(RUST_FONTCONFIG_PREFIX="")
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(product.read_text(), "another installation\n")
        self.assertEqual(marker.read_text(), "0" * 64 + "\n")
        self.assertFalse((outside / ".rust-fontconfig-managed").exists())
        self.assertEqual({path.name for path in outside.iterdir()}, {"lib", ".input"})
        self.assertEqual({path.name for path in (outside / "lib").iterdir()}, {product.name})

    def test_symlinked_default_parent_does_not_claim_legacy_products(self):
        outside = self.root / "outside-parent"
        prefix = outside / "x86_64-unknown-linux-musl"
        (prefix / "lib").mkdir(parents=True)
        product = prefix / "lib/librust_fontconfig.so.5.0.2"
        product.write_text("another installation\n")
        marker = prefix / ".input"
        marker.write_text("0" * 64 + "\n")
        default_parent = self.root / ".work/deps/rust-fontconfig"
        default_parent.parent.mkdir(parents=True)
        default_parent.symlink_to(outside, target_is_directory=True)
        result = self.build(RUST_FONTCONFIG_PREFIX="")
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(product.read_text(), "another installation\n")
        self.assertEqual(marker.read_text(), "0" * 64 + "\n")
        self.assertFalse((prefix / ".rust-fontconfig-managed").exists())
        self.assertEqual({path.name for path in prefix.iterdir()}, {"lib", ".input"})
        self.assertEqual({path.name for path in (prefix / "lib").iterdir()}, {product.name})

    def test_missing_default_prefix_does_not_follow_a_symlinked_parent(self):
        outside = self.root / "outside-parent"
        outside.mkdir()
        default_parent = self.root / ".work/deps/rust-fontconfig"
        default_parent.parent.mkdir(parents=True)
        default_parent.symlink_to(outside, target_is_directory=True)
        result = self.build(RUST_FONTCONFIG_PREFIX="")
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(list(outside.iterdir()), [])

    def test_symlinked_default_can_reuse_an_owned_prefix(self):
        self.assertEqual(self.build().returncode, 0)
        default = self.root / ".work/deps/rust-fontconfig/x86_64-unknown-linux-musl"
        default.parent.mkdir(parents=True)
        default.symlink_to(self.prefix, target_is_directory=True)
        result = self.build(RUST_FONTCONFIG_PREFIX="")
        self.assertEqual(result.returncode, 0, result.stderr)
        metadata = (self.prefix / "lib/pkgconfig/rust-fontconfig.pc").read_text()
        self.assertTrue(metadata.startswith(f"prefix={default}\n"))

    def test_unrepresentable_prefixes_fail_before_writing(self):
        for name in ("with space", "with\ttab", "with\nnewline", "with#hash", "with'quote", 'with"quote',
                     "with\\backslash", "with:colon", "with${variable}"):
            with self.subTest(name=name):
                prefix = self.root / name
                result = self.build(RUST_FONTCONFIG_PREFIX=str(prefix))
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn("cannot be represented in rust-fontconfig pkg-config metadata", result.stderr)
                self.assertFalse(prefix.exists())

    def test_non_utf8_prefix_round_trips_through_pkgconfig(self):
        prefix = self.root / os.fsdecode(b"prefix-\xff")
        result = self.build(RUST_FONTCONFIG_PREFIX=str(prefix))
        self.assertEqual(result.returncode, 0, result.stderr)
        contents = (prefix / "lib/pkgconfig/rust-fontconfig.pc").read_bytes()
        self.assertTrue(contents.startswith(b"prefix=" + os.fsencode(prefix) + b"\n"))
        self.assertEqual(self.build(RUST_FONTCONFIG_PREFIX=str(prefix)).returncode, 0)
        self.assertEqual(self.record.read_text().splitlines(), ["build"])

    def test_representable_prefixes_remain_usable(self):
        for name in ("with$dollar", "café"):
            with self.subTest(name=name):
                prefix = self.root / name
                result = self.build(RUST_FONTCONFIG_PREFIX=str(prefix))
                self.assertEqual(result.returncode, 0, result.stderr)
                query = subprocess.run(["pkg-config", "--variable=prefix", "rust-fontconfig"],
                                       env=dict(os.environ, PKG_CONFIG_LIBDIR=str(prefix / "lib/pkgconfig")),
                                       capture_output=True, text=True, check=True)
                self.assertEqual(query.stdout.rstrip("\n"), str(prefix))

    def test_symlink_to_unrepresentable_prefix_fails_without_writes(self):
        target = self.root / "outside path"
        target.mkdir()
        prefix = self.root / "safe-link"
        prefix.symlink_to(target, target_is_directory=True)
        result = self.build(RUST_FONTCONFIG_PREFIX=str(prefix))
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("cannot be represented in rust-fontconfig pkg-config metadata", result.stderr)
        self.assertEqual(list(target.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
