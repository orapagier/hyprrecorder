/* Internal ABI bridge: FFmpeg owns codec structures; Rust owns lifecycle/fd.
 * The CPU path converts/uploads; HR_GPU_ENCODE adds direct DRM import and VPP.
 * There is no automatic software encoder fallback. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <unistd.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/hwcontext.h>
#include <libavutil/imgutils.h>
#include <libswscale/swscale.h>
#ifdef HR_GPU_ENCODE
#include <libavutil/hwcontext_drm.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersrc.h>
#include <libavfilter/buffersink.h>
#endif

/* Kept in sync with Rust Geometry (repr(C)); all values are physical pixels. */
typedef struct {
    uint32_t x, y, width, height, output_width, output_height;
    uint32_t content_width, content_height, pad_x, pad_y;
} HrGeometry;

typedef struct {
    int fd, width, height;
    AVFormatContext *mux;
    AVCodecContext *codec;
    AVIOContext *io;
    AVBufferRef *device, *frames;
    AVFrame *cpu, *gpu;
    AVPacket *packet, *held_packet;
    int64_t end_us;
    struct SwsContext *conversion;
    int64_t last_pts, last_audio_end;
    int audio;
#ifdef HR_GPU_ENCODE
    AVFrame *drm, *mapped, *sync;
    AVBufferRef *drm_device, *drm_frames, *import_frames;
    AVFilterGraph *graph;
    AVFilterContext *source, *sink;
#endif
} HrVideo;

/* SDR desktop pixels are full-range sRGB. YUV uses the BT.709 matrix and
 * limited range while retaining the source sRGB transfer curve. Describe the
 * same conversion in the frame, H.264 VUI and container; never leave playback
 * to guess BT.601/709 or full/limited range. */
static void sdr_yuv_properties(AVFrame *frame) {
    frame->colorspace = AVCOL_SPC_BT709;
    frame->color_range = AVCOL_RANGE_MPEG;
    frame->color_primaries = AVCOL_PRI_BT709;
    frame->color_trc = AVCOL_TRC_IEC61966_2_1;
    frame->chroma_location = AVCHROMA_LOC_LEFT;
}

static int write_bytes(void *opaque, const uint8_t *data, int size) {
    HrVideo *v = opaque;
    int written = 0;
    while (written < size) {
        ssize_t n = write(v->fd, data + written, size - written);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) return AVERROR(n < 0 ? errno : EIO);
        written += (int)n;
    }
    return written;
}

static int64_t seek_bytes(void *opaque, int64_t offset, int whence) {
    HrVideo *v = opaque;
    if (whence == AVSEEK_SIZE) {
        struct stat info;
        return fstat(v->fd, &info) ? AVERROR(errno) : info.st_size;
    }
    off_t result = lseek(v->fd, offset, whence & ~AVSEEK_FORCE);
    return result < 0 ? AVERROR(errno) : result;
}

void hr_video_free(HrVideo *v) {
    if (!v) return;
#ifdef HR_GPU_ENCODE
    avfilter_graph_free(&v->graph);
    av_frame_free(&v->sync);
    av_frame_free(&v->mapped);
    av_frame_free(&v->drm);
    av_buffer_unref(&v->import_frames);
    av_buffer_unref(&v->drm_frames);
    av_buffer_unref(&v->drm_device);
#endif
    av_frame_free(&v->cpu);
    av_frame_free(&v->gpu);
    av_packet_free(&v->packet);
    av_packet_free(&v->held_packet);
    avcodec_free_context(&v->codec);
    av_buffer_unref(&v->frames);
    av_buffer_unref(&v->device);
    sws_freeContext(v->conversion);
    if (v->mux) v->mux->pb = NULL;
    avformat_free_context(v->mux);
    if (v->io) av_freep(&v->io->buffer);
    avio_context_free(&v->io);
    free(v);
}

#ifdef HR_GPU_ENCODE
static void free_drm(void *opaque, uint8_t *data) {
    (void)opaque;
    AVDRMFrameDescriptor *desc = (AVDRMFrameDescriptor *)data;
    close(desc->objects[0].fd);
    av_free(desc);
}

