/*
 * This file is part of mpv.
 *
 * mpv is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * mpv is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with mpv.  If not, see <http://www.gnu.org/licenses/>.
 */

#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <math.h>
#include <assert.h>
#include <string.h>
#include <locale.h>

#include "config.h"

#include <libplacebo/config.h>

#include "mpv_talloc.h"

#include "misc/dispatch.h"
#include "misc/random.h"
#include "misc/thread_pool.h"
#include "osdep/io.h"
#include "osdep/terminal.h"
#include "osdep/threads.h"
#include "osdep/timer.h"
#include "osdep/main-fn.h"

#include "common/av_log.h"
#include "common/codecs.h"
#include "options/m_config.h"
#include "options/m_config_frontend.h"
#include "options/m_option.h"
#include "options/m_property.h"
#include "common/common.h"
#include "common/msg.h"
#include "common/msg_control.h"
#include "common/stats.h"
#include "common/global.h"
#include "filters/f_decoder_wrapper.h"
#include "options/parse_configfile.h"
#include "options/parse_commandline.h"
#include "common/playlist.h"
#include "options/options.h"
#include "options/path.h"
#include "input/input.h"
#include "demux/packet_pool.h"

#include "audio/out/ao.h"
#include "misc/thread_tools.h"
#include "sub/osd.h"
#include "video/out/vo.h"

#include "core.h"
#include "client.h"
#include "player_ffi.h"
#include "player_startup.h"
#include "player_state.h"
#include "player_event.h"
#include "command.h"
#include "screenshot.h"
#include "tui.h"

static const char def_config[] =
#include "etc/builtin.conf.inc"
;

#if HAVE_COCOA
#include <mach-o/dyld.h>
#include <unistd.h>
#include "osdep/mac/app_bridge.h"
#endif

#ifndef FULLCONFIG
#define FULLCONFIG "(missing)\n"
#endif

enum exit_reason {
    EXIT_NONE,
    EXIT_NORMAL,
    EXIT_ERROR,
};

const char mp_help_text[] =
"Usage:   mpv [options] [url|path/]filename\n"
"\n"
"Basic options:\n"
" --start=<time>    seek to given (percent, seconds, or hh:mm:ss) position\n"
" --no-audio        do not play sound\n"
" --no-video        do not play video\n"
" --fs              fullscreen playback\n"
" --tui             play audio in the terminal\n"
" --sub-file=<file> specify subtitle file to use\n"
" --playlist=<file> specify playlist file\n"
"\n"
" --list-options    list all mpv options\n"
" --h=<string>      print options which contain the given string in their name\n";

static mp_static_mutex terminal_owner_lock = MP_STATIC_MUTEX_INITIALIZER;
static struct MPContext *terminal_owner;

static bool cas_terminal_owner(struct MPContext *old, struct MPContext *new)
{
    mp_mutex_lock(&terminal_owner_lock);
    bool r = terminal_owner == old;
    if (r)
        terminal_owner = new;
    mp_mutex_unlock(&terminal_owner_lock);
    return r;
}

void mp_update_logging(struct MPContext *mpctx, bool preinit)
{
    bool had_log_file = mp_msg_has_log_file(mpctx->global);

    mp_msg_update_msglevels(mpctx->global, mpctx->opts);

    bool enable = mpctx->opts->use_terminal;
    bool enabled = cas_terminal_owner(mpctx, mpctx);
    if (enable != enabled) {
        if (enable && cas_terminal_owner(NULL, mpctx)) {
            terminal_init();
            enabled = true;
        } else if (!enable) {
            terminal_uninit();
            cas_terminal_owner(mpctx, NULL);
        }
    }

    if (mp_msg_has_log_file(mpctx->global) && !had_log_file) {
        // for log-file=... in config files.
        // we did flush earlier messages, but they were in a cyclic buffer, so
        // the version might have been overwritten. ensure we have it.
        mp_print_version(mpctx->log, false);
    }

    if (enabled && !preinit && mpctx->opts->consolecontrols)
        terminal_setup_getch(mpctx->input);
}

void mp_print_version(struct mp_log *log, int always)
{
    int v = always ? MSGL_INFO : MSGL_V;
    mp_msg(log, v, "%s %s\n", mpv_version, mpv_copyright);
    if (strcmp(mpv_builddate, "UNKNOWN"))
        mp_msg(log, v, " built on %s\n", mpv_builddate);
    mp_msg(log, v, "libplacebo version: %s\n", PL_VERSION);
    check_library_versions(log, v);
    // Only in verbose mode.
    if (!always) {
        mp_msg(log, MSGL_V, "Configuration: " CONFIGURATION "\n");
        mp_msg(log, MSGL_V, "List of enabled features: " FULLCONFIG "\n");
        #ifdef NDEBUG
            mp_msg(log, MSGL_V, "Built with NDEBUG.\n");
        #endif
    }
}

