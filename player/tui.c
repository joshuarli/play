#include <stddef.h>
#include <stdio.h>
#include <unistd.h>

#include "common/common.h"
#include "common/msg.h"
#include "core.h"
#include "client.h"
#include "mpv/client.h"
#include "osdep/threads.h"

#include "album_art.h"
#include "player_tui.h"
#include "tui.h"

struct tui_host {
    struct MPContext *mpctx;
    mpv_handle *client;
};

static int cover_art(void *ctx, const char *path, int size,
                     unsigned char *out, size_t cap)
{
    return player_tui_cover_art(ctx, path, size, out, cap);
}

static MP_THREAD_VOID tui_thread(void *p)
{
    struct tui_host *host = p;
    char error[512] = {0};
    const player_tui_callbacks callbacks = {
        .ctx = NULL,
        .cover_art = cover_art,
    };
    if (player_ffi_tui_run(host->client, &callbacks, error, sizeof(error)) < 0) {
        MP_ERR(host->mpctx, "terminal UI: %s\n", error[0] ? error : "unknown error");
        mpv_command(host->client, (const char *[]){"quit", NULL});
    }
    mpv_destroy(host->client);
    talloc_free(host);
    MP_THREAD_RETURN();
}

int tui_start(struct MPContext *mpctx)
{
    if (!isatty(STDIN_FILENO) || !isatty(STDOUT_FILENO)) {
        fprintf(stderr, "player: --tui needs an interactive terminal\n");
        return -1;
    }

    mpv_handle *client = mp_new_client(mpctx->clients, "tui");
    if (!client)
        return -1;
    mp_client_set_weak(client);

    struct tui_host *host = talloc_zero(NULL, struct tui_host);
    host->mpctx = mpctx;
    host->client = client;

    mp_thread thread;
    if (mp_thread_create(&thread, tui_thread, host)) {
        mpv_destroy(client);
        talloc_free(host);
        return -1;
    }
    mp_thread_detach(thread);
    return 0;
}
