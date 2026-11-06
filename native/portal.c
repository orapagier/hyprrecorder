/* Owned portal/PipeWire source experiment. No pixel processing or media writes. */
#include <gio/gio.h>
#include <gio/gunixfdlist.h>
#include <pipewire/pipewire.h>
#include <spa/param/video/format-utils.h>
#include <spa/buffer/meta.h>
#include <spa/utils/result.h>
#include <stdio.h>
#include <inttypes.h>
#include <string.h>
#include <unistd.h>

#define PORTAL "org.freedesktop.portal.Desktop"
#define OBJECT "/org/freedesktop/portal/desktop"
#define IFACE "org.freedesktop.portal.ScreenCast"

struct hr_timing {
    uint64_t count;
    double total_ms, min_ms, max_ms;
    uint64_t over_25ms, over_50ms, over_100ms;
};
static void timing_add(struct hr_timing *t, uint64_t ns) {
    double ms = ns / 1000000.0;
    if (!t->count || ms < t->min_ms) t->min_ms = ms;
    if (!t->count || ms > t->max_ms) t->max_ms = ms;
    t->count++; t->total_ms += ms;
    if (ns > 25000000) t->over_25ms++;
    if (ns > 50000000) t->over_50ms++;
    if (ns > 100000000) t->over_100ms++;
}
struct hr_portal_result {
    uint64_t frames, rejected, pts_samples, nonincreasing_pts;
    int64_t first_pts, last_pts;
    double elapsed;
    uint32_t width, height, format, memory_type;
    uint32_t session_started, remote_opened, interrupted, cleanup_ok;
    uint32_t rate_num, rate_denom, max_rate_num, max_rate_denom;
    uint64_t format_events, process_callbacks, empty_dequeues;
    double delivery_span_seconds;
    struct hr_timing delivery_intervals, pts_intervals, loop_iterations;
    char error[512];
};
struct probe {
    struct hr_portal_result *r;
    int (*stop)(void);
    GDBusConnection *bus;
    char *session, *request;
    GVariant *response;
    uint32_t response_code;
    struct pw_main_loop *loop;
    struct pw_stream *stream;
    struct spa_video_info_raw format;
    gboolean closed;
    gboolean tearing_down;
    int64_t first_delivery_us, last_delivery_us;
};
static void fail(struct probe *p, const char *message) {
    if (!p->r->error[0]) snprintf(p->r->error, sizeof(p->r->error), "%s", message);
}
static void response_cb(GDBusConnection *c, const char *sender, const char *path,
                        const char *iface, const char *signal, GVariant *args, void *data) {
    (void)c; (void)sender; (void)iface; (void)signal;
    struct probe *p = data;
    if (p->request && !strcmp(path, p->request) && !p->response)
        g_variant_get(args, "(u@a{sv})", &p->response_code, &p->response);
}
static void closed_cb(GDBusConnection *c, const char *sender, const char *path,
                      const char *iface, const char *signal, GVariant *args, void *data) {
    (void)c; (void)sender; (void)iface; (void)signal; (void)args;
    struct probe *p = data;
    if (p->session && !strcmp(path, p->session)) {
        p->closed = TRUE;
        fail(p, "Portal session closed by the source");
    }
}
static void dispatch_bus(void) {
    /* Bounded dispatch prevents a busy bus starving time/cancellation checks. */
    for (int i = 0; i < 32 && g_main_context_pending(NULL); i++)
        g_main_context_iteration(NULL, FALSE);
}
static gboolean cancelled(struct probe *p) {
    if (p->stop()) { p->r->interrupted = 1; return TRUE; }
    return FALSE;
}
static void close_object(struct probe *p, const char *path, const char *interface) {
    GError *e = NULL;
    GVariant *v = g_dbus_connection_call_sync(p->bus, PORTAL, path, interface, "Close",
        NULL, G_VARIANT_TYPE_UNIT, G_DBUS_CALL_FLAGS_NONE, 2000, NULL, &e);
    if (v) g_variant_unref(v);
    /* An already closed request/session needs no second close. */
    if (e) {
        char *name = g_dbus_error_get_remote_error(e);
        if (!name || (!g_str_has_suffix(name, ".UnknownObject") &&
                      !g_str_has_suffix(name, ".UnknownMethod"))) p->r->cleanup_ok = 0;
        g_free(name); g_error_free(e);
    }
}
/* Tokens are object-path elements, so UUID punctuation is normalized. */
static void add_token(GVariantBuilder *options, const char *key) {
    char *uuid = g_uuid_string_random();
    g_strdelimit(uuid, "-", '_');
    g_variant_builder_add(options, "{sv}", key, g_variant_new_string(uuid));
    g_free(uuid);
}
static GVariant *create_session_args(void) {
    GVariantBuilder options;
    g_variant_builder_init(&options, G_VARIANT_TYPE_VARDICT);
    add_token(&options, "handle_token");
    add_token(&options, "session_handle_token");
    return g_variant_new("(a{sv})", &options);
}
static gboolean request(struct probe *p, const char *method, GVariant *args) {
    if (p->r->error[0] || cancelled(p)) {
        /* Sink/free parameters that were not passed to GDBus. */
        g_variant_ref_sink(args); g_variant_unref(args);
        return FALSE;
    }
    GError *e = NULL;
    GVariant *v = g_dbus_connection_call_sync(p->bus, PORTAL, OBJECT, IFACE, method,
        args, G_VARIANT_TYPE("(o)"), G_DBUS_CALL_FLAGS_NONE, 5000, NULL, &e);
    if (!v) {
        char message[512]; snprintf(message, sizeof(message), "%s: %s", method, e->message);
        fail(p, message); g_error_free(e); return FALSE;
    }
    g_free(p->request); p->request = NULL;
    g_variant_get(v, "(o)", &p->request); g_variant_unref(v);
    int64_t deadline = g_get_monotonic_time() + 120 * G_TIME_SPAN_SECOND;
    while (!p->response && !p->r->error[0] && !cancelled(p) && g_get_monotonic_time() < deadline) {
        dispatch_bus(); g_usleep(10000);
    }
    if (!p->response) {
        if (!p->r->interrupted) fail(p, "Portal request timed out or session closed");
        close_object(p, p->request, "org.freedesktop.portal.Request");
        return FALSE;
    }
    if (p->r->error[0] || p->r->interrupted) return FALSE;
    if (p->response_code != 0) {
        fail(p, p->response_code == 1 ? "Portal selection cancelled by user" : "Portal request failed");
        return FALSE;
    }
    return TRUE;
}
static void clear_response(struct probe *p) {
    g_clear_pointer(&p->response, g_variant_unref);
    g_clear_pointer(&p->request, g_free);
}
static void state_changed(void *data, enum pw_stream_state old, enum pw_stream_state state, const char *error) {
    (void)old;
    struct probe *p = data;
    if (state == PW_STREAM_STATE_ERROR ||
        (state == PW_STREAM_STATE_UNCONNECTED && !p->tearing_down))
        fail(p, error ? error : "PipeWire stream disconnected");
}
static void core_error(void *data, uint32_t id, int seq, int res, const char *message) {
    (void)id; (void)seq; (void)res; fail(data, message);
}
static const struct pw_core_events core_events = { PW_VERSION_CORE_EVENTS, .error = core_error };
static void param_changed(void *data, uint32_t id, const struct spa_pod *param) {
    struct probe *p = data;
    if (id != SPA_PARAM_Format || !param) return;
    uint32_t type, subtype;
    struct spa_video_info_raw format = { 0 };
    if (spa_format_parse(param, &type, &subtype) < 0 || type != SPA_MEDIA_TYPE_video ||
        subtype != SPA_MEDIA_SUBTYPE_raw || spa_format_video_raw_parse(param, &format) < 0 ||
        !format.size.width || !format.size.height || format.size.width > 8192 || format.size.height > 8192 ||
        (format.format != SPA_VIDEO_FORMAT_BGRx && format.format != SPA_VIDEO_FORMAT_RGBx &&
         format.format != SPA_VIDEO_FORMAT_BGRA && format.format != SPA_VIDEO_FORMAT_RGBA)) {
        fail(p, "Unsupported PipeWire video format"); return;
    }
    if (p->r->width && (format.size.width != p->r->width || format.size.height != p->r->height ||
                        format.format != p->r->format)) {
        fail(p, "PipeWire source geometry/format changed during probe"); return;
    }
    p->format = format;
    p->r->width = p->format.size.width; p->r->height = p->format.size.height;
    p->r->format = p->format.format;
    p->r->rate_num = format.framerate.num; p->r->rate_denom = format.framerate.denom;
    p->r->max_rate_num = format.max_framerate.num; p->r->max_rate_denom = format.max_framerate.denom;
    p->r->format_events++;
    uint8_t storage[1024]; struct spa_pod_builder b = SPA_POD_BUILDER_INIT(storage, sizeof(storage));
    const struct spa_pod *params[2];
    params[0] = spa_pod_builder_add_object(&b, SPA_TYPE_OBJECT_ParamBuffers, SPA_PARAM_Buffers,
        SPA_PARAM_BUFFERS_dataType, SPA_POD_CHOICE_FLAGS_Int((1 << SPA_DATA_MemFd) | (1 << SPA_DATA_MemPtr)));
    params[1] = spa_pod_builder_add_object(&b, SPA_TYPE_OBJECT_ParamMeta, SPA_PARAM_Meta,
        SPA_PARAM_META_type, SPA_POD_Id(SPA_META_Header),
        SPA_PARAM_META_size, SPA_POD_Int(sizeof(struct spa_meta_header)));
    int rc = pw_stream_update_params(p->stream, params, 2);
    if (rc < 0) fail(p, spa_strerror(rc));
}
static gboolean valid_buffer(uint32_t width, uint32_t height, const struct spa_data *d,
                             const struct spa_meta_header *h) {
    if (!width || !height || width > 8192 || height > 8192 || !d || !d->data || !d->chunk)
        return FALSE;
    const struct spa_chunk *c = d->chunk;
    uint64_t required = c->stride > 0 ? (uint64_t)(height - 1) * c->stride + (uint64_t)width * 4 : UINT64_MAX;
    return (d->type == SPA_DATA_MemFd || d->type == SPA_DATA_MemPtr) &&
        c->offset <= d->maxsize && c->size <= d->maxsize - c->offset &&
        c->stride >= (int64_t)width * 4 && required <= c->size &&
        !(c->flags & (SPA_CHUNK_FLAG_CORRUPTED | SPA_CHUNK_FLAG_EMPTY)) &&
        !(h && (h->flags & SPA_META_HEADER_FLAG_CORRUPTED));
}
static void process_frame(void *data) {
    struct probe *p = data;
    p->r->process_callbacks++;
    struct pw_buffer *b = pw_stream_dequeue_buffer(p->stream);
    if (!b) { p->r->empty_dequeues++; return; }
    struct spa_buffer *buf = b->buffer;
    struct spa_data *d = buf->n_datas == 1 ? &buf->datas[0] : NULL;
    struct spa_meta_header *h = spa_buffer_find_meta_data(buf, SPA_META_Header, sizeof(*h));
    if (!valid_buffer(p->r->width, p->r->height, d, h)) {
        p->r->rejected++;
    } else {
        int64_t now = g_get_monotonic_time();
        if (!p->r->frames) p->first_delivery_us = now;
        else timing_add(&p->r->delivery_intervals, (uint64_t)(now - p->last_delivery_us) * 1000);
        p->last_delivery_us = now;
        p->r->delivery_span_seconds = (now - p->first_delivery_us) / 1000000.0;
        p->r->frames++; p->r->memory_type = d->type;
        if (h && h->pts != SPA_TIME_INVALID) {
            if (!p->r->pts_samples) p->r->first_pts = h->pts;
            else if (h->pts <= p->r->last_pts) p->r->nonincreasing_pts++;
            else timing_add(&p->r->pts_intervals, (uint64_t)h->pts - (uint64_t)p->r->last_pts);
            p->r->last_pts = h->pts; p->r->pts_samples++;
        }
    }
    int rc = pw_stream_queue_buffer(p->stream, b);
    if (rc < 0) fail(p, spa_strerror(rc));
}
static const struct pw_stream_events stream_events = {
    PW_VERSION_STREAM_EVENTS, .state_changed = state_changed,
    .param_changed = param_changed, .process = process_frame
};

