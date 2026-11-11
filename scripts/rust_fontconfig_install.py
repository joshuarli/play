import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib


DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
FILE_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
MANAGED = ".rust-fontconfig-managed"
MANAGED_CONTENT = b"rust-fontconfig\n"


def directories(prefix, create, missing_ok=False):
    stack = contextlib.ExitStack()
    try:
        def open_directory(parent, name):
            if parent is None:
                return None
            if create:
                try:
                    os.mkdir(name, dir_fd=parent)
                except FileExistsError:
                    pass
            try:
                fd = os.open(name, DIRECTORY_FLAGS, dir_fd=parent)
            except FileNotFoundError:
                if missing_ok:
                    return None
                raise
            stack.callback(os.close, fd)
            return fd

        lib = open_directory(prefix, "lib")
        pkgconfig = open_directory(lib, "pkgconfig")
        include = open_directory(prefix, "include")
        headers = open_directory(include, "rust-fontconfig")
        return stack, lib, pkgconfig, headers
    except BaseException:
        stack.close()
        raise


def digest_file(directory, name, mode):
    fd = os.open(name, FILE_FLAGS, dir_fd=directory)
    with os.fdopen(fd, "rb") as file:
        metadata = os.fstat(file.fileno())
        if not stat.S_ISREG(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != mode:
            raise ValueError(f"invalid rust-fontconfig product: {name}")
        return hashlib.file_digest(file, "sha256").hexdigest()


def names(directory):
    return set(os.listdir(directory)) if directory is not None else set()


def reject_unowned(lib, pkgconfig, headers, soname):
    allowed_libraries = {soname, "librust_fontconfig.so"}
    for name in names(lib):
        if (name in ("librust_fontconfig.so", "librust_fontconfig.a") or name.startswith("librust_fontconfig.so.")) and name not in allowed_libraries:
            raise ValueError(f"unowned rust-fontconfig library: {name}")
    for name in names(headers):
        if name != "rust_fontconfig.h":
            raise ValueError(f"unowned rust-fontconfig header: {name}")
    if "rust-fontconfig-uninstalled.pc" in names(pkgconfig):
        raise ValueError("unowned rust-fontconfig pkg-config file: rust-fontconfig-uninstalled.pc")


def inventory(lib, pkgconfig, headers, soname):
    reject_unowned(lib, pkgconfig, headers, soname)
    entries = ((lib, soname, 0o755, f"lib/{soname}"),
               (headers, "rust_fontconfig.h", 0o644, "include/rust-fontconfig/rust_fontconfig.h"),
               (pkgconfig, "rust-fontconfig.pc", 0o644, "lib/pkgconfig/rust-fontconfig.pc"))
    lines = [f"{path} {mode:o} {digest_file(directory, name, mode)}"
             for directory, name, mode, path in entries]
    if os.readlink("librust_fontconfig.so", dir_fd=lib) != soname:
        raise ValueError("invalid rust-fontconfig library symlink")
    lines.append(f"lib/librust_fontconfig.so -> {soname}")
    return "\n".join(lines)


def validate(prefix, identity, soname):
    try:
        if not managed(prefix):
            return False
        fd = os.open(".input", FILE_FLAGS, dir_fd=prefix)
        with os.fdopen(fd, "rb") as file:
            if not stat.S_ISREG(os.fstat(file.fileno()).st_mode):
                return False
            recorded = file.read()
        stack, lib, pkgconfig, headers = directories(prefix, False)
        with stack:
            expected = f"{identity}\n{inventory(lib, pkgconfig, headers, soname)}\n".encode()
        return recorded == expected
    except (OSError, ValueError):
        return False


def managed(prefix):
    fd = os.open(MANAGED, FILE_FLAGS, dir_fd=prefix)
    with os.fdopen(fd, "rb") as file:
        return stat.S_ISREG(os.fstat(file.fileno()).st_mode) and file.read(len(MANAGED_CONTENT) + 1) == MANAGED_CONTENT


def legacy_default(prefix, default_path):
    try:
        if os.path.abspath(default_path) != os.path.realpath(default_path):
            return False
        actual, default = os.fstat(prefix), os.stat(default_path)
        if (actual.st_dev, actual.st_ino) != (default.st_dev, default.st_ino):
            return False
        fd = os.open(".input", FILE_FLAGS, dir_fd=prefix)
        with os.fdopen(fd, "rb") as file:
            if not stat.S_ISREG(os.fstat(file.fileno()).st_mode):
                return False
            value = file.read(66)
        return len(value) == 65 and value.endswith(b"\n") and all(byte in b"0123456789abcdef" for byte in value[:-1])
    except FileNotFoundError:
        return False


def mark_managed(prefix, lib, pkgconfig, headers, soname, default_path, allow_legacy):
    entries = set(os.listdir(prefix))
    if MANAGED in entries:
        if not managed(prefix):
            raise ValueError("invalid rust-fontconfig ownership marker")
        return
    occupied = (".input" in entries or soname in names(lib) or "librust_fontconfig.so" in names(lib) or
                "rust-fontconfig.pc" in names(pkgconfig) or "rust_fontconfig.h" in names(headers))
    if occupied and not (allow_legacy and legacy_default(prefix, default_path)):
        raise ValueError("unowned rust-fontconfig products in installation prefix")
    try:
        fd = os.open(MANAGED, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o644, dir_fd=prefix)
    except FileExistsError:
        if not managed(prefix):
            raise ValueError("invalid rust-fontconfig ownership marker")
    else:
        with os.fdopen(fd, "wb") as file:
            file.write(MANAGED_CONTENT)


def invalidate(prefix):
    try:
        if not managed(prefix):
            return
    except FileNotFoundError:
        return
    try:
        os.unlink(".input", dir_fd=prefix)
    except FileNotFoundError:
        pass


def atomic_file(directory, name, mode, write):
    temporary = f".rust-fontconfig-{secrets.token_hex(16)}"
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode, dir_fd=directory)
    try:
        with os.fdopen(fd, "wb") as target:
            os.fchmod(target.fileno(), mode)
            write(target)
        os.replace(temporary, name, src_dir_fd=directory, dst_dir_fd=directory)
    finally:
        try:
            os.unlink(temporary, dir_fd=directory)
        except FileNotFoundError:
            pass


