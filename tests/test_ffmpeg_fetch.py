import hashlib
import http.server
import json
import os
import shutil
import socket
import ssl
import subprocess
import tarfile
import tempfile
import threading
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BOOTSTRAP = ROOT / "bootstrap" / "buildlib-run"
HELPER = ROOT / "scripts" / "fetch_ffmpeg_runtime.py"
WRAPPER = ROOT / "scripts" / "fetch-ffmpeg-runtime.sh"
CACHE = ROOT / ".work" / "tmp" / "buildlib-test-cache"

TARGET = "x86_64-unknown-linux-musl"
FFMPEG = "9.0.2"
COMMIT = "ac29c7720327f7dee4ed4677ce8421673fcc3563"
RELEASE = f"release-{COMMIT}"
ASSET = f"ffmpeg-runtime-{FFMPEG}-{TARGET}.tar.xz"
STEM = ASSET.removesuffix(".tar.xz")
LIBRARIES = ["libavformat", "libavcodec", "libswscale", "libavutil"]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def make_archive(base, target=TARGET, archive_name=ASSET):
    stem = archive_name.removesuffix(".tar.xz")
    tree = base / "tree" / stem / "share" / "ffmpeg-build"
    tree.mkdir(parents=True)
    manifest = {
        "profile": "runtime",
        "target": target,
        "sources": {"ffmpeg": {"version": FFMPEG}},
        "producer": {"commit": COMMIT},
        "release": {"archive": archive_name},
        "libraries": {name: {"soname": f"{name}.so.0"} for name in LIBRARIES},
    }
    (tree / "manifest.json").write_text(json.dumps(manifest))
    lib = base / "tree" / stem / "lib"
    lib.mkdir()
    (lib / "libavcodec.so.0").write_bytes(b"fixture library\n")
    archive = base / archive_name
    with tarfile.open(archive, "w:xz") as tar:
        tar.add(base / "tree" / stem, arcname=stem)
    return archive


def make_pin(base, archive, *, sha=None, size=None, target=TARGET, repository="fixture/ffmpeg"):
    pin = {
        "contract": 2,
        "repository": repository,
        "profile": "runtime",
        "ffmpeg": FFMPEG,
        "release": RELEASE,
        "libraries": LIBRARIES,
        "sha256": {target: sha or digest(archive)},
    }
    if size is not None:
        pin["size"] = {target: size}
    path = base / "ffmpeg-runtime.lock.json"
    path.write_text(json.dumps(pin))
    return path


def environment(**extra):
    env = {key: value for key, value in os.environ.items() if key not in ("PYTHONPATH", "PYTHONHOME")}
    env["BUILDLIB_CACHE"] = str(CACHE)
    env.pop("BUILDLIB_PYTHON", None)
    env.update(extra)
    return env


def run_helper(base, *arguments, **extra):
    command = [str(BOOTSTRAP), "--", str(HELPER), str(base), *arguments]
    return subprocess.run(command, capture_output=True, text=True, timeout=300, env=environment(**extra))


def warm_bootstrap():
    subprocess.run([str(BOOTSTRAP), "--", "-c", "pass"], capture_output=True, timeout=300, env=environment())


def start_tunnel(server_root, certificate, key):
    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(server_root), **kwargs)

        def log_message(self, *args):
            pass

    tls = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certificate, key)
    tls.socket = context.wrap_socket(tls.socket, server_side=True)
    threading.Thread(target=tls.serve_forever, daemon=True).start()

    proxy = socket.socket()
    proxy.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    proxy.bind(("127.0.0.1", 0))
    proxy.listen(1)

    def tunnel():
        client, _ = proxy.accept()
        request = b""
        while b"\r\n\r\n" not in request:
            chunk = client.recv(4096)
            if not chunk:
                return
            request += chunk
        client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        upstream = socket.create_connection(("127.0.0.1", tls.server_port))

        def pump(source, destination):
            try:
                while chunk := source.recv(65536):
                    destination.sendall(chunk)
            except OSError:
                pass
            finally:
                try:
                    destination.shutdown(socket.SHUT_WR)
                except OSError:
                    pass

        threading.Thread(target=pump, args=(client, upstream), daemon=True).start()
        pump(upstream, client)
        client.close()
        upstream.close()

    threading.Thread(target=tunnel, daemon=True).start()
    return tls, proxy


