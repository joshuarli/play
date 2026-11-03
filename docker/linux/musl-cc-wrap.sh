#!/bin/sh
# Linker wrapper for x86_64-unknown-linux-musl cargo links. The musl
# target resolves `-l` flags in the `-Bstatic` region against static
# archives only, so the shared-only system Wayland client never
# resolves there. Drop the flag and re-add the library by absolute
# path at the end of the command line, where `-Bdynamic` is in
# effect. Everything else passes through untouched.
set -u
rewritten=""
seen=""
for arg in "$@"; do
    case "$arg" in
    -lwayland-client)
        seen="yes"
        continue
        ;;
    esac
    # Container paths never contain single quotes; quote each word so
    # arguments with spaces survive the exec below.
    rewritten="$rewritten '$arg'"
done
if [ -n "$seen" ]; then
    rewritten="$rewritten '/usr/lib/libwayland-client.so'"
fi
eval "exec cc $rewritten"
