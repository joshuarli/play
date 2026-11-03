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

#include <math.h>

#include <libplacebo/colorspace.h>

#include "options/m_option.h"
#include "options/gl_video_opts.h"
#include "video/out/filter_kernels.h"
#include "lcms.h"
#include "hwdec.h"

#define FIXED_SCALE_KERNELS \
    {"bilinear",             SCALER_BILINEAR}, \
    {"bicubic_fast",         SCALER_BICUBIC_FAST}, \
    {"oversample",           SCALER_OVERSAMPLE}, \

#define NON_POLAR_FILTER_KERNELS \
    {"spline16",      SCALER_SPLINE16}, \
    {"spline36",      SCALER_SPLINE36}, \
    {"spline64",      SCALER_SPLINE64}, \
    {"sinc",          SCALER_SINC}, \
    {"lanczos",       SCALER_LANCZOS}, \
    {"ginseng",       SCALER_GINSENG}, \
    {"bicubic",       SCALER_BICUBIC}, \
    {"hermite",       SCALER_HERMITE}, \
    {"catmull_rom",   SCALER_CATMULL_ROM}, \
    {"mitchell",      SCALER_MITCHELL}, \
    {"robidoux",      SCALER_ROBIDOUX}, \
    {"robidouxsharp", SCALER_ROBIDOUXSHARP}, \
    {"box",           SCALER_BOX}, \
    {"nearest",       SCALER_NEAREST}, \
    {"triangle",      SCALER_TRIANGLE}, \
    {"gaussian",      SCALER_GAUSSIAN}, \

#define POLAR_FILTER_KERNELS \
    {"jinc",                 SCALER_JINC}, \
    {"ewa_lanczos",          SCALER_EWA_LANCZOS}, \
    {"ewa_hanning",          SCALER_EWA_HANNING}, \
    {"ewa_ginseng",          SCALER_EWA_GINSENG}, \
    {"ewa_lanczossharp",     SCALER_EWA_LANCZOSSHARP}, \
    {"ewa_lanczos4sharpest", SCALER_EWA_LANCZOS4SHARPEST}, \
    {"ewa_lanczossoft",      SCALER_EWA_LANCZOSSOFT}, \
    {"haasnsoft",            SCALER_HAASNSOFT}, \
    {"ewa_robidoux",         SCALER_EWA_ROBIDOUX}, \
    {"ewa_robidouxsharp",    SCALER_EWA_ROBIDOUXSHARP}, \

#define FILTER_WINDOWS \
    {"bartlett", WINDOW_BARTLETT}, \
    {"cosine",   WINDOW_COSINE}, \
    {"hanning",  WINDOW_HANNING}, \
    {"tukey",    WINDOW_TUKEY}, \
    {"hamming",  WINDOW_HAMMING}, \
    {"quadric",  WINDOW_QUADRIC}, \
    {"welch",    WINDOW_WELCH}, \
    {"kaiser",   WINDOW_KAISER}, \
    {"blackman", WINDOW_BLACKMAN}, \
    {"sphinx",   WINDOW_SPHINX}, \

static const struct m_opt_choice_alternatives scale_filters[] = {
    FIXED_SCALE_KERNELS
    NON_POLAR_FILTER_KERNELS
    POLAR_FILTER_KERNELS
    FILTER_WINDOWS
    {0},
};

static const struct m_opt_choice_alternatives cdscale_filters[] = {
    {"", SCALER_INHERIT},
    FIXED_SCALE_KERNELS
    NON_POLAR_FILTER_KERNELS
    POLAR_FILTER_KERNELS
    FILTER_WINDOWS
    {0},
};

static const struct m_opt_choice_alternatives tscale_filters[] = {
    {"oversample", SCALER_OVERSAMPLE},
    {"linear",     SCALER_LINEAR},
    NON_POLAR_FILTER_KERNELS
    FILTER_WINDOWS
    {"jinc",       WINDOW_JINC},
    {0},
};

static const struct m_opt_choice_alternatives filter_windows[] = {
    {"",     WINDOW_PREFERRED},
    FILTER_WINDOWS
    {"jinc", WINDOW_JINC},
    {0},
};

