#ifndef PLAYER_ALBUM_ART_H
#define PLAYER_ALBUM_ART_H

#include <stddef.h>

int player_tui_cover_art(void *ctx, const char *path, int size,
                         unsigned char *out, size_t cap);

#endif
