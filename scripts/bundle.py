#!/usr/bin/env python3
"""Assemble Player.app from the release build and its pinned dependency prefix."""

import hashlib
import json
import os
import platform
import plistlib
import shutil
import stat
import subprocess
import sys
from pathlib import Path

ROOT = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
PREFIX = ROOT / ".work/deps/release"
RUST_TARGET = ROOT / "target/dist"
BINARY = RUST_TARGET / "player"
APP = ROOT / "dist/Player.app"
MACOS = APP / "Contents/MacOS"
FRAMEWORKS = APP / "Contents/Frameworks"
RESOURCES = APP / "Contents/Resources"
ICD_DIR = RESOURCES / "vulkan/icd.d"
MOLTENVK_LIB = PREFIX / "lib/libMoltenVK.dylib"
MOLTENVK_ICD = PREFIX / "share/vulkan/icd.d/MoltenVK_icd.json"
TARGET = "aarch64-apple-darwin"
PIN = json.loads((ROOT / "ffmpeg-runtime.lock.json").read_text())
RUNTIME = Path(os.environ.get("FFMPEG_PREFIX") or ROOT / ".work/deps/ffmpeg-runtime" / TARGET)
RUNTIME_LINK = ROOT / "dist/lib"


def remove_generated_linux_runtime_links():
    linux_target = "x86_64-unknown-linux-musl"
    runtime = ROOT / ".work/deps/ffmpeg-runtime" / linux_target / "lib"
    fontconfig = ROOT / ".work/deps/rust-fontconfig" / linux_target / "lib"
    if not runtime.is_dir() or not fontconfig.is_dir():
        return False
    runtime_sources = [path for path in runtime.iterdir() if path.exists()]
    fontconfig_sources = [path for path in fontconfig.glob("librust_fontconfig.so*") if path.exists()]
    if not runtime_sources or not fontconfig_sources:
        return False
    expected = {path.name: os.path.relpath(path, RUNTIME_LINK) for path in runtime_sources + fontconfig_sources}
    entries = list(RUNTIME_LINK.iterdir())
    if {path.name for path in entries} != set(expected):
        return False
    if any(not path.is_symlink() or os.readlink(path) != expected[path.name] for path in entries):
        return False
    for path in entries:
        path.unlink()
    RUNTIME_LINK.rmdir()
    return True


def runtime_install_names():
    manifest = json.loads((RUNTIME / "share/ffmpeg-build/manifest.json").read_text())
    return {manifest["libraries"][name]["install_name"] for name in PIN["libraries"]}


def package_version():
    for line in (ROOT / "crates/player-app/Cargo.toml").read_text().splitlines():
        key, separator, value = line.partition("=")
        if separator and key.strip() == "version":
            return value.strip().strip('"')
    raise SystemExit("player-app/Cargo.toml has no version")


def otool(path, flag):
    return subprocess.check_output(["otool", flag, str(path)], text=True)


def dependencies(path):
    result = []
    for line in otool(path, "-L").splitlines()[1:]:
        line = line.strip()
        if line:
            result.append(line.split(" (compatibility")[0].strip())
    return result


def rpaths_of(path):
    result = []
    lines = otool(path, "-l").splitlines()
    for index, line in enumerate(lines):
        if line.strip() == "cmd LC_RPATH":
            for detail in lines[index + 1:index + 4]:
                detail = detail.strip()
                if detail.startswith("path "):
                    result.append(detail[len("path "):].split(" (offset")[0])
    return result


def resolve(dep):
    if dep.startswith("@rpath/"):
        name = dep[len("@rpath/"):]
        candidate = PREFIX / "lib" / name
        if candidate.exists():
            return candidate
        return RUST_TARGET / name
    if dep.startswith("@loader_path/"):
        return PREFIX / "lib" / dep[len("@loader_path/"):]
    if dep.startswith(str(PREFIX)):
        return Path(dep)
    return None


def collect(binary, external):
    closure = {}
    queue = [binary]
    while queue:
        current = queue.pop()
        for dep in dependencies(current):
            if dep.startswith("/opt/homebrew"):
                raise SystemExit(f"Homebrew dependency in {current}: {dep}")
            if dep in external:
                continue
            resolved = resolve(dep)
            if resolved is None:
                continue
            if not resolved.exists():
                raise SystemExit(f"missing dependency {dep} for {current}")
            if resolved.name not in closure:
                closure[resolved.name] = resolved
                queue.append(resolved)
    return closure


