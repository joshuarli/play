#!/usr/bin/env python3
"""Assemble Player.app from the release build and its pinned dependency prefix."""

import hashlib
import json
import platform
import plistlib
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
PREFIX = ROOT / ".work/deps/release"
RUST_TARGET = ROOT / ".work/deps/player-audio/target/dist"
BINARY = ROOT / "build-release/mpv"
APP = ROOT / "dist/Player.app"
VERSION = (ROOT / "MPV_VERSION").read_text().strip()
MACOS = APP / "Contents/MacOS"
FRAMEWORKS = APP / "Contents/Frameworks"
RESOURCES = APP / "Contents/Resources"
ICD_DIR = RESOURCES / "vulkan/icd.d"
MOLTENVK_LIB = PREFIX / "lib/libMoltenVK.dylib"
MOLTENVK_ICD = PREFIX / "share/vulkan/icd.d/MoltenVK_icd.json"


def otool(path, flag):
    return subprocess.check_output(["otool", flag, str(path)], text=True)


def dependencies(path):
    result = []
    for line in otool(path, "-L").splitlines()[1:]:
        line = line.strip()
        if line:
            result.append(line.split(" (compatibility")[0].strip())
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


def collect(binary):
    closure = {}
    queue = [binary]
    while queue:
        current = queue.pop()
        for dep in dependencies(current):
            if dep.startswith("/opt/homebrew"):
                raise SystemExit(f"Homebrew dependency in {current}: {dep}")
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

    closure = collect(executable)
    for name, source in closure.items():
        shutil.copy2(source, FRAMEWORKS / name)

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
    subprocess.run(["strip", "-x", str(moltenvk)], check=True)
    icd = json.loads(MOLTENVK_ICD.read_text())
    icd["ICD"]["library_path"] = "../../../Frameworks/libMoltenVK.dylib"
    (ICD_DIR / "MoltenVK_icd.json").write_text(json.dumps(icd, indent=4) + "\n")

    shutil.copy2(
        ROOT / "TOOLS/osxbundle/mpv.app/Contents/Resources/icon.icns",
        RESOURCES / "icon.icns",
    )

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
        "CFBundleIconFile": "icon.icns",
        "CFBundleIdentifier": "local.player",
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": "Player",
        "CFBundleDisplayName": "Player",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": VERSION,
        "CFBundleVersion": VERSION,
        "LSApplicationCategoryType": "public.app-category.video",
        "LSEnvironment": {"MPVBUNDLE": "true"},
        "LSMinimumSystemVersion": "27.0",
        "NSHighResolutionCapable": True,
        "NSPrincipalClass": "NSApplication",
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
    subprocess.run(
        ["codesign", "--force", "--deep", "--sign", "-", str(APP)],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    print(f"bundled {APP}")
    print("frameworks: " + ", ".join(sorted(closure)))


if __name__ == "__main__":
    main()