void mp_destroy(struct MPContext *mpctx)
{
    mp_shutdown_clients(mpctx);

    mp_uninit_ipc(mpctx->ipc_ctx);
    mpctx->ipc_ctx = NULL;

    uninit_audio_out(mpctx);
    uninit_video_out(mpctx);

    command_uninit(mpctx);

    mp_clients_destroy(mpctx);

    osd_free(mpctx->osd);

#if HAVE_COCOA
    cocoa_set_input_context(NULL);
#endif

    if (cas_terminal_owner(mpctx, mpctx)) {
        terminal_uninit();
        cas_terminal_owner(mpctx, NULL);
    }

    mp_input_uninit(mpctx->input);
    mp_clipboard_destroy(mpctx->clipboard);

    uninit_libav(mpctx->global);

    mp_msg_uninit(mpctx->global);
    mp_assert(!mpctx->num_abort_list);
    talloc_free(mpctx->abort_list);
    mp_mutex_destroy(&mpctx->abort_lock);
    player_ffi_state_destroy(mpctx->control_state);
    mpctx->control_state = NULL;
    player_ffi_event_destroy(mpctx->event_bus);
    mpctx->event_bus = NULL;
    talloc_free(mpctx->mconfig); // destroy before dispatch
    talloc_free(mpctx);
}

static bool handle_help_options(struct MPContext *mpctx)
{
    struct MPOpts *opts = mpctx->opts;
    struct mp_log *log = mpctx->log;
    if (opts->ao_opts->audio_device &&
        strcmp(opts->ao_opts->audio_device, "help") == 0)
    {
        ao_print_devices(mpctx->global, log, mpctx->ao);
        return true;
    }
    if (opts->property_print_help) {
        property_print_help(mpctx);
        return true;
    }
    return false;
}

static int cfg_include(void *ctx, char *filename, int flags)
{
#ifdef FUZZING_BUILD_MODE_UNSAFE_FOR_PRODUCTION
    return 1;
#endif
    struct MPContext *mpctx = ctx;
    char *fname = mp_get_user_path(NULL, mpctx->global, filename);
    int r = m_config_parse_config_file(mpctx->mconfig, mpctx->global, fname, NULL, flags);
    talloc_free(fname);
    return r;
}

// We mostly care about LC_NUMERIC, and how "." vs. "," is treated,
// Other locale stuff might break too, but probably isn't too bad.
static bool check_locale(void)
{
    char *name = setlocale(LC_NUMERIC, NULL);
    return !name || strcmp(name, "C") == 0 || strcmp(name, "C.UTF-8") == 0;
}

struct MPContext *mp_create(void)
{
    if (!check_locale()) {
        // Normally, we never print anything (except if the "terminal" option
        // is enabled), so this is an exception.
        fprintf(stderr, "Non-C locale detected. This is not supported.\n"
                        "Call 'setlocale(LC_NUMERIC, \"C\");' in your code.\n");
        return NULL;
    }

    char *enable_talloc = getenv("MPV_LEAK_REPORT");
    if (enable_talloc && strcmp(enable_talloc, "1") == 0)
        talloc_enable_leak_report();

    mp_time_init();

    struct MPContext *mpctx = talloc(NULL, MPContext);
    *mpctx = (struct MPContext){
        .last_chapter = -2,
        .term_osd_contents = talloc_strdup(mpctx, ""),
        .osd_progbar = { .type = -1 },
        .playlist = playlist_new(mpctx),
        .control_state = player_ffi_state_create(),
        .event_bus = player_ffi_event_create(),
        .dispatch = mp_dispatch_create(mpctx),
        .playback_abort = mp_cancel_new(mpctx),
        .thread_pool = mp_thread_pool_create(mpctx, 0, 1, 30),
        .stop_play = PT_NEXT_ENTRY,
        .play_dir = 1,
    };

    mp_mutex_init(&mpctx->abort_lock);

    mpctx->global = talloc_zero(mpctx, struct mpv_global);

    demux_packet_pool_init(mpctx->global);
    stats_global_init(mpctx->global);

    // Nothing must call mp_msg*() and related before this
    mp_msg_init(mpctx->global);
    mpctx->log = mp_log_new(mpctx, mpctx->global->log, "!cplayer");
    mpctx->statusline = mp_log_new(mpctx, mpctx->log, "!statusline");

