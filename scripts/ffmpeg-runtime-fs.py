#!/usr/bin/env python3
import fcntl
import hashlib
import json
import os
import secrets
import shutil
import stat
import sys


FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW


def directory(parent, name, create=False):
    if create:
        try:
            os.mkdir(name, 0o777, dir_fd=parent)
        except FileExistsError:
            pass
    return os.open(name, FLAGS, dir_fd=parent)


def open_path(root, names, create=False):
    current = os.dup(root)
    try:
        for name in names:
            child = directory(current, name, create)
            os.close(current)
            current = child
        return current
    except BaseException:
        os.close(current)
        raise


def descriptor(name):
    value = int(os.environ[name])
    if value < 3:
        raise ValueError("invalid FFmpeg runtime directory descriptor")
    return value


def same_inode(first, second):
    left, right = os.fstat(first), os.fstat(second)
    return (left.st_dev, left.st_ino) == (right.st_dev, right.st_ino)


def check_directories(root, root_fd, cache_fd, dest_fd):
    opened_root = os.open(root, FLAGS)
    try:
        if not same_inode(root_fd, opened_root):
            raise ValueError("FFmpeg runtime root changed")
    finally:
        os.close(opened_root)
    for names, expected in ((('.work', 'tmp', 'ffmpeg-runtime'), cache_fd),
                            (('.work', 'deps', 'ffmpeg-runtime'), dest_fd)):
        current = open_path(root_fd, names)
        try:
            if not same_inode(current, expected):
                raise ValueError("FFmpeg runtime directory changed")
        finally:
            os.close(current)


def check_lock(dest_fd, lock_fd, target):
    locked = os.fstat(lock_fd)
    current = os.stat('.lock-' + target, dir_fd=dest_fd, follow_symlinks=False)
    if not stat.S_ISREG(locked.st_mode) or (locked.st_dev, locked.st_ino) != (current.st_dev, current.st_ino):
        raise ValueError("FFmpeg runtime lock changed")


def check(root, target):
    root_fd = descriptor("PLAYER_FFMPEG_RUNTIME_ROOT_FD")
    cache_fd = descriptor("PLAYER_FFMPEG_RUNTIME_CACHE_FD")
    dest_fd = descriptor("PLAYER_FFMPEG_RUNTIME_DEST_FD")
    lock_fd = descriptor("PLAYER_FFMPEG_RUNTIME_LOCK_FD")
    check_directories(root, root_fd, cache_fd, dest_fd)
    check_lock(dest_fd, lock_fd, target)
    fcntl.flock(lock_fd, fcntl.LOCK_EX)
    check_directories(root, root_fd, cache_fd, dest_fd)
    check_lock(dest_fd, lock_fd, target)
    pin_digest = os.environ.get('PLAYER_FFMPEG_RUNTIME_PIN_SHA256')
    if os.environ.get('PLAYER_FFMPEG_RUNTIME_ARCHIVE_FD') and pin_digest is None:
        raise ValueError("missing FFmpeg runtime pin identity")
    if pin_digest is not None:
        with open(os.path.join(root, 'ffmpeg-runtime.lock.json'), 'rb') as pin_file:
            if hashlib.sha256(pin_file.read()).hexdigest() != pin_digest:
                raise ValueError("FFmpeg runtime pin changed")
    return cache_fd, dest_fd


def setup(root, target, script, arguments):
    root_fd = os.open(root, FLAGS)
    cache_fd = open_path(root_fd, ('.work', 'tmp', 'ffmpeg-runtime'), create=True)
    dest_fd = open_path(root_fd, ('.work', 'deps', 'ffmpeg-runtime'), create=True)
    lock_fd = os.open('.lock-' + target, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600, dir_fd=dest_fd)
    fcntl.flock(lock_fd, fcntl.LOCK_EX)
    for name, fd in (("ROOT", root_fd), ("CACHE", cache_fd), ("DEST", dest_fd), ("LOCK", lock_fd)):
        os.environ['PLAYER_FFMPEG_RUNTIME_' + name + '_FD'] = str(fd)
        os.set_inheritable(fd, True)
    for name in ('ARCHIVE_FD', 'ARCHIVE_ASSET', 'ARCHIVE_SHA256', 'PIN_SHA256', 'RELEASE'):
        os.environ.pop('PLAYER_FFMPEG_RUNTIME_' + name, None)
    check(root, target)
    if len(arguments) == 2:
        arguments[1] = os.path.abspath(arguments[1])
    os.fchdir(cache_fd)
    os.execv(script, [script, *arguments])


