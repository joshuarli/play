#ifndef PLAYER_TUI_CALLBACKS_H
#define PLAYER_TUI_CALLBACKS_H

#include <stddef.h>

typedef struct player_tui_callbacks {
    void *ctx;
    int (*cover_art)(void *ctx, const char *path, int size,
                     unsigned char *out, size_t cap);
} player_tui_callbacks;

int player_ffi_tui_run(void *client, const player_tui_callbacks *callbacks,
                       char *err, size_t err_cap);

#endif
