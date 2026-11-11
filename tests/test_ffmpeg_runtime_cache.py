import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tarfile
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-unknown-linux-musl"
ASSET = f"ffmpeg-9.0.2+fixture-{TARGET}-runtime.tar.gz"


class FFmpegRuntimeCacheTest(unittest.TestCase):
    def setUp(self):
        scratch = ROOT / ".work/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=scratch, prefix="ffmpeg-cache-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "scripts").mkdir()
        for name in ("fetch-ffmpeg-runtime.sh", "verify-ffmpeg-runtime.py"):
            shutil.copy2(ROOT / "scripts" / name, self.root / "scripts" / name)
        self.destination = self.root / ".work/deps/ffmpeg-runtime" / TARGET
        cache = self.root / ".work/tmp/ffmpeg-runtime"
        cache.mkdir(parents=True)
        archive = cache / ASSET
        root = ASSET.removesuffix(".tar.gz")
        manifest = {"profile": "runtime", "target": TARGET, "sources": {"ffmpeg": {"version": "9.0.2"}},
                    "release": {"tag": "fixture", "archive": ASSET}, "libraries": {"libavcodec": {}}}
        files = {"include/fixture.h": b"original header\n", "lib/libfixture.so.1": b"original library\n",
                 "share/ffmpeg-build/manifest.json": json.dumps(manifest).encode()}
        with tarfile.open(archive, "w:gz") as tar:
            for directory in ("", "include", "lib", "share", "share/ffmpeg-build"):
                entry = tarfile.TarInfo(root + ("/" + directory if directory else ""))
                entry.type = tarfile.DIRTYPE
                entry.mode = 0o755
                tar.addfile(entry)
            for name, data in files.items():
                entry = tarfile.TarInfo(root + "/" + name)
                entry.mode = 0o644
                entry.size = len(data)
                tar.addfile(entry, io.BytesIO(data))
            entry = tarfile.TarInfo(root + "/lib/libfixture.so")
            entry.type = tarfile.SYMTYPE
            entry.linkname = "libfixture.so.1"
            tar.addfile(entry)
        pin = {"contract": 2, "profile": "runtime", "ffmpeg": "9.0.2", "release": "fixture", "repository": "unused/fixture",
               "sha256": {TARGET: hashlib.sha256(archive.read_bytes()).hexdigest()}, "libraries": ["libavcodec"]}
        (self.root / "ffmpeg-runtime.lock.json").write_text(json.dumps(pin))

    def fetch(self):
        return subprocess.run([str(self.root / "scripts/fetch-ffmpeg-runtime.sh"), TARGET], capture_output=True, text=True)

    def test_corrupted_header_is_repaired_before_reuse(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        header = self.destination / "include/fixture.h"
        header.write_bytes(b"corrupted header\n")
        second = self.fetch()
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(header.read_bytes(), b"original header\n")

    def test_restored_runtime_is_validated_before_reuse(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        restored = self.root / "restored"
        shutil.copytree(self.destination, restored, symlinks=True)
        shutil.rmtree(self.destination)
        shutil.copytree(restored, self.destination, symlinks=True)
        header = self.destination / "include/fixture.h"
        header.write_bytes(b"restored corruption\n")
        result = self.fetch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(header.read_bytes(), b"original header\n")
        self.assertIn("cache invalid", result.stdout)

    def test_missing_corrupted_and_extra_products_are_repaired(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        library = self.destination / "lib/libfixture.so.1"
        link = self.destination / "lib/libfixture.so"
        outside = self.root / "outside"
        outside.write_bytes(b"leave alone\n")
        mutations = ("missing", "library", "symlink")
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                if mutation == "missing":
                    library.unlink()
                elif mutation == "library":
                    original = library.read_bytes()
                    metadata = library.stat()
                    library.write_bytes(bytes([original[0] ^ 1]) + original[1:])
                    os.utime(library, ns=(metadata.st_atime_ns, metadata.st_mtime_ns))
                elif mutation == "symlink":
                    link.unlink()
                    link.symlink_to(outside)
                result = self.fetch()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(library.read_bytes(), b"original library\n")
                self.assertEqual(os.readlink(link), "libfixture.so.1")
                self.assertEqual(outside.read_bytes(), b"leave alone\n")

    def test_unowned_product_is_preserved_and_rejected(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        extra = self.destination / "include/obsolete.h"
        extra.write_bytes(b"leave alone\n")
        result = self.fetch()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("unowned FFmpeg runtime product", result.stderr)
        self.assertEqual(extra.read_bytes(), b"leave alone\n")
        self.assertEqual((self.destination / "include/fixture.h").read_bytes(), b"original header\n")

    def test_unowned_nested_product_survives_a_damaged_root(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        extra = self.destination / "include/extra.h"
        extra.write_bytes(b"leave alone\n")
        self.destination.chmod(0o700)
        result = self.fetch()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("unowned FFmpeg runtime product", result.stderr)
        self.assertEqual(extra.read_bytes(), b"leave alone\n")

    def test_directory_in_place_of_owned_file_is_preserved(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        library = self.destination / "lib/libfixture.so.1"
        library.unlink()
        library.mkdir()
        sentinel = library / "user-data"
        sentinel.write_bytes(b"leave alone\n")
        result = self.fetch()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("unowned FFmpeg runtime product", result.stderr)
        self.assertEqual(sentinel.read_bytes(), b"leave alone\n")

    def test_symlinked_prefix_does_not_modify_its_target(self):
        outside = self.root / "outside-dir"
        outside.mkdir()
        sentinel = outside / "sentinel"
        sentinel.write_bytes(b"leave alone\n")
        self.destination.parent.mkdir(parents=True)
        self.destination.symlink_to(outside, target_is_directory=True)
        result = self.fetch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sentinel.read_bytes(), b"leave alone\n")
        self.assertFalse(self.destination.is_symlink())
        self.assertEqual((self.destination / "include/fixture.h").read_bytes(), b"original header\n")

    def test_unsafe_archive_entries_are_rejected_before_extraction(self):
        archive = self.root / ".work/tmp/ffmpeg-runtime" / ASSET
        root = ASSET.removesuffix(".tar.gz")
        outside = self.root / "outside"
        outside.write_bytes(b"leave alone\n")
        cases = ("traversal", "symlink", "duplicate")
        for case in cases:
            with self.subTest(case=case):
                with tarfile.open(archive, "w:gz") as tar:
                    entry = tarfile.TarInfo(root)
                    entry.type = tarfile.DIRTYPE
                    entry.mode = 0o755
                    tar.addfile(entry)
                    if case == "traversal":
                        entry = tarfile.TarInfo(root + "/../../outside")
                        entry.size = len(b"damage\n")
                        tar.addfile(entry, io.BytesIO(b"damage\n"))
                    elif case == "symlink":
                        entry = tarfile.TarInfo(root + "/escape")
                        entry.type = tarfile.SYMTYPE
                        entry.linkname = "../../outside"
                        tar.addfile(entry)
                    else:
                        entry = tarfile.TarInfo(root + "/duplicate")
                        tar.addfile(entry, io.BytesIO(b""))
                        tar.addfile(entry, io.BytesIO(b""))
                pin_path = self.root / "ffmpeg-runtime.lock.json"
                pin = json.loads(pin_path.read_text())
                pin["sha256"][TARGET] = hashlib.sha256(archive.read_bytes()).hexdigest()
                pin_path.write_text(json.dumps(pin))
                result = self.fetch()
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertEqual(outside.read_bytes(), b"leave alone\n")
                self.assertFalse(self.destination.exists())

    def test_damaged_cached_archive_does_not_validate_an_installed_runtime(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        archive = self.root / ".work/tmp/ffmpeg-runtime" / ASSET
        archive.write_bytes(b"damaged archive")
        tools = self.root / "tools"
        tools.mkdir()
        curl = tools / "curl"
        curl.write_text('#!/bin/sh\nexit 37\n')
        curl.chmod(0o755)
        env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"])
        result = subprocess.run([str(self.root / "scripts/fetch-ffmpeg-runtime.sh"), TARGET], env=env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertNotIn("already in", result.stdout)
        self.assertEqual((self.destination / "include/fixture.h").read_bytes(), b"original header\n")
        self.assertEqual(list(archive.parent.glob(".download-*")), [])

    def test_failed_extraction_can_be_retried(self):
        tools = self.root / "tools"
        tools.mkdir()
        tar = tools / "tar"
        tar.write_text('#!/bin/sh\nexit 37\n')
        tar.chmod(0o755)
        env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"])
        failed = subprocess.run([str(self.root / "scripts/fetch-ffmpeg-runtime.sh"), TARGET], env=env, capture_output=True, text=True)
        self.assertNotEqual(failed.returncode, 0, failed.stdout)
        self.assertFalse(self.destination.exists())
        retry = self.fetch()
        self.assertEqual(retry.returncode, 0, retry.stderr)
        self.assertEqual((self.destination / "include/fixture.h").read_bytes(), b"original header\n")

    def test_killed_fetch_releases_lock_and_recovers_partial_extraction(self):
        tools = self.root / "tools"
        tools.mkdir()
        tar = tools / "tar"
        tar.write_text('#!/bin/sh\nprintf reached > "$TEST_MARKER"\nsleep 30\nexec "$TEST_REAL_TAR" "$@"\n')
        tar.chmod(0o755)
        marker = self.root / "started"
        env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                   TEST_MARKER=str(marker), TEST_REAL_TAR=shutil.which("tar"))
        command = [str(self.root / "scripts/fetch-ffmpeg-runtime.sh"), TARGET]
        process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            deadline = time.monotonic() + 10
            while not marker.exists() and process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(marker.exists(), "fetch did not start extracting")
            os.killpg(process.pid, signal.SIGKILL)
            process.communicate(timeout=10)
            self.assertFalse(self.destination.exists())
            retry = self.fetch()
            self.assertEqual(retry.returncode, 0, retry.stderr)
            self.assertEqual((self.destination / "include/fixture.h").read_bytes(), b"original header\n")
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate()

    def test_explicit_archive_repairs_an_installed_runtime(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        header = self.destination / "include/fixture.h"
        header.write_bytes(b"damaged\n")
        archive = self.root / ".work/tmp/ffmpeg-runtime" / ASSET
        result = subprocess.run([str(self.root / "scripts/fetch-ffmpeg-runtime.sh"), TARGET, str(archive)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(header.read_bytes(), b"original header\n")

    def test_target_traversal_is_rejected(self):
        result = subprocess.run([str(self.root / "scripts/fetch-ffmpeg-runtime.sh"), "../../outside"], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertFalse((self.root / "outside").exists())

    def test_concurrent_fetches_publish_one_complete_runtime(self):
        tools = self.root / "tools"
        tools.mkdir()
        tar = tools / "tar"
        tar.write_text('#!/bin/sh\nprintf "extract\\n" >> "$TEST_RECORD"\nsleep 1\nexec "$TEST_REAL_TAR" "$@"\n')
        tar.chmod(0o755)
        record = self.root / "extracts"
        env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                   TEST_RECORD=str(record), TEST_REAL_TAR=shutil.which("tar"))
        command = [str(self.root / "scripts/fetch-ffmpeg-runtime.sh"), TARGET]
        first = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        second = None
        try:
            deadline = time.monotonic() + 10
            while not record.exists() and first.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(record.exists(), "first fetch did not start extracting")
            second = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            first_output = first.communicate(timeout=15)
            second_output = second.communicate(timeout=15)
            self.assertEqual(first.returncode, 0, first_output)
            self.assertEqual(second.returncode, 0, second_output)
            self.assertEqual(record.read_text().splitlines(), ["extract"])
            self.assertEqual((self.destination / "include/fixture.h").read_bytes(), b"original header\n")
        finally:
            for process in (first, second):
                if process is not None and process.poll() is None:
                    process.kill()
                    process.communicate()

    def test_unchanged_runtime_is_reused(self):
        first = self.fetch()
        self.assertEqual(first.returncode, 0, first.stderr)
        header = self.destination / "include/fixture.h"
        before = header.stat().st_mtime_ns
        second = self.fetch()
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertIn("already in", second.stdout)
        self.assertEqual(header.stat().st_mtime_ns, before)


if __name__ == "__main__":
    unittest.main()
