#!/usr/bin/env python3
"""rustode bench: drops files from the operating system's page cache.

Usage: python3 -I evict.py <path>...

Each path is a file, or a folder whose files (symlinks not followed) are all
evicted. Uses posix_fadvise(POSIX_FADV_DONTNEED) after a sync, which needs no
privileges and only drops clean pages of files the user can open. It does not
touch libraries shared with other programs unless a path names them, so the
caller names only the editor's own files. Prints one summary line.
"""
import os
import sys


def files(path):
    if os.path.isfile(path) and not os.path.islink(path):
        yield path
    elif os.path.isdir(path):
        for root, _dirs, names in os.walk(path, followlinks=False):
            for name in names:
                full = os.path.join(root, name)
                if os.path.isfile(full) and not os.path.islink(full):
                    yield full


def main(paths):
    os.sync()  # dirty pages cannot be dropped
    count = size = 0
    for path in paths:
        for name in files(path):
            try:
                fd = os.open(name, os.O_RDONLY)
            except OSError:
                continue
            try:
                os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
                count += 1
                size += os.fstat(fd).st_size
            except OSError:
                pass
            finally:
                os.close(fd)
    print(f"evicted {count} files, {size // (1 << 20)} MiB")


if __name__ == "__main__":
    main(sys.argv[1:])
