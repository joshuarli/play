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

#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#include <libavcodec/avcodec.h>

#include "common/common.h"
#include "common/codecs.h"
#include "common/msg.h"
#include "audio/aframe.h"
#include "audio/chmap.h"
#include "audio/format.h"
#include "demux/packet.h"
#include "demux/stheader.h"
#include "filters/f_decoder_wrapper.h"
#include "filters/filter_internal.h"
#include "mpv_talloc.h"
#include "player_audio.h"

struct priv {
    struct mp_codec_params *codec;
    struct player_audio_decoder *decoder;
    double next_pts;
    struct mp_decoder public;
};

static bool map_codec(const char *codec, enum player_audio_codec *out)
{
    if (!codec)
        return false;
    if (!strcmp(codec, "aac"))
        *out = PLAYER_AUDIO_CODEC_AAC;
    else if (!strcmp(codec, "alac"))
        *out = PLAYER_AUDIO_CODEC_ALAC;
    else if (!strcmp(codec, "flac"))
        *out = PLAYER_AUDIO_CODEC_FLAC;
    else if (!strcmp(codec, "mp3"))
        *out = PLAYER_AUDIO_CODEC_MP3;
    else if (!strcmp(codec, "opus"))
        *out = PLAYER_AUDIO_CODEC_OPUS;
    else if (!strcmp(codec, "vorbis"))
        *out = PLAYER_AUDIO_CODEC_VORBIS;
    else if (!strcmp(codec, "pcm_s8"))
        *out = PLAYER_AUDIO_CODEC_PCM_S8;
    else if (!strcmp(codec, "pcm_u8"))
        *out = PLAYER_AUDIO_CODEC_PCM_U8;
    else if (!strcmp(codec, "pcm_s16le"))
        *out = PLAYER_AUDIO_CODEC_PCM_S16LE;
    else if (!strcmp(codec, "pcm_s24le"))
        *out = PLAYER_AUDIO_CODEC_PCM_S24LE;
    else if (!strcmp(codec, "pcm_s32le"))
        *out = PLAYER_AUDIO_CODEC_PCM_S32LE;
    else if (!strcmp(codec, "pcm_f32le"))
        *out = PLAYER_AUDIO_CODEC_PCM_F32LE;
    else if (!strcmp(codec, "pcm_f64le"))
        *out = PLAYER_AUDIO_CODEC_PCM_F64LE;
    else
        return false;
    return true;
}

static int default_bits(enum player_audio_codec codec)
{
    switch (codec) {
    case PLAYER_AUDIO_CODEC_PCM_S8:
    case PLAYER_AUDIO_CODEC_PCM_U8:
        return 8;
    case PLAYER_AUDIO_CODEC_PCM_S16LE:
        return 16;
    case PLAYER_AUDIO_CODEC_PCM_S24LE:
        return 24;
    case PLAYER_AUDIO_CODEC_PCM_S32LE:
    case PLAYER_AUDIO_CODEC_PCM_F32LE:
        return 32;
    case PLAYER_AUDIO_CODEC_PCM_F64LE:
        return 64;
    default:
        return 0;
    }
}

