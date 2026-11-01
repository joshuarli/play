/*
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

#include <assert.h>
#include "playlist.h"
#include "common/common.h"
#include "common/global.h"
#include "common/msg.h"
#include "mpv_talloc.h"
#include "options/path.h"

#include "demux/demux.h"
#include "stream/stream.h"

#include "player_playlist.h"

static void playlist_destructor(void *ptr)
{
    struct playlist *pl = ptr;
    player_ffi_playlist_destroy(pl->handle);
    pl->handle = NULL;
}

struct playlist *playlist_new(void *talloc_ctx)
{
    struct playlist *pl = talloc_zero(talloc_ctx, struct playlist);
    pl->handle = player_ffi_playlist_create();
    talloc_set_destructor(pl, playlist_destructor);
    return pl;
}

struct playlist_entry *playlist_entry_new(const char *filename)
{
    struct playlist_entry *e = talloc_zero(NULL, struct playlist_entry);
    e->filename = talloc_strdup(e, filename);
    e->stream_flags = STREAM_ORIGIN_DIRECT;
    return e;
}

void playlist_entry_add_param(struct playlist_entry *e, bstr name, bstr value)
{
    struct playlist_param p = {bstrdup(e, name), bstrdup(e, value)};
    MP_TARRAY_APPEND(e, e->params, e->num_params, p);
}

void playlist_entry_add_params(struct playlist_entry *e,
                               struct playlist_param *params,
                               int num_params)
{
    for (int n = 0; n < num_params; n++)
        playlist_entry_add_param(e, params[n].name, params[n].value);
}

void playlist_insert_at(struct playlist *pl, struct playlist_entry *add,
                        struct playlist_entry *at)
{
    mp_assert(add->filename);
    mp_assert(!at || at->pl == pl);

    int index = at ? playlist_entry_to_index(pl, at) : playlist_entry_count(pl);
    uint64_t id = player_ffi_playlist_insert_at(pl->handle, add, index);
    mp_assert(id);
    add->pl = pl;
    add->id = id;
    talloc_steal(pl, add);
}

void playlist_entry_unref(struct playlist_entry *e)
{
    e->reserved--;
    if (e->reserved < 0) {
        mp_assert(!e->pl);
        talloc_free(e);
    }
}

void playlist_remove(struct playlist *pl, struct playlist_entry *entry)
{
    mp_assert(pl && entry->pl == pl);

    player_ffi_playlist_remove(pl->handle, entry);

    entry->pl = NULL;
    entry->removed = true;
    ta_set_parent(entry, NULL);
    playlist_entry_unref(entry);
}

void playlist_clear(struct playlist *pl)
{
    while (playlist_entry_count(pl))
        playlist_remove(pl, playlist_entry_from_index(pl,
                                                      playlist_entry_count(pl) - 1));
    player_ffi_playlist_set_current(pl->handle, NULL);
    player_ffi_playlist_set_current_was_replaced(pl->handle, 0);
    player_ffi_playlist_set_completed(pl->handle, 0);
    player_ffi_playlist_set_started(pl->handle, 0);
    TA_FREEP(&pl->playlist_dir);
}

void playlist_clear_except_current(struct playlist *pl)
{
    struct playlist_entry *current = playlist_get_current(pl);
    for (int n = playlist_entry_count(pl) - 1; n >= 0; n--) {
        struct playlist_entry *entry = playlist_entry_from_index(pl, n);
        if (entry != current)
            playlist_remove(pl, entry);
    }
    playlist_set_completed(pl, false);
    playlist_set_started(pl, false);
}

void playlist_move(struct playlist *pl, struct playlist_entry *entry,
                   struct playlist_entry *at)
{
    if (entry == at)
        return;

    mp_assert(entry && entry->pl == pl);
    mp_assert(!at || at->pl == pl);
    bool moved = player_ffi_playlist_move(pl->handle, entry, at);
    mp_assert(moved);
}

void playlist_append_file(struct playlist *pl, const char *filename)
{
    playlist_insert_at(pl, playlist_entry_new(filename), NULL);
}

void playlist_populate_playlist_path(struct playlist *pl, const char *path)
{
    char *playlist_path = talloc_strdup(pl, path);
    for (int n = 0; n < playlist_entry_count(pl); n++)
        playlist_entry_from_index(pl, n)->playlist_path = playlist_path;
}

struct playlist_entry *playlist_get_first(struct playlist *pl)
{
    return player_ffi_playlist_first(pl->handle);
}

struct playlist_entry *playlist_get_last(struct playlist *pl)
{
    return player_ffi_playlist_last(pl->handle);
}

struct playlist_entry *playlist_get_next(struct playlist *pl, int direction)
{
    mp_assert(direction == -1 || direction == +1);
    return player_ffi_playlist_next(pl->handle, direction);
}

struct playlist_entry *playlist_entry_get_rel(struct playlist_entry *e,
                                              int direction)
{
    mp_assert(direction == -1 || direction == +1);
    if (!e->pl)
        return NULL;
    return player_ffi_playlist_entry_rel(e->pl->handle, e, direction);
}

struct playlist_entry *playlist_get_first_in_next_playlist(struct playlist *pl,
                                                           int direction)
{
    struct playlist_entry *current = playlist_get_current(pl);
    struct playlist_entry *entry = playlist_get_next(pl, direction);
    if (!entry)
        return NULL;

    while (entry && entry->playlist_path && current->playlist_path &&
           strcmp(entry->playlist_path, current->playlist_path) == 0)
        entry = playlist_entry_get_rel(entry, direction);

    if (direction < 0)
        entry = playlist_get_first_in_same_playlist(entry,
                                                    current->playlist_path);

    return entry;
}

struct playlist_entry *playlist_get_first_in_same_playlist(
    struct playlist_entry *entry, char *current_playlist_path)
{
    void *tmp = talloc_new(NULL);

    if (!entry || !entry->playlist_path)
        goto exit;

    char* playlist_path = talloc_strdup(tmp, entry->playlist_path);
    mp_path_strip_trailing_separator(playlist_path);
    if (bstr_startswith(bstr0(current_playlist_path),
                        bstr0(talloc_strdup_append(playlist_path, "/")))
#if HAVE_DOS_PATHS
        ||
        bstr_startswith(bstr0(current_playlist_path),
                        bstr0(talloc_strdup_append(playlist_path, "\\")))
#endif
       )
        goto exit;

    struct playlist_entry *prev = playlist_entry_get_rel(entry, -1);

    while (prev && prev->playlist_path &&
           strcmp(prev->playlist_path, entry->playlist_path) == 0) {
        entry = prev;
        prev = playlist_entry_get_rel(entry, -1);
    }

exit:
    talloc_free(tmp);
    return entry;
}

void playlist_add_base_path(struct playlist *pl, bstr base_path)
{
    if (base_path.len == 0 || bstrcmp0(base_path, ".") == 0)
        return;
    for (int n = 0; n < playlist_entry_count(pl); n++) {
        struct playlist_entry *e = playlist_entry_from_index(pl, n);
        if (!mp_is_url(bstr0(e->filename))) {
            char *new_file = mp_path_join_bstr(e, base_path, bstr0(e->filename));
            talloc_free(e->filename);
            e->filename = new_file;
        }
    }
}

void playlist_set_stream_flags(struct playlist *pl, int flags)
{
    for (int n = 0; n < playlist_entry_count(pl); n++)
        playlist_entry_from_index(pl, n)->stream_flags = flags;
}

int64_t playlist_transfer_entries_to(struct playlist *pl, int dst_index,
                                     struct playlist *source_pl)
{
    mp_assert(pl != source_pl);
    int count = playlist_entry_count(source_pl);
    struct playlist_entry **entries =
        talloc_array(NULL, struct playlist_entry *, count ? count : 1);
    for (int n = 0; n < count; n++)
        entries[n] = playlist_entry_from_index(source_pl, n);

    int64_t first_id = 0;
    for (int n = 0; n < count; n++) {
        struct playlist_entry *e = entries[n];
        player_ffi_playlist_remove(source_pl->handle, e);
        uint64_t id = player_ffi_playlist_insert_at(pl->handle, e,
                                                    dst_index + n);
        mp_assert(id);
        e->pl = pl;
        e->id = id;
        talloc_steal(pl, e);
        if (e->playlist_path)
            talloc_steal(pl, e->playlist_path);
        if (n == 0)
            first_id = id;
    }

    playlist_set_completed(pl, playlist_completed(source_pl));
    playlist_set_started(pl, playlist_started(source_pl));

    talloc_free(entries);
    return first_id;
}

int64_t playlist_transfer_entries(struct playlist *pl, struct playlist *source_pl)
{
    int add_at = playlist_entry_count(pl);
    struct playlist_entry *current = playlist_get_current(pl);
    if (current) {
        add_at = playlist_entry_to_index(pl, current) + 1;
        if (playlist_current_was_replaced(pl))
            add_at += 1;
    }
    mp_assert(add_at >= 0);
    mp_assert(add_at <= playlist_entry_count(pl));

    return playlist_transfer_entries_to(pl, add_at, source_pl);
}

int64_t playlist_append_entries(struct playlist *pl, struct playlist *source_pl)
{
    return playlist_transfer_entries_to(pl, playlist_entry_count(pl), source_pl);
}

int playlist_entry_to_index(struct playlist *pl, struct playlist_entry *e)
{
    return player_ffi_playlist_index_of(pl->handle, e);
}

int playlist_entry_count(struct playlist *pl)
{
    return player_ffi_playlist_count(pl->handle);
}

struct playlist_entry *playlist_entry_from_index(struct playlist *pl, int index)
{
    return player_ffi_playlist_entry_at(pl->handle, index);
}

struct playlist *playlist_parse_file(const char *file, struct mp_cancel *cancel,
                                     struct mpv_global *global)
{
    struct mp_log *log = mp_log_new(NULL, global->log, "!playlist_parser");
    mp_verbose(log, "Parsing playlist file %s...\n", file);

    char *path = mp_get_user_path(NULL, global, file);
    struct demuxer_params p = {
        .force_format = "playlist",
        .stream_flags = STREAM_ORIGIN_DIRECT,
    };
    struct demuxer *d = demux_open_url(path, &p, cancel, global);
    struct playlist *ret = NULL;
    if (!d)
        goto done;

    if (d && d->playlist) {
        ret = playlist_new(NULL);
        playlist_populate_playlist_path(d->playlist, file);
        playlist_transfer_entries(ret, d->playlist);
    }
    demux_free(d);

    if (ret) {
        mp_verbose(log, "Playlist successfully parsed\n");
    } else {
        mp_err(log, "Error while parsing playlist\n");
    }

    if (ret && !playlist_entry_count(ret))
        mp_warn(log, "Warning: empty playlist\n");

done:
    talloc_free(log);
    talloc_free(path);
    return ret;
}

void playlist_set_current(struct playlist *pl)
{
    if (!pl->playlist_dir)
        return;

    for (int i = 0; i < playlist_entry_count(pl); ++i) {
        struct playlist_entry *entry = playlist_entry_from_index(pl, i);
        if (!entry->playlist_path)
            continue;
        char *path = entry->playlist_path;
        if (path[0] != '.')
            path = mp_path_join(NULL, pl->playlist_dir,
                                mp_basename(entry->playlist_path));
        bool same = !strcmp(entry->filename, path);
        if (path != entry->playlist_path)
            talloc_free(path);
        if (same) {
            playlist_set_current_entry(pl, entry);
            break;
        }
    }
}

struct playlist_entry *playlist_get_current(struct playlist *pl)
{
    return player_ffi_playlist_current(pl->handle);
}

void playlist_set_current_entry(struct playlist *pl, struct playlist_entry *e)
{
    player_ffi_playlist_set_current(pl->handle, e);
}

bool playlist_current_was_replaced(struct playlist *pl)
{
    return player_ffi_playlist_current_was_replaced(pl->handle) != 0;
}

void playlist_set_current_was_replaced(struct playlist *pl, bool value)
{
    player_ffi_playlist_set_current_was_replaced(pl->handle, value);
}

bool playlist_completed(struct playlist *pl)
{
    return player_ffi_playlist_completed(pl->handle) != 0;
}

void playlist_set_completed(struct playlist *pl, bool value)
{
    player_ffi_playlist_set_completed(pl->handle, value);
}

bool playlist_started(struct playlist *pl)
{
    return player_ffi_playlist_started(pl->handle) != 0;
}

void playlist_set_started(struct playlist *pl, bool value)
{
    player_ffi_playlist_set_started(pl->handle, value);
}