class FetchTest(unittest.TestCase):
    def setUp(self):
        CACHE.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=ROOT / ".work" / "tmp", prefix="ffmpeg-fetch-")
        self.base = Path(self.directory.name)
        self.destination = self.base / ".work" / "deps" / "ffmpeg-runtime" / TARGET / "share" / "ffmpeg-build" / "manifest.json"

    def tearDown(self):
        self.directory.cleanup()

    def test_installs_the_verified_local_archive_then_reuses_the_cache(self):
        archive = make_archive(self.base)
        make_pin(self.base, archive)
        result = run_helper(self.base, TARGET, str(archive))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.destination.is_file())
        self.assertIn(f"in {self.destination.parents[2]}", result.stdout)

        cache = self.base / ".work" / "tmp" / "ffmpeg-runtime" / ASSET
        shutil.copy2(archive, cache)
        shutil.rmtree(self.destination.parents[2])
        make_pin(self.base, archive, repository="invalid.example")
        result = run_helper(self.base, TARGET)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("download", result.stdout)
        self.assertTrue(self.destination.is_file())

    def test_reuses_an_installed_runtime_without_a_cache(self):
        archive = make_archive(self.base)
        make_pin(self.base, archive)
        self.assertEqual(run_helper(self.base, TARGET, str(archive)).returncode, 0)
        shutil.rmtree(self.base / ".work" / "tmp")
        make_pin(self.base, archive, repository="invalid.example")
        result = run_helper(self.base, TARGET)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already in", result.stdout)

    def test_rejects_an_unsupported_target(self):
        archive = make_archive(self.base)
        pin = json.loads(make_pin(self.base, archive).read_text())
        pin["sha256"]["riscv64-unknown-linux-musl"] = digest(archive)
        (self.base / "ffmpeg-runtime.lock.json").write_text(json.dumps(pin))
        result = run_helper(self.base, "riscv64-unknown-linux-musl", str(archive))
        self.assertEqual(result.returncode, 1)
        self.assertIn("target not supported: 'riscv64-unknown-linux-musl'", result.stderr)

    def test_rejects_a_wrong_digest(self):
        archive = make_archive(self.base)
        make_pin(self.base, archive, sha="0" * 64)
        result = run_helper(self.base, TARGET, str(archive))
        self.assertEqual(result.returncode, 1)
        self.assertIn("sha256", result.stderr)
        self.assertIn("pinned", result.stderr)
        self.assertFalse(self.destination.exists())

    def test_rejects_a_wrong_size(self):
        archive = make_archive(self.base)
        make_pin(self.base, archive, size=archive.stat().st_size + 1)
        result = run_helper(self.base, TARGET, str(archive))
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"size {archive.stat().st_size} != pinned {archive.stat().st_size + 1}", result.stderr)
        self.assertFalse(self.destination.exists())

    def test_rejects_a_truncated_archive(self):
        archive = make_archive(self.base)
        make_pin(self.base, archive)
        data = archive.read_bytes()
        archive.write_bytes(data[: len(data) // 2])
        result = run_helper(self.base, TARGET, str(archive))
        self.assertEqual(result.returncode, 1)
        self.assertIn("sha256", result.stderr)
        self.assertFalse(self.destination.exists())

    def test_rejects_a_truncated_download(self):
        archive = make_archive(self.base)
        make_pin(self.base, archive)
        data = archive.read_bytes()
        base = self.base
        server_root = base / "server"
        target = server_root / "laputa-systems" / "ffmpeg-build" / "releases" / "download" / RELEASE / ASSET
        target.parent.mkdir(parents=True)
        target.write_bytes(data[: len(data) // 2])

        openssl = shutil.which("openssl")
        if not openssl:
            self.skipTest("requires openssl to generate a test certificate")
        certificate = base / "cert.pem"
        key = base / "key.pem"
        subprocess.run(
            [openssl, "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(key),
             "-out", str(certificate), "-days", "1", "-subj", "/CN=github.com",
             "-addext", "subjectAltName=DNS:github.com"],
            check=True, capture_output=True,
        )

        warm_bootstrap()
        tls, proxy = start_tunnel(server_root, certificate, key)
        try:
            make_pin(self.base, archive, repository="laputa-systems/ffmpeg-build")
            result = run_helper(
                self.base, TARGET, SSL_CERT_FILE=str(certificate),
                https_proxy=f"http://127.0.0.1:{proxy.getsockname()[1]}",
                HTTPS_PROXY=f"http://127.0.0.1:{proxy.getsockname()[1]}",
                no_proxy="", NO_PROXY="",
            )
        finally:
            proxy.close()
            tls.shutdown()
            tls.server_close()
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("no URL served the pinned bytes", result.stderr)
        self.assertIn("sha256", result.stderr)
        self.assertFalse(self.destination.exists())
        self.assertFalse((self.base / ".work" / "tmp" / "ffmpeg-runtime" / ASSET).exists())

    def test_rejects_a_manifest_that_disagrees_with_the_pin(self):
        archive = make_archive(self.base, target="aarch64-apple-darwin")
        make_pin(self.base, archive)
        result = run_helper(self.base, TARGET, str(archive))
        self.assertEqual(result.returncode, 1)
        self.assertIn("disagrees with ffmpeg-runtime.lock.json", result.stderr)
        self.assertFalse(self.destination.exists())

    def test_rejects_an_archive_with_the_wrong_name(self):
        archive = make_archive(self.base, archive_name="ffmpeg-runtime-8.0.0-x86_64-unknown-linux-musl.tar.xz")
        make_pin(self.base, archive)
        result = run_helper(self.base, TARGET, str(archive))
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"is not the pinned archive {ASSET}", result.stderr)

    def test_wrapper_preserves_usage_and_exit_code(self):
        result = subprocess.run([str(WRAPPER)], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 2)
        self.assertIn("usage: ", result.stderr)


if __name__ == "__main__":
    unittest.main()