def require_current_directories(prefix, prefix_path, expected):
    def identity(metadata):
        return metadata.st_dev, metadata.st_ino

    if identity(os.stat(prefix_path)) != identity(os.fstat(prefix)):
        raise ValueError("rust-fontconfig prefix changed during installation")
    stack, *current = directories(prefix, False)
    with stack:
        if any(identity(os.fstat(actual)) != identity(os.fstat(original))
               for actual, original in zip(current, expected)):
            raise ValueError("rust-fontconfig output directory changed during installation")


def check_target(path):
    def fail(error):
        raise error

    try:
        root = os.lstat(path)
    except FileNotFoundError:
        return
    if not stat.S_ISDIR(root.st_mode):
        raise ValueError(f"invalid rust-fontconfig target path: {path}")
    for directory, names, files in os.walk(path, followlinks=False, onerror=fail):
        for name in names + files:
            entry = os.path.join(directory, name)
            mode = os.lstat(entry).st_mode
            if not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                raise ValueError(f"invalid rust-fontconfig target path: {entry}")


def executable_identity(tree, executable, *, kache_companion=False):
    digest = hashlib.sha256()
    if executable is None:
        digest.update(b"unset")
        return digest.hexdigest()
    digest.update(b"set\0" + os.fsencode(executable) + b"\0")
    if os.sep in executable:
        candidates = [executable if os.path.isabs(executable) else os.path.join(tree, executable)]
    else:
        candidates = [os.path.join(directory if os.path.isabs(directory) else os.path.join(tree, directory), executable)
                      for directory in os.environ.get("PATH", os.defpath).split(os.pathsep)]
    for candidate in candidates:
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            resolved = os.path.realpath(candidate)
            digest.update(os.fsencode(resolved) + b"\0")
            with open(candidate, "rb") as file:
                for chunk in iter(lambda: file.read(1024 * 1024), b""):
                    digest.update(chunk)
            if kache_companion and os.path.basename(resolved) == "kache":
                engine = os.path.join(os.path.dirname(resolved), "kache-engine")
                if os.path.lexists(engine):
                    if not os.path.isfile(engine) or not os.access(engine, os.X_OK):
                        return None
                    digest.update(b"\0engine\0" + os.fsencode(os.path.realpath(engine)) + b"\0")
                    with open(engine, "rb") as file:
                        for chunk in iter(lambda: file.read(1024 * 1024), b""):
                            digest.update(chunk)
                else:
                    digest.update(b"\0no-engine")
            return digest.hexdigest()
    return None