    mpctx->stats = stats_ctx_create(mpctx, mpctx->global, "main");

    // Create the config context and register the options
    mpctx->mconfig = m_config_new(mpctx, mpctx->log, &mp_opt_root);
    mpctx->opts = mpctx->mconfig->optstruct;
    mpctx->global->config = mpctx->mconfig->shadow;
    mpctx->mconfig->includefunc = cfg_include;
    mpctx->mconfig->includefunc_ctx = mpctx;
    mpctx->mconfig->use_profiles = true;
    mpctx->mconfig->is_toplevel = true;
    mpctx->mconfig->global = mpctx->global;
    m_config_parse(mpctx->mconfig, "", bstr0(def_config), NULL, 0);

    mpctx->input = mp_input_init(mpctx->global, mp_wakeup_core_cb, mpctx);
    screenshot_init(mpctx);
    command_init(mpctx);
    init_libav(mpctx->global);
    mp_clients_init(mpctx);
    mpctx->osd = osd_create(mpctx->global);

#if HAVE_COCOA
    cocoa_set_input_context(mpctx->input);
#endif

    char *verbose_env = getenv("MPV_VERBOSE");
    if (verbose_env)
        mpctx->opts->verbose = atoi(verbose_env);

    mp_cancel_trigger(mpctx->playback_abort);

    return mpctx;
}

struct startup_ctx {
    struct MPContext *mpctx;
    void *tmp;
    char **remaining;
    int num_remaining;
};

static void startup_add_file(void *p, const char *path)
{
    struct startup_ctx *ctx = p;
    playlist_append_file(ctx->mpctx->playlist, path);
}

static int startup_add_playlist(void *p, const char *path)
{
    struct startup_ctx *ctx = p;
    struct playlist *pl = playlist_parse_file(path, NULL, ctx->mpctx->global);
    if (!pl)
        return -1;
    playlist_transfer_entries(ctx->mpctx->playlist, pl);
    talloc_free(pl);
    return 0;
}

static void startup_set_fullscreen(void *p, int value)
{
    struct startup_ctx *ctx = p;
    ctx->mpctx->opts->vo->fullscreen = value != 0;
}

static void startup_set_tui(void *p, int value)
{
    struct startup_ctx *ctx = p;
    ctx->mpctx->tui = value != 0;
}

static void startup_set_start(void *p, int type, double pos)
{
    struct startup_ctx *ctx = p;
    ctx->mpctx->opts->play_start = (struct m_rel_time){
        .pos = pos,
        .type = (enum m_rel_time_type)type,
    };
}

static void startup_pathlist_append(struct MPOpts *opts, char ***list,
                                    const char *path)
{
    int count = 0;
    while (*list && (*list)[count])
        count++;
    *list = talloc_realloc(opts, *list, char *, count + 2);
    (*list)[count] = talloc_strdup(*list, path);
    (*list)[count + 1] = NULL;
}

static void startup_append_path(void *p, int is_sub, const char *path)
{
    struct startup_ctx *ctx = p;
    struct MPOpts *opts = ctx->mpctx->opts;
    startup_pathlist_append(opts,
        is_sub ? &opts->sub_name : &opts->audio_files, path);
}

static void startup_set_paths(void *p, int is_sub, const char *const *paths,
                              size_t count)
{
    struct startup_ctx *ctx = p;
    struct MPOpts *opts = ctx->mpctx->opts;
    char ***list = is_sub ? &opts->sub_name : &opts->audio_files;
    talloc_free(*list);
    *list = NULL;
    for (size_t i = 0; i < count; i++)
        startup_pathlist_append(opts, list, paths[i]);
}

static void startup_add_remaining(void *p, const char *arg)
{
    struct startup_ctx *ctx = p;
    ctx->remaining = talloc_realloc(ctx->tmp, ctx->remaining, char *,
                                    ctx->num_remaining + 2);
    ctx->remaining[ctx->num_remaining++] = talloc_strdup(ctx->tmp, arg);
    ctx->remaining[ctx->num_remaining] = NULL;
}

static bool startup_requests_tui(char **options)
{
    bool tui = false;
    for (int index = 0; options && options[index]; index++) {
        if (!strcmp(options[index], "--"))
            break;
        if (!strcmp(options[index], "--tui") || !strcmp(options[index], "--tui=yes") ||
            !strcmp(options[index], "--tui=true") || !strcmp(options[index], "--tui=1"))
            tui = true;
        else if (!strcmp(options[index], "--no-tui") || !strcmp(options[index], "--tui=no") ||
                 !strcmp(options[index], "--tui=false") || !strcmp(options[index], "--tui=0"))
            tui = false;
    }
    return tui;
}

