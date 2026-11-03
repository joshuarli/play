#include <stddef.h>
#include <stdint.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libswscale/swscale.h>

#include "album_art.h"

int player_tui_cover_art(void *ctx, const char *path, int size,
                         unsigned char *out, size_t cap)
{
    (void)ctx;
    if (!path || !out || size <= 0)
        return -1;
    size_t target = (size_t)size * (size_t)size * 3;
    if (cap < target)
        return -1;

    AVFormatContext *format = NULL;
    AVCodecContext *decoder = NULL;
    AVFrame *frame = NULL;
    AVPacket *packet = NULL;
    struct SwsContext *scaler = NULL;
    int result = -1;

    if (avformat_open_input(&format, path, NULL, NULL) < 0)
        goto done;
    if (avformat_find_stream_info(format, NULL) < 0)
        goto done;

    AVStream *stream = NULL;
    for (unsigned int index = 0; index < format->nb_streams; index++) {
        AVStream *candidate = format->streams[index];
        if ((candidate->disposition & AV_DISPOSITION_ATTACHED_PIC) &&
            candidate->attached_pic.size > 0)
        {
            stream = candidate;
            break;
        }
    }
    if (!stream) {
        result = 1;
        goto done;
    }

    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (!codec)
        goto done;
    decoder = avcodec_alloc_context3(codec);
    if (!decoder)
        goto done;
    if (avcodec_parameters_to_context(decoder, stream->codecpar) < 0)
        goto done;
    if (avcodec_open2(decoder, codec, NULL) < 0)
        goto done;

    packet = av_packet_alloc();
    frame = av_frame_alloc();
    if (!packet || !frame)
        goto done;
    if (av_packet_ref(packet, &stream->attached_pic) < 0)
        goto done;
    if (avcodec_send_packet(decoder, packet) < 0)
        goto done;
    if (avcodec_receive_frame(decoder, frame) < 0)
        goto done;

    scaler = sws_getContext(frame->width, frame->height, frame->format,
                            size, size, AV_PIX_FMT_RGB24,
                            SWS_BILINEAR, NULL, NULL, NULL);
    if (!scaler)
        goto done;

    uint8_t *destination[4] = {out, NULL, NULL, NULL};
    int strides[4] = {size * 3, 0, 0, 0};
    sws_scale(scaler, (const uint8_t *const *)frame->data, frame->linesize,
              0, frame->height, destination, strides);
    result = 0;

done:
    sws_freeContext(scaler);
    av_frame_free(&frame);
    av_packet_free(&packet);
    avcodec_free_context(&decoder);
    if (format)
        avformat_close_input(&format);
    return result;
}