def cargo_configuration(tree):
    environment = dict(os.environ, RUSTC_BOOTSTRAP="1", KACHE_DAEMON_AUTOSTART="0")
    try:
        result = subprocess.run(["cargo", "-Z", "unstable-options", "config", "get", "--format", "json"],
                                cwd=tree, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if result.returncode:
            return None
        configuration = json.loads(result.stdout)
        return configuration if isinstance(configuration, dict) else None
    except (OSError, ValueError):
        return None


def configured_program(tree, key, selected, parts):
    if not isinstance(selected, str):
        return None
    if os.sep not in selected or os.path.isabs(selected):
        return selected
    environment = dict(os.environ, RUSTC_BOOTSTRAP="1", KACHE_DAEMON_AUTOSTART="0")
    try:
        origin = subprocess.run(["cargo", "-Z", "unstable-options", "config", "get", key, "--show-origin"],
                                cwd=tree, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if origin.returncode:
            return None
        pattern = r'(.+?)\s*=\s*("(?:[^"\\]|\\.)*")\s+# ([^\n]+)\n'
        match = re.fullmatch(pattern, origin.stdout.decode("utf-8"))
        if match is None:
            return None
        value = tomllib.loads(match[1] + " = " + match[2])
        for part in parts:
            value = value[part]
        if value != selected:
            return None
        config_path = match[3]
        if not os.path.isabs(config_path):
            return None
        return os.path.normpath(os.path.join(os.path.dirname(os.path.dirname(config_path)), selected))
    except (OSError, ValueError, TypeError, KeyError):
        return None


def matching_linker_conditions(tree, predicates):
    environment = dict(os.environ, KACHE_DAEMON_AUTOSTART="0")
    try:
        with tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"], prefix="fontconfig-cfg-") as directory:
            probe = Path(directory)
            (probe / "src").mkdir()
            (probe / "src/lib.rs").write_text("")
            manifest = ['[package]\nname = "fontconfig_cfg_probe"\nversion = "0.0.0"\nedition = "2024"\n[workspace]\n']
            for index, predicate in enumerate(predicates):
                dependency = probe / f"dependency-{index}"
                (dependency / "src").mkdir(parents=True)
                (dependency / "src/lib.rs").write_text("")
                (dependency / "Cargo.toml").write_text(f'[package]\nname = "fontconfig_cfg_{index}"\nversion = "0.0.0"\nedition = "2024"\n')
                manifest.append(f'[target.{json.dumps(predicate, ensure_ascii=False)}.dependencies]\nfontconfig_cfg_{index} = {{ path = "dependency-{index}" }}\n')
            (probe / "Cargo.toml").write_text("".join(manifest))
            result = subprocess.run(["cargo", "metadata", "--format-version", "1", "--manifest-path", str(probe / "Cargo.toml"),
                                     "--filter-platform", "x86_64-unknown-linux-musl", "--offline"],
                                    cwd=tree, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            if result.returncode or b"non-trivial mutual dependency between target-specific configuration and RUSTFLAGS" in result.stderr:
                return None
            resolution = json.loads(result.stdout)["resolve"]
            nodes = [node for node in resolution["nodes"] if node["id"] == resolution["root"]]
            if len(nodes) != 1 or not isinstance(nodes[0]["deps"], list):
                return None
            names = {dependency["name"] for dependency in nodes[0]["deps"]}
            return [predicate for index, predicate in enumerate(predicates) if f"fontconfig_cfg_{index}" in names]
    except (OSError, ValueError, KeyError, TypeError):
        return None


def encoded_linker():
    arguments = os.environ.get("CARGO_ENCODED_RUSTFLAGS", "").split("\x1f")
    selected = None
    for index, argument in enumerate(arguments):
        if argument == "-C" and index + 1 < len(arguments) and arguments[index + 1].startswith("linker="):
            selected = arguments[index + 1][len("linker="):]
        elif argument.startswith("-Clinker="):
            selected = argument[len("-Clinker="):]
    return selected


def linker_identity(tree, configuration):
    linker = encoded_linker()
    if linker is None:
        linker = os.environ.get("CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER")
    if linker is None:
        if configuration is None:
            return None
        targets = configuration.get("target", {})
        if not isinstance(targets, dict):
            return None
        target = targets.get("x86_64-unknown-linux-musl", {})
        if not isinstance(target, dict):
            return None
        if "linker" in target:
            linker = configured_program(tree, "target.x86_64-unknown-linux-musl.linker", target["linker"], ("target", "x86_64-unknown-linux-musl", "linker"))
            if linker is None:
                return None
        else:
            predicates = [name for name, value in targets.items()
                          if isinstance(name, str) and name.startswith("cfg(") and (not isinstance(value, dict) or "linker" in value)]
            if predicates:
                matched = matching_linker_conditions(tree, predicates)
                if matched is None or len(matched) > 1:
                    return None
                if matched:
                    predicate = matched[0]
                    target = targets[predicate]
                    if not isinstance(target, dict):
                        return None
                    linker = configured_program(tree, "target." + predicate + ".linker", target["linker"], ("target", predicate, "linker"))
                    if linker is None:
                        return None
    return executable_identity(tree, linker)


def configured_rustc(tree, configuration):
    if "RUSTC" in os.environ:
        return os.environ["RUSTC"]
    if "CARGO_BUILD_RUSTC" in os.environ:
        return os.environ["CARGO_BUILD_RUSTC"]
    if configuration is None:
        return None
    build = configuration.get("build", {})
    if not isinstance(build, dict):
        return None
    return configured_program(tree, "build.rustc", build.get("rustc", "rustc"), ("build", "rustc"))


def configured_wrapper(tree, configuration, variable, cargo_variable, key):
    if variable in os.environ:
        return os.environ[variable]
    if cargo_variable in os.environ:
        return os.environ[cargo_variable]
    if configuration is None:
        return None
    build = configuration.get("build", {})
    if not isinstance(build, dict):
        return None
    selected = build.get(key, "")
    if selected == "":
        return ""
    return configured_program(tree, "build." + key, selected, ("build", key))


def build_environment_identity(tree):
    needs_configuration = ("RUSTC" not in os.environ and "CARGO_BUILD_RUSTC" not in os.environ) or (
        encoded_linker() is None and "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER" not in os.environ) or (
        "RUSTC_WRAPPER" not in os.environ and "CARGO_BUILD_RUSTC_WRAPPER" not in os.environ) or (
        "RUSTC_WORKSPACE_WRAPPER" not in os.environ and "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER" not in os.environ)
    configuration = cargo_configuration(tree) if needs_configuration else None
    linker = linker_identity(tree, configuration)
    selected = configured_rustc(tree, configuration)
    wrapper = configured_wrapper(tree, configuration, "RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER", "rustc-wrapper")
    workspace_wrapper = configured_wrapper(tree, configuration, "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", "rustc-workspace-wrapper")
    if selected is None or wrapper is None or workspace_wrapper is None:
        return None
    compiler = executable_identity(tree, selected)
    outer = executable_identity(tree, wrapper or None, kache_companion=True)
    inner = executable_identity(tree, workspace_wrapper or None, kache_companion=True)
    if linker is None or compiler is None or outer is None or inner is None:
        return None
    try:
        version = subprocess.run([selected, "-vV"], cwd=tree, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    except OSError:
        return None
    if version.returncode or not version.stdout:
        return None
    digest = hashlib.sha256(linker.encode() + b"\0" + compiler.encode() + b"\0" + outer.encode() + b"\0" + inner.encode() + b"\0" + version.stdout)
    ignored = {"CARGO_PROFILE_RELEASE_LTO", "CARGO_PROFILE_RELEASE_OPT_LEVEL", "CARGO_PROFILE_RELEASE_CODEGEN_UNITS"}
    for name in sorted(os.environ):
        if name.startswith("CARGO_PROFILE_RELEASE_") and name not in ignored:
            digest.update(b"\0" + os.fsencode(name) + b"\0" + os.fsencode(os.environ[name]))
    return digest.hexdigest()


def publish(prefix, identity, soname, library, header, prefix_path, version):
    stack, lib, pkgconfig, headers = directories(prefix, False)
    with stack:
        for directory, name, mode, source in ((lib, soname, 0o755, library),
                                               (headers, "rust_fontconfig.h", 0o644, header)):
            with open(source, "rb") as source_file:
                atomic_file(directory, name, mode, lambda target: shutil.copyfileobj(source_file, target))
        temporary = f".rust-fontconfig-{secrets.token_hex(16)}"
        os.symlink(soname, temporary, dir_fd=lib)
        try:
            os.replace(temporary, "librust_fontconfig.so", src_dir_fd=lib, dst_dir_fd=lib)
        finally:
            try:
                os.unlink(temporary, dir_fd=lib)
            except FileNotFoundError:
                pass
        pc = (f"prefix={prefix_path}\nlibdir=${{prefix}}/lib\nincludedir=${{prefix}}/include/rust-fontconfig\n\n"
              "Name: rust-fontconfig\nDescription: Pure-Rust font discovery, matching, and fallback with a C API\n"
              f"Version: {version}\nLibs: -L${{libdir}} -lrust_fontconfig\nCflags: -I${{includedir}}\n")
        atomic_file(pkgconfig, "rust-fontconfig.pc", 0o644, lambda target: target.write(os.fsencode(pc)))
        require_current_directories(prefix, prefix_path, (lib, pkgconfig, headers))
        marker = f"{identity}\n{inventory(lib, pkgconfig, headers, soname)}\n".encode()
        atomic_file(prefix, ".input", 0o644, lambda target: target.write(marker))


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "build-environment-identity":
        identity = build_environment_identity(sys.argv[2])
        if identity is None:
            return 1
        print(identity)
        return 0
    if len(sys.argv) == 3 and sys.argv[1] == "check-target":
        check_target(sys.argv[2])
        return 0
    if len(sys.argv) == 4 and sys.argv[1] == "check-prefix":
        path = sys.argv[2]
        for value in (path, os.path.realpath(path)):
            if any(character.isspace() or character in "\\#:'\"" for character in value) or "${" in value:
                raise ValueError("prefix cannot be represented in rust-fontconfig pkg-config metadata")
        if sys.argv[3] == "1" and os.path.abspath(path) != os.path.realpath(path):
            try:
                fd = os.open(os.path.realpath(path), DIRECTORY_FLAGS)
                try:
                    owned = managed(fd)
                finally:
                    os.close(fd)
            except FileNotFoundError:
                owned = False
            if not owned:
                raise ValueError("unowned rust-fontconfig default prefix traverses a symlink")
        return 0
    prefix = int(sys.argv[1])
    command = sys.argv[2]
    if command == "prepare":
        stack, lib, pkgconfig, headers = directories(prefix, False, missing_ok=True)
        with stack:
            reject_unowned(lib, pkgconfig, headers, sys.argv[3])
            mark_managed(prefix, lib, pkgconfig, headers, sys.argv[3], sys.argv[4], sys.argv[5] == "1")
        stack, _, _, _ = directories(prefix, True)
        stack.close()
    elif command == "validate":
        return 0 if validate(prefix, sys.argv[3], sys.argv[4]) else 1
    elif command == "invalidate":
        invalidate(prefix)
    elif command == "publish":
        publish(prefix, *sys.argv[3:])
    else:
        raise ValueError(f"unknown rust-fontconfig operation: {command}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"rust-fontconfig: {error}", file=sys.stderr)
        sys.exit(1)
