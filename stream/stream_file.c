/*
 * Original authors: Albeu, probably Arpi
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

#include "config.h"

#include <stdio.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <unistd.h>
#include <errno.h>

#include "common/common.h"
#include "common/msg.h"
#include "stream.h"
#include "options/path.h"
#include "player_local_file.h"

struct priv {
    struct player_local_file *file;
    int fd;
    bool close_fd;
};

static int fill_buffer(stream_t *s, void *buffer, int max_len)
{
    struct priv *p = s->priv;
    if (p->file) {
        size_t read = 0;
        char err[160] = {0};
        if (player_ffi_local_file_read(p->file, buffer, max_len, &read,
                                       err, sizeof(err)) < 0)
        {
            MP_ERR(s, "Read error: %s\n", err);
            return -1;
        }
        return (int)read;
    }
    int res = read(p->fd, buffer, max_len);
    return res < 0 ? -1 : res;
}

static int write_buffer(stream_t *s, void *buffer, int len)
{
    struct priv *p = s->priv;
    if (p->file) {
        size_t written = 0;
        char err[160] = {0};
        if (player_ffi_local_file_write(p->file, buffer, len, &written,
                                        err, sizeof(err)) < 0)
        {
            MP_ERR(s, "Write error: %s\n", err);
            return -1;
        }
        return (int)written;
    }
    return write(p->fd, buffer, len);
}

static int seek(stream_t *s, int64_t newpos)
{
    struct priv *p = s->priv;
    if (p->file) {
        uint64_t position = 0;
        char err[160] = {0};
        if (player_ffi_local_file_seek(p->file, newpos, 0, &position,
                                       err, sizeof(err)) < 0)
        {
            MP_ERR(s, "Seek error: %s\n", err);
            return 0;
        }
        return 1;
    }
    return lseek(p->fd, newpos, SEEK_SET) != (off_t)-1;
}

static int64_t get_size(stream_t *s)
{
    struct priv *p = s->priv;
    if (p->file) {
        uint64_t size = 0;
        char err[160] = {0};
        if (player_ffi_local_file_size(p->file, &size, err, sizeof(err)) < 0)
            return -1;
        return size;
    }
    struct stat st;
    if (fstat(p->fd, &st) == 0 && st.st_size >= 0)
        return st.st_size;
    return -1;
}

static void s_close(stream_t *s)
{
    struct priv *p = s->priv;
    if (p->file)
        player_ffi_local_file_destroy(p->file);
    p->file = NULL;
    if (p->close_fd)
        close(p->fd);
}

// If url is a file:// URL, return the local filename, otherwise return NULL.
char *mp_file_url_to_filename(void *talloc_ctx, bstr url)
{
    bstr proto = mp_split_proto(url, &url);
    if (bstrcasecmp0(proto, "file") != 0)
        return NULL;
    char *filename = bstrto0(talloc_ctx, url);
    mp_url_unescape_inplace(filename);
    return filename;
}

// Return talloc_strdup's filesystem path if local, otherwise NULL.
char *mp_file_get_path(void *talloc_ctx, bstr url)
{
    if (mp_split_proto(url, &(bstr){0}).len) {
        return mp_file_url_to_filename(talloc_ctx, url);
    } else {
        return bstrto0(talloc_ctx, url);
    }
}

static int open_f(stream_t *stream, const struct stream_open_args *args)
{
    struct priv *p = talloc_ptrtype(stream, p);
    *p = (struct priv) {
        .fd = -1,
    };
    stream->priv = p;
    stream->is_local_fs = true;

    bool strict_fs = args->flags & STREAM_LOCAL_FS_ONLY;
    bool write = stream->mode == STREAM_WRITE;

    char *filename = stream->path;
    if (!strict_fs) {
        char *fn = mp_file_url_to_filename(stream, bstr0(stream->url));
        if (fn)
            filename = stream->path = fn;
    }

    if (!strict_fs && !strcmp(filename, "-")) {
        stream->is_local_fs = false;
        p->fd = write ? 1 : 0;
    } else {
        char err[160] = {0};
        int is_directory = player_ffi_local_file_is_directory(filename, err,
                                                              sizeof(err));
        if (is_directory < 0) {
            MP_ERR(stream, "Cannot open file '%s': %s\n", filename, err);
            return STREAM_ERROR;
        }
        p->file = player_ffi_local_file_open(filename, write, err, sizeof(err));
        if (!p->file) {
            MP_ERR(stream, "Cannot open file '%s': %s\n", filename, err);
            return STREAM_ERROR;
        }
        stream->is_directory = is_directory > 0;
        stream->seekable = true;
    }

    stream->fast_skip = true;
    stream->fill_buffer = fill_buffer;
    stream->write_buffer = write_buffer;
    stream->seek = seek;
    stream->get_size = get_size;
    stream->close = s_close;
    return STREAM_OK;
}

const stream_info_t stream_info_file = {
    .name = "file",
    .open2 = open_f,
    .protocols = (const char*const[]){ "file", "", NULL },
    .can_write = true,
    .local_fs = true,
    .stream_origin = STREAM_ORIGIN_FS,
};