def main():
    if not BINARY.exists():
        raise SystemExit(f"missing release binary: {BINARY}")
    version = package_version()
    for path in (MOLTENVK_LIB, MOLTENVK_ICD):
        if not path.exists():
            raise SystemExit(f"missing dependency {path}")

    if APP.exists():
        shutil.rmtree(APP)
    MACOS.mkdir(parents=True)
    FRAMEWORKS.mkdir(parents=True)
    ICD_DIR.mkdir(parents=True)

    executable = MACOS / "player"
    shutil.copy2(BINARY, executable)

    ffmpeg = runtime_install_names()
    linked_ffmpeg = {dep for dep in dependencies(executable) if dep in ffmpeg or "/libav" in dep or "/libsw" in dep}
    if linked_ffmpeg != ffmpeg:
        raise SystemExit(f"player links FFmpeg {sorted(linked_ffmpeg)}, the pin wants {sorted(ffmpeg)}")
    closure = collect(executable, ffmpeg)
    for name, source in closure.items():
        shutil.copy2(source, FRAMEWORKS / name)

    copied = [executable] + [FRAMEWORKS / name for name in closure]
    modes = {path: stat.S_IMODE(path.stat().st_mode) for path in copied}
    for path, mode in modes.items():
        if not mode & stat.S_IWUSR:
            path.chmod(mode | stat.S_IWUSR)

    subprocess.run(["strip", "-x", str(executable)], check=True)
    for name in closure:
        subprocess.run(["strip", "-x", str(FRAMEWORKS / name)], check=True)

    moltenvk = FRAMEWORKS / "libMoltenVK.dylib"
    shutil.copy2(MOLTENVK_LIB, moltenvk)
    thin = FRAMEWORKS / "libMoltenVK.arm64.dylib"
    subprocess.run(
        ["lipo", "-thin", platform.machine(), str(moltenvk), "-output", str(thin)],
        check=True,
    )
    thin.replace(moltenvk)
    modes.pop(moltenvk, None)
    subprocess.run(["strip", "-x", str(moltenvk)], check=True)
    icd = json.loads(MOLTENVK_ICD.read_text())
    icd["ICD"]["library_path"] = "../../../Frameworks/libMoltenVK.dylib"
    (ICD_DIR / "MoltenVK_icd.json").write_text(json.dumps(icd, indent=4) + "\n")

    path_map = {}
    for name, source in closure.items():
        path_map[str(source)] = f"@rpath/{name}"
        path_map[f"@rpath/{name}"] = f"@rpath/{name}"
        path_map[f"@loader_path/{name}"] = f"@rpath/{name}"

    targets = [executable] + [FRAMEWORKS / name for name in closure]
    for target in targets:
        if target != executable:
            subprocess.run(
                ["install_name_tool", "-id", f"@rpath/{target.name}", str(target)],
                check=True,
            )
        for dep in dependencies(target):
            resolved = resolve(dep)
            new = path_map.get(dep) or (
                path_map.get(str(resolved)) if resolved else None
            )
            if new and new != dep:
                subprocess.run(
                    ["install_name_tool", "-change", dep, new, str(target)],
                    check=True,
                )

    for rpath in rpaths_of(executable):
        if not rpath.startswith("@"):
            subprocess.run(
                ["install_name_tool", "-delete_rpath", rpath, str(executable)],
                check=True,
            )
    rpaths = otool(executable, "-l")
    if "@executable_path/../Frameworks" not in rpaths:
        subprocess.run(
            [
                "install_name_tool",
                "-add_rpath",
                "@executable_path/../Frameworks",
                str(executable),
            ],
            check=True,
        )

    info = {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleDocumentTypes": [
            {
                "CFBundleTypeName": "Movie",
                "CFBundleTypeRole": "Viewer",
                "LSHandlerRank": "Alternate",
                "LSItemContentTypes": ["public.movie"],
            },
            {
                "CFBundleTypeName": "Audio",
                "CFBundleTypeRole": "Viewer",
                "LSHandlerRank": "Alternate",
                "LSItemContentTypes": ["public.audio"],
            },
            {
                "CFBundleTypeName": "Playlist",
                "CFBundleTypeRole": "Viewer",
                "LSHandlerRank": "Alternate",
                "LSItemContentTypes": ["public.m3u-playlist", "public.pls-playlist"],
            },
        ],
        "CFBundleExecutable": "player",
        "CFBundleIdentifier": "local.player",
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": "Player",
        "CFBundleDisplayName": "Player",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSApplicationCategoryType": "public.app-category.video",
        "LSEnvironment": {"MPVBUNDLE": "true"},
        "LSMinimumSystemVersion": "27.0",
        "NSHighResolutionCapable": True,
        "NSPrincipalClass": "NSApplication",
        "PlayerFFmpegRuntime": f"{PIN['ffmpeg']}+{PIN['release']}",
    }
    with open(APP / "Contents/Info.plist", "wb") as handle:
        plistlib.dump(info, handle, sort_keys=True)

    stamp = {
        "release_binary_sha256": hashlib.sha256(BINARY.read_bytes()).hexdigest(),
        "revision": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
    }
    (RESOURCES / "build.json").write_text(json.dumps(stamp, indent=2) + "\n")

    for binary in [executable] + list(FRAMEWORKS.glob("*.dylib")):
        subprocess.run(
            ["codesign", "--force", "--sign", "-", str(binary)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    for path, mode in modes.items():
        path.chmod(mode)
    subprocess.run(
        ["codesign", "--force", "--deep", "--sign", "-", str(APP)],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    if RUNTIME_LINK.is_symlink():
        RUNTIME_LINK.unlink()
    elif RUNTIME_LINK.exists() and not (RUNTIME_LINK.is_dir() and remove_generated_linux_runtime_links()):
        raise SystemExit(f"{RUNTIME_LINK} exists and is not the runtime link")
    RUNTIME_LINK.symlink_to(os.path.relpath(RUNTIME / "lib", RUNTIME_LINK.parent))

    print(f"bundled {APP}")
    print("frameworks: " + ", ".join(sorted(closure)))


if __name__ == "__main__":
    main()