int hr_portal_source(uint32_t seconds, uint32_t fps, uint32_t cursor,
                     int (*stop)(void), struct hr_portal_result *r) {
    memset(r, 0, sizeof(*r)); r->cleanup_ok = 1;
    struct probe p = { .r = r, .stop = stop };
    GError *e = NULL;
    guint response_sub = 0, closed_sub = 0;
    int fd = -1;
    struct pw_context *context = NULL;
    struct pw_core *core = NULL;
    struct spa_hook listener;
    gboolean pw_initialized = FALSE;
    GVariant *v = NULL, *streams = NULL, *props = NULL;
    GUnixFDList *fds = NULL;
    /* A dedicated connection owns all requests, session and FD transfer. */
    char *address = g_dbus_address_get_for_bus_sync(G_BUS_TYPE_SESSION, NULL, &e);
    if (!address) goto dbus_error;
    p.bus = g_dbus_connection_new_for_address_sync(address,
        G_DBUS_CONNECTION_FLAGS_AUTHENTICATION_CLIENT | G_DBUS_CONNECTION_FLAGS_MESSAGE_BUS_CONNECTION,
        NULL, NULL, &e);
    g_free(address);
    if (!p.bus) goto dbus_error;
    g_dbus_connection_set_exit_on_close(p.bus, FALSE);
    response_sub = g_dbus_connection_signal_subscribe(p.bus, PORTAL,
        "org.freedesktop.portal.Request", "Response", NULL, NULL, G_DBUS_SIGNAL_FLAGS_NONE, response_cb, &p, NULL);
    closed_sub = g_dbus_connection_signal_subscribe(p.bus, PORTAL,
        "org.freedesktop.portal.Session", "Closed", NULL, NULL, G_DBUS_SIGNAL_FLAGS_NONE, closed_cb, &p, NULL);
    if (!request(&p, "CreateSession", create_session_args())) goto done;
    if (!g_variant_lookup(p.response, "session_handle", "s", &p.session) ||
        !g_variant_is_object_path(p.session)) { fail(&p, "Portal returned no valid session handle"); goto done; }
    clear_response(&p);
    GVariantBuilder options;
    g_variant_builder_init(&options, G_VARIANT_TYPE_VARDICT);
    add_token(&options, "handle_token");
    g_variant_builder_add(&options, "{sv}", "types", g_variant_new_uint32(1));
    g_variant_builder_add(&options, "{sv}", "multiple", g_variant_new_boolean(FALSE));
    g_variant_builder_add(&options, "{sv}", "cursor_mode", g_variant_new_uint32(cursor ? 2 : 1));
    if (!request(&p, "SelectSources", g_variant_new("(oa{sv})", p.session, &options))) goto done;
    clear_response(&p);
    g_variant_builder_init(&options, G_VARIANT_TYPE_VARDICT);
    add_token(&options, "handle_token");
    if (!request(&p, "Start", g_variant_new("(osa{sv})", p.session, "", &options))) goto done;
    r->session_started = 1;
    streams = g_variant_lookup_value(p.response, "streams", G_VARIANT_TYPE("a(ua{sv})"));
    if (!streams || g_variant_n_children(streams) != 1) { fail(&p, "Portal must return exactly one monitor stream"); goto done; }
    uint32_t node; g_variant_get_child(streams, 0, "(u@a{sv})", &node, &props);
    uint64_t serial = 0;
    g_variant_lookup(props, "pipewire-serial", "t", &serial);
    clear_response(&p);
    v = g_dbus_connection_call_with_unix_fd_list_sync(p.bus, PORTAL, OBJECT, IFACE,
        "OpenPipeWireRemote", g_variant_new("(oa{sv})", p.session, NULL), G_VARIANT_TYPE("(h)"),
        G_DBUS_CALL_FLAGS_NONE, 5000, NULL, &fds, NULL, &e);
    if (!v) goto dbus_error;
    int index; g_variant_get(v, "(h)", &index);
    fd = g_unix_fd_list_get(fds, index, &e);
    if (fd < 0) goto dbus_error;
    r->remote_opened = 1;
    fprintf(stderr, "Portal selected. Acquisition starts in 3 seconds; keep motion visible. No audio/video file.\n");
    int64_t countdown = g_get_monotonic_time() + 3 * G_TIME_SPAN_SECOND;
    while (g_get_monotonic_time() < countdown && !cancelled(&p) && !r->error[0]) {
        dispatch_bus(); g_usleep(10000);
    }
    if (r->interrupted || r->error[0]) goto done;
    pw_init(NULL, NULL); pw_initialized = TRUE;
    p.loop = pw_main_loop_new(NULL);
    if (!p.loop) { fail(&p, "Cannot create PipeWire loop"); goto done; }
    context = pw_context_new(pw_main_loop_get_loop(p.loop), NULL, 0);
    if (!context) { fail(&p, "Cannot create PipeWire context"); goto done; }
    core = pw_context_connect_fd(context, fd, NULL, 0); fd = -1; /* ownership transferred */
    if (!core) { fail(&p, "Cannot connect to portal PipeWire remote"); goto done; }
    pw_core_add_listener(core, &listener, &core_events, &p);
    char target[32]; snprintf(target, sizeof(target), "%" PRIu64, serial ? serial : (uint64_t)node);
    struct pw_properties *properties = pw_properties_new(PW_KEY_MEDIA_TYPE, "Video",
        PW_KEY_MEDIA_CATEGORY, "Capture", PW_KEY_MEDIA_ROLE, "Screen", PW_KEY_TARGET_OBJECT, target, NULL);
    p.stream = pw_stream_new(core, "Hyprrecorder portal source probe", properties);
    if (!p.stream) { fail(&p, "Cannot create PipeWire stream"); goto done; }
    struct spa_hook stream_listener;
    pw_stream_add_listener(p.stream, &stream_listener, &stream_events, &p);
    uint8_t storage[1024]; struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(storage, sizeof(storage));
    const struct spa_pod *param = spa_pod_builder_add_object(&builder,
        SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
        SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video),
        SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
        SPA_FORMAT_VIDEO_format, SPA_POD_CHOICE_ENUM_Id(4, SPA_VIDEO_FORMAT_BGRx,
            SPA_VIDEO_FORMAT_RGBx, SPA_VIDEO_FORMAT_BGRA, SPA_VIDEO_FORMAT_RGBA),
        SPA_FORMAT_VIDEO_size, SPA_POD_CHOICE_RANGE_Rectangle(
            &SPA_RECTANGLE(1920,1080), &SPA_RECTANGLE(1,1), &SPA_RECTANGLE(8192,8192)),
        SPA_FORMAT_VIDEO_framerate, SPA_POD_CHOICE_RANGE_Fraction(
            &SPA_FRACTION(fps,1), &SPA_FRACTION(0,1), &SPA_FRACTION(fps,1)));
    int rc = pw_stream_connect(p.stream, PW_DIRECTION_INPUT, PW_ID_ANY,
        PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS, &param, 1);
    if (rc < 0) { fail(&p, spa_strerror(rc)); goto done; }
    int64_t began = g_get_monotonic_time(), first_frame = 0;
    /* Allow 10 seconds for stream setup, then measure requested acquisition duration. */
    while (!r->error[0] && !cancelled(&p)) {
        int64_t iteration_start = g_get_monotonic_time();
        dispatch_bus();
        if (g_dbus_connection_is_closed(p.bus)) { fail(&p, "Portal bus disconnected"); break; }
        int64_t now = g_get_monotonic_time();
        if (!first_frame && r->frames) first_frame = now;
        if (!first_frame && now - began >= 10 * G_TIME_SPAN_SECOND) { fail(&p, "No valid PipeWire frame within 10 seconds"); break; }
        if (first_frame && now - first_frame >= seconds * G_TIME_SPAN_SECOND) break;
        rc = pw_loop_iterate(pw_main_loop_get_loop(p.loop), 20);
        timing_add(&r->loop_iterations, (uint64_t)(g_get_monotonic_time() - iteration_start) * 1000);
        if (rc < 0 && rc != -EINTR) { fail(&p, spa_strerror(rc)); break; }
    }
    if (first_frame) r->elapsed = (g_get_monotonic_time() - first_frame) / 1000000.0;
    goto done;
 dbus_error:
    fail(&p, e ? e->message : "Portal D-Bus operation failed");
    g_clear_error(&e);
 done:
    p.tearing_down = TRUE;
    if (p.stream) pw_stream_destroy(p.stream);
    if (core) pw_core_disconnect(core);
    if (context) pw_context_destroy(context);
    if (p.loop) pw_main_loop_destroy(p.loop);
    if (pw_initialized) pw_deinit();
    if (fd >= 0) close(fd);
    if (p.bus) {
        if (p.session && !p.closed) close_object(&p, p.session, "org.freedesktop.portal.Session");
        if (response_sub) g_dbus_connection_signal_unsubscribe(p.bus, response_sub);
        if (closed_sub) g_dbus_connection_signal_unsubscribe(p.bus, closed_sub);
        g_dbus_connection_close_sync(p.bus, NULL, NULL); g_object_unref(p.bus);
    }
    clear_response(&p); g_free(p.session);
    if (v) g_variant_unref(v);
    if (streams) g_variant_unref(streams);
    if (props) g_variant_unref(props);
    if (fds) g_object_unref(fds);
    return r->error[0] || !r->cleanup_ok ? -1 : 0;
}