static int mp_parse_startup(struct MPContext *mpctx, char **options)
{
    void *tmp = talloc_new(NULL);
    struct startup_ctx ctx = {
        .mpctx = mpctx,
        .tmp = tmp,
    };
    const struct player_startup_sink sink = {
        .ctx = &ctx,
        .add_file = startup_add_file,
        .add_playlist = startup_add_playlist,
        .set_fullscreen = startup_set_fullscreen,
        .set_tui = startup_set_tui,
        .set_start = startup_set_start,
        .append_path = startup_append_path,
        .set_paths = startup_set_paths,
        .add_remaining = startup_add_remaining,
    };
    int argc = 0;
    while (options[argc])
        argc++;
    char err[256] = {0};
    int result = player_ffi_startup_parse(argc, options, &sink, err, sizeof(err));
    if (result < 0) {
        MP_FATAL(mpctx, "Error parsing command line: %s\n",
                 err[0] ? err : "unknown error");
        talloc_free(tmp);
        return -1;
    }
    if (mpctx->tui) {
        startup_add_remaining(&ctx, "--terminal=no");
        startup_add_remaining(&ctx, "--input-terminal=no");
        startup_add_remaining(&ctx, "--vo=null");
        startup_add_remaining(&ctx, "--force-window=no");
        startup_add_remaining(&ctx, "--osc=no");
    }
    if (ctx.num_remaining) {
        int r = m_config_parse_mp_command_line(mpctx->mconfig, mpctx->playlist,
                                               mpctx->global, ctx.remaining);
        if (r < 0) {
            talloc_free(tmp);
            return r == M_OPT_EXIT ? 1 : -1;
        }
    }
    talloc_free(tmp);
    return 0;
}

// Finish mpctx initialization. This must be done after setting up all options.
// Some of the initializations depend on the options, and can't be changed or
// undone later.
// If options is not NULL, apply them as command line player arguments.
// Returns: 0 on success, -1 on error, 1 if exiting normally (e.g. help).
int mp_initialize(struct MPContext *mpctx, char **options)
{
    struct MPOpts *opts = mpctx->opts;

    mp_assert(!mpctx->initialized);

    // Preparse the command line, so we can init the terminal early.
    if (options) {
        m_config_preparse_command_line(mpctx->mconfig, mpctx->global,
                                       &opts->verbose, options);
    }
    if (options && startup_requests_tui(options)) {
        opts->use_terminal = false;
        opts->consolecontrols = false;
    }

    mp_init_paths(mpctx->global, opts);
    mp_msg_set_early_logging(mpctx->global, true);
    mp_update_logging(mpctx, true);

    if (options) {
        MP_VERBOSE(mpctx, "Command line options:");
        for (int i = 0; options[i]; i++)
            MP_VERBOSE(mpctx, " '%s'", options[i]);
        MP_VERBOSE(mpctx, "\n");
    }

    mp_print_version(mpctx->log, false);
    MP_VERBOSE(mpctx, "player-core ABI %u\n", player_ffi_abi_version());

    mp_parse_cfgfiles(mpctx);

    if (options) {
        int r = mp_parse_startup(mpctx, options);
        if (r)
            return r;
    }

    if (mpctx->tui) {
        opts->use_terminal = false;
        opts->consolecontrols = false;
        opts->lua_load_osc = false;
        opts->force_vo = 0;
        if (m_config_set_option_cli(mpctx->mconfig, bstr0("vo"), bstr0("null"), 0) < 0 ||
            m_config_set_option_cli(mpctx->mconfig, bstr0("force-window"), bstr0("no"), 0) < 0)
            return -1;
    }

    if (opts->operation_mode == 1) {
        m_config_set_profile(mpctx->mconfig, "builtin-pseudo-gui",
                             M_SETOPT_NO_OVERWRITE);
        m_config_set_profile(mpctx->mconfig, "pseudo-gui", 0);
    }

#if HAVE_COCOA
    if (getenv("MPVBUNDLE") && opts->player_idle_mode == 0 &&
        !playlist_entry_count(mpctx->playlist))
        opts->player_idle_mode = 2;
#endif

    // Backup the default settings, which should not be stored in the resume
    // config files. This explicitly includes values set by config files and
    // the command line.
    m_config_backup_watch_later_opts(mpctx->mconfig);

    mp_control_state_sync(mpctx);

    mp_input_load_config(mpctx->input);

    // From this point on, all mpctx members are initialized.
    mpctx->initialized = true;
    mpctx->mconfig->option_change_callback = mp_option_change_callback;
    mpctx->mconfig->option_change_callback_ctx = mpctx;
    m_config_set_update_dispatch_queue(mpctx->mconfig, mpctx->dispatch);
    // Run all update handlers.
    mp_option_change_callback(mpctx, NULL, UPDATE_OPTS_MASK, false);
    handle_option_callbacks(mpctx);

    if (handle_help_options(mpctx))
        return 1; // help

    check_library_versions(mp_null_log, 0);

    if (!playlist_entry_count(mpctx->playlist) && !opts->player_idle_mode &&
        options)
    {
        // nothing to play
        mp_print_version(mpctx->log, true);
        MP_INFO(mpctx, "%s", mp_help_text);
        return 1;
    }

    MP_STATS(mpctx, "start init");

#if HAVE_COCOA
    mpv_handle *ctx = mp_new_client(mpctx->clients, "mac");
    cocoa_set_mpv_handle(ctx);
#endif

    mpctx->ipc_ctx = mp_init_ipc(mpctx->clients, mpctx->global);

    mp_load_scripts(mpctx);

    if (opts->force_vo == 2 && handle_force_window(mpctx, false) < 0)
        return -1;

    // Needed to properly enter _initial_ idle mode if playlist empty.
    if (mpctx->opts->player_idle_mode && !playlist_entry_count(mpctx->playlist))
        mpctx->stop_play = PT_STOP;

    if (mpctx->tui && tui_start(mpctx) < 0)
        return -1;

    MP_STATS(mpctx, "end init");

    return 0;
}

