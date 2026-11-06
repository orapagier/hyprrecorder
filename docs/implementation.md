# Application implementation — 2026-10-22

This is an application implementation awaiting live acceptance, not a release
certification. The user-accepted Phase 2 video samples remain historical evidence
for their exact prototype. They do not validate the new application/audio path.

## Architecture

The `recorder` feature reuses the app-owned ext-image-copy-capture/GBM/linear
DMA-BUF primitives and embedded FFmpeg/VA-API bridge. No recorder executable or
FFmpeg executable performs production recording. `ffmpeg`/`ffprobe` are used in
synthetic tests and acceptance inspection. No software H.264 fallback exists.

The `desktop` feature adds a Qt 6 Widgets view through a small C ABI. Rust owns
settings, geometry validation, the recording process, capture and finalization.
Qt supplies native accessible controls, keyboard focus, event-driven layout and
HiDPI handling. Widgets avoid an idle animation/render loop; no custom animation
needs a reduced-motion override. Actual keyboard, pointer, text scale and idle
cost must still be measured in the desktop. References: [Qt HiDPI](https://doc.qt.io/qt-6/highdpi.html),
[FFmpeg filters](https://ffmpeg.org/ffmpeg-filters.html).

The view uses QProcess without a shell and remains unmapped through preparation,
countdown, recording and finalization. Reopening during capture keeps it hidden.
CLI acquisition also requests a hide acknowledgement from any existing view.
A region picker in progress rejects that request. The child receives SIGTERM if
the view dies; SIGINT/SIGTERM request graceful recording completion. The CLI stop
command requests finalization; it does not wait for it to finish. `status` reports
current state while the worker exists. View success is based on a saved event
and a successful worker exit, never merely on a file's existence.

A private runtime directory, advisory file lock, bounded typed JSON messages and
same-UID peer credentials protect stop/status control. A separate view lock and
socket coordinate repeated UI launches. No services, login capture, uploads,
background capture or idle device pipelines are installed.

## Geometry and bounded media ownership

The focused eligible monitor is selected once and pinned. Explicit selection is
remembered. Region rectangles use compositor logical coordinates, including
negative positions, converted with monitor scale. Inward rounding and even crop
extents never expand beyond the selected region. Cross-monitor regions fail.
Output presets/custom even sizes use GPU fit plus black padding, with chroma
aligned content and padding. At most two pixels of alignment rounding apply.
Upscaling is labeled. This build rejects rotated/flipped and odd physical source
sizes; automatic one-pixel native padding remains open rather than silently
cropping them. The core limit is 8192 pixels per dimension; the older config
schema permits up to 16384, with capability rejection before capture.

One RGB buffer and one outstanding compositor frame bound acquisition. Encoding
synchronizes conversion before RGB reuse. A retained, separately converted NV12
frame supplies repeats when compositor damage is idle; repeated frames do not
represent new source detail. Scheduling debt is discarded instead of creating
catch-up queues. Video timestamps and audio share CLOCK_MONOTONIC. The final
packet duration extends to the stop instant without reading an in-flight RGB
buffer. No unbounded per-frame diagnostic vector is retained by the application.

Monitor identity/mode/position/scale/transform and logind LockedHint are checked
while recording; changes or an unavailable lock-state query fail with a retained
partial file. CLOCK_BOOTTIME versus monotonic time detects suspend. These new
runtime behaviors require live acceptance. Native GPU calls remain synchronous;
a hung driver can delay graceful stop. The UI can die without orphaning capture,
but an uninterruptible driver call has no proven recovery deadline.

## Audio

At start, resolve the default playback sink's actual monitor source and the
default input independently. Reject a muted/unavailable requested source and a
playback monitor configured as the microphone. Turn off a requested source in the
window to proceed without it. No route, global volume or default device changes
are made. Pin source identities with Pulse DONT_MOVE; PipeWire's Pulse service is
supported by the API, subject to live checks.

Open requested streams corked, start after countdown, and ask the server for
48 kHz stereo float conversion/resampling. Correct callback arrival against
reported stream latency, then place samples on a bounded two-second monotonic
sample timeline. Stream loss, server errors, overflow, prolonged missing timing
or a >100 ms continuous timeline gap fail explicitly. This timestamp placement
is an initial implementation; long-run clock drift and quality need measurement.

Mix at unity for one source, half gain per source for two (6 dB headroom), then
clip to signed 16-bit PCM. Matroska carries H.264 plus one 48 kHz stereo PCM track;
PCM avoids another distribution-dependent audio encoder. Silent mode has no
audio stream and opens no audio server. Acoustic echo with speakers is explained
in the window; no automatic DSP or playback stream is created.

## Persistence, files and installation

Preferences live at `$XDG_CONFIG_HOME/hyprrecorder/settings.toml` (usual fallback
`~/.config`). Write a synced private sibling file, rename, then sync its directory.
Before replacing valid preferences, retain `settings.last-good.toml`. Malformed
preferences are preserved, described by the window, and block Record until fixed.
XDG Videos discovery runs only when no preferences exist, with `~/Videos` fallback.

Recording files are mode 0600. Exclusive partial-file creation and no-replace
hard-link publication prevent overwrites. Codec/trailer/AVIO/file sync errors
cannot produce a saved event. Failures retain `.partial.mkv`; pre-frame cancellation
removes only its own empty partial. Completed files are MKV; MP4 export is absent.
The filesystem must support hard links. A power loss may damage the file tail.

`tools/install.py` previews by default and installs only a binary, icon, desktop
entry and receipt. Updates/uninstall require matching hashes for owned files;
customized or unowned files block the whole operation. Verified local backups
precede writes and an immediate write failure restores the prior managed files.
Preferences, recordings, unrelated files and backup history survive uninstall.
The installer makes no package/service/shortcut changes. Fedora and Arch manifest
files are build inputs, not claims of clean-distro testing.

Project source is MIT licensed. Cargo metadata records Rust dependency licenses;
the validation tool exports that inventory. Dynamic Qt, FFmpeg, Pulse, GBM/DRM
libraries retain their upstream licenses, build options and obligations. Inspect
the exact distribution libraries before redistributing a binary: Qt can be
LGPL/GPL/commercial; FFmpeg licensing depends on its configured components. GPU
drivers and codec distribution constraints are separate from the app license.

## Validation boundary

`python3 tools/validate.py` runs formatting, strict default/all-feature lint,
default/all-feature tests, Python harness/installer tests, the offscreen Qt smoke
and a release desktop build. It writes logs, native versions and a Rust license
inventory under a new private temporary evidence directory. It captures no screen
or microphone. Tests exercise native codec/mux decode, PCM mixing, malformed
preferences, non-overwrite publication, disk errors, typed IPC, locks, scaling,
negative monitor origins and installation preservation.

Unix socket binding and socket transmission are denied by this agent sandbox.
The real listener/slow-client/stale-socket test is explicitly ignored here;
`python3 tools/validate.py --session` includes it in the user desktop terminal.
Pure protocol parsing/state and file-lock tests remain executable here. No
compositor/GPU/audio session access is available to this agent. Exact results
and the installed-copy status are recorded in PLAN.md after validation. The
remaining release gate is [the live acceptance checklist](release-checklist.md).

## Monitor eligibility correction — 2026-10-22

The initial discovery filter accepted only an empty `mirrorOf` field. Hyprland
uses `"none"` for ordinary non-mirrored outputs, so the filter incorrectly removed
them all and the window submitted an empty connector when Record was clicked.
The filter now accepts `"none"`, empty and omitted fields while excluding actual
mirror targets and disabled outputs. A genuinely empty result disables Record
and region selection with a discovery message.

Regression checks cover the raw monitor response, the actual `monitors` CLI via
an isolated hyprctl fixture, and an offscreen window fed through the production
parser. The UI check now requires a selected connector and enabled Record.
Seventy all-feature Rust tests, strict Clippy and the desktop release build pass;
the existing real-listener test remains explicitly ignored in the sandbox. The
installed binary passed the monitor fixture and enhanced UI checks. Real desktop
confirmation requires reopening the app; the agent cannot access its compositor.
Checkpoint: `20261022T031709Z-s3d1vmqp` under the shared agent-backups directory.

## SDR color metadata correction — 2026-10-22

A user reported altered text colors while finding audio acceptable. Read-only
inspection of the finalized 1920×1080 H.264 recording found no range, matrix,
primaries or transfer tags. The GPU conversion requested BT.709 limited range,
but the codec context left its color fields unspecified. This is a confirmed
metadata omission; it does not prove which matrix the user's player selected.

The bridge now consistently describes full-range sRGB input and limited-range
BT.709-matrix YUV with BT.709 primaries, sRGB transfer and left chroma placement,
through conversion frames, codec context/VUI and container. The retained CPU
spike/test conversion explicitly uses the same matrix/range before assigning
those tags. The codec, bitrate, chroma format and audio path remain unchanged.
References: [FFmpeg scale_vaapi](https://github.com/FFmpeg/FFmpeg/blob/n8.0/libavfilter/vf_scale_vaapi.c),
[VAAPI color properties](https://github.com/FFmpeg/FFmpeg/blob/n8.0/libavfilter/vaapi_vpp.c).

A native encode/decode regression verifies all four color tags and <=8/255
channel error at centers of 12 solid RGB patches. All-feature validation passes
71 Rust tests; the real-listener test remains explicitly ignored in this sandbox.
Clippy, formatting, desktop release and installed offscreen UI checks pass. Live
GPU pixel fidelity and subjective improvement still need a fresh recording.

A short metadata-only preview was created without replacing the original video;
its full decode and copied audio payload hash pass. It is a diagnostic comparison,
not restoration of chroma detail. The 4:2:0 format can still alter fine colored
edges or subpixel-antialiased text; correct metadata cannot restore discarded
chroma samples. SDR assumptions do not establish HDR/ICC color management.
Checkpoints: implementation/install `20261022T040324Z-q4n2max1`, preview
`20261022T040702Z-cijsp8nk`; the first contains a rollback.sh for both.
