#ifndef MPLAYER_PLAYLIST_H
#define MPLAYER_PLAYLIST_H

#include <stdbool.h>
#include <stdint.h>
#include "misc/bstr.h"

struct player_playlist;

struct playlist_param {
    bstr name, value;
};

struct playlist_entry {
    struct playlist *pl;

    uint64_t id;

    char *filename;
    char *playlist_path;

    struct playlist_param *params;
    int num_params;

    char *title;

    bool playlist_prev_attempt : 1;
    bool init_failed : 1;
    bool removed : 1;
    int reserved;
    int stream_flags;
};

struct playlist {
    struct player_playlist *handle;
    char *playlist_dir;
};

struct playlist *playlist_new(void *talloc_ctx);

void playlist_entry_add_param(struct playlist_entry *e, bstr name, bstr value);
void playlist_entry_add_params(struct playlist_entry *e,
                               struct playlist_param *params,
                               int params_count);

struct playlist_entry *playlist_entry_new(const char *filename);

void playlist_insert_at(struct playlist *pl, struct playlist_entry *entry,
                        struct playlist_entry *at);

void playlist_remove(struct playlist *pl, struct playlist_entry *entry);
void playlist_clear(struct playlist *pl);
void playlist_clear_except_current(struct playlist *pl);

void playlist_move(struct playlist *pl, struct playlist_entry *entry,
                   struct playlist_entry *at);

void playlist_append_file(struct playlist *pl, const char *filename);
void playlist_populate_playlist_path(struct playlist *pl, const char *path);
struct playlist_entry *playlist_get_first(struct playlist *pl);
struct playlist_entry *playlist_get_last(struct playlist *pl);
struct playlist_entry *playlist_get_next(struct playlist *pl, int direction);
struct playlist_entry *playlist_entry_get_rel(struct playlist_entry *e,
                                              int direction);
struct playlist_entry *playlist_get_first_in_next_playlist(struct playlist *pl,
                                                           int direction);
struct playlist_entry *playlist_get_first_in_same_playlist(struct playlist_entry *entry,
                                                           char *current_playlist_path);
void playlist_add_base_path(struct playlist *pl, bstr base_path);
void playlist_set_stream_flags(struct playlist *pl, int flags);
int64_t playlist_transfer_entries_to(struct playlist *pl, int dst_index,
                                     struct playlist *source_pl);
int64_t playlist_transfer_entries(struct playlist *pl, struct playlist *source_pl);
int64_t playlist_append_entries(struct playlist *pl, struct playlist *source_pl);

int playlist_entry_to_index(struct playlist *pl, struct playlist_entry *e);
int playlist_entry_count(struct playlist *pl);
struct playlist_entry *playlist_entry_from_index(struct playlist *pl, int index);

struct mp_cancel;
struct mpv_global;
struct playlist *playlist_parse_file(const char *file, struct mp_cancel *cancel,
                                     struct mpv_global *global);

void playlist_entry_unref(struct playlist_entry *e);

void playlist_set_current(struct playlist *pl);

struct playlist_entry *playlist_get_current(struct playlist *pl);
void playlist_set_current_entry(struct playlist *pl, struct playlist_entry *e);
bool playlist_current_was_replaced(struct playlist *pl);
void playlist_set_current_was_replaced(struct playlist *pl, bool value);
bool playlist_completed(struct playlist *pl);
void playlist_set_completed(struct playlist *pl, bool value);
bool playlist_started(struct playlist *pl);
void playlist_set_started(struct playlist *pl, bool value);

#endif
