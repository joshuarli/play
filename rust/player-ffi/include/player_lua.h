#ifndef PLAYER_LUA_H
#define PLAYER_LUA_H

#include <stdint.h>

typedef struct player_lua_callbacks {
    void *ctx;
    void (*log)(void *ctx, int level, const char *message);
    char *(*find_config_file)(void *ctx, const char *name);
    void (*free_string)(void *ctx, char *value);
    void (*input_set_section_mouse_area)(void *ctx, const char *section,
                                         int x0, int y0, int x1, int y1);
} player_lua_callbacks;

int player_ffi_lua_run(void *client, const char *name, const char *filename,
                       const char *path, const player_lua_callbacks *callbacks);

#endif
