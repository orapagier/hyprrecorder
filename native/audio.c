/* Bounded capture-only Pulse client. Server performs format/rate conversion.
 * Each stream is pinned (DONT_MOVE); timestamps are latency-corrected against
 * CLOCK_MONOTONIC. No default routes, volumes or playback streams are changed. */
#define _GNU_SOURCE
#include <pulse/pulseaudio.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
#include <time.h>
#include <unistd.h>
#define RATE 48000
#define CAPACITY 96000

typedef struct HrAudio HrAudio;
typedef struct {
    HrAudio *owner;
    pa_stream *stream;
    float samples[CAPACITY][2];
    int64_t tags[CAPACITY], latest;
    int64_t received_us;
    int enabled;
    int missing;
} Input;
struct HrAudio {
    pa_mainloop *loop;
    pa_context *context;
    Input inputs[2];
    int64_t epoch, next;
    int begun;
    char error[512];
};
static int64_t now_us(void) {
    struct timespec ts; clock_gettime(CLOCK_MONOTONIC,&ts);
    return (int64_t)ts.tv_sec*1000000 + ts.tv_nsec/1000;
}
static void fail(HrAudio *a, const char *text) {
    if (!a->error[0]) snprintf(a->error,sizeof(a->error),"%s",text);
}
static void overflow(pa_stream *s, void *data) {
    (void)s; Input *in=data; fail(in->owner,"Audio input overflow; partial retained");
}
static void suspended(pa_stream *s, void *data) {
    Input *in=data;
    if(in->owner->begun && pa_stream_is_suspended(s)>0) fail(in->owner,"Pinned audio source suspended or disappeared");
}
static void moved(pa_stream *s, void *data) {
    (void)s; Input *in=data; fail(in->owner,"Pinned audio source moved unexpectedly");
}
static void read_input(pa_stream *s, size_t requested, void *data) {
    (void)requested;
    Input *in=data; HrAudio *a=in->owner;
    const void *raw=NULL; size_t bytes=0;
    if(pa_stream_peek(s,&raw,&bytes)<0) {fail(a,"Cannot read requested audio source");return;}
    if(!bytes) return;
    size_t frames=bytes/(sizeof(float)*2);
    pa_usec_t latency=0; int negative=0;
    int timing=pa_stream_get_latency(s,&latency,&negative);
    int64_t now=now_us();
    if(a->begun && timing>=0) {
        int64_t start=(int64_t)llround((double)(now-a->epoch+(negative?(int64_t)latency:-(int64_t)latency))*RATE/1000000.0);
        if(start-a->next+ (int64_t)frames >= CAPACITY) fail(a,"Audio queue exceeded two-second bound");
        else {
            const float *samples=raw;
            for(size_t i=0;i<frames;i++) {
                int64_t at=start+(int64_t)i;
                if(at<a->next || at<0) continue;
                size_t slot=(size_t)(at%CAPACITY);
                in->tags[slot]=at;
                in->samples[slot][0]=samples?samples[i*2]:0.0f;
                in->samples[slot][1]=samples?samples[i*2+1]:0.0f;
            }
            if(start+(int64_t)frames>in->latest) in->latest=start+(int64_t)frames;
            in->received_us=now;
        }
    }
    /* Before timing is available, drain without assigning unverifiable PTS.
       Startup timeout below makes prolonged timing failure explicit. */
    pa_stream_drop(s);
}
static int pump(HrAudio *a) {
    int ignored;
    for(int i=0;i<64;i++) {
        int n=pa_mainloop_iterate(a->loop,0,&ignored);
        if(n<0) {fail(a,"Audio event loop failed");break;}
        if(!n) break;
    }
    if(!PA_CONTEXT_IS_GOOD(pa_context_get_state(a->context))) fail(a,"Audio server disconnected");
    for(int i=0;i<2;i++) if(a->inputs[i].enabled && !PA_STREAM_IS_GOOD(pa_stream_get_state(a->inputs[i].stream))) fail(a,"Requested audio source disconnected");
    return a->error[0]?-1:0;
}
void hr_audio_free(HrAudio *a) {
    if(!a)return;
    for(int i=0;i<2;i++) if(a->inputs[i].stream) {pa_stream_disconnect(a->inputs[i].stream);pa_stream_unref(a->inputs[i].stream);}
    if(a->context) {pa_context_disconnect(a->context);pa_context_unref(a->context);}
    if(a->loop)pa_mainloop_free(a->loop);
    free(a);
}
HrAudio *hr_audio_open(const char *system, const char *microphone, char *error) {
    HrAudio *a=calloc(1,sizeof(*a));
    if(!a){snprintf(error,512,"Cannot allocate audio buffers");return NULL;}
    a->loop=pa_mainloop_new();
    if(!a->loop)goto failed;
    a->context=pa_context_new(pa_mainloop_get_api(a->loop),"Hyprrecorder");
    if(!a->context || pa_context_connect(a->context,NULL,PA_CONTEXT_NOAUTOSPAWN,NULL)<0) goto failed;
    int64_t deadline=now_us()+3000000;
    while(pa_context_get_state(a->context)!=PA_CONTEXT_READY) {
        int ignored;pa_mainloop_iterate(a->loop,0,&ignored);
        if(!PA_CONTEXT_IS_GOOD(pa_context_get_state(a->context)) || now_us()>deadline)goto failed;
        usleep(1000);
    }
    const char *names[]={system,microphone};
    for(int i=0;i<2;i++)if(names[i]) {
        Input *in=&a->inputs[i];in->owner=a;
        for(int j=0;j<CAPACITY;j++)in->tags[j]=-1;
        pa_sample_spec spec={PA_SAMPLE_FLOAT32LE,RATE,2};
        in->stream=pa_stream_new(a->context,i?"Microphone":"System audio",&spec,NULL);
        if(!in->stream)goto failed;
        pa_stream_set_read_callback(in->stream,read_input,in);
        pa_stream_set_overflow_callback(in->stream,overflow,in);
        pa_stream_set_suspended_callback(in->stream,suspended,in);
        pa_stream_set_moved_callback(in->stream,moved,in);
        pa_buffer_attr attr={.maxlength=RATE*8/2,.tlength=(uint32_t)-1,.prebuf=(uint32_t)-1,.minreq=(uint32_t)-1,.fragsize=480*8};
        pa_stream_flags_t flags=PA_STREAM_START_CORKED|PA_STREAM_DONT_MOVE|PA_STREAM_INTERPOLATE_TIMING|PA_STREAM_AUTO_TIMING_UPDATE|PA_STREAM_ADJUST_LATENCY;
        if(pa_stream_connect_record(in->stream,names[i],&attr,flags)<0)goto failed;
        in->enabled=1;
    }
    deadline=now_us()+3000000;
    for(;;) {
        int ready=1;
        if(pump(a)<0 || now_us()>deadline)goto failed;
        for(int i=0;i<2;i++)if(a->inputs[i].enabled && pa_stream_get_state(a->inputs[i].stream)!=PA_STREAM_READY)ready=0;
        if(ready)break;
        usleep(1000);
    }
    return a;
failed:
    snprintf(error,512,"%s",a->error[0]?a->error:"Cannot prepare requested audio source (server, device or timeout); turn it off to continue");
    hr_audio_free(a);return NULL;
}
int hr_audio_begin(HrAudio *a, int64_t epoch) {
    a->epoch=epoch;a->begun=1;
    for(int i=0;i<2;i++)if(a->inputs[i].enabled) {
        a->inputs[i].received_us=now_us();
        pa_operation *op=pa_stream_cork(a->inputs[i].stream,0,NULL,NULL);
        if(!op){fail(a,"Cannot start requested audio source");return -1;}pa_operation_unref(op);
    }
    return 0;
}
/* Returns interleaved stereo frames and their sample-clock offset. Missing
 * timing for >2s, disconnection and overflow fail rather than announce silence. */