static uint64_t chmap_mask(const struct mp_chmap *map)
{
    uint64_t mask = 0;
    bool discrete = false;
    for (int n = 0; n < map->num; n++) {
        int id = map->speaker[n];
        if (id >= 0 && id <= MP_SPEAKER_ID_TBR) {
            mask |= 1ULL << id;
            continue;
        }
        switch (id) {
        case MP_SPEAKER_ID_DL:  mask |= 1ULL << MP_SPEAKER_ID_FL; break;
        case MP_SPEAKER_ID_DR:  mask |= 1ULL << MP_SPEAKER_ID_FR; break;
        case MP_SPEAKER_ID_WL:  mask |= 1ULL << 24; break;
        case MP_SPEAKER_ID_WR:  mask |= 1ULL << 25; break;
        case MP_SPEAKER_ID_SDL: mask |= 1ULL << MP_SPEAKER_ID_SL; break;
        case MP_SPEAKER_ID_SDR: mask |= 1ULL << MP_SPEAKER_ID_SR; break;
        case MP_SPEAKER_ID_LFE2: mask |= 1ULL << 18; break;
        case MP_SPEAKER_ID_TSL: mask |= 1ULL << 19; break;
        case MP_SPEAKER_ID_TSR: mask |= 1ULL << 20; break;
        case MP_SPEAKER_ID_BFC: mask |= 1ULL << 21; break;
        case MP_SPEAKER_ID_BFL: mask |= 1ULL << 22; break;
        case MP_SPEAKER_ID_BFR: mask |= 1ULL << 23; break;
        case MP_SPEAKER_ID_NA: discrete = true; break;
        default: discrete = true; break;
        }
    }
    return discrete ? 0 : mask;
}

static bool init(struct mp_filter *da, struct mp_codec_params *codec,
                 const char *decoder)
{
    struct priv *ctx = da->priv;
    enum player_audio_codec codec_id;
    if (!map_codec(codec->codec, &codec_id)) {
        MP_VERBOSE(da, "Symphonia does not handle codec '%s'\n",
                   codec->codec ? codec->codec : "<?>");
        return false;
    }

    const uint8_t *extradata = codec->extradata;
    int extradata_size = codec->extradata_size;
    if ((!extradata || extradata_size <= 0) && codec->lav_codecpar) {
        extradata = codec->lav_codecpar->extradata;
        extradata_size = codec->lav_codecpar->extradata_size;
    }

    struct player_audio_params params = {
        .codec = codec_id,
        .sample_rate = codec->samplerate,
        .channels = codec->channels.num,
        .channel_mask = chmap_mask(&codec->channels),
        .bits_per_sample = codec->bits_per_coded_sample,
        .extradata = extradata,
        .extradata_size = extradata_size,
    };
    if (!params.bits_per_sample)
        params.bits_per_sample = default_bits(codec_id);

    char error[256] = {0};
    ctx->decoder = player_audio_create(&params, error, sizeof(error));
    if (!ctx->decoder) {
        MP_ERR(da, "Symphonia init failed: %s\n", error);
        return false;
    }

    ctx->codec = codec;
    ctx->next_pts = MP_NOPTS_VALUE;
    MP_INFO(da, "Using Symphonia for %s\n", codec->codec);
    return true;
}

static void destroy(struct mp_filter *da)
{
    struct priv *ctx = da->priv;
    player_audio_destroy(ctx->decoder);
    ctx->decoder = NULL;
}

static void reset(struct mp_filter *da)
{
    struct priv *ctx = da->priv;
    player_audio_reset(ctx->decoder);
    ctx->next_pts = MP_NOPTS_VALUE;
}

