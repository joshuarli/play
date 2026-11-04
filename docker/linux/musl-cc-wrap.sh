#!/bin/sh
# Linker wrapper for x86_64-unknown-linux-musl cargo links. The musl
# target resolves `-l` flags in the `-Bstatic` region against static
# archives only, so the shared-only system Wayland client never
# resolves there. Drop the flag and re-add the library by absolute
# path at the end of the command line, where `-Bdynamic` is in
# effect.
#
# Rust's std for a dynamically linked musl target passes `-lgcc_s`
# for its unwinder, and rustc links with `-nodefaultlibs`, so
# `-static-libgcc` alone cannot keep libgcc_s out. With
# `-static-libgcc`, replace `-lgcc_s` in place with GCC's static
# unwinder, libgcc_eh.a. Everything else passes through untouched.
set -u
static_libgcc=""
for arg in "$@"; do
    [ "$arg" = "-static-libgcc" ] && static_libgcc="yes"
done
rewritten=""
seen=""
for arg in "$@"; do
    case "$arg" in
    -lwayland-client)
        seen="yes"
        continue
        ;;
    -lgcc_s)
        if [ -n "$static_libgcc" ]; then
            rewritten="$rewritten '$(cc -print-file-name=libgcc_eh.a)'"
            continue
        fi
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
