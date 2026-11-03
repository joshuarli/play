#ifndef PLAYER_STATE_H
#define PLAYER_STATE_H

#include <stddef.h>
#include <stdint.h>

struct player_state;

struct player_state *player_ffi_state_create(void);
void player_ffi_state_destroy(struct player_state *state);
int player_ffi_state_set_bool(struct player_state *state, const char *name, int value);
int player_ffi_state_set_number(struct player_state *state, const char *name,
                                double value);
int player_ffi_state_set_track(struct player_state *state, const char *name,
                               int value);
int player_ffi_state_get_track(struct player_state *state, const char *name,
                               int *out);
int player_ffi_state_get_bool(struct player_state *state, const char *name,
                              int *out);
int player_ffi_state_get_number(struct player_state *state, const char *name,
                                double *out);
int player_ffi_state_set_time_pos(struct player_state *state, double value);
int player_ffi_state_set_duration(struct player_state *state, double value);
int player_ffi_state_set_eof_reached(struct player_state *state, int value);
int player_ffi_state_describe(struct player_state *state, char *out, size_t cap);
uint64_t player_ffi_state_sequence(struct player_state *state);

#endif