/* Linear, single-object RGB only. The duplicate fd owns the imported storage. */
static int gpu_input(HrVideo *v, const char *device, int input_fd,
                     uint32_t pitch, uint32_t offset, uint32_t format, int fps, const HrGeometry *g) {
    int result;
    if (format != 0x34325258 && format != 0x34325241) return AVERROR(EINVAL);
    off_t size = lseek(input_fd, 0, SEEK_END);
    if (size <= 0 || pitch < (uint32_t)v->width * 4 ||
        (uint64_t)offset + (uint64_t)pitch * v->height > (uint64_t)size)
        return AVERROR(EINVAL);
    v->drm = av_frame_alloc(); v->mapped = av_frame_alloc(); v->sync = av_frame_alloc();
    if (!v->drm || !v->mapped || !v->sync) return AVERROR(ENOMEM);
    AVDRMFrameDescriptor *desc = av_mallocz(sizeof(*desc));
    if (!desc) return AVERROR(ENOMEM);
    int duplicate = fcntl(input_fd, F_DUPFD_CLOEXEC, 0);
    if (duplicate < 0) { av_free(desc); return AVERROR(errno); }
    desc->nb_objects = 1;
    desc->objects[0] = (AVDRMObjectDescriptor){ .fd = duplicate, .size = size, .format_modifier = 0 };
    desc->nb_layers = 1; desc->layers[0].format = format; desc->layers[0].nb_planes = 1;
    desc->layers[0].planes[0] = (AVDRMPlaneDescriptor){ .object_index = 0, .offset = offset, .pitch = pitch };
    v->drm->buf[0] = av_buffer_create((uint8_t *)desc, sizeof(*desc), free_drm, NULL, 0);
    if (!v->drm->buf[0]) { free_drm(NULL, (uint8_t *)desc); return AVERROR(ENOMEM); }
    v->drm->data[0] = (uint8_t *)desc;
    v->drm->format = AV_PIX_FMT_DRM_PRIME;
    v->drm->width = v->width; v->drm->height = v->height;
    if ((result = av_hwdevice_ctx_create(&v->drm_device, AV_HWDEVICE_TYPE_DRM, device, NULL, 0)) < 0) return result;
    v->drm_frames = av_hwframe_ctx_alloc(v->drm_device);
    v->import_frames = av_hwframe_ctx_alloc(v->device);
    if (!v->drm_frames || !v->import_frames) return AVERROR(ENOMEM);
    enum AVPixelFormat sw = format == 0x34325258 ? AV_PIX_FMT_BGR0 : AV_PIX_FMT_BGRA;
    AVBufferRef *contexts[] = {v->drm_frames, v->import_frames};
    for (int i = 0; i < 2; i++) {
        AVHWFramesContext *ctx = (AVHWFramesContext *)contexts[i]->data;
        ctx->format = i ? AV_PIX_FMT_VAAPI : AV_PIX_FMT_DRM_PRIME;
        ctx->sw_format = sw; ctx->width = v->width; ctx->height = v->height;
        if ((result = av_hwframe_ctx_init(contexts[i])) < 0) return result;
    }
    v->drm->hw_frames_ctx = av_buffer_ref(v->drm_frames);
    v->mapped->hw_frames_ctx = av_buffer_ref(v->import_frames);
    if (!v->drm->hw_frames_ctx || !v->mapped->hw_frames_ctx) return AVERROR(ENOMEM);
    v->mapped->format = AV_PIX_FMT_VAAPI;
    if ((result = av_hwframe_map(v->mapped, v->drm, AV_HWFRAME_MAP_READ | AV_HWFRAME_MAP_DIRECT)) < 0) return result;
    /* scale_vaapi consumes these as its VA surface_region, without CPU mapping. */
    v->mapped->crop_left = g->x; v->mapped->crop_top = g->y;
    v->mapped->crop_right = v->width - g->x - g->width;
    v->mapped->crop_bottom = v->height - g->y - g->height;
    v->mapped->color_range = AVCOL_RANGE_JPEG; v->mapped->colorspace = AVCOL_SPC_RGB;
    v->mapped->color_primaries = AVCOL_PRI_BT709;
    v->mapped->color_trc = AVCOL_TRC_IEC61966_2_1;
    v->graph = avfilter_graph_alloc();
    if (!v->graph) return AVERROR(ENOMEM);
    v->graph->nb_threads = 1;
    const AVFilter *buffer = avfilter_get_by_name("buffer"), *scale = avfilter_get_by_name("scale_vaapi"), *sink = avfilter_get_by_name("buffersink");
    if (!buffer || !scale || !sink) return AVERROR_FILTER_NOT_FOUND;
    v->source = avfilter_graph_alloc_filter(v->graph, buffer, "capture");
    AVBufferSrcParameters *params = av_buffersrc_parameters_alloc();
    if (!v->source || !params) { av_free(params); return AVERROR(ENOMEM); }
    params->format = AV_PIX_FMT_VAAPI; params->width = v->width; params->height = v->height;
    params->time_base = (AVRational){1,60000}; params->frame_rate = (AVRational){fps,1};
    params->sample_aspect_ratio = (AVRational){1,1};
    params->color_space = AVCOL_SPC_RGB; params->color_range = AVCOL_RANGE_JPEG;
    params->hw_frames_ctx = v->mapped->hw_frames_ctx; /* borrowed; set makes its own reference */
    result = av_buffersrc_parameters_set(v->source, params); av_free(params);
    if (result < 0 || (result = avfilter_init_str(v->source, NULL)) < 0) return result;
    AVFilterContext *conversion = NULL;
    char scale_options[256];
    snprintf(scale_options, sizeof(scale_options),
        "w=%u:h=%u:format=nv12:out_color_matrix=bt709:out_range=limited:"
        "out_color_primaries=bt709:out_color_transfer=iec61966-2-1:"
        "out_chroma_location=left:reset_sar=1",
        g->content_width, g->content_height);
    if ((result = avfilter_graph_create_filter(&conversion, scale, "gpu_convert",
        scale_options, NULL, v->graph)) < 0) return result;
    if ((result = avfilter_graph_create_filter(&v->sink, sink, "encoded_input", NULL, NULL, v->graph)) < 0) return result;
    if ((result = avfilter_link(v->source, 0, conversion, 0)) < 0) return result;
    AVFilterContext *tail = conversion;
    if (g->content_width != g->output_width || g->content_height != g->output_height) {
        const AVFilter *pad = avfilter_get_by_name("pad_vaapi");
        if (!pad) return AVERROR_FILTER_NOT_FOUND;
        AVFilterContext *padding = NULL;
        char options[160];
        snprintf(options,sizeof(options),"w=%u:h=%u:x=%u:y=%u:color=black",g->output_width,g->output_height,g->pad_x,g->pad_y);
        if ((result=avfilter_graph_create_filter(&padding,pad,"gpu_padding",options,NULL,v->graph))<0 ||
            (result=avfilter_link(tail,0,padding,0))<0) return result;
        tail=padding;
    }
    if ((result=avfilter_link(tail,0,v->sink,0))<0 ||
        (result=avfilter_graph_config(v->graph,NULL))<0) return result;
    AVBufferRef *frames = av_buffersink_get_hw_frames_ctx(v->sink);
    if (!frames) return AVERROR(EINVAL);
    v->frames = av_buffer_ref(frames);
    return v->frames ? 0 : AVERROR(ENOMEM);
}
#endif

