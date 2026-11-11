#!/usr/bin/env python3
import os
import posixpath
import stat
import sys
import tarfile


class UnownedProduct(Exception):
    pass


def archive_entries(archive, root):
    entries = {}
    for member in archive.getmembers():
        name = member.name.rstrip("/")
        parts = name.split("/")
        if not parts or parts[0] != root or any(part in ("", ".", "..") for part in parts):
            raise ValueError("unsafe archive path")
        path = tuple(parts[1:])
        if path in entries or not (member.isdir() or member.isfile() or member.issym()):
            raise ValueError("duplicate or unsupported archive entry")
        if member.issym():
            target = member.linkname
            resolved = posixpath.normpath(posixpath.join(posixpath.dirname("/".join(path)), target))
            if not target or posixpath.isabs(target) or resolved == ".." or resolved.startswith("../"):
                raise ValueError("unsafe archive symlink")
        entries[path] = member
    if () not in entries or not entries[()].isdir():
        raise ValueError("missing archive root")
    for path in entries:
        if path and (path[:-1] not in entries or not entries[path[:-1]].isdir()):
            raise ValueError("missing archive parent")
    return entries


def matches(archive, entries, destination):
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    try:
        metadata = os.lstat(destination)
    except FileNotFoundError:
        return False
    except OSError as error:
        raise UnownedProduct(f"cannot inspect runtime prefix: {error}")
    if stat.S_ISLNK(metadata.st_mode):
        return False
    if not stat.S_ISDIR(metadata.st_mode):
        raise UnownedProduct("runtime prefix is not a directory")
    try:
        root = os.open(destination, flags)
    except OSError as error:
        raise UnownedProduct(f"cannot inspect runtime prefix: {error}")

    files = []

    def check_ownership(fd, path):
        with os.scandir(fd) as children:
            names = {child.name for child in children}
        for name in names:
            child = path + (name,)
            member = entries.get(child)
            if member is None:
                raise UnownedProduct("/".join(child))
            metadata = os.stat(name, dir_fd=fd, follow_symlinks=False)
            if stat.S_ISDIR(metadata.st_mode):
                if not member.isdir():
                    raise UnownedProduct("/".join(child))
                opened = os.open(name, flags, dir_fd=fd)
                try:
                    check_ownership(opened, child)
                finally:
                    os.close(opened)

    def directory(fd, path):
        expected = {key[-1] for key in entries if len(key) == len(path) + 1 and key[:-1] == path}
        with os.scandir(fd) as children:
            names = {child.name for child in children}
        unexpected = names - expected
        if unexpected:
            raise UnownedProduct("/".join((*path, sorted(unexpected)[0])))
        if names != expected:
            return False
        for name in names:
            child = path + (name,)
            member = entries[child]
            metadata = os.stat(name, dir_fd=fd, follow_symlinks=False)
            if member.issym():
                if not stat.S_ISLNK(metadata.st_mode) or os.readlink(name, dir_fd=fd) != member.linkname:
                    return False
            elif member.isdir():
                if not stat.S_ISDIR(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != member.mode:
                    return False
                opened = os.open(name, flags, dir_fd=fd)
                try:
                    if not directory(opened, child):
                        return False
                finally:
                    os.close(opened)
            else:
                if not stat.S_ISREG(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != member.mode or metadata.st_size != member.size:
                    return False
                files.append((child, member))
        return True

    def file_matches(path, member):
        parent = os.dup(root)
        try:
            for name in path[:-1]:
                opened = os.open(name, flags, dir_fd=parent)
                os.close(parent)
                parent = opened
            opened = os.open(path[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
            with os.fdopen(opened, "rb") as installed, archive.extractfile(member) as expected_file:
                metadata = os.fstat(installed.fileno())
                if not stat.S_ISREG(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != member.mode or metadata.st_size != member.size:
                    return False
                while True:
                    expected_chunk = expected_file.read(1024 * 1024)
                    if installed.read(len(expected_chunk) or 1) != expected_chunk:
                        return False
                    if not expected_chunk:
                        return True
        finally:
            os.close(parent)

    try:
        check_ownership(root, ())
        if stat.S_IMODE(os.fstat(root).st_mode) != entries[()].mode or not directory(root, ()):
            return False
        return all(file_matches(path, member) for path, member in sorted(files, key=lambda item: item[1].offset_data))
    except OSError as error:
        raise UnownedProduct(f"cannot inspect runtime product: {error}")
    finally:
        os.close(root)


def main():
    if len(sys.argv) != 4:
        raise ValueError("expected archive, root name and destination")
    with tarfile.open(sys.argv[1], "r:gz") as archive:
        entries = archive_entries(archive, sys.argv[2])
        return 0 if matches(archive, entries, sys.argv[3]) else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except UnownedProduct as error:
        print(f"error: unowned FFmpeg runtime product: {error}", file=sys.stderr)
        sys.exit(3)
    except (OSError, tarfile.TarError, ValueError) as error:
        print(f"error: invalid FFmpeg runtime archive: {error}", file=sys.stderr)
        sys.exit(2)