static void process(struct mp_filter *da)
{
    struct priv *ctx = da->priv;

    if (!mp_pin_can_transfer_data(da->ppins[1], da->ppins[0]))
        return;

    struct mp_frame inframe = mp_pin_out_read(da->ppins[0]);
    if (inframe.type == MP_FRAME_EOF) {
        mp_pin_in_write(da->ppins[1], inframe);
        return;
    }
    if (inframe.type != MP_FRAME_PACKET) {
        if (inframe.type) {
            MP_ERR(da, "unknown frame type\n");
            mp_filter_internal_mark_failed(da);
        }
        return;
    }

    struct demux_packet *packet = inframe.data;
    if (!packet->buffer || !packet->len) {
        talloc_free(packet);
        mp_filter_internal_mark_progress(da);
        return;
    }

    double pts = packet->pts;
    if (pts == MP_NOPTS_VALUE)
        pts = ctx->next_pts;
    int64_t pts_us = pts == MP_NOPTS_VALUE ? 0 : (int64_t)(pts * 1000000.0);
    int64_t dur_us = packet->duration == MP_NOPTS_VALUE
        ? 0 : (int64_t)(packet->duration * 1000000.0);

    const float *samples = NULL;
    uint32_t frames = 0, channels = 0, rate = 0;
    char error[256] = {0};
    int status = player_audio_decode(ctx->decoder, packet->buffer, packet->len,
                                     pts_us, dur_us, &samples, &frames,
                                     &channels, &rate, error, sizeof(error));
    talloc_free(packet);

    if (status != 0) {
        MP_ERR(da, "Symphonia decode failed: %s\n", error);
        mp_filter_internal_mark_failed(da);
        return;
    }
    if (!frames) {
        mp_filter_internal_mark_progress(da);
        return;
    }

    struct mp_chmap chmap = ctx->codec->channels;
    if (channels != chmap.num)
        mp_chmap_from_channels(&chmap, channels);

    struct mp_aframe *out = mp_aframe_create();
    mp_aframe_set_format(out, AF_FORMAT_FLOAT);
    mp_aframe_set_rate(out, rate);
    mp_aframe_set_chmap(out, &chmap);
    if (!mp_aframe_alloc_data(out, frames)) {
        talloc_free(out);
        MP_ERR(da, "could not allocate output frame\n");
        mp_filter_internal_mark_failed(da);
        return;
    }
    uint8_t **data = mp_aframe_get_data_rw(out);
    memcpy(data[0], samples, frames * channels * sizeof(float));
    mp_aframe_set_pts(out, pts);
    ctx->next_pts = rate ? pts + (double)frames / rate : pts;

    mp_pin_in_write(da->ppins[1], MAKE_FRAME(MP_FRAME_AUDIO, out));
}

static const struct mp_filter_info ad_symphonia_filter = {
    .name = "ad_symphonia",
    .priv_size = sizeof(struct priv),
    .process = process,
    .reset = reset,
    .destroy = destroy,
};

static struct mp_decoder *create(struct mp_filter *parent,
                                 struct mp_codec_params *codec,
                                 const char *decoder)
{
    struct mp_filter *da = mp_filter_create(parent, &ad_symphonia_filter);
    if (!da)
        return NULL;

    mp_filter_add_pin(da, MP_PIN_IN, "in");
    mp_filter_add_pin(da, MP_PIN_OUT, "out");
    da->log = mp_log_new(da, parent->log, NULL);

    struct priv *priv = da->priv;
    priv->public.f = da;

    if (!init(da, codec, decoder)) {
        talloc_free(da);
        return NULL;
    }

    return &priv->public;
}

static void add_decoders(struct mp_decoder_list *list)
{
    static const struct {
        const char *codec;
        const char *desc;
    } codecs[] = {
        {"aac", "Symphonia AAC"},
        {"alac", "Symphonia ALAC"},
        {"flac", "Symphonia FLAC"},
        {"mp3", "Symphonia MP3"},
        {"opus", "Symphonia Opus"},
        {"vorbis", "Symphonia Vorbis"},
        {"pcm_s8", "Symphonia PCM s8"},
        {"pcm_u8", "Symphonia PCM u8"},
        {"pcm_s16le", "Symphonia PCM s16le"},
        {"pcm_s24le", "Symphonia PCM s24le"},
        {"pcm_s32le", "Symphonia PCM s32le"},
        {"pcm_f32le", "Symphonia PCM f32le"},
        {"pcm_f64le", "Symphonia PCM f64le"},
    };
    for (int n = 0; n < MP_ARRAY_SIZE(codecs); n++) {
        mp_add_decoder_ext(list, codecs[n].codec, "symphonia", codecs[n].desc,
                           MP_DECODER_DRIVER_SYMPHONIA);
    }
}

const struct mp_decoder_fns ad_symphonia = {
    .create = create,
    .add_decoders = add_decoders,
};