static int video_open(HrVideo **out, int fd, int width, int height, int fps,
                      const char *device, int dma_fd, uint32_t pitch, uint32_t offset, uint32_t format, const HrGeometry *g, int audio) {
#ifndef HR_GPU_ENCODE
    (void)pitch; (void)offset; (void)format;
#endif
    int result = AVERROR(ENOMEM);
    HrVideo *v = calloc(1, sizeof(*v));
    if (!v) return result;
    *out = NULL;
    v->fd = fd; v->width = width; v->height = height; v->last_pts = -1; v->audio=audio;
    /* A NULL device is used only by the Rust software-fixture unit tests.
       The recording command always passes an explicit VA-API device. */
    const AVCodec *encoder = avcodec_find_encoder_by_name(device ? "h264_vaapi" : "mpeg4");
    if (!encoder) { result = AVERROR_ENCODER_NOT_FOUND; goto fail; }
    v->codec = avcodec_alloc_context3(encoder);
    v->cpu = av_frame_alloc(); v->gpu = av_frame_alloc(); v->packet = av_packet_alloc(); v->held_packet=av_packet_alloc();
    if (!v->codec || !v->cpu || !v->gpu || !v->packet || !v->held_packet) goto fail;
    if ((result = avformat_alloc_output_context2(&v->mux, NULL, "matroska", NULL)) < 0) goto fail;
    v->codec->width = g ? (int)g->output_width : width;
    v->codec->height = g ? (int)g->output_height : height;
    if (g) v->codec->sample_aspect_ratio = (AVRational){1, 1};
    /* Microsecond capture timestamps, quantized to a 1/60000 encoder clock. */
    v->codec->time_base = (AVRational){1, 60000};
    v->codec->framerate = (AVRational){fps, 1};
    v->codec->bit_rate = 12000000;
    v->codec->gop_size = fps * 2; v->codec->max_b_frames = 0;
    v->codec->pix_fmt = device ? AV_PIX_FMT_VAAPI : AV_PIX_FMT_YUV420P;
    v->codec->colorspace = AVCOL_SPC_BT709;
    v->codec->color_range = AVCOL_RANGE_MPEG;
    v->codec->color_primaries = AVCOL_PRI_BT709;
    v->codec->color_trc = AVCOL_TRC_IEC61966_2_1;
    v->codec->chroma_sample_location = AVCHROMA_LOC_LEFT;
    if (v->mux->oformat->flags & AVFMT_GLOBALHEADER) v->codec->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    if (device) {
        if ((result = av_hwdevice_ctx_create(&v->device, AV_HWDEVICE_TYPE_VAAPI, device, NULL, 0)) < 0) goto fail;
        if (dma_fd >= 0) {
#ifdef HR_GPU_ENCODE
            if ((result = gpu_input(v, device, dma_fd, pitch, offset, format, fps, g)) < 0) goto fail;
#else
            result = AVERROR(ENOSYS); goto fail;
#endif
        } else {
            v->frames = av_hwframe_ctx_alloc(v->device);
            if (!v->frames) { result = AVERROR(ENOMEM); goto fail; }
            AVHWFramesContext *frames = (AVHWFramesContext *)v->frames->data;
            frames->format = AV_PIX_FMT_VAAPI; frames->sw_format = AV_PIX_FMT_NV12;
            frames->width = width; frames->height = height; frames->initial_pool_size = 4;
            if ((result = av_hwframe_ctx_init(v->frames)) < 0) goto fail;
        }
        v->codec->hw_frames_ctx = av_buffer_ref(v->frames);
        if (!v->codec->hw_frames_ctx) { result = AVERROR(ENOMEM); goto fail; }
    }
    if ((result = avcodec_open2(v->codec, encoder, NULL)) < 0) goto fail;
    if (dma_fd < 0) {
        v->cpu->format = device ? AV_PIX_FMT_NV12 : AV_PIX_FMT_YUV420P;
        v->cpu->width = width; v->cpu->height = height;
        if ((result = av_frame_get_buffer(v->cpu, 32)) < 0) goto fail;
        v->conversion = sws_getContext(width, height, AV_PIX_FMT_BGRA, width, height,
                                      v->cpu->format, SWS_BILINEAR, NULL, NULL, NULL);
        if (!v->conversion) { result = AVERROR(EINVAL); goto fail; }
        const int *coefficients = sws_getCoefficients(SWS_CS_ITU709);
        if ((result = sws_setColorspaceDetails(v->conversion, coefficients, 1,
            coefficients, 0, 0, 1 << 16, 1 << 16)) < 0) goto fail;
    }
    AVStream *stream = avformat_new_stream(v->mux, NULL);
    if (!stream) { result = AVERROR(ENOMEM); goto fail; }
    stream->time_base = v->codec->time_base;
    if ((result = avcodec_parameters_from_context(stream->codecpar, v->codec)) < 0) goto fail;
    if(audio) {
        AVStream *sound=avformat_new_stream(v->mux,NULL);
        if(!sound){result=AVERROR(ENOMEM);goto fail;}
        sound->time_base=(AVRational){1,48000};
        sound->codecpar->codec_type=AVMEDIA_TYPE_AUDIO;
        sound->codecpar->codec_id=AV_CODEC_ID_PCM_S16LE;
        sound->codecpar->sample_rate=48000;
        sound->codecpar->bits_per_coded_sample=16;
        sound->codecpar->block_align=4;
        sound->codecpar->bit_rate=48000*4*8;
        av_channel_layout_default(&sound->codecpar->ch_layout,2);
    }
    unsigned char *buffer = av_malloc(32768);
    if (!buffer) { result = AVERROR(ENOMEM); goto fail; }
    v->io = avio_alloc_context(buffer, 32768, 1, v, NULL, write_bytes, seek_bytes);
    if (!v->io) { av_free(buffer); result = AVERROR(ENOMEM); goto fail; }
    v->mux->pb = v->io; v->mux->flags |= AVFMT_FLAG_CUSTOM_IO;
    v->mux->max_interleave_delta=200000;
    if ((result = avformat_write_header(v->mux, NULL)) < 0) goto fail;
    *out = v;
    return 0;
fail:
    hr_video_free(v);
    return result;
}

