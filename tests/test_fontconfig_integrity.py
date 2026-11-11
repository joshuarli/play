import os
import fcntl
from pathlib import Path
import shutil
import signal
import subprocess
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
                        TEST_RECORD=str(self.record), TEST_FLAGS=str(self.root / "flags"))

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
