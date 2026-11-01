#ifndef PLAYER_AUDIO_H
#define PLAYER_AUDIO_H

#include <stddef.h>
#include <stdint.h>

enum player_audio_codec {
    PLAYER_AUDIO_CODEC_AAC = 0,
    PLAYER_AUDIO_CODEC_ALAC,
    PLAYER_AUDIO_CODEC_FLAC,
    PLAYER_AUDIO_CODEC_MP3,
    PLAYER_AUDIO_CODEC_OPUS,
    PLAYER_AUDIO_CODEC_VORBIS,
    PLAYER_AUDIO_CODEC_PCM_S8,
    PLAYER_AUDIO_CODEC_PCM_U8,
    PLAYER_AUDIO_CODEC_PCM_S16LE,
    PLAYER_AUDIO_CODEC_PCM_S24LE,
    PLAYER_AUDIO_CODEC_PCM_S32LE,
    PLAYER_AUDIO_CODEC_PCM_F32LE,
    PLAYER_AUDIO_CODEC_PCM_F64LE,
    PLAYER_AUDIO_CODEC_COUNT,
};

struct player_audio_params {
    uint32_t codec;
    uint32_t sample_rate;
    uint32_t channels;
    uint64_t channel_mask;
    uint32_t bits_per_sample;
    const uint8_t *extradata;
    size_t extradata_size;
};

struct player_audio_decoder;

enum player_audio_status {
    PLAYER_AUDIO_OK = 0,
    PLAYER_AUDIO_ERR_ARG = 1,
    PLAYER_AUDIO_ERR_UNSUPPORTED = 2,
    PLAYER_AUDIO_ERR_DECODE = 3,
    PLAYER_AUDIO_ERR_PANIC = 4,
};

struct player_audio_decoder *player_audio_create(
    const struct player_audio_params *params, char *err, size_t err_cap,
    int *err_code);

int player_audio_decode(struct player_audio_decoder *dec,
                        const uint8_t *data, size_t len, int64_t pts, int64_t dur,
                        const float **samples, uint32_t *frames,
                        uint32_t *channels, uint32_t *sample_rate,
                        char *err, size_t err_cap);

void player_audio_reset(struct player_audio_decoder *dec);
void player_audio_destroy(struct player_audio_decoder *dec);

#endif