int hr_video_open(HrVideo **out, int fd, int width, int height, int fps, const char *device) {
    return video_open(out, fd, width, height, fps, device, -1, 0, 0, 0, NULL, 0);
}
#ifdef HR_GPU_ENCODE
int hr_video_open_gpu(HrVideo **out, int fd, int width, int height, int fps,
                      const char *device, int dma_fd, uint32_t pitch, uint32_t offset, uint32_t format, const HrGeometry *g) {
    if (!device || dma_fd < 0 || !g || width < 2 || height < 2 || width > 8192 || height > 8192 ||
        width % 2 || height % 2 || fps < 1 || fps > 240 ||
        g->width < 2 || g->height < 2 || g->output_width < 2 || g->output_height < 2 ||
        g->output_width > 8192 || g->output_height > 8192 ||
        g->width % 2 || g->height % 2 || g->output_width % 2 || g->output_height % 2 ||
        (uint64_t)g->x + g->width > (uint32_t)width ||
        (uint64_t)g->y + g->height > (uint32_t)height ||
        g->content_width < 2 || g->content_height < 2 ||
        g->content_width % 2 || g->content_height % 2 ||
        (uint64_t)g->pad_x + g->content_width > g->output_width ||
        (uint64_t)g->pad_y + g->content_height > g->output_height)
        return AVERROR(EINVAL);
    return video_open(out, fd, width, height, fps, device, dma_fd, pitch, offset, format, g, 0);
}
int hr_video_open_gpu_audio(HrVideo **out, int fd, int width, int height, int fps,
    const char *device, int dma_fd, uint32_t pitch, uint32_t offset, uint32_t format, const HrGeometry *g, int audio) {
    /* Rust validates geometry before this internal ABI entry. */
    return video_open(out,fd,width,height,fps,device,dma_fd,pitch,offset,format,g,audio);
}
#endif