#if HAVE_COCOA
static void set_bundled_vulkan_icd(void)
{
    if (getenv("VK_DRIVER_FILES") || getenv("VK_ICD_FILENAMES"))
        return;
    uint32_t size = 0;
    _NSGetExecutablePath(NULL, &size);
    if (!size)
        return;
    char *path = malloc(size);
    if (!path)
        return;
    if (_NSGetExecutablePath(path, &size) != 0) {
        free(path);
        return;
    }
    char *resolved = realpath(path, NULL);
    free(path);
    if (!resolved)
        return;
    char *slash = strrchr(resolved, '/');
    if (!slash) {
        free(resolved);
        return;
    }
    *slash = '\0';
    char icd[4096];
    snprintf(icd, sizeof(icd),
             "%s/../Resources/vulkan/icd.d/MoltenVK_icd.json", resolved);
    free(resolved);
    if (access(icd, R_OK) == 0) {
        setenv("VK_DRIVER_FILES", icd, 0);
        setenv("VK_ICD_FILENAMES", icd, 0);
    }
}
#endif

static void *mp_host_create(void *ctx)
{
    (void)ctx;
    struct MPContext *mpctx = mp_create();
    if (mpctx)
        mpctx->is_cli = true;
    return mpctx;
}

static int mp_host_init(void *ctx, void *mpctx, char **options)
{
    (void)ctx;
    return mp_initialize(mpctx, options);
}

static void mp_host_play(void *ctx, void *mpctx)
{
    (void)ctx;
    mp_play_files(mpctx);
}

static int mp_host_finish(void *ctx, void *mpctx, int r)
{
    (void)ctx;
    struct MPContext *m = mpctx;
    int rc = 0;
    const char *reason = NULL;
    if (r < 0) {
        reason = "Fatal error";
        rc = 1;
    } else if (r > 0) {
        // nothing
    } else if (m->stop_play == PT_QUIT) {
        reason = "Quit";
    } else if (m->files_played) {
        if (m->files_errored || m->files_broken) {
            reason = "Some errors happened";
            rc = 3;
        } else {
            reason = "End of file";
        }
    } else if (m->files_broken && !m->files_errored) {
        reason = "Errors when loading file";
        rc = 2;
    } else if (m->files_errored) {
        reason = "Interrupted by error";
        rc = 2;
    } else {
        reason = "No files played";
    }

    if (reason)
        MP_INFO(m, "Exiting... (%s)\n", reason);
    if (m->has_quit_custom_rc)
        rc = m->quit_custom_rc;
    return rc;
}

static void mp_host_destroy(void *ctx, void *mpctx)
{
    (void)ctx;
    mp_destroy(mpctx);
}

int mpv_main(int argc, char *argv[])
{
#if HAVE_COCOA
    set_bundled_vulkan_icd();
#endif
    static const player_host_steps steps = {
        .ctx = NULL,
        .create = mp_host_create,
        .init = mp_host_init,
        .play = mp_host_play,
        .finish = mp_host_finish,
        .destroy = mp_host_destroy,
    };
    return player_ffi_run(argc, argv, &steps);
}