static const struct gl_video_opts gl_video_opts_def = {
    .dither_algo = DITHER_FRUIT,
    .dither_size = 6,
    .temporal_dither_period = 1,
    .error_diffusion = "sierra-lite",
    .fbo_format = "auto",
    .sigmoid_center = 0.75,
    .sigmoid_slope = 6.5,
    .scaler = {
        [SCALER_SCALE] =  {
            {SCALER_LANCZOS, .params = {NAN, NAN}, .functions = scale_filters},
            {WINDOW_PREFERRED, .params = {NAN, NAN}, .functions = filter_windows},
        },
        [SCALER_DSCALE] = {
            {SCALER_HERMITE, .params = {NAN, NAN}, .functions = cdscale_filters},
            {WINDOW_PREFERRED, .params = {NAN, NAN}, .functions = filter_windows},
        },
        [SCALER_CSCALE] = {
            {SCALER_INHERIT, .params = {NAN, NAN}, .functions = cdscale_filters},
            {WINDOW_PREFERRED, .params = {NAN, NAN}, .functions = filter_windows},
        },
        [SCALER_TSCALE] = {
            {SCALER_OVERSAMPLE, .params = {NAN, NAN}, .functions = tscale_filters},
            {WINDOW_PREFERRED, .params = {NAN, NAN}, .functions = filter_windows},
        },
    },
    .scaler_resizes_only = true,
    .correct_downscaling = true,
    .linear_downscaling = true,
    .sigmoid_upscaling = true,
    .interpolation_threshold = 0.01,
    .background = BACKGROUND_TILES,
    .background_color = {0, 0, 0, 255},
    .background_tile_color = {{237, 237, 237, 255},
                              {222, 222, 222, 255}},
    .background_tile_size = 16,
    .gamma = 1.0f,
    .tone_map = {
        .curve = TONE_MAPPING_AUTO,
        .curve_param = NAN,
        .max_boost = 1.0,
        .decay_rate = 20.0,
        .scene_threshold_low = 1.0,
        .scene_threshold_high = 3.0,
        .contrast_smoothness = 3.5,
    },
    .early_flush = -1,
    .shader_cache = true,
    .hwdec_interop = "auto",
    .treat_srgb_as_power22 = 1|2|4, // auto
};

#define OPT_BASE_STRUCT struct gl_video_opts

// Use for options which use NAN for defaults.
#define OPT_FLOATDEF(field) \
    OPT_FLOAT(field), \
    .flags = M_OPT_DEFAULT_NAN

#define SCALER_OPTS(n, i) \
    {n"-param1", OPT_FLOATDEF(scaler[i].kernel.params[0])},                \
    {n"-param2", OPT_FLOATDEF(scaler[i].kernel.params[1])},                \
    {n"-blur",   OPT_FLOAT(scaler[i].kernel.blur)},                        \
    {n"-taper",  OPT_FLOAT(scaler[i].kernel.taper), M_RANGE(0.0, 1.0)},    \
    {n"-wparam", OPT_FLOATDEF(scaler[i].window.params[0])},                \
    {n"-wtaper", OPT_FLOAT(scaler[i].window.taper), M_RANGE(0.0, 1.0)},    \
    {n"-clamp",  OPT_FLOAT(scaler[i].clamp), M_RANGE(0.0, 1.0)},           \
    {n"-radius", OPT_FLOAT(scaler[i].radius), M_RANGE(0.5, 16.0)},         \
    {n"-antiring", OPT_FLOAT(scaler[i].antiring), M_RANGE(0.0, 1.0)},      \
    {n"-window", OPT_CHOICE_C(scaler[i].window.function, filter_windows)}

static const struct deband_opts deband_opts_def = {
    .iterations = 1,
    .threshold = 48.0,
    .range = 16.0,
    .grain = 32.0,
};

#undef OPT_BASE_STRUCT
#define OPT_BASE_STRUCT struct deband_opts
static const struct m_sub_options deband_conf = {
    .opts = (const m_option_t[]) {
        {"iterations", OPT_INT(iterations), M_RANGE(0, 16)},
        {"threshold", OPT_FLOAT(threshold), M_RANGE(0.0, 4096.0)},
        {"range", OPT_FLOAT(range), M_RANGE(1.0, 64.0)},
        {"grain", OPT_FLOAT(grain), M_RANGE(0.0, 4096.0)},
        {0}
    },
    .size = sizeof(struct deband_opts),
    .defaults = &deband_opts_def,
};
#undef OPT_BASE_STRUCT
#define OPT_BASE_STRUCT struct gl_video_opts

