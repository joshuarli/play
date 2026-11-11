import contextlib
import hashlib
import os
import secrets
import shutil
import stat
import sys


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