def snapshot(root, target, archive, expected, asset, script, arguments):
    cache_fd, _ = check(root, target)
    with open(os.path.join(root, 'ffmpeg-runtime.lock.json'), 'rb') as pin_file:
        pin_bytes = pin_file.read()
    pin = json.loads(pin_bytes)
    pinned_asset = f"ffmpeg-{pin['ffmpeg']}+{pin['release']}-{target}-runtime.tar.gz"
    if pin.get('contract') != 2 or pin.get('profile') != 'runtime' or pinned_asset != asset or pin['sha256'][target] != expected:
        raise ValueError("FFmpeg runtime pin changed")
    source = os.open(archive, os.O_RDONLY | os.O_NONBLOCK)
    if not stat.S_ISREG(os.fstat(source).st_mode):
        raise ValueError("FFmpeg runtime archive is not a regular file")
    name = '.snapshot-' + secrets.token_hex(16)
    result = os.open(name, os.O_CREAT | os.O_EXCL | os.O_RDWR | os.O_NOFOLLOW, 0o600, dir_fd=cache_fd)
    os.unlink(name, dir_fd=cache_fd)
    digest = hashlib.sha256()
    while True:
        chunk = os.read(source, 1024 * 1024)
        if not chunk:
            break
        digest.update(chunk)
        remaining = memoryview(chunk)
        while remaining:
            written = os.write(result, remaining)
            if not written:
                raise OSError("short FFmpeg runtime archive write")
            remaining = remaining[written:]
    os.close(source)
    if digest.hexdigest() != expected:
        raise ValueError("FFmpeg runtime archive changed while being copied")
    os.lseek(result, 0, os.SEEK_SET)
    os.set_inheritable(result, True)
    os.environ['PLAYER_FFMPEG_RUNTIME_ARCHIVE_FD'] = str(result)
    os.environ['PLAYER_FFMPEG_RUNTIME_ARCHIVE_ASSET'] = asset
    os.environ['PLAYER_FFMPEG_RUNTIME_PIN_SHA256'] = hashlib.sha256(pin_bytes).hexdigest()
    os.environ['PLAYER_FFMPEG_RUNTIME_RELEASE'] = pin['release']
    os.fchdir(cache_fd)
    os.execv(script, [script, *arguments])


def stage_directory(cache_fd, name):
    if not name.startswith('.extract-') or '/' in name or name in ('.', '..'):
        raise ValueError("invalid FFmpeg runtime staging directory")
    return directory(cache_fd, name)


def remove_directory(name):
    if not shutil.rmtree.avoids_symlink_attacks:
        raise ValueError("safe FFmpeg runtime directory removal is unavailable")
    shutil.rmtree(name)


def prepare_stage(root, target):
    cache_fd, _ = check(root, target)
    name = '.extract-' + target + '.staging'
    os.fchdir(cache_fd)
    try:
        current = os.lstat(name)
    except FileNotFoundError:
        pass
    else:
        if stat.S_ISDIR(current.st_mode):
            remove_directory(name)
        else:
            os.unlink(name)
    os.mkdir(name, 0o700)
    print(name)


def run(root, target, location, archive, command):
    cache_fd, dest_fd = check(root, target)
    if location == 'dest':
        cwd_fd = dest_fd
    else:
        cwd_fd = stage_directory(cache_fd, location)
    if archive != '-':
        archive_fd = descriptor('PLAYER_FFMPEG_RUNTIME_ARCHIVE_FD') if archive == '@snapshot' else os.open(archive, os.O_RDONLY | os.O_NONBLOCK)
        if not stat.S_ISREG(os.fstat(archive_fd).st_mode):
            raise ValueError("FFmpeg runtime archive is not a regular file")
        os.lseek(archive_fd, 0, os.SEEK_SET)
        os.dup2(archive_fd, 0, inheritable=True)
        command = ['-' if item == '@archive' else item for item in command]
    os.fchdir(cwd_fd)
    os.execvp(command[0], command)


def publish(root, target, stage, archive_root):
    cache_fd, dest_fd = check(root, target)
    stage_fd = stage_directory(cache_fd, stage)
    os.fchdir(dest_fd)
    try:
        current = os.lstat(target)
    except FileNotFoundError:
        pass
    else:
        if stat.S_ISDIR(current.st_mode):
            remove_directory(target)
        else:
            os.unlink(target)
    check(root, target)
    os.rename(archive_root, target, src_dir_fd=stage_fd, dst_dir_fd=dest_fd)
    os.close(stage_fd)
    os.rmdir(stage, dir_fd=cache_fd)


def main():
    operation, root, target, *arguments = sys.argv[1:]
    if target not in ('aarch64-apple-darwin', 'x86_64-unknown-linux-musl'):
        raise ValueError("unsupported FFmpeg runtime target")
    if operation == 'setup':
        setup(root, target, arguments[0], arguments[1:])
    elif operation == 'snapshot':
        snapshot(root, target, arguments[0], arguments[1], arguments[2], arguments[3], arguments[4:])
    elif operation == 'check':
        check(root, target)
    elif operation == 'prepare-stage':
        prepare_stage(root, target)
    elif operation == 'run':
        run(root, target, arguments[0], arguments[1], arguments[2:])
    elif operation == 'publish':
        publish(root, target, arguments[0], arguments[1])
    else:
        raise ValueError("invalid FFmpeg runtime filesystem operation")


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError) as error:
        sys.exit(f"error: FFmpeg runtime filesystem: {error}")