int hr_video_open_fixture_audio(HrVideo **out,int fd,int width,int height,int fps) {
    return video_open(out,fd,width,height,fps,NULL,-1,0,0,0,NULL,1);
}
int hr_video_push_audio(HrVideo *v,const int16_t *samples,int frames,int64_t position) {
    if(!v->audio || frames<1 || frames>4800 || position<v->last_audio_end) return AVERROR(EINVAL);
    AVPacket *packet=av_packet_alloc();if(!packet)return AVERROR(ENOMEM);
    int result=av_new_packet(packet,frames*4);
    if(result>=0) {
        for(int i=0;i<frames*2;i++) {packet->data[i*2]=(uint8_t)samples[i];packet->data[i*2+1]=(uint8_t)((uint16_t)samples[i]>>8);}
        packet->stream_index=1;packet->pts=packet->dts=position;packet->duration=frames;
        av_packet_rescale_ts(packet,(AVRational){1,48000},v->mux->streams[1]->time_base);
        result=av_interleaved_write_frame(v->mux,packet);
    }
    av_packet_free(&packet);v->last_audio_end=position+frames;
    return result<0?result:v->io->error;
}

static int drain_packets(HrVideo *v) {
    for (;;) {
        int result = avcodec_receive_packet(v->codec, v->packet);
        if (result == AVERROR(EAGAIN) || result == AVERROR_EOF) return 0;
        if (result < 0) return result;
        av_packet_rescale_ts(v->packet, v->codec->time_base, v->mux->streams[0]->time_base);
        v->packet->stream_index = 0;
        if(v->held_packet->buf) {
            v->held_packet->duration=v->packet->pts-v->held_packet->pts;
            result=av_interleaved_write_frame(v->mux,v->held_packet);
            av_packet_unref(v->held_packet);
            if(result<0)return result;
        }
        av_packet_move_ref(v->held_packet,v->packet);
        if (v->io->error < 0) return v->io->error;
    }
}