#ifdef HR_PORTAL_SELFTEST
#define CHECK(test) do { if (!(test)) { fprintf(stderr, "Buffer test failed at %d\n", __LINE__); return 1; } } while (0)
int main(void) {
    struct hr_timing timing = { 0 };
    timing_add(&timing, 0); timing_add(&timing, 25000000); timing_add(&timing, 50000000);
    timing_add(&timing, 100000000); timing_add(&timing, 100000001);
    CHECK(timing.count == 5 && timing.min_ms == 0 && timing.max_ms > 100);
    CHECK(timing.over_25ms == 3 && timing.over_50ms == 2 && timing.over_100ms == 1);
    CHECK(timing.total_ms > 275 && timing.total_ms < 276);
    /* pw_stream_destroy emits UNCONNECTED during our own cleanup. */
    struct hr_portal_result r = { 0 };
    struct probe p = { .r = &r, .tearing_down = TRUE };
    state_changed(&p, PW_STREAM_STATE_STREAMING, PW_STREAM_STATE_UNCONNECTED, NULL);
    CHECK(!r.error[0]);
    p.tearing_down = FALSE;
    state_changed(&p, PW_STREAM_STATE_STREAMING, PW_STREAM_STATE_UNCONNECTED, NULL);
    CHECK(!strcmp(r.error, "PipeWire stream disconnected"));
    p.tearing_down = TRUE;
    state_changed(&p, PW_STREAM_STATE_STREAMING, PW_STREAM_STATE_UNCONNECTED, NULL);
    CHECK(!strcmp(r.error, "PipeWire stream disconnected"));
    memset(&r, 0, sizeof(r));
    state_changed(&p, PW_STREAM_STATE_STREAMING, PW_STREAM_STATE_ERROR, "genuine failure");
    CHECK(!strcmp(r.error, "genuine failure"));
    /* Exercise the exact CreateSession arguments sent over D-Bus. */
    GVariant *args = g_variant_ref_sink(create_session_args());
    CHECK(g_variant_is_of_type(args, G_VARIANT_TYPE("(a{sv})")));
    GVariant *options = g_variant_get_child_value(args, 0);
    const char *request_token = NULL, *session_token = NULL;
    CHECK(g_variant_lookup(options, "handle_token", "&s", &request_token));
    CHECK(g_variant_lookup(options, "session_handle_token", "&s", &session_token));
    CHECK(request_token[0] && session_token[0] && strcmp(request_token, session_token));
    char *path = g_strconcat("/", session_token, NULL);
    CHECK(g_variant_is_object_path(path)); g_free(path);
    path = g_strconcat("/", request_token, NULL);
    CHECK(g_variant_is_object_path(path)); g_free(path);
    GVariant *next = g_variant_ref_sink(create_session_args());
    CHECK(!g_variant_equal(args, next));
    g_variant_unref(next); g_variant_unref(options); g_variant_unref(args);
    uint8_t pixel = 0;
    struct spa_chunk chunk = { .offset = 0, .size = 64, .stride = 16 };
    struct spa_data data = { .type = SPA_DATA_MemFd, .maxsize = 64, .data = &pixel, .chunk = &chunk };
    struct spa_meta_header header = { 0 };
    CHECK(valid_buffer(4, 4, &data, &header));
    CHECK(valid_buffer(4, 4, &data, NULL));
    CHECK(!valid_buffer(0, 4, &data, NULL));
    CHECK(!valid_buffer(UINT32_MAX, UINT32_MAX, &data, NULL));
    CHECK(!valid_buffer(4, 4, NULL, NULL));
    data.type = SPA_DATA_DmaBuf; CHECK(!valid_buffer(4, 4, &data, NULL)); data.type = SPA_DATA_MemPtr;
    chunk.offset = 1; CHECK(!valid_buffer(4, 4, &data, NULL)); chunk.offset = 0;
    chunk.size = 63; CHECK(!valid_buffer(4, 4, &data, NULL)); chunk.size = 64;
    chunk.stride = -16; CHECK(!valid_buffer(4, 4, &data, NULL)); chunk.stride = 16;
    chunk.flags = SPA_CHUNK_FLAG_EMPTY; CHECK(!valid_buffer(4, 4, &data, NULL));
    chunk.flags = SPA_CHUNK_FLAG_CORRUPTED; CHECK(!valid_buffer(4, 4, &data, NULL)); chunk.flags = 0;
    header.flags = SPA_META_HEADER_FLAG_CORRUPTED; CHECK(!valid_buffer(4, 4, &data, &header));
    data.data = NULL; CHECK(!valid_buffer(4, 4, &data, NULL));
    data.data = &pixel; data.chunk = NULL; CHECK(!valid_buffer(4, 4, &data, NULL));
    return 0;
}
#endif
