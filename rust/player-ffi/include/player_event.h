#ifndef PLAYER_EVENT_H
#define PLAYER_EVENT_H

#include <stddef.h>
#include <stdint.h>

struct player_event_bus;

struct player_event_bus *player_ffi_event_create(void);
void player_ffi_event_destroy(struct player_event_bus *bus);
uint64_t player_ffi_event_push(struct player_event_bus *bus, const char *name);
uint64_t player_ffi_event_next_sequence(struct player_event_bus *bus);

#endif