static int mix(HrAudio *a,int16_t *out,int capacity,int64_t *position,char *error) {
    int64_t end=INT64_MAX;
    int count=0;
    for(int i=0;i<2;i++)if(a->inputs[i].enabled) {
        count++;
        if(now_us()-a->inputs[i].received_us>2000000){fail(a,"Requested audio source stopped delivering timed samples");goto failed;}
        if(a->inputs[i].latest<end)end=a->inputs[i].latest;
    }
    if(!count)return 0;
    int n=(int)(end-a->next);if(n<0)n=0;if(n>capacity)n=capacity;
    *position=a->next;
    for(int j=0;j<n;j++) {
        int64_t at=a->next+j;size_t slot=(size_t)(at%CAPACITY);
        for(int ch=0;ch<2;ch++) {
            float value=0;
            for(int i=0;i<2;i++)if(a->inputs[i].enabled) {
                Input *in=&a->inputs[i];
                if(in->tags[slot]==at) {value+=in->samples[slot][ch];if(ch==0)in->missing=0;}
                else if(ch==0 && ++in->missing>4800) {fail(a,"Audio timing gap exceeded 100 ms; partial retained");goto failed;}
            }
            /* 6 dB headroom for two inputs, unity for one. No playback/DSP. */
            value/=count;if(!isfinite(value))value=0;
            if(value>1)value=1;
            if(value<-1)value=-1;
            out[j*2+ch]=(int16_t)lrintf(value*32767.0f);
        }
    }
    a->next+=n;return n;
failed:
    snprintf(error,512,"%s",a->error);return -1;
}

int hr_audio_read(HrAudio *a,int16_t *out,int capacity,int64_t *position,char *error) {
    if(pump(a)<0){snprintf(error,512,"%s",a->error);return -1;}
    return mix(a,out,capacity,position,error);
}
/* Pure synthetic fixture through the production mixer; no server is opened. */
int hr_audio_fixture(int enabled,int16_t *out,int missing,char *error) {
    HrAudio *a=calloc(1,sizeof(*a));if(!a)return -1;
    for(int i=0;i<2;i++)if(enabled&(1<<i)) {
        Input *in=&a->inputs[i];in->enabled=1;in->latest=480;in->received_us=now_us();
        for(int j=0;j<480;j++){in->tags[j]=missing?-1:j;in->samples[j][0]=in->samples[j][1]=i?0.5f:0.25f;}
        in->missing=missing?4800:0;
    }
    int64_t position=0;int n=mix(a,out,480,&position,error);free(a);return n;
}