const struct m_sub_options gl_video_conf = {
    .opts = (const m_option_t[]) {
        {"gpu-dumb-mode", OPT_CHOICE(dumb_mode,
            {"auto", 0}, {"yes", 1}, {"no", -1})},
        {"gamma-factor", OPT_FLOAT(gamma), M_RANGE(0.1, 2.0)},
        {"gamma-auto", OPT_BOOL(gamma_auto),
            .deprecation_message = "replacement: gamma-auto.lua"},
        {"target-prim", OPT_CHOICE_C(target_prim, pl_csp_prim_names)},
        {"target-trc", OPT_CHOICE_C(target_trc, pl_csp_trc_names)},
        {"target-peak", OPT_CHOICE(target_peak, {"auto", 0}),
            M_RANGE(10, 10000)},
        {"hdr-reference-white", OPT_CHOICE(hdr_reference_white, {"auto", 0}),
            M_RANGE(10, 10000)},
        {"sdr-adjust-gamma", OPT_CHOICE(sdr_adjust_gamma,
            {"auto", 0}, {"yes", 1}, {"no", -1})},
        {"treat-srgb-as-power22", OPT_CHOICE(treat_srgb_as_power22,
            {"no", 0}, {"input", 1}, {"output", 2}, {"both", 1|2}, {"auto", 1|2|4})},
        {"target-contrast", OPT_CHOICE(target_contrast, {"auto", 0}, {"inf", -1}),
            M_RANGE(10, 10 / PL_COLOR_HDR_BLACK)},
        {"target-gamut", OPT_CHOICE_C(target_gamut, pl_csp_prim_names)},
        {"tone-mapping", OPT_CHOICE(tone_map.curve,
            {"auto",     TONE_MAPPING_AUTO},
            {"clip",     TONE_MAPPING_CLIP},
            {"mobius",   TONE_MAPPING_MOBIUS},
            {"reinhard", TONE_MAPPING_REINHARD},
            {"hable",    TONE_MAPPING_HABLE},
            {"gamma",    TONE_MAPPING_GAMMA},
            {"linear",   TONE_MAPPING_LINEAR},
            {"spline",   TONE_MAPPING_SPLINE},
            {"bt.2390",  TONE_MAPPING_BT_2390},
            {"bt.2446a", TONE_MAPPING_BT_2446A},
            {"st2094-40", TONE_MAPPING_ST2094_40},
            {"st2094-10", TONE_MAPPING_ST2094_10})},
        {"tone-mapping-param", OPT_FLOATDEF(tone_map.curve_param)},
        {"inverse-tone-mapping", OPT_BOOL(tone_map.inverse)},
        {"tone-mapping-max-boost", OPT_FLOAT(tone_map.max_boost),
            M_RANGE(1.0, 10.0)},
        {"tone-mapping-visualize", OPT_BOOL(tone_map.visualize)},
        {"gamut-mapping-mode", OPT_CHOICE(tone_map.gamut_mode,
            {"auto",        GAMUT_AUTO},
            {"clip",        GAMUT_CLIP},
            {"perceptual",  GAMUT_PERCEPTUAL},
            {"relative",    GAMUT_RELATIVE},
            {"saturation",  GAMUT_SATURATION},
            {"absolute",    GAMUT_ABSOLUTE},
            {"desaturate",  GAMUT_DESATURATE},
            {"darken",      GAMUT_DARKEN},
            {"warn",        GAMUT_WARN},
            {"linear",      GAMUT_LINEAR})},
        {"hdr-compute-peak", OPT_CHOICE(tone_map.compute_peak,
            {"auto", 0},
            {"yes", 1},
            {"no", -1})},
        {"hdr-peak-percentile", OPT_FLOAT(tone_map.peak_percentile),
            M_RANGE(0.0, 100.0)},
        {"hdr-peak-decay-rate", OPT_FLOAT(tone_map.decay_rate),
            M_RANGE(0.0, 1000.0)},
        {"hdr-scene-threshold-low", OPT_FLOAT(tone_map.scene_threshold_low),
            M_RANGE(0, 20.0)},
        {"hdr-scene-threshold-high", OPT_FLOAT(tone_map.scene_threshold_high),
            M_RANGE(0, 20.0)},
        {"hdr-contrast-recovery", OPT_FLOAT(tone_map.contrast_recovery),
            M_RANGE(0, 2.0)},
        {"hdr-contrast-smoothness", OPT_FLOAT(tone_map.contrast_smoothness),
            M_RANGE(1.0, 100.0)},
        {"opengl-pbo", OPT_BOOL(pbo)},
        {"scale", OPT_CHOICE_C(scaler[SCALER_SCALE].kernel.function, scale_filters)},
        SCALER_OPTS("scale",  SCALER_SCALE),
        {"dscale", OPT_CHOICE_C(scaler[SCALER_DSCALE].kernel.function, cdscale_filters)},
        SCALER_OPTS("dscale", SCALER_DSCALE),
        {"cscale", OPT_CHOICE_C(scaler[SCALER_CSCALE].kernel.function, cdscale_filters)},
        SCALER_OPTS("cscale", SCALER_CSCALE),
        {"tscale", OPT_CHOICE_C(scaler[SCALER_TSCALE].kernel.function, tscale_filters)},
        SCALER_OPTS("tscale", SCALER_TSCALE),
        {"scaler-resizes-only", OPT_BOOL(scaler_resizes_only)},
        {"correct-downscaling", OPT_BOOL(correct_downscaling)},
        {"linear-downscaling", OPT_BOOL(linear_downscaling)},
        {"linear-upscaling", OPT_BOOL(linear_upscaling)},
        {"sigmoid-upscaling", OPT_BOOL(sigmoid_upscaling)},
        {"sigmoid-center", OPT_FLOAT(sigmoid_center), M_RANGE(0.0, 1.0)},
        {"sigmoid-slope", OPT_FLOAT(sigmoid_slope), M_RANGE(1.0, 20.0)},
        {"fbo-format", OPT_STRING(fbo_format)},
        {"dither-depth", OPT_CHOICE(dither_depth, {"no", -1}, {"auto", 0}),
            M_RANGE(-1, 16)},
        {"dither", OPT_CHOICE(dither_algo,
            {"fruit", DITHER_FRUIT},
            {"ordered", DITHER_ORDERED},
            {"error-diffusion", DITHER_ERROR_DIFFUSION},
            {"no", DITHER_NONE})},
        {"dither-size-fruit", OPT_INT(dither_size), M_RANGE(2, 8)},
        {"temporal-dither", OPT_BOOL(temporal_dither)},
        {"temporal-dither-period", OPT_INT(temporal_dither_period),
            M_RANGE(1, 128)},
        {"error-diffusion", OPT_STRING(error_diffusion)},
        {"background", OPT_CHOICE(background,
            {"none", BACKGROUND_NONE},
            {"color", BACKGROUND_COLOR},
            {"tiles", BACKGROUND_TILES})},
        {"opengl-rectangle-textures", OPT_BOOL(use_rectangle)},
        {"background-color", OPT_COLOR(background_color)},
        {"background-tile-color-0", OPT_COLOR(background_tile_color[0])},
        {"background-tile-color-1", OPT_COLOR(background_tile_color[1])},
        {"background-tile-size", OPT_INT(background_tile_size), M_RANGE(1, 4096)},
        {"interpolation", OPT_BOOL(interpolation)},
        {"interpolation-threshold", OPT_FLOAT(interpolation_threshold)},
        {"blend-subtitles", OPT_CHOICE(blend_subs,
            {"no", BLEND_SUBS_NO},
            {"yes", BLEND_SUBS_YES},
            {"video", BLEND_SUBS_VIDEO})},
        {"glsl-shaders", OPT_PATHLIST(user_shaders), .flags = M_OPT_FILE},
        {"glsl-shader", OPT_CLI_ALIAS("glsl-shaders-append")},
        {"glsl-shader-opts", OPT_KEYVALUELIST(user_shader_opts)},
        {"deband", OPT_BOOL(deband)},
        {"deband", OPT_SUBSTRUCT(deband_opts, deband_conf)},
        {"sharpen", OPT_FLOAT(unsharp)},
        {"gpu-tex-pad-x", OPT_INT(tex_pad_x), M_RANGE(0, 4096)},
        {"gpu-tex-pad-y", OPT_INT(tex_pad_y), M_RANGE(0, 4096)},
        {"", OPT_SUBSTRUCT(icc_opts, mp_icc_conf)},
        {"gpu-shader-cache", OPT_BOOL(shader_cache)},
        {"gpu-shader-cache-dir", OPT_STRING(shader_cache_dir), .flags = M_OPT_FILE},
        {"gpu-hwdec-interop",
            OPT_STRING_VALIDATE(hwdec_interop, ra_hwdec_validate_opt)},
        {"gamut-warning", OPT_REMOVED("Replaced by --gamut-mapping-mode=warn")},
        {"gamut-clipping", OPT_REMOVED("Replaced by --gamut-mapping-mode=desaturate")},
        {"tone-mapping-desaturate", OPT_REMOVED("Replaced by --tone-mapping-mode")},
        {"tone-mapping-desaturate-exponent", OPT_REMOVED("Replaced by --tone-mapping-mode")},
        {0}
    },
    .size = sizeof(struct gl_video_opts),
    .defaults = &gl_video_opts_def,
    .change_flags = UPDATE_VIDEO,
};
