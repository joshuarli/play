/*
 * I/O helpers
 *
 * This file is part of mpv.
 *
 * mpv is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * mpv is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with mpv.  If not, see <http://www.gnu.org/licenses/>.
 */

#ifndef MPLAYER_OSDEP_IO
#define MPLAYER_OSDEP_IO

#include "config.h"
#include <stdbool.h>
#include <stdint.h>
#include <limits.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <locale.h>

#include "compiler.h"

#if HAVE_GLOB_POSIX
#include <glob.h>
#endif

#if HAVE_ANDROID
#  include <unistd.h>
#  include <stdio.h>

// replace lseek with the 64bit variant
#ifdef lseek
#  undef lseek
#endif
#define lseek(f,p,w) lseek64((f), (p), (w))

// replace possible fseeko with a
// lseek64 based solution.
#ifdef fseeko
#  undef fseeko
#endif
static inline int mp_fseeko(FILE* fp, off64_t offset, int whence) {
    int ret = -1;
    if ((ret = fflush(fp)) != 0) {
        return ret;
    }

    return lseek64(fileno(fp), offset, whence) >= 0 ? 0 : -1;
}
#define fseeko(f,p,w) mp_fseeko((f), (p), (w))

#endif // HAVE_ANDROID

#ifndef O_BINARY
#define O_BINARY 0
#endif

// This is in POSIX.1-2008, but support outside of Linux is scarce.
#ifndef O_CLOEXEC
#define O_CLOEXEC 0
#endif
#ifndef FD_CLOEXEC
#define FD_CLOEXEC 0
#endif

#include <dirent.h>
#include <unistd.h>
#include <sys/mman.h>

extern char **environ;

bool mp_set_cloexec(int fd);
int mp_make_cloexec_pipe(int pipes[2]);
int mp_make_wakeup_pipe(int pipes[2]);
void mp_flush_wakeup_pipe(int pipe_end);

#endif
