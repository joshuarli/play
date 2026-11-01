#include "config.h"

#include "common/common.h"
#include "common/msg.h"
#include "input/input.h"
#include "options/path.h"
#include "player/core.h"
#include "mpv/client.h"
#include "player_lua.h"

static void host_log(void *ctx, int level, const char *message)
{
    struct mp_script_args *args = ctx;
    mp_msg(args->log, level, "%s", message);
}

static char *host_find_config_file(void *ctx, const char *name)
{
    struct mp_script_args *args = ctx;
    return mp_find_config_file(NULL, args->mpctx->global, name);
}

static void host_free_string(void *ctx, char *value)
{
    talloc_free(value);
}

static void host_set_mouse_area(void *ctx, const char *section,
                                int x0, int y0, int x1, int y1)
{
    struct mp_script_args *args = ctx;
    mp_input_set_section_mouse_area(args->mpctx->input, (char *)section,
                                    x0, y0, x1, y1);
}

static int load_lua_host(struct mp_script_args *args)
{
    const struct player_lua_callbacks callbacks = {
        .ctx = args,
        .log = host_log,
        .find_config_file = host_find_config_file,
        .free_string = host_free_string,
        .input_set_section_mouse_area = host_set_mouse_area,
    };
    int rc = player_ffi_lua_run(args->client, mpv_client_name(args->client),
                                args->filename, args->path, &callbacks);
    return rc < 0 ? -1 : 0;
}

const struct mp_scripting mp_scripting_lua = {
    .name = "lua",
    .file_ext = "lua",
    .load = load_lua_host,
};
