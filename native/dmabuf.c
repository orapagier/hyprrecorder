#define _GNU_SOURCE
#include <gbm.h>
#include <xf86drm.h>
#include <drm_fourcc.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Private ABI: every exported plane fd is owned by Buffer, borrowed by Rust. */
typedef struct {
    uint32_t width, height, format, planes;
    uint64_t modifier;
    int32_t fds[4];
    uint32_t strides[4], offsets[4];
} Descriptor;
typedef struct { int fd; struct gbm_device *gbm; char node[256]; } Device;
typedef struct { struct gbm_bo *bo; Descriptor desc; } Buffer;

static void error(char *out, const char *message) {
    snprintf(out, 512, "%s", message);
}
void hr_gpu_close(Device *device) {
    if (!device) return;
    if (device->gbm) gbm_device_destroy(device->gbm);
    if (device->fd >= 0) close(device->fd);
    free(device);
}
Device *hr_gpu_open(uint64_t dev_id, const char *override, char *err) {
    Device *device = calloc(1, sizeof(*device));
    drmDevicePtr advertised = NULL, selected = NULL;
    if (!device) { error(err, "Cannot allocate GBM device state"); return NULL; }
    device->fd = -1;
    /* Fail an explicit inaccessible path before touching any other GPU. */
    if (override) {
        device->fd = open(override, O_RDWR | O_CLOEXEC);
        if (device->fd < 0) { error(err, "Cannot open explicit render device"); goto fail; }
    }
    int result = drmGetDeviceFromDevId((dev_t)dev_id, 0, &advertised);
    if (result < 0 || !advertised) {
        error(err, "Cannot resolve compositor DMA-BUF device through libdrm"); goto fail;
    }
    if (!(advertised->available_nodes & (1 << DRM_NODE_RENDER))) {
        error(err, "Compositor device has no accessible render node"); goto fail;
    }
    const char *node = override ? override : advertised->nodes[DRM_NODE_RENDER];
    if (strlen(node) >= sizeof(device->node)) {
        error(err, "Render node path too long"); goto fail;
    }
    snprintf(device->node, sizeof(device->node), "%s", node);
    if (!override) device->fd = open(node, O_RDWR | O_CLOEXEC);
    if (device->fd < 0) { error(err, "Cannot open compositor render device"); goto fail; }
    if (drmGetNodeTypeFromFd(device->fd) != DRM_NODE_RENDER ||
        drmGetDevice2(device->fd, 0, &selected) < 0 || !selected ||
        !drmDevicesEqual(advertised, selected)) {
        error(err, "Selected render node does not match compositor DRM device"); goto fail;
    }
    device->gbm = gbm_create_device(device->fd);
    if (!device->gbm) { error(err, "gbm_create_device failed"); goto fail; }
    drmFreeDevice(&selected);
    drmFreeDevice(&advertised);
    return device;
fail:
    if (selected) drmFreeDevice(&selected);
    if (advertised) drmFreeDevice(&advertised);
    hr_gpu_close(device);
    return NULL;
}
const char *hr_gpu_node(const Device *device) { return device->node; }
void hr_gpu_free_buffer(Buffer *buffer) {
    if (!buffer) return;
    for (unsigned i = 0; i < 4; i++)
        if (buffer->desc.fds[i] >= 0) close(buffer->desc.fds[i]);
    if (buffer->bo) gbm_bo_destroy(buffer->bo);
    free(buffer);
}
Buffer *hr_gpu_buffer(Device *device, uint32_t width, uint32_t height,
                      uint32_t format, const uint64_t *modifiers, uint32_t count,
                      Descriptor *out, char *err) {
    if (width < 2 || height < 2 || width > 8192 || height > 8192 ||
        width % 2 || height % 2 || count == 0 || count > 256 ||
        (format != DRM_FORMAT_XRGB8888 && format != DRM_FORMAT_ARGB8888)) {
        error(err, "Unsupported GBM buffer constraints"); return NULL;
    }
    Buffer *buffer = calloc(1, sizeof(*buffer));
    if (!buffer) { error(err, "Cannot allocate GBM buffer state"); return NULL; }
    for (unsigned i = 0; i < 4; i++) buffer->desc.fds[i] = -1;
    buffer->bo = gbm_bo_create_with_modifiers2(device->gbm, width, height, format,
                                             modifiers, count, GBM_BO_USE_RENDERING);
    if (!buffer->bo) { error(err, "GBM rejected negotiated format/modifiers"); goto fail; }
    Descriptor *desc = &buffer->desc;
    desc->width = gbm_bo_get_width(buffer->bo);
    desc->height = gbm_bo_get_height(buffer->bo);
    desc->format = gbm_bo_get_format(buffer->bo);
    desc->modifier = gbm_bo_get_modifier(buffer->bo);
    int planes = gbm_bo_get_plane_count(buffer->bo);
    if (planes < 1 || planes > 4 || desc->width != width || desc->height != height ||
        desc->format != format || desc->modifier == DRM_FORMAT_MOD_INVALID) {
        error(err, "GBM returned unsupported buffer metadata"); goto fail;
    }
    int matched = 0;
    for (unsigned i = 0; i < count; i++) matched |= modifiers[i] == desc->modifier;
    if (!matched) { error(err, "GBM returned an unnegotiated modifier"); goto fail; }
    desc->planes = (uint32_t)planes;
    for (int i = 0; i < planes; i++) {
        desc->fds[i] = gbm_bo_get_fd_for_plane(buffer->bo, i);
        desc->strides[i] = gbm_bo_get_stride_for_plane(buffer->bo, i);
        desc->offsets[i] = gbm_bo_get_offset(buffer->bo, i);
        if (desc->fds[i] < 0 || !desc->strides[i] ||
            fcntl(desc->fds[i], F_SETFD, FD_CLOEXEC) < 0) {
            error(err, "Cannot export GBM plane descriptor"); goto fail;
        }
    }
    *out = *desc;
    return buffer;
fail:
    hr_gpu_free_buffer(buffer);
    return NULL;
}
