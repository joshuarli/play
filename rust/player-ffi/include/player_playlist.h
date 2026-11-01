#ifndef PLAYER_PLAYLIST_H
#define PLAYER_PLAYLIST_H

#include <stdint.h>

struct player_playlist;

struct player_playlist *player_ffi_playlist_create(void);
void player_ffi_playlist_destroy(struct player_playlist *handle);
uint64_t player_ffi_playlist_insert_at(struct player_playlist *handle,
                                       void *entry, int index);
int player_ffi_playlist_remove(struct player_playlist *handle, void *entry);
int player_ffi_playlist_count(struct player_playlist *handle);
void *player_ffi_playlist_entry_at(struct player_playlist *handle, int index);
int player_ffi_playlist_index_of(struct player_playlist *handle, void *entry);
void *player_ffi_playlist_first(struct player_playlist *handle);
void *player_ffi_playlist_last(struct player_playlist *handle);
void *player_ffi_playlist_next(struct player_playlist *handle, int direction);
void *player_ffi_playlist_entry_rel(struct player_playlist *handle,
                                    void *entry, int direction);
void *player_ffi_playlist_current(struct player_playlist *handle);
void player_ffi_playlist_set_current(struct player_playlist *handle,
                                     void *entry);
int player_ffi_playlist_current_was_replaced(struct player_playlist *handle);
void player_ffi_playlist_set_current_was_replaced(struct player_playlist *handle,
                                                  int value);
int player_ffi_playlist_completed(struct player_playlist *handle);
void player_ffi_playlist_set_completed(struct player_playlist *handle,
                                       int value);
int player_ffi_playlist_started(struct player_playlist *handle);
void player_ffi_playlist_set_started(struct player_playlist *handle, int value);
int player_ffi_playlist_move(struct player_playlist *handle, void *entry,
                             void *at);

#endif
