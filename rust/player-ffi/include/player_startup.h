#ifndef PLAYER_STARTUP_H
#define PLAYER_STARTUP_H

#include <stddef.h>

typedef struct player_startup_sink {
    void *ctx;
    void (*add_file)(void *ctx, const char *path);
    void (*set_fullscreen)(void *ctx, int value);
    void (*set_start)(void *ctx, int type, double pos);
    void (*append_path)(void *ctx, int is_sub, const char *path);
    void (*set_paths)(void *ctx, int is_sub, const char *const *paths, size_t count);
    void (*add_remaining)(void *ctx, const char *arg);
} player_startup_sink;

int player_ffi_startup_parse(int argc, char **argv,
                             const player_startup_sink *sink,
                             char *err, size_t err_cap);

#endif