int hr_video_push(HrVideo *v, const uint8_t *pixels, int stride, int inverted, int64_t pts_us) {
    if (!v->conversion) return AVERROR(EINVAL);
    int result = av_frame_make_writable(v->cpu);
    if (result < 0) return result;
    const uint8_t *source[4] = {pixels, NULL, NULL, NULL};
    int strides[4] = {stride, 0, 0, 0};
    if (inverted) { source[0] += (size_t)(v->height - 1) * stride; strides[0] = -stride; }
    result = sws_scale(v->conversion, source, strides, 0, v->height, v->cpu->data, v->cpu->linesize);
    if (result != v->height) return result < 0 ? result : AVERROR(EIO);
    int64_t pts = av_rescale_q(pts_us, (AVRational){1, 1000000}, v->codec->time_base);
    if (pts <= v->last_pts) return AVERROR(EINVAL);
    v->last_pts = pts;
    AVFrame *frame = v->cpu;
    if (v->frames) {
        av_frame_unref(v->gpu);
        if ((result = av_hwframe_get_buffer(v->frames, v->gpu, 0)) < 0) return result;
        if ((result = av_hwframe_transfer_data(v->gpu, v->cpu, 0)) < 0) return result;
        frame = v->gpu;
    }
    frame->pts = pts;
    sdr_yuv_properties(frame);
    if ((result = avcodec_send_frame(v->codec, frame)) < 0) return result;
    return drain_packets(v);
}

#ifdef HR_GPU_ENCODE
int hr_video_repeat_gpu(HrVideo *v,int64_t pts_us) {
    if(!v->gpu || !v->gpu->buf[0])return AVERROR(EINVAL);
    int64_t pts=av_rescale_q(pts_us,(AVRational){1,1000000},v->codec->time_base);
    if(pts<=v->last_pts)return AVERROR(EINVAL);
    AVFrame *repeat=av_frame_clone(v->gpu);if(!repeat)return AVERROR(ENOMEM);
    repeat->pts=pts;v->last_pts=pts;
    int result=avcodec_send_frame(v->codec,repeat);av_frame_free(&repeat);
    return result<0?result:drain_packets(v);
}
int hr_video_push_gpu(HrVideo *v, int64_t pts_us) {
    if (!v->graph) return AVERROR(EINVAL);
    int64_t pts = av_rescale_q(pts_us, (AVRational){1,1000000}, v->codec->time_base);
    if (pts <= v->last_pts) return AVERROR(EINVAL);
    v->last_pts = pts; v->mapped->pts = pts;
    int result = av_buffersrc_add_frame_flags(v->source, v->mapped, AV_BUFFERSRC_FLAG_KEEP_REF);
    if (result < 0) return result;
    av_frame_unref(v->gpu);
    if ((result = av_buffersink_get_frame(v->sink, v->gpu)) < 0) return result;
    sdr_yuv_properties(v->gpu);
    /* A readable direct DRM export on FFmpeg's VAAPI PRIME2 path synchronizes
       VPP completion. Never reuse the captured RGB buffer before this succeeds.
       Legacy exports have an unknown modifier and are rejected, not CPU mapped. */
    av_frame_unref(v->sync); v->sync->format = AV_PIX_FMT_DRM_PRIME;
    result = av_hwframe_map(v->sync, v->gpu, AV_HWFRAME_MAP_READ | AV_HWFRAME_MAP_DIRECT);
    if (result < 0) return result;
    AVDRMFrameDescriptor *sync = (AVDRMFrameDescriptor *)v->sync->data[0];
    if (!sync || sync->nb_objects < 1 || sync->nb_objects > AV_DRM_MAX_PLANES) return AVERROR(EINVAL);
    for (int i = 0; i < sync->nb_objects; i++)
        if (sync->objects[i].format_modifier == 0x00ffffffffffffffULL) return AVERROR(ENOSYS);
    av_frame_unref(v->sync);
    if ((result = avcodec_send_frame(v->codec, v->gpu)) < 0) return result;
    return drain_packets(v);
}
#endif

int hr_video_end_at(HrVideo *v,int64_t end_us) {
    if(end_us<0)return AVERROR(EINVAL);
    v->end_us=end_us;return 0;
}
int hr_video_finish(HrVideo *v) {
    int result = avcodec_send_frame(v->codec, NULL);
    if (result < 0) return result;
    if ((result = drain_packets(v)) < 0) return result;
    if(v->held_packet->buf) {
        AVPacket *last=v->held_packet;
        int64_t end=v->end_us?av_rescale_q(v->end_us,(AVRational){1,1000000},v->mux->streams[0]->time_base):last->pts+av_rescale_q(1,av_inv_q(v->codec->framerate),v->mux->streams[0]->time_base);
        last->duration=end>last->pts?end-last->pts:1;
        if((result=av_interleaved_write_frame(v->mux,last))<0)return result;
        av_packet_unref(last);
    }
    if ((result = av_write_trailer(v->mux)) < 0) return result;
    avio_flush(v->io);
    return v->io->error < 0 ? v->io->error : 0;
}

void hr_video_error(int code, char *text, size_t capacity) {
    av_strerror(code, text, capacity);
}
