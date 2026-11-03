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

/* Rust playback engine (S3): opaque handle plus the probe the TUI and
 * startup use to decide whether a file bypasses the C playloop. */

struct player_audio_engine;

struct player_audio_probe {
    int engine_ready;
    double duration;
    unsigned int sample_rate;
    unsigned short channels;
    char title[128];
    char artist[128];
    char album[128];
    char date[32];
};

struct player_audio_snapshot {
    double position;
    double duration;
    float volume;
    int paused;
    int muted;
    int ended;
    int has_error;
    char error[256];
};

/* 1 when the Rust engine can play the file, 0 when the C engine keeps it,
 * negative on probe error. `out` may be NULL to skip the metadata. */
int player_audio_probe(const char *path, struct player_audio_probe *out,
                       char *err, size_t err_cap);

struct player_audio_engine *player_audio_engine_load(const char *path,
                                                    int null_sink,
                                                    char *err, size_t err_cap);
void player_audio_engine_set_paused(struct player_audio_engine *engine,
                                    int paused);
void player_audio_engine_seek(struct player_audio_engine *engine,
                              double seconds);
void player_audio_engine_set_volume(struct player_audio_engine *engine,
                                    float volume);
void player_audio_engine_set_muted(struct player_audio_engine *engine,
                                   int muted);
int player_audio_engine_snapshot(struct player_audio_engine *engine,
                                 struct player_audio_snapshot *out);
void player_audio_engine_destroy(struct player_audio_engine *engine);

#endif
