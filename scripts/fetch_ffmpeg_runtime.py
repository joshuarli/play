#!/usr/bin/env python3
"""Fetch and validate the pinned FFmpeg runtime.

buildlib provides the verified download and the target check; this file keeps
the runtime contract, the manifest policy and the installed layout.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

import buildlib
from buildlib import locks, targets

PIN_NAME = "ffmpeg-runtime.lock.json"
CONTRACT = 2
PROFILE = "runtime"
RELEASE_PREFIX = "release-"
ASSET_FORMAT = "ffmpeg-runtime-{ffmpeg}-{target}.tar.xz"
MANIFEST = "share/ffmpeg-build/manifest.json"
WHEEL_SUFFIX = ".whl/buildlib/__init__.py"
HEX = "0123456789abcdef"


class PinError(Exception):
    """The pinned runtime lock violates the Player contract."""


class ManifestError(Exception):
    """The extracted runtime manifest violates the pinned contract."""


class LayoutError(Exception):
    """The archive or its installation does not match the pinned layout."""


def verify_provenance() -> None:
    origin = getattr(buildlib, "__file__", "") or ""
    if not origin.endswith(WHEEL_SUFFIX):
        raise PinError(f"buildlib imported from {origin!r}, not the pinned wheel")


def load_pin(path: Path) -> dict:
    try:
        pin = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PinError(f"{PIN_NAME} unreadable: {error}") from error
    if not isinstance(pin, dict):
        raise PinError(f"{PIN_NAME} must be a JSON object")
    if pin.get("contract") != CONTRACT or pin.get("profile") != PROFILE:
        raise PinError(f"{PIN_NAME} must be contract {CONTRACT}, profile {PROFILE}")
    release = pin.get("release")
    commit = release.removeprefix(RELEASE_PREFIX) if isinstance(release, str) else release
    if not isinstance(release, str) or commit == release or len(commit) != 40 or commit.strip(HEX):
        raise PinError(f"{PIN_NAME} release must be release-<40 hex commit>")
    libraries = pin.get("libraries")
    if not isinstance(libraries, list) or not libraries or not all(isinstance(name, str) and name for name in libraries):
        raise PinError(f"{PIN_NAME} libraries must be a nonempty list of names")
    return pin


def pinned_input(pin: dict, target: str) -> tuple[locks.Input, str, str]:
    release = pin["release"]
    targets.target_for_triple(target)
    ffmpeg = pin.get("ffmpeg")
    if not isinstance(ffmpeg, str) or not ffmpeg:
        raise PinError(f"{PIN_NAME} ffmpeg must be a nonempty string")
    asset = ASSET_FORMAT.format(ffmpeg=ffmpeg, target=target)
    digests = pin.get("sha256")
    digest = digests.get(target) if isinstance(digests, dict) else None
    if digest is None:
        raise PinError(f"{PIN_NAME} pins no sha256 for {asset}")
    if not isinstance(digest, str) or len(digest) != 64 or digest.strip(HEX):
        raise PinError(f"{PIN_NAME} sha256 for {asset} must be 64 lowercase hex digits")
    sizes = pin.get("size")
    size = sizes.get(target) if isinstance(sizes, dict) else None
    if size is not None and (type(size) is not int or size < 0):
        raise PinError(f"{PIN_NAME} size for {asset} must be a nonnegative integer")
    repository = pin.get("repository")
    if not isinstance(repository, str) or not repository:
        raise PinError(f"{PIN_NAME} repository must be a nonempty string")
    url = f"https://github.com/{repository}/releases/download/{release}/{asset}"
    item = locks.Input(name=asset, version=ffmpeg, sha256=digest, urls=(url,), size=size)
    return item, release, asset


def check_manifest(pin: dict, target: str, asset: str, root: Path) -> None:
    path = root / MANIFEST
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ManifestError(f"runtime manifest unreadable: {error}") from error
    if not isinstance(manifest, dict):
        raise ManifestError("runtime manifest must be a JSON object")
    wanted = {
        ("profile",): PROFILE,
        ("target",): target,
        ("sources", "ffmpeg", "version"): pin["ffmpeg"],
        ("producer", "commit"): pin["release"].removeprefix(RELEASE_PREFIX),
        ("release", "archive"): asset,
    }
    for keys, value in wanted.items():
        found = manifest
        for key in keys:
            found = found.get(key) if isinstance(found, dict) else None
        if found != value:
            raise ManifestError(f"runtime manifest {'.'.join(keys)} is {found!r}, pin wants {value!r}")
    libraries = manifest.get("libraries")
    if not isinstance(libraries, dict):
        raise ManifestError("runtime manifest libraries must be an object")
    missing = [name for name in pin["libraries"] if name not in libraries]
    if missing:
        raise ManifestError(f"runtime manifest lacks pinned libraries {missing}")


def extract(pin: dict, target: str, asset: str, archive: Path, cache: Path, dest: Path) -> None:
    root_name = asset.removesuffix(".tar.xz")
    staging = cache / f"extract-{target}"
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(parents=True)
    subprocess.run(["tar", "-xJf", str(archive), "-C", str(staging)], check=True)
    entries = sorted(path.name for path in staging.iterdir())
    if entries != [root_name]:
        raise LayoutError(f"{asset} does not contain exactly {root_name}/")
    try:
        check_manifest(pin, target, asset, staging / root_name)
    except ManifestError as error:
        raise LayoutError(f"{asset} disagrees with {PIN_NAME}: {error}") from error
    shutil.rmtree(dest, ignore_errors=True)
    shutil.move(str(staging / root_name), str(dest))
    staging.rmdir()


def run(root: Path, target: str, local_archive: str | None) -> int:
    verify_provenance()
    pin = load_pin(root / PIN_NAME)
    item, release, asset = pinned_input(pin, target)
    dest = root / ".work/deps/ffmpeg-runtime" / target
    cache = root / ".work/tmp/ffmpeg-runtime"
    cache.mkdir(parents=True, exist_ok=True)
    dest.parent.mkdir(parents=True, exist_ok=True)
    if local_archive is None and (dest / MANIFEST).is_file():
        try:
            check_manifest(pin, target, asset, dest)
        except ManifestError:
            pass
        else:
            print(f"FFmpeg runtime {release} for {target} already in {dest}")
            return 0
    if local_archive is not None:
        archive = Path(local_archive)
        if archive.name != asset:
            raise LayoutError(f"{local_archive} is not the pinned archive {asset}")
        if archive.is_symlink():
            archive = archive.resolve()
        locks.verify_file(archive, item)
    else:
        archive = cache / asset
        try:
            locks.verify_file(archive, item)
        except locks.FetchError:
            print(f"download {item.url}")
        archive = locks.fetch_input(item, archive)
    extract(pin, target, asset, archive, cache, dest)
    print(f"FFmpeg runtime {release} for {target} in {dest}")
    return 0


def main(argv: list[str]) -> int:
    if len(argv) not in (3, 4):
        print(f"usage: {Path(argv[0]).name} ROOT TARGET [ARCHIVE]", file=sys.stderr)
        return 2
    root = Path(argv[1]).resolve()
    target = argv[2]
    local_archive = argv[3] if len(argv) == 4 else None
    try:
        return run(root, target, local_archive)
    except (PinError, ManifestError, LayoutError, locks.FetchError, locks.LockError,
            targets.UnsupportedTargetError, OSError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
