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

#ifndef OPTIONS_GL_VIDEO_OPTS_H
#define OPTIONS_GL_VIDEO_OPTS_H

#include <stdbool.h>

#include "options/m_option.h"

struct mp_icc_opts;

enum scaler_unit {
    SCALER_SCALE,  // luma/video
    SCALER_DSCALE, // luma-video downscaling
    SCALER_CSCALE, // chroma upscaling
    SCALER_TSCALE, // temporal scaling (interpolation)
    SCALER_COUNT
};

enum background_type {
    BACKGROUND_NONE = 0,
    BACKGROUND_COLOR,
    BACKGROUND_TILES,
    BACKGROUND_BLUR,
};

enum blend_subs_mode {
    BLEND_SUBS_NO = 0,
    BLEND_SUBS_YES,
    BLEND_SUBS_VIDEO,
};

enum tone_mapping {
    TONE_MAPPING_AUTO,
    TONE_MAPPING_CLIP,
    TONE_MAPPING_MOBIUS,
    TONE_MAPPING_REINHARD,
    TONE_MAPPING_HABLE,
    TONE_MAPPING_GAMMA,
    TONE_MAPPING_LINEAR,
    TONE_MAPPING_SPLINE,
    TONE_MAPPING_BT_2390,
    TONE_MAPPING_BT_2446A,
    TONE_MAPPING_ST2094_40,
    TONE_MAPPING_ST2094_10,
};

enum gamut_mode {
    GAMUT_AUTO,
    GAMUT_CLIP,
    GAMUT_PERCEPTUAL,
    GAMUT_RELATIVE,
    GAMUT_SATURATION,
    GAMUT_ABSOLUTE,
    GAMUT_DESATURATE,
    GAMUT_DARKEN,
    GAMUT_WARN,
    GAMUT_LINEAR,
};

enum dither_algo {
    DITHER_NONE = 0,
    DITHER_FRUIT,
    DITHER_ORDERED,
    DITHER_ERROR_DIFFUSION,
};

struct deband_opts {
    int iterations;
    float threshold;
    float range;
    float grain;
};

struct scaler_fun {
    int function;
    float params[2];
    float blur;
    float taper;
    const struct m_opt_choice_alternatives *functions;
};

struct scaler_config {
    struct scaler_fun kernel;
    struct scaler_fun window;
    float radius;
    float antiring;
    float clamp;
};

struct gl_tone_map_opts {
    int curve;
    float curve_param;
    float max_boost;
    bool inverse;
    int compute_peak;
    float decay_rate;
    float scene_threshold_low;
    float scene_threshold_high;
    float peak_percentile;
    float contrast_recovery;
    float contrast_smoothness;
    int gamut_mode;
    bool visualize;
};

struct gl_video_opts {
    int dumb_mode;
    struct scaler_config scaler[4];
    float gamma;
    bool gamma_auto;
    int target_prim;
    int target_trc;
    int target_peak;
    int hdr_reference_white;
    int sdr_adjust_gamma;
    int treat_srgb_as_power22;
    int target_contrast;
    int target_gamut;
    struct gl_tone_map_opts tone_map;
    bool correct_downscaling;
    bool linear_downscaling;
    bool linear_upscaling;
    bool sigmoid_upscaling;
    float sigmoid_center;
    float sigmoid_slope;
    bool scaler_resizes_only;
    bool pbo;
    int dither_depth;
    int dither_algo;
    int dither_size;
    bool temporal_dither;
    int temporal_dither_period;
    char *error_diffusion;
    char *fbo_format;
    int background;
    bool use_rectangle;
    struct m_color background_color;
    struct m_color background_tile_color[2];
    int background_tile_size;
    bool interpolation;
    float interpolation_threshold;
    int blend_subs;
    char **user_shaders;
    char **user_shader_opts;
    bool deband;
    struct deband_opts *deband_opts;
    float unsharp;
    int tex_pad_x, tex_pad_y;
    struct mp_icc_opts *icc_opts;
    bool shader_cache;
    int early_flush;
    char *shader_cache_dir;
    char *hwdec_interop;
};

extern const struct m_sub_options gl_video_conf;

#endif
