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

#ifndef MP_GL_VIDEO_H
#define MP_GL_VIDEO_H

#include <stdbool.h>

#include "options/m_option.h"
#include "options/gl_video_opts.h"
#include "sub/osd.h"
#include "utils.h"
#include "lcms.h"
#include "shader_cache.h"
#include "video/csputils.h"
#include "video/out/filter_kernels.h"

struct scaler {
    int index;
    struct scaler_config conf;
    double scale_factor;
    bool initialized;
    struct filter_kernel *kernel;
    struct ra_tex *lut;
    struct ra_tex *sep_fbo;
    bool insufficient;

    // kernel points here
    struct filter_kernel kernel_storage;
};

struct gl_video;
struct vo_frame;
struct voctrl_screenshot;

enum {
    RENDER_FRAME_SUBS = 1 << 0,
    RENDER_FRAME_OSD = 1 << 1,
    RENDER_FRAME_VF_SUBS = 1 << 2,
    RENDER_SCREEN_COLOR = 1 << 3, // 3D LUT and dithering
    RENDER_FRAME_DEF = RENDER_FRAME_SUBS | RENDER_FRAME_OSD | RENDER_SCREEN_COLOR,
};

struct gl_video *gl_video_init(struct ra *ra, struct mp_log *log,
                               struct mpv_global *g);
void gl_video_uninit(struct gl_video *p);
void gl_video_set_osd_source(struct gl_video *p, struct osd_state *osd);
bool gl_video_check_format(struct gl_video *p, int mp_format);
void gl_video_config(struct gl_video *p, struct mp_image_params *params);
void gl_video_render_frame(struct gl_video *p, struct vo_frame *frame,
                           const struct ra_fbo *fbo, int flags);
void gl_video_resize(struct gl_video *p,
                     struct mp_rect *src, struct mp_rect *dst,
                     struct mp_osd_res *osd);
void gl_video_set_fb_depth(struct gl_video *p, int fb_depth);
void gl_video_perfdata(struct gl_video *p, struct voctrl_performance_data *out);
void gl_video_set_clear_color(struct gl_video *p, struct m_color color);
void gl_video_set_osd_pts(struct gl_video *p, double pts);
bool gl_video_check_osd_change(struct gl_video *p, struct mp_osd_res *osd,
                               double pts);

void gl_video_screenshot(struct gl_video *p, struct vo_frame *frame,
                         struct voctrl_screenshot *args);

double gl_video_scale_ambient_lux(float lmin, float lmax,
                                  float rmin, float rmax, double lux);
void gl_video_set_ambient_lux(struct gl_video *p, double lux);
void gl_video_set_icc_profile(struct gl_video *p, bstr icc_data);
bool gl_video_icc_auto_enabled(struct gl_video *p);
bool gl_video_gamma_auto_enabled(struct gl_video *p);

void gl_video_reset(struct gl_video *p);
bool gl_video_showing_interpolated_frame(struct gl_video *p);

struct mp_hwdec_devices;
void gl_video_init_hwdecs(struct gl_video *p, struct ra_ctx *ra_ctx,
                          struct mp_hwdec_devices *devs,
                          bool load_all_by_default);
struct hwdec_imgfmt_request;
void gl_video_load_hwdecs_for_img_fmt(struct gl_video *p, struct mp_hwdec_devices *devs,
                                      struct hwdec_imgfmt_request *params);

struct vo;
void gl_video_configure_queue(struct gl_video *p, struct vo *vo);

struct mp_image *gl_video_get_image(struct gl_video *p, int imgfmt, int w, int h,
                                    int stride_align, int flags);

struct mp_image_params *gl_video_get_target_params_ptr(struct gl_video *p);

#endif
