# Phase 2 — capture spike and decision gate

2026-10-22 application continuation: the owned ext/linear DMA-BUF/VA-API path
is selected for implementation based on the accepted prototype samples below.
This is a provisional engineering choice, not a performance winner or completion
of the controlled comparison gate. Portal and external comparison tools remain
available. No external recorder became a permanent dependency. The Qt desktop
view, private lifecycle/IPC, file publication, fit/padding and audio implementation
are described in [implementation.md](implementation.md). Their new live acceptance
and long audio-sync gates remain open; historical sample results do not certify
the new path.

## Historical investigation record

2026-10-21. **In progress; no backend selected.** The first owned capture now
finalizes and decodes correctly, with user-accepted motion, colors and cursor.
The controlled performance comparison remains open. The user also confirmed
synthetic hardware encoding.
The production `record` command is unimplemented. An opt-in `spike` command now
implements our own experimental capture-to-encoder path. The Python spike is
separate disposable external baseline tooling.

## Observed prerequisites

Initial read-only probes in this agent session (updated prerequisites below):

| Component | Observed result |
| --- | --- |
| Fedora | 44 |
| FFmpeg runtime | ffmpeg-free 8.1.3-1.fc44; ffmpeg/ffprobe commands available |
| GStreamer runtime | 1.28.8-1.fc44; launch/inspect available |
| PipeWire headers | pipewire-devel 1.6.9-1.fc44 |
| FFmpeg development | libavcodec/format/util/filter/device, swscale/swresample pkg-config metadata absent |
| GStreamer development | core/video/pbutils pkg-config metadata absent |
| PipeWire GStreamer source | pipewiresrc element absent |
| VA H.264 GStreamer encoders | vah264enc and vaapih264enc elements absent |
| External comparison tools | wl-screenrec, gpu-screen-recorder, wf-recorder absent |
| GPU | /dev/dri not visible to this agent |
| Hyprland | monitor IPC query failed; no session diagnosis follows |

Runtime libraries do not provide headers/link metadata. An installed encoder
catalog is not a successful encode. These restrictions prevent actual captures
and the required comparison. The agent changed no packages, audio routes,
desktop files or bindings. The user subsequently installed build prerequisites
from their terminal; GPU/session access remains restricted for the agent.

### User-provided graphical-session evidence

The user ran the native protocol probe and encoder diagnostics in a graphical
terminal. This is supplied live evidence, separate from agent sandbox access:

- Both wlr-screencopy and ext output capture managers, plus DMA-BUF, advertised.
- One focused 1920×1080 monitor at scale 1 was reachable; render node opens.
- Default playback and microphone identities resolved (device names omitted).
- Synthetic VA-API H.264 probe encoded 30 frames successfully.
- At that point, FFmpeg and GStreamer development metadata were absent.
- No wl-screenrec, GPU Screen Recorder or wf-recorder command was found.

Protocol discovery and synthetic encoding passed. Neither establishes a real
screen recording, format negotiation, crop/scale, cursor behavior or audio sync.

The supplied Fedora DNF preview for libdrm-devel, freeglut-devel, clang and
ffmpeg-free-devel proposed 16 new packages, 37 MiB download, 176 MiB installed,
with no upgrades/removals. The user aborted that preview; nothing was installed.
For the app-owned direct Wayland/FFmpeg experiment, omit freeglut-devel: it came
from the external baseline's build instructions, not an established app need.
The user then installed libdrm-devel, clang and ffmpeg-free-devel in successful
**DNF transaction 32**, with 13 additions and no upgrades/removals. Versions:
clang/clang-libs 22.1.8-4.fc44, FFmpeg headers 8.1.3-1.fc44,
libdrm-devel 2.4.134-1.fc44. pkg-config now resolves libavcodec 62.28.103,
libavformat 62.12.103, libavutil 60.26.103, libavfilter 11.14.103 and libdrm
2.4.134 in both the supplied output and the agent's subsequent check.

No external recorder is a product prerequisite. Defer its installation while
preparing our own capture prototype. Required comparison evidence remains open.
For an additions-only installation, retain the exact successful DNF transaction
ID and package list; native `dnf history undo ID` can reverse that transaction.
Inspect its undo preview before applying and preserve intervening package work.
An upgrade/removal would require a stronger recovery point before proceeding.
A file checkpoint does not back up system packages or RPM scriptlet effects.
For this exact package transaction, preview recovery with
`sudo dnf history undo 32 --assumeno`; applying it is a separate explicit action.

This evidence update's file checkpoint is
`/home/ramlej/.local/state/hyprshell/agent-backups/20261021T135708Z-5w1k2ds4`.
Undo only this documentation update with:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T135708Z-5w1k2ds4 --apply
```

## Dependency inventory for the graphical continuation

These are candidate build/runtime inputs, not an installation script or a
verified clean-install manifest. Confirm provider availability and ABI versions
before any recoverable package transaction; do not mix incompatible providers.

| Path | Fedora candidate packages | Arch candidate packages |
| --- | --- | --- |
| Shared tools | rust, cargo, pkgconf-pkg-config, ffmpeg-free | rust, pkgconf, ffmpeg |
| Direct Wayland/FFmpeg build | wayland-devel, libdrm-devel, libva-devel, clang-devel, matching ffmpeg-free-devel or selected FFmpeg provider's development package | wayland, libdrm, libva, clang, ffmpeg |
| Portal/GStreamer build | gstreamer1-devel, gstreamer1-plugins-base-devel, pipewire-devel | gstreamer, gst-plugins-base, pipewire |
| Portal runtime | xdg-desktop-portal, xdg-desktop-portal-hyprland, pipewire, gstreamer1-plugin-pipewire | xdg-desktop-portal, xdg-desktop-portal-hyprland, pipewire, gst-plugin-pipewire |
| GStreamer encode/mux | gstreamer1-plugins-bad-free, gstreamer1-plugins-good; inspect actual H.264/VA elements | gst-plugins-bad, gst-plugins-good; inspect actual VA elements |
| GPU driver | hardware-matching VA provider; validate with vainfo and real encode | hardware-matching VA provider; validate with vainfo and real encode |

Fedora codec/provider restrictions must be resolved explicitly if the selected
distribution packages lack the required encoder. Do not assume installing a
similarly named plugin supplies working H.264. No repository/provider switch is
authorized by this inventory. Arch rows are documentation candidates, not tested
on an Arch machine. Exact resolved versions remain part of the open gate.

## Owned Rust capture and embedded FFmpeg spike

Build with `--features capture-spike`. The default diagnostics build retains its
header-free dependency contract. A small original C ABI bridge isolates FFmpeg
structures; Rust owns Wayland discovery/capture, memory/fd lifetimes, pacing,
output creation, cancellation and reporting. It launches no recorder or encoder
subprocess. There is no silent fallback from h264_vaapi to a software encoder.

The initial path intentionally uses shared memory, CPU RGB-to-NV12 conversion
and an explicit GPU upload. It establishes acquisition/library integration
before pursuing shared GPU buffers. It is not the optimized candidate or an
architecture decision. Wlr-screencopy is deprecated in favor of ext image-copy;
this spike uses the already advertised compatibility protocol only for the
experiment. Production protocol choice remains open.

Run from the graphical terminal after preparing nonprivate scrolling/motion:

```sh
cargo run --locked --features capture-spike -- spike --monitor eDP-1 --seconds 10 --directory /tmp/hyprrecorder-owned-01
```

The directory must not exist, and its parent must exist. A three-second countdown
precedes acquisition; hide the terminal yourself. No microphone or system audio
is opened, and TOML settings are not used by this explicit video-only experiment.
The required GPU device is `/dev/dri/renderD128` unless `--render-device` changes
it. `--fps 30|60` and `--no-cursor` are available. Stop with Ctrl+C/SIGTERM or let
the requested duration finish. Cancellation without acquired frames is an error.

Supported inputs are wlr-screencopy v3, wl_output v4 names, unrotated scale-1
outputs and even 2–8192 pixel XRGB8888/ARGB8888 frames. Unsupported transforms,
scale, dimensions and formats fail explicitly. Each capture buffer is bounded
to 256 MiB. Capture and encoding now overlap, with one queue slot and at most
three retained capture buffers (being captured/submitted, queued, and encoding).
Backpressure lowers actual FPS rather than growing queues. Format/dimension or
monitor mode/scale/transform change stops the
sample as a failure and retains its partial file. Wayland waits time out after
three seconds; synchronous GPU/codec calls have no watchdog yet and may delay
cancellation, so this is not production lifecycle/recovery validation.

Successful finalization flushes the codec, writes/checks the muxer trailer and
AVIO errors, syncs the file, publishes `capture.mkv` without replacing any existing
name, and syncs the directory. Failures retain `capture.partial.mkv`; an error
report is written when possible, never a saved-success message. An error after
publication can leave `capture.mkv` present while the result remains a failure.
Publication is
not a claim of crash-proof atomic durability. No saved notification is emitted.
No IPC, region selection, scaling, audio or UI is implemented in this spike.

`report.json` contains actual source dimensions/stride, frame count, source
presentation timestamps and monotonic acquisition PTS, per-frame capture/queue/encode
times, process CPU seconds/RSS and byte size, interruption and finalization state.
The nominal target is 12 Mbit/s, two-second GOP, no B-frames; rate-control policy
uses the encoder/driver default and needs characterization before quality-matched
comparisons. No quality equivalence is claimed from the nominal bitrate alone.
RSS is the process high-water mark including startup; CPU deltas cover the capture
loop/finalization. Hardware/driver allocations and system/GPU/power costs are not
measured. Late iterations are not measured dropped-source frames. The first
sample includes encoder initialization; use a warm-up before comparisons.
Source timestamps may repeat for static content; acquisition PTS remain monotonic.
These data do not establish correct colors/cursor, quality, text/motion or hardware
memory negotiation. Visual acceptance remains explicitly unverified.

Offline checks: default 15 Rust tests, 29 Rust tests with capture-spike, and five
Python harness tests pass; formatting and both feature/default Clippy pass.
New tests encode a synthetic software-only fixture through the same library
writer and decode it with FFmpeg, checking frame count, timestamps, padded stride
and vertical inversion. Separate tests exercise undersized frames, repeated
finalization, missing GPU failure with no fallback, and `/dev/full` write failure.
The software fixture has no recording CLI option and is not a hardware test.
At implementation validation, live acquisition and VA-API library encode were
still unverified; the subsequent first real capture is documented below.
An explicit unavailable-session CLI check also exits 2 before creating a sample
directory. No real screen/audio was captured during agent validation. Native
sanitizer/leak tooling is unavailable here; those checks remain open.

File checkpoint for this continuation:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261021T140749Z-j62elrto`.
Only the Hyprrecorder checkout changed; no desktop reload is needed. File rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T140749Z-j62elrto --apply
```

This preserves earlier uncommitted Phase 2 work and does not undo DNF transaction
32. The checkpoint's notes retain the baseline and validation/recovery plan.

### First real owned capture — 2026-10-21

The user ran the ten-second, 30-FPS owned spike in their graphical session. The
agent independently read the sample's JSON and ran FFprobe with frame counting
against `/tmp/hyprrecorder-owned-01/capture.mkv`; no captured content was copied
into the repository and no desktop/GPU access from the agent was assumed.

| Check | Observed result |
| --- | --- |
| Encoded video | H.264, 1920×1080, yuv420p, no audio requested |
| Finalization | true; no reported error or interruption |
| Decoder frame count | 287, matching the writer report |
| Container duration | 9.979 seconds |
| Output size | 11,322,499 bytes |
| Acquisition PTS | 287 entries, strictly increasing; span 9.979237 seconds |
| Actual acquisition rate | about 28.66 FPS over that PTS span, below requested 30 |

This establishes an actual screen-to-embedded-VA-API-H.264/MKV path. The recorder
has no software encoding fallback. After playback troubleshooting, the user
confirmed that motion, colors and cursor looked correct. This accepts that
sample's visible video behavior only. Text-legibility checks and explicit
first/last-frame UI exclusion remain open; frame counting alone does not
establish them. FFprobe's nominal average-rate metadata is not the
actual acquisition rate above. No hardware-buffer negotiation claim follows.

Exploratory sample diagnostics: 10.020935 seconds including finalization,
2.226962 process user-CPU seconds, 0.253156 system-CPU seconds, 83,716 KiB peak
RSS, and 44 late iterations. Mean measured capture and push/encode times were
20.492 ms and 7.967 ms respectively. These figures come from this one uncontrolled
ten-second desktop sample, not the required warmed-up repeated benchmark. They
exclude system/GPU/power cost, do not count dropped source frames, and support
no comparative performance or quality claim. Workload and comparable quality
were not established. The CPU-copy path remains a provisional experiment.

Evidence-only documentation checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261021T142038Z-n5pp82g6`.
Recovery (does not alter the recording or code):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T142038Z-n5pp82g6 --apply
```

The subsequent visual-acceptance documentation update has checkpoint
`/home/ramlej/.local/state/hyprshell/agent-backups/20261021T150345Z-t0uwmhyh`.
Undo only that update with:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T150345Z-t0uwmhyh --apply
```

No code, recording media or desktop settings changed for this acceptance update.
The following exploratory 1080p60 run does not replace the plan's repeated,
quality-matched comparison measurements.

### Interrupted 1080p60 run and bounded overlap continuation

The user requested 60 FPS for 60 seconds, then pressed Ctrl+C. The original
serial path finalized successfully after 41.633 seconds including shutdown.
FFprobe counted all 1861 reported H.264 frames at 1920×1080, over a 41.586-second
file. Acquisition PTS were strictly increasing, spanning 41.586043 seconds:
**44.7265 FPS** over that span. This is evidence of interrupt/finalization and
successful decoding, not a full-duration 60-FPS sample. The supplied mpv log
selected VA-API decoding and reached EOF after earlier Vulkan/CUDA probe errors.
No additional visual-quality acceptance is inferred from that log.

The report recorded 1858 late iterations, 11.282584 user-CPU seconds, 1.414767
system-CPU seconds, 85,092 KiB peak RSS and 38,762,446 bytes. Mean acquisition
and encoding times were 16.095 ms and 6.259 ms. These serial stages sum to roughly
22.35 ms, consistent with the observed rate; this motivates overlap but does not
prove the new version can sustain 60 FPS. Workload/quality were not controlled
and no comparative or power claim follows from this interrupted sample.

The updated path moves each completed mapped capture buffer to a dedicated
encoder thread through a capacity-one channel. FFmpeg contexts and the output fd
are created, used and released on that thread. Each capture has a distinct memfd;
no second pixel copy is added, and the compositor cannot overwrite the encoder's
previous buffer. The previous synchronous encoding step no longer delays the next
capture request when the queue has capacity. Queue saturation waits and checks
cancellation; it never grows the queue or silently drops captured frames.

On normal stop, the queue drains before codec flush, trailer, sync and publication.
On source failure the worker is joined without finalizing/publishing success;
worker initialization/encode/write errors propagate to the main error report.
Repeated stop signals remain the same idempotent flag. Native GPU calls still
have no watchdog, so a stalled driver can delay join/cancellation. The protocol,
cursor/FPS options, quality settings, CPU conversion/upload path and video-only
scope are unchanged. This is still a Phase 2 experiment, not the chosen backend.

Reports now identify `pipeline: bounded-capture-encode-overlap`, queue capacity
and per-frame queue-wait time. Frame counts/timings come from frames accepted by
the encoder worker, not merely enqueued frames. Source and acquisition clocks
are preserved. New synthetic tests exercise capacity/backpressure, frame order,
draining/finalization, source-failure cleanup, missing-GPU startup and disk-write
error propagation; 23 feature-enabled Rust tests pass. These tests do not exercise
live compositor buffer lifetimes or hardware encoding; both need a new sample.

Retest with a new directory and let the minute finish automatically:

```sh
cargo run --locked --features capture-spike -- spike --monitor eDP-1 --fps 60 --seconds 60 --directory /tmp/hyprrecorder-overlap-60fps-01
```

The previous recordings remain untouched. Only the Hyprrecorder checkout changed.
Checkpoint: `/home/ramlej/.local/state/hyprshell/agent-backups/20261021T150932Z-i9k6wblh`.
Rollback this continuation (no desktop reload or package rollback required):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T150932Z-i9k6wblh --apply
```

### Nonblocking I/O repair after the first overlap attempt

The first overlap sample failed after 6.892488 seconds with
`Io error: Resource temporarily unavailable (os error 11)`. Its report contains
191 encoded/accepted frames, `finalized: false`, `interrupted: false` and the
error; the partial MKV remains in `/tmp/hyprrecorder-overlap-60fps-01`. It was
not published as a successful recording. This sample neither validates the
overlap performance nor replaces any required full-duration comparison.

Inspection of the pinned wayland-backend 0.3.17 source confirms that a read guard
can return `WouldBlock` after readable traffic containing only internal display
notifications, such as object deletions. Flush can also return `WouldBlock` when
outgoing socket buffers are full. Our wait loop propagated both as fatal errors.
The unannotated original report cannot distinguish which call returned it;
both paths are now handled correctly instead of diagnosing the compositor/GPU.

The loop retains its prepared read across polling, requests POLLOUT only while
the outgoing flush is blocked, and retries transient reads/flushes. Write-only
readiness never invokes the read closure. Timeouts/interrupted polling cancel
the prepared read and return to the existing cancellation/deadline checks.
The same three-second deadline is retained across retries. Actual socket
disconnects, protocol errors and other I/O failures remain fatal; new messages
identify read/flush/poll context. Queue, buffers, encoder and source settings,
artifact preservation and finalization behavior are unchanged.

Deterministic regression tests replay blocked flush → write-only readiness →
read WouldBlock → a delivered event, and verify fatal flush/protocol/disconnect
errors plus poll timeout/interruption behavior. They need no compositor or GPU.
All 26 feature-enabled Rust tests, feature Clippy, formatting and diff checks
pass; the default build check also passes. These validate the retry logic, not
the repaired path's live hardware behavior or performance.

Retry in a fresh directory:

```sh
cargo run --locked --features capture-spike -- spike --monitor eDP-1 --fps 60 --seconds 60 --directory /tmp/hyprrecorder-overlap-60fps-02
```

Only the checkout changed; no recording, package or desktop files were altered.
Checkpoint: `/home/ramlej/.local/state/hyprshell/agent-backups/20261021T152248Z-gx4woxdr`.
File rollback (returns to the pre-fix overlap implementation, including its known
retry bug; no reload required):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T152248Z-gx4woxdr --apply
```

### Completed overlap retest — requested 1080p60

After the I/O repair, the user ran `/tmp/hyprrecorder-overlap-60fps-02` for the
full minute. The agent independently read the report and counted decoded frames
with FFprobe. Observations:

| Check | Result |
| --- | --- |
| Completion | 60.027964 seconds including finalization, no interruption or error |
| Published video | H.264, 1920×1080, 1809 decoded frames matching the report |
| Media duration | 59.972 seconds; acquisition PTS span 59.972318 seconds |
| Actual acquisition | **30.1472 FPS**, below requested 60 FPS |
| Output size | 41,174,585 bytes |
| PTS | Strictly increasing |
| Mean capture time | 33.144 ms; median 31.189 ms; p95 50.447 ms |
| Mean encode time | 15.870 ms; median 15.840 ms; p95 21.059 ms |
| Encoder queue wait | Mean 0.000081 ms; max 0.000255 ms (no observed queue saturation) |

Other exploratory counters: 1807 late iterations, 27.711956 process user-CPU
seconds, 2.740189 system-CPU seconds and 85,228 KiB process peak RSS. The supplied
mpv log used VA-API and reached EOF. Visual quality for this latest sample
remains unverified. This single uncontrolled run confirms successful
completion after the EAGAIN repair; it does not establish universal reliability,
quality equivalence or a performance improvement over the serial samples.

The report shows acquisition taking roughly a 30-FPS interval while the encoder
queue has essentially no wait. That makes acquisition/shared GPU work the next
investigation; it does not identify the root cause. The source refresh rate was
not recorded in this report and cannot currently be queried through agent
Hyprland IPC. Obtain it from the user session before blaming a display mode,
compositor or driver. The 60-FPS requirement and controlled comparisons remain
open. This is not accepted as a 60-FPS recording merely because 60 was requested.

This evidence update changes only PLAN.md and this document. Checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261021T154207Z-qby0fxan`.
File rollback (no code, media, packages or desktop settings affected):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T154207Z-qby0fxan --apply
```

### Confirmed display refresh and source-only diagnostic

The user supplied a current Hyprland monitor query reporting **60.008 Hz at
scale 1** for the selected output. A current 30-Hz display mode therefore does
not explain the approximately 30-FPS sample. This does not prove the compositor
delivers every refresh through the capture API or identify a GPU/driver cause.

`probe-source` now reuses the same named-output discovery, wlr-screencopy v3,
buffer negotiation/copy, cursor flag, pacing, cancellation and nonblocking I/O
path as `spike`. It removes the encoder worker, CPU color conversion, GPU upload
and muxing. Completed anonymous mapped buffers are discarded immediately after
acquisition without pixel processing. No encoder render device or audio source
is opened, no raw frames/video file is written, and existing directories are
refused. It writes only a private `report.json`:

```sh
cargo run --locked --features capture-spike -- probe-source --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-01
```

Close the previous media player and keep scrolling/motion comparable. The
result will distinguish source-only acquisition limits from the added work and
resource interactions of recording. A single uncontrolled probe is not a
quality/performance comparison or evidence for any one driver fault.

The report labels `mode: source_only`, `encoder: none`, zero media bytes and
`finalized: false`. `completed` indicates successful core execution; check
`interrupted` separately for early stop. Video acceptance is `not_applicable`.
Recording mode retains its existing artifact/finalization behavior and is now
labeled `mode: recording`. Both modes report/print actual acquisition FPS from
frame timestamps, not the requested cap or nominal codec metadata. At least
two frames with a positive PTS span are needed for a rate; otherwise it is null.
Current `wl_output` refresh is also reported when available, and refresh changes
join the existing mode/scale/transform stop condition so metadata does not silently
refer to a previous display mode.

Default diagnostics builds retain their original dependencies; this command
requires the existing capture-spike feature and installed FFmpeg build inputs
even though the probe creates no encoder. `--render-device` is rejected for the
source-only command because it cannot select the compositor's capture device.

All 29 feature-enabled Rust tests and Clippy/fmt checks pass. New checks exercise
source-only no-media output, refusal to overwrite an existing destination,
retention of recording's partial-file staging, irregular acquisition timestamp
rate calculation and rejection of an unused encoder option before session access.
The default build check passes. An explicit unavailable-session CLI probe is
checked separately without acquiring screen content or creating an output path.
The user subsequently ran this source-only command successfully; results follow
below. The agent cannot acquire from the live compositor, and a source probe does
not inherit the old recording's visual acceptance.

Only the Hyprrecorder checkout changed; recordings, packages and desktop settings
are untouched. Checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261021T154539Z-76ls3abm`.
Rollback (no reload needed):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T154539Z-76ls3abm --apply
```

### Source-only result and explicit ext SHM comparison

`/tmp/hyprrecorder-source-01/report.json` records 287 frames at **28.687 FPS**
over 10.0047 seconds, completed without interruption/error. The selected output
reports 60.007 Hz through wl_output (the user's Hyprland query reports 60.008).
Mean acquisition latency was 34.816 ms, median 32.091 ms, p95 53.031 ms.
Encoder/queue timings and media bytes are zero; CPU user/system time was
0.103834/0.061637 seconds and peak RSS 44,940 KiB. This rules out encoding as a
necessary cause of that run's acquisition limit. It does not establish why
acquisition waits or compare controlled workloads.

The next experiment explicitly selects `--capture-backend ext` for either
experimental command. The existing wlr path remains the default, with no silent
fallback. ext-image-copy-capture v1 plus ext-output-image-capture-source v1
negotiate SHM constraints once per session instead of wlr's per-frame offer/copy
handshake. This is a testable alternative, not a demonstrated speed improvement.
The implementation uses the pinned wayland-protocols 0.32.13 staging bindings
and their protocol XML as its contract; it requires no new system packages.

Only XRGB8888/ARGB8888 SHM buffers are used; all existing size, scale, transform,
memory limits and bounded encoder ownership remain. DMA-BUF offers are ignored,
so this is still CPU-copy capture. Constraint changes or session/frame failure
stop with an error. Each new buffer is damaged in full. At most one protocol
frame exists per session, destroyed before another request. Pixel access waits
for Ready after presentation metadata, rather than treating timestamp arrival
as permission to read. The encoder can retain the buffer after frame destruction.

The protocol permits waiting for content changes after the first frame. Its
frame wait therefore checks cancellation/source failure while respecting the
overall requested deadline, without falsely reporting an idle source as broken.
Constraint negotiation retains the three-second timeout. A run with no frames
still fails. A trailing static interval is not yet extended in the video output;
this prototype does not duplicate frames or claim a constant acquisition rate.
Use continuous motion and comparable workload for the FPS experiment:

```sh
cargo run --locked --features capture-spike -- probe-source --capture-backend ext --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-ext-01
```

The first live ext source-only result is recorded below; ext recording/encoding
and visual acceptance remain unverified. Phase 2 stays
open for live comparison, GPU buffers, portal capture and repeatable benchmarks.
All 33 feature-enabled Rust tests pass, including incomplete/unsupported SHM
constraints, metadata readiness ordering, timestamp validation, frame failure
and explicit backend selection. Feature Clippy with warnings denied, default
build, formatting and whitespace checks pass. No live ext result is inferred
from those checks.
Only repository files changed; no desktop/package/player changes were made.
Checkpoint and rollback (no reload needed):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T155437Z-8hi3q7kx --apply
```

### First live ext source-only result

`/tmp/hyprrecorder-source-ext-01/report.json` records a successful, uninterrupted
10.0049-second run: 120 1920x1080 frames, **11.951 FPS**, no error, no encoder,
zero media bytes, peak RSS 45,108 KiB, CPU user/system 0.046880/0.020977 seconds.
The capture wait averaged 83.202 ms, but its median was **32.572 ms**, p95
397.749 ms and maximum 499.887 ms. Median presentation interval was 32.793 ms.
The preceding wlr source run's median acquisition was 32.091 ms.

The lower average includes long waits, rather than uniform delivery at roughly
84 ms per frame. The user subsequently confirmed scrolling this conversation
during the run. Therefore do not attribute these waits to an idle screen.
Manual scrolling still does not establish damage at every display refresh, and
the report cannot distinguish gaps in content updates from compositor/copy
delays. Do not label these waits dropped frames or rank the two backends from
uncontrolled samples. Neither source path has yet demonstrated 60-FPS
acquisition in these samples. No code/desktop/player or
package changes were made for this evidence update; only PLAN.md and this file.
Validation: report statistics recalculated and documentation diff checked.
Checkpoint and rollback (no reload needed):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T203459Z-awzfusiy --apply
```

### Next acquisition experiment after confirmed scrolling

Move to DMA-BUF acquisition with GBM allocation instead of repeating the same
SHM probe. This tests a different buffer transport without assuming it will fix
the observed pacing. At that checkpoint runtime mesa-libgbm 26.2.4-1.fc44 was
installed, but gbm.h, gbm.pc and mesa-libgbm-devel were absent. Cached Fedora repository metadata listed
mesa-libgbm-devel 26.2.4-1.fc44 as the provider of pkgconfig(gbm). EGL, libdrm and
libavutil development metadata are present. No packages were installed by the
agent. The proposed implementation must negotiate device/format/modifiers and
retain explicit failure rather than fall back silently to SHM. Capture-only GPU
buffers will not establish GPU conversion/encoding, image correctness or the
complete recording path. The implementation continuation follows below; live
validation remains outstanding.

This documentation clarification is checkpointed separately. Rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T203745Z-k6hixd4g --apply
```

### Owned DMA-BUF acquisition probe

The user completed the GBM development installation. Local RPM/pkg-config checks
now confirm mesa-libgbm-devel, libgbm, DRI, EGL, GL, filesystem and Vulkan drivers
at **26.2.5-1.fc44**. The previous SHM samples used 26.2.4, so those historical
numbers cannot isolate the effect of transport from the Mesa update. The failed
download did not install the development files; the subsequent user transaction
did. The agent made no package, player, desktop or service changes.

`--features dmabuf-spike` adds a new `probe-dmabuf` command; existing default and
capture-spike builds keep their previous dependency requirements and commands.
The new command uses the same output discovery, scale/rotation policy, signals,
nonblocking event loop and ext metadata readiness as the SHM comparison. GBM and
libdrm are linked only with the new feature; no new Rust dependency was added.

The ext session's buffer size, DRM device and explicit XRGB8888/ARGB8888 modifier
offers must complete before allocation. Invalid/missing device IDs, incomplete
constraints, odd/oversized geometry, implicit-only layouts and malformed or
excessive modifier arrays fail explicitly. Modifiers must also be advertised
by linux-dmabuf v3's import interface. GBM allocates only from this intersection;
the exported format/modifier/dimensions and one-to-four plane metadata are
checked before submitting the borrowed CLOEXEC fds for asynchronous import.
An import failure is an error, with no SHM or software fallback.

libdrm resolves the advertised DRM device to its render node, handling primary
versus render node identity. A user-selected --render-device must be a render
node on the same physical DRM device, checked with drmDevicesEqual. The default
probe device follows the compositor rather than assuming renderD128. A private
C bridge owns GBM/BO/fd resources; Rust owners retain device-before-buffer and
buffer-before-import lifetimes. Protocol frame objects are destroyed before
another capture request. A single imported buffer is reused only after Ready;
each request damages it in full because this experiment does not track damage.

The client never maps/reads GPU pixels, creates an encoder, opens audio sources
or saves a video. A new 0700 directory contains only report.json. Successful
constraint negotiation/import precedes the countdown; acquisition wall time
starts afterwards. CPU totals include setup, countdown, acquisition and cleanup;
RSS is the process peak and excludes unobservable GPU allocations. Report fields
include source timestamps, measured acquisition rate, negotiated capture/import
offers, selected node, hexadecimal DRM format/modifier and per-plane strides
and offsets. source_stride is plane zero's pitch, not evidence of a linear CPU
layout. finalized remains false; completed/error/interrupted express probe
outcome. No-frame, source removal, constraint changes, frame/import failures and
backwards timestamps are errors. Static ext sources may wait until the overall
duration expires. Native GBM/driver calls still have no watchdog and may delay
cancellation. Buffer delivery alone cannot validate pixel quality, cursor
appearance, GPU conversion/encoding or an end-to-end zero-copy recording.

Run from the graphical session with continuous motion:

```sh
cargo run --locked --features dmabuf-spike -- probe-dmabuf --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-dmabuf-01
```

The native bridge and DMA-BUF feature build pass. All 39 DMA-BUF-feature Rust
tests pass, including malformed constraints, explicit modifier intersection,
reordered versus changed constraints, absent-device failure with no fallback,
and CLI feature/session failures without creating files. Feature Clippy with
warnings denied passes. An initial test build exhausted the agent's temporary
output quota; only its disposable Cargo build artifacts were removed, and the
tests passed with incremental compilation disabled. User captures/checkpoints
were retained. Live GPU import/capture is unavailable to the agent; the subsequent
user-session result follows below. Regression tests also pass with capture-spike
alone (34) and default
features (16), plus formatting/whitespace and strict native C syntax checks.
Phase 2 stays open; no architecture winner or 60-FPS claim is made.

Only Hyprrecorder repository copies changed. This file checkpoint restores the
source/doc changes, not the user's Mesa package transaction:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T204956Z-gghcqq3o --apply
```

### First live DMA-BUF delivery and client pacing experiment

`/tmp/hyprrecorder-source-dmabuf-01/report.json` records 398 frames at **39.800 FPS**
over 10.0030 seconds, completed without error/interruption. The selected compositor
device resolved to /dev/dri/renderD128. GBM allocated/imported XRGB8888
(0x34325258), modifier 0x0100000000000008, with three exported planes; strides
7680/960/64 and offsets 0/8355840/8388608. One buffer was reused. No client CPU
pixel mapping, encoder/audio or media file was involved. Setup took 0.04744 s,
CPU user/system totals were 0.356113/0.114895 s, peak RSS 79,608 KiB. These CPU
totals include setup and cleanup; GPU/system costs remain unmeasured.

Capture wait mean/median/p95/max were 23.827/17.018/43.242/68.885 ms. Acquisition
interval median was 21.730 ms; presentation interval median 17.890 ms, with a
mix of roughly one- and two-refresh intervals. 210 acquisition iterations
exceeded the nominal 60-FPS budget; this counter is not a dropped-frame count.
The result establishes buffer delivery through the owned DMA-BUF path. It does
not establish image/cursor correctness, GPU encoding or sustained 60 FPS. Its
higher average than earlier SHM samples is not a controlled transport comparison:
the Mesa runtime and manually scrolled content differ.

The client currently caps requests by sleeping for the remainder of 1/requested
FPS after acquisition. An explicit `--unpaced` option now permits isolating that
sleep as a contributor to request timing, without assuming it causes the
remaining limit. Default capped behavior is unchanged. Unpaced acquisition sends
the next request after Ready and still has only one protocol frame outstanding;
no busy polling, frame duplication or additional buffer allocation is introduced.
The recording duration, STOP handling, source failures and ext damage waits are
unchanged. `requested_fps` remains a reference for the budget counter; the new
`pacing` and nullable `frame_rate_limit` fields identify whether a cap is active.
The console explicitly labels the mode. This option applies only to the source
probe and does not change recording or SHM pacing.

```sh
cargo run --locked --features dmabuf-spike -- probe-dmabuf --unpaced --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-dmabuf-unpaced-01
```

Use continuous motion with comparable content; its natural update rate may still
limit delivery. All 40 DMA-BUF-feature Rust tests and Clippy with warnings denied
pass, including explicit unpaced selection and rejection of repeated flags
without consuming flag-shaped option values. Formatting and whitespace checks
pass. The subsequent live unpaced result follows below. Phase 2 stays open for
GPU encoder integration, pixel/visual acceptance, portal capture and controlled
repeatable measurements. Only repository files changed. Checkpoint and rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T205739Z-ygxhpjqi --apply
```

### First live unpaced result and repeatable source fixture

`/tmp/hyprrecorder-source-dmabuf-unpaced-01/report.json` records 478 frames at
**47.897 FPS** over 10.0114 seconds, completed without interruption/error.
`pacing: unpaced` and `frame_rate_limit: null` confirm no client FPS-cap sleep.
The same renderD128 device, XRGB8888 format, 0x0100000000000008 modifier and
three-plane layout were imported and reused. Acquisition wait mean/median/p95/max
were 20.926/17.449/35.604/62.935 ms. Setup was 0.05936 s, CPU user/system totals
0.439084/0.099743 s, process peak RSS 79,556 KiB; media bytes zero, encoder none.
Two-second acquisition windows contained 87/108/88/94/101 frames. The run did
not demonstrate sustained 60 FPS. The increased average over the preceding
capped run does not isolate pacing as the cause because scrolling was manual
and content timing was uncontrolled. No additional pacing/transport change is
justified solely by these two samples.

`tools/capture-workload.html` is a local dependency-free fixture: a continuously
moving repeating pattern, scrolling synthetic text, pause/full-screen controls
and a one-second browser callback-rate/largest-interval readout. Animation uses
fixed speeds rather than scroll input; no network, recorder API or personal
content is involved. Hidden/paused periods reset the measurement, and full-screen
changes discard the previous sample. requestAnimationFrame rates describe browser
callback opportunities, not proof of compositor presentation or recorder FPS.
The fixture is intended to make repeated workloads more comparable, not to prove
60-Hz output on its own.

Open it from the repository, click Full screen on the selected monitor, then
start the probe and switch back during the countdown:

```sh
xdg-open "$PWD/tools/capture-workload.html"
cargo run --locked --features dmabuf-spike -- probe-dmabuf --unpaced --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-dmabuf-motion-01
```

Record the visible browser rate during capture and keep browser/window/display,
Mesa, source and power conditions comparable. HTML structure and JavaScript syntax
checks pass. Synthetic callback replay verifies the 60-Hz rate calculation,
pause/resume and visibility resets; it is not a browser rendering test.
The subsequent user-session fixture evidence follows below. Existing recorder,
SHM, DMA-BUF, audio and native code are unchanged. Only the fixture and repository
documentation changed. Phase 2 remains open. Checkpoint and rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T211320Z-6id8z6ly --apply
```

### Controlled fixture result and explicit linear-layout comparison

`/tmp/hyprrecorder-source-dmabuf-motion-01/report.json` records 327 frames at
**32.750 FPS** over 10.0272 seconds, completed without interruption/error and
without a client FPS cap. The device, format, modifier and three-plane layout
match the earlier DMA-BUF samples. Capture wait mean/median/p95/max were
30.486/19.132/68.365/71.161 ms. Two-second acquisition windows contained
102/89/35/37/64 frames, so this is not a uniform 30-FPS delivery limit.

The user supplied a fixture screenshot reading **60.0 FPS browser callbacks**
and **16.7 ms largest interval**, with motion active. This is a browser callback
sample, not a trace of all compositor presentations during the ten-second run.
Do not dismiss the animation as an idle or simply 30-FPS source; the remaining
gap needs capture/compositor/layout investigation. Neither CPU encoding nor the
probe's explicit FPS sleep is present in this path. It still does not establish
which compositor/driver operation contributes the delay.

The selected modifier is 0x0100000000000008 with three planes, identified by the
installed drm_fourcc.h as I915_FORMAT_MOD_Y_TILED_GEN12_RC_CCS_CC. Both capture
and import also advertise DRM_FORMAT_MOD_LINEAR (zero). A new explicit
`--linear` flag restricts the existing intersection to modifier zero before GBM
allocation. The C allocator already checks that the exported modifier belongs
to the supplied list; it now receives only zero in this mode. Unsupported linear
negotiation, allocation or import fails without choosing another layout or SHM.
The default remains the existing negotiated driver choice. This tests a layout
variable, without assuming compression is faulty or linear capture is faster.

`requested_modifier_hex` records explicit intent separately from the existing
allocated modifier/plane metadata; console output confirms a linear import and
its plane count. The flag composes with --unpaced and retains existing timeout,
source failure, resource ownership and no-pixel-read behavior. It is limited to
the source probe; no native, recording, SHM, package or desktop change was made.

Use the same visible full-screen animation, with motion running:

```sh
cd ~/repos/hyprrecorder
cargo run --locked --features dmabuf-spike -- probe-dmabuf --unpaced --linear --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-dmabuf-linear-01
```

All 42 DMA-BUF-feature Rust tests and Clippy with warnings denied pass. New
checks require linear support on both sides, reject substitution when only a
tiled layout is available, and verify explicit/default/duplicate flag handling.
Formatting and whitespace checks pass. Live linear allocation/acquisition and
performance require live evidence; the subsequent results follow below. Keep
comparable conditions and retain both probe and browser rates. Phase 2 remains
open for live measurement, encoder interoperability and image correctness.
Only repository source/docs changed. Checkpoint and rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T214524Z-ya26n6el --apply
```

### Linear delivery results and browser history retention

The first linear run `/tmp/hyprrecorder-source-dmabuf-linear-01` imported modifier
zero with one plane and acquired 368 frames at 36.799 FPS without interruption or
error. The user subsequently explained that the animation had not opened for
that run. Do not use it as an animation-layout benchmark. The earlier repeated
motion-01 directory error refused to overwrite the prior report as intended.

After the user confirmed opening the animation, the run
`/tmp/hyprrecorder-source-dmabuf-linear-motion-02/report.json` acquired 361 frames
at **36.146 FPS** over 10.0072 seconds, without interruption/error or an FPS cap.
It retained the same linear modifier/device. Capture mean/median/p95/max were
27.646/17.293/68.635/73.126 ms. This establishes linear buffer delivery but neither
sustained 60 FPS nor a causal comparison with the three-plane layout.

The user reported the fixture's browser rate falling below 40, below 30 and
sometimes below 10, then clarified that the stable 60 FPS observation was after
the probe ended. The exact timing of each dip versus capture is not retained in
those reports. The screenshots reading 60 FPS are point samples and cannot stand
in for continuous 60-Hz content during the whole probe. Source rendering and
capture/compositor interaction remain unresolved; do not assume one is faulty.
GPU encoder integration remains planned, not implemented by this continuation.

Instead of changing the animation scene or desktop settings, the fixture now
keeps its last 20 timestamped completed FPS samples and cumulative lowest sampled
rate/largest callback interval until explicit Reset measurements or reload.
Visible sampling windows target at least one second. Paused, hidden or
full-screen transitions reset the partial window, without inventing 0-FPS
samples or erasing retained history. Time labels refer to elapsed time from the
first active callback after reset and therefore preserve gaps for hidden periods.
This lets the user inspect a slowdown after the current live counter recovers.
There is no automatic marker connecting browser samples to a recorder session;
window switches and other system work can also affect the minimum.

Reload the fixture, enter full screen, click Reset measurements, then run from
the repository and switch back during the countdown:

```sh
cd ~/repos/hyprrecorder
cargo run --locked --features dmabuf-spike -- probe-dmabuf --unpaced --linear --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-dmabuf-linear-history-01
```

JavaScript syntax and synthetic callback playback checks pass: 60-to-5-to-60 FPS
retains the dip, hidden/paused periods preserve samples, the recent list stays
bounded at 20 and explicit reset clears it. Whitespace checks pass. These checks
do not verify browser rendering or establish the cause of any live slowdown.
The scene, pause/full-screen features, all capture/native/encoding code and
desktop/package state are unchanged. Only the fixture and repository docs
changed. Live history behavior remains unverified by the agent. Checkpoint:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T220134Z-w3l_4zls --apply
```

### Visible-workspace source result and GPU recording integration

The user identified the browser on workspace 2 and terminal on workspace 1.
Earlier pauses can include the browser being hidden while capture sees the
terminal workspace; those runs cannot establish steady animation delivery or
rank layouts. With the animation workspace kept visible,
`/tmp/hyprrecorder-source-dmabuf-linear-history-02/report.json` records
**595 frames at 59.648 FPS** over 10.0048 s, completed without interruption/error.
Mean/median/p95/max acquisition waits were 16.784/16.653/18.333/35.044 ms. It
retained linear XRGB8888, one plane, stride 7680, renderD128. Setup 0.02353 s,
CPU user/system 0.505509/0.149511 s, peak RSS 79,944 KiB. CPU includes setup and
cleanup; GPU allocations/system cost remain unmeasured. This validates that
source run near the 60.007-Hz display refresh, not sustained recording or pixels.

`gpu-encode-spike` now adds `spike-gpu`, reusing the owned ext/GBM loop and file
publication rules. Existing default/capture-spike/dmabuf-spike requirements are
preserved; libavfilter is linked only with this new feature. No new Rust
dependency or package install is introduced. Recording requires explicit linear
XRGB8888/ARGB8888 with one plane; the writer duplicates the DMA fd and checks
pitch, dimensions, offset and object size before direct DRM-to-VAAPI import.
It uses scale_vaapi for RGB-to-NV12 conversion and h264_vaapi for encoding,
12 Mbps, GOP 2×FPS, no B-frames, no audio, with no software/CPU pixel fallback.
This is an SDR 8-bit matrix conversion experiment, not HDR color management.

The initial loop serializes acquisition, conversion and encoding. GPU conversion
must finish reading the captured RGB storage before another capture reuses it.
It explicitly requests a readable direct DRM export of the converted VAAPI frame
before encoding. FFmpeg 8.1.3's PRIME2 export path synchronizes the VPP surface
for READ; legacy exports with unknown modifiers are rejected. Exported fds are
released immediately after this completion step; encoding retains its separate
NV12 output frames. No pixel mapping/download or CPU upload is performed. The
implementation rationale was checked against the pinned
[FFmpeg 8.1.3 VAAPI source](https://raw.githubusercontent.com/FFmpeg/FFmpeg/n8.1.3/libavutil/hwcontext_vaapi.c).
The subsequent live video result follows below; synchronization robustness
beyond that short sample remains unverified.

Rust enforces CPU/GPU writer modes and finalization state. The common native
writer retains existing checked file I/O, muxing, timestamp and flush handling;
the CPU fixture and recording path continue to allocate their original swscale
buffers. GPU writer initialization never allocates CPU pixel buffers. Capture,
conversion or encode failure leaves a partial file and error report; successful
flush/fsync precedes no-overwrite publication. Interruptions after frames may
finalize and remain labeled interrupted; no-frame runs fail. Driver calls have
no watchdog. Trailing idle duration is not extended with duplicate frames.

The report labels recording mode, ext-dmabuf-vaapi and serialized GPU pipeline;
encode_ms includes conversion, completion synchronization, encode and packet
writes. Frames count successful submissions; decoded-frame agreement and image
quality must be verified from the resulting video. The source-only commands
retain their no-video behavior. Production record/stop/UI/audio remain absent.

```sh
cd ~/repos/hyprrecorder
cargo run --locked --features gpu-encode-spike -- spike-gpu --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-gpu-video-01
```

Switch to the animation workspace during the countdown, keep it visible for the
duration, then inspect capture.mkv for image orientation/colors, text, motion
and cursor. Source-only near 60 FPS does not establish the recording rate or
select this architecture. Phase 2 remains open for live video acceptance,
repeatable comparisons, overlap and portal/crop/scale work.

All 45 GPU-feature Rust tests and all-targets Clippy with warnings denied pass.
The new checks reject CPU/GPU mode confusion, invalid GPU layouts and absent
hardware without fallback, plus feature/session failures before file creation.
Existing CPU codec decode/PTS/color tests, no-overwrite destination checks and
write/finalization failure checks pass. Regression tests pass with dmabuf-spike
(43), capture-spike (35), and default features (17). Both native build variants
pass strict C syntax checks; formatting/whitespace checks pass. These tests
cannot establish live GPU import/conversion, synchronization or image quality.

Only Hyprrecorder source/docs changed. This checkpoint preserves prior dirty
work and does not undo the user's Mesa
transaction. Rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T221024Z-bmfob0xo --apply
```

### First finalized GPU video and visual acceptance

`/tmp/hyprrecorder-gpu-video-01/report.json` records 555 frames at **55.528 FPS**
over 10.0185 seconds, finalized/completed without interruption/error. It used
linear single-plane XRGB8888 on renderD128, direct VA-API import, GPU NV12
conversion and h264_vaapi. Setup took 0.05887 seconds; the file is 14,508,944 bytes.
CPU user/system totals were 1.397603/0.452396 seconds, peak RSS 122,432 KiB.
These include setup/cleanup and exclude unmeasured GPU/system costs. Audio is
false, CPU pixel access false, buffer reused true, requested FPS cap 60.

Capture wait mean/median/p95/max were 13.287/12.092/26.922/29.316 ms. Combined
conversion/synchronization/encode/write times were 2.872/2.653/3.935/21.570 ms.
109 iterations exceeded the nominal frame budget; this is not a source dropped
frame count. The prototype still serializes capture and encoding.

ffprobe counted all **555 decoded H.264 High-profile 1920x1080 progressive
8-bit frames**, matching the report. The file contains one video stream and no
audio. A full software decode with FFmpeg completed with exit 0 and no errors.
All decoded PTS increase strictly from 0 to 9.977 seconds; their measured rate
is 55.5277 FPS, matching acquisition. Nominal container rate fields (including
an inferred avg_frame_rate of 62.5) are not the actual observed capture rate.
The user subsequently accepted motion, text, colors, orientation and cursor.
This acceptance applies to this video only and does not cover crop/scale, HDR,
other devices or long-duration synchronization/quality.

The next sample is a full minute on the same visible animation workspace.
The existing --unpaced option is also accepted by spike-gpu; removing the cap
tests pacing under recording load, without assuming it improves throughput:

```sh
cd ~/repos/hyprrecorder
cargo run --locked --features gpu-encode-spike -- spike-gpu --unpaced --monitor eDP-1 --fps 60 --seconds 60 --directory /tmp/hyprrecorder-gpu-video-60s-01
```

Keep workspace 2 visible throughout, return only after the duration, and retain
actual rate, decoded-frame agreement and visual evidence. A single short run
does not complete Phase 2, establish sustained 60 FPS, rank backends or replace
the required repeated 1080p30/60 samples and portal/crop/scale investigation.
No encoder/capture/native code was changed for this result; only repository
documentation changed. The captured file was not copied into the checkout.
Checkpoint and rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T222234Z-bu7rw95b --apply
```

### First full-minute GPU sample

`/tmp/hyprrecorder-gpu-video-60s-01/report.json` records **3,582 frames at
59.719 FPS** over 60.0131 seconds. The file finalized/completed without capture
error or interruption; the client cap was disabled. The pipeline remains the
same serialized linear DMA-BUF → VA-API import → GPU NV12 conversion → H.264
path. The video-only file is 91,433,236 bytes, setup 0.04038 seconds, CPU
user/system totals 8.483486/2.399812 seconds, process peak RSS 122,456 KiB.
CPU includes setup/cleanup; unmeasured GPU/system costs are separate.

Capture wait mean/median/p95/max were 14.015/14.053/15.634/29.045 ms. Combined
conversion/completion synchronization/encode/write times were
2.722/2.535/3.883/17.455 ms. 1,792 iterations exceeded the nominal 16.667-ms
budget; this measures elapsed iterations, not dropped or corrupt frames.

ffprobe decoded all 3,582 frames, matching the report. Frame PTS increase
strictly from 0 to 59.964 seconds; rate from their span is 59.7192 FPS. All
3,582 packet PTS and DTS also increase strictly and match each other. A full
software decode with `-xerror -fps_mode passthrough -enc_time_base demux -f null -`
completed with exit 0 and no errors. An initial default null-output check warned
of a duplicate output DTS after timestamp conversion; the source packet/frame
checks and demux-time-base decode establish that this was not a duplicate source
timestamp or a decoding error. Preserve the source time base in future checks.
No native change is justified by that verification-output warning.

The user accepted the earlier ten-second video's visuals; visual acceptance of
this full minute is pending separately. This run supports a near-refresh-rate
minute on this device/workload; it does not by itself establish repeatability,
other devices, 1080p30 behavior or architecture selection. Two further matching
60-second samples are needed for the plan's three-run 1080p60 check. Run the
same fixture on workspace 2, with the same conditions and no interruption:

```sh
cd ~/repos/hyprrecorder
for sample in 02 03; do
  cargo run --locked --features gpu-encode-spike -- spike-gpu \
    --unpaced --monitor eDP-1 --fps 60 --seconds 60 \
    --directory "/tmp/hyprrecorder-gpu-video-60s-$sample" || break
done
```

Switch to the animation during the first countdown and keep that workspace
visible through both runs (about two minutes plus countdowns). Return afterwards
for reports. Three 1080p30 samples and other Phase 2 comparison/crop/scale/portal
work remain; audio is not implemented. No source/native/runtime files changed
for this evidence update. Only repository docs changed; captures were not copied
into the checkout. Checkpoint and rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T223925Z-cb3nujud --apply
```

### Three 1080p60 GPU minute samples

The two subsequent unpaced runs completed the three-sample GPU 1080p60 set:

| Sample | Report/decoded frames | Acquisition FPS | CPU user+system seconds | Peak RSS MiB | File MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| gpu-video-60s-01 | 3,582 / 3,582 | 59.719 | 10.883 | 119.59 | 87.20 |
| gpu-video-60s-02 | 3,542 / 3,542 | 59.052 | 10.765 | 119.79 | 86.17 |
| gpu-video-60s-03 | 3,586 / 3,586 | 59.785 | 10.718 | 119.63 | 87.30 |

All reports are completed/finalized with error null and interrupted false.
Arithmetic mean acquisition rate is **59.519 FPS**, range 59.052–59.785.
Wall times are 60.013/60.019/60.012 seconds. Full software decoding with source
time base preserved succeeds without errors for each file; decoded frame counts
match their reports. Samples 02/03 have strictly increasing decoded PTS from 0
to 59.964/59.965 seconds, respectively. The first sample's frame and packet
timestamp checks are recorded above. These are successful technical runs; visual
review of all three full minutes remains pending. Earlier ten-second acceptance
does not substitute for that review.

Samples 02/03 capture wait mean/p95/max were 14.205/15.727/65.869 and
14.045/15.640/36.233 ms. Conversion/synchronization/encode/write mean/p95/max
were 2.723/3.834/14.961 and 2.673/3.809/14.177 ms. The second run has a lower
average rate, but these reports do not isolate a cause. Process peaks do not
include GPU/system memory or prove long-duration memory stability. CPU totals
include initialization and cleanup and are not whole-system utilization.

The next required mode is paced 1080p30, three 60-second runs. Omit --unpaced so
the requested 30 FPS cap is enforced; otherwise it remains a reference rate and
would not provide a 30 FPS sample:

```sh
cd ~/repos/hyprrecorder
for sample in 01 02 03; do
  cargo run --locked --features gpu-encode-spike -- spike-gpu \
    --monitor eDP-1 --fps 30 --seconds 60 \
    --directory "/tmp/hyprrecorder-gpu-video-30fps-$sample" || break
done
```

Switch to workspace 2 during the first countdown and keep the animation visible
for all three runs (about 3 minutes 15 seconds). Keep other workload/display/browser
conditions comparable. This dataset does not complete Phase 2: full-minute visual
review, 30 FPS measurements and comparison/crop/scale/portal work remain. No
capture/encoder/native/runtime changes were made; only repository docs changed.
Checkpoint and rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T225007Z-9ro7frl6 --apply
```

### Three paced 1080p30 GPU minute samples

The completed samples 01, 02 and replacement 04 used the requested 30 FPS cap
without `--unpaced`, at 1920×1080 with the same GPU pipeline.

| Sample | Report/decoded frames | Acquisition FPS | CPU user+system seconds | Peak RSS MiB | File MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| 30fps-01 | 1,790 / 1,790 | 29.838 | 6.408 | 119.75 | 86.81 |
| 30fps-02 | 1,789 / 1,789 | 29.820 | 6.381 | 120.05 | 86.82 |
| 30fps-04 | 1,790 / 1,790 | 29.834 | 6.450 | 119.51 | 86.85 |

All three reports are completed/finalized, without interruption or capture error.
Full software decoding with the demux time base preserved passes without errors.
Decoded counts match reports; decoded PTS increase strictly from zero to
59.957/59.960/59.965 seconds. The user subsequently confirmed that all six
completed minute-long recordings look correct, accepting motion, text, colors
and cursor appearance for both sample sets. This supersedes the pending visual
review status above. CPU/RSS scope and workload caveats still apply.

Sample 03 failed with disk quota exceeded, leaving a partial MKV and an empty
report. It is excluded from successful measurements. Removing the disposable
agent build cache freed space; the user then captured replacement sample 04 in
a new directory. No recordings were overwritten or deleted. This confirms a
real storage failure can prevent even the diagnostic report from being written;
production disk-error reporting still needs coverage in Phase 3.

The three-sample technical sets at 1080p30 and 1080p60 are now complete. Phase 2
remains open for matched comparisons, crop/scale/cursor controls and source
cancellation coverage, portal feasibility, and the backend decision.
Only repository documentation changed in this continuation. Restore this update:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T233735Z-csyzl6fs --apply
```

## Temporary external comparison capture harness

Python 3 standard library only. It invokes installed `wl-screenrec` directly,
without a shell, with an explicit `h264_vaapi` encoder and MKV muxer. Audio is
off; no microphone is opened. The output monitor must be supplied explicitly.
Flags follow the upstream main branch inspected on 2026-10-21; check your
installed version's `--help` before a run. An incompatible version fails rather
than substituting a software encoder.

From a graphical terminal, first inspect a dry run (creates nothing):

```sh
python3 tools/capture_spike.py --monitor eDP-1 --directory /tmp/hyprrecorder-sample-01
```

Replace the monitor name with an actual `hyprctl -j monitors` output. To record
that monitor's current native size for 60 seconds:

```sh
python3 tools/capture_spike.py --monitor eDP-1 --fps 30 --seconds 60 --directory /tmp/hyprrecorder-sample-01 --run
```

`--run` captures visible desktop content; prepare a nonprivate workload first.
Each directory must be new; its parent must exist. The private directory contains
`capture.mkv`, capped `recorder.log` and `report.json`. Ctrl+C/SIGTERM requests
graceful finalization; a ten-second timeout kills/reaps the recorder and marks
failure. Interrupted/short/nonzero samples remain failures. Files are retained
for inspection, never automatically deleted or reported as successful recordings.
The harness has no GUI to hide; hide the terminal yourself during visual tests.

Metrics are recorder-only CPU seconds/peak RSS sampled every 100 ms, excluding
helpers/GPU/system power and possibly the last CPU interval. Elapsed time includes
startup/finalization. FFprobe counts decoded frames and reports stream/container
metadata. This does not prove hardware use, correct cursor, undamaged first/last
frames, legibility or motion. `verified_capture` stays false pending acceptance.
Low FPS on static content is not by itself proof of dropped frames.

The harness intentionally has no scaling/region/audio options: those paths need
negotiated semantics and live verification. Native captures alone cannot satisfy
the required 1080p comparison on a differently sized desktop.

## Comparison and architecture gate still open

### Native Rust protocol probe continuation

The follow-up implements `hyprrecorder probe-wayland --json` using
`wayland-client` 0.31.15's Rust backend. It builds offline from cached crates
without installing Wayland/multimedia development packages. This is app-owned
source discovery preparation, not a capture-to-encoder prototype or selection
of the final recording backend.

It connects through the normal environment-specified Wayland display, reads the
registry once, reports interface names/versions and handles global removal. It
does not bind capture interfaces or acquire video/audio buffers. A separate
worker is killed/reaped after three seconds if the compositor stalls. An
inaccessible display yields an unverified JSON report, with no desktop diagnosis.

The output distinguishes wlr-screencopy, the pair of ext image-copy/output-source
managers, and DMA-BUF advertisement. None establishes buffer format/modifier
negotiation, cursor support, permission or hardware encode. Run from a graphical
terminal before the live prototype:

```sh
CARGO_TARGET_DIR=/tmp/hyprrecorder-target cargo run --offline --locked -- probe-wayland --json
```

The actual check in this agent returned `session_reachable: false` and
`capture_verified: false`; acquisition remains untested. Fifteen Rust tests and
five Python harness tests pass, with formatting and Clippy clean. New coverage
checks partial ext protocol advertisement and the inaccessible-session CLI; the
existing process timeout test covers the shared bounded subprocess mechanism.
An additional socket-pair simulation of a stalled compositor could not reach
the timeout: this sandbox rejected Wayland socket I/O with `Operation not
permitted`. The report correctly remained unverified, but this does not validate
the stalled-Wayland path. The minimum Rust toolchain and live registry success
path remain untested.

Follow-up checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261021T134055Z-cetd2c2k`.
Rollback this continuation before the original Phase 2 checkpoint when undoing
both; the new checkpoint preserves the existing uncommitted spike work:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T134055Z-cetd2c2k --apply
```

Only the Hyprrecorder checkout changed. No desktop reload is required.

### Remaining live gate

1. Use the installed build prerequisites and an accessible user GPU/session.
   Check exact tool versions and supported protocol/encoder options; keep any
   further package changes recoverable.
2. Exercise real direct Wayland capture, cursor on/off, format negotiation,
   cancellation, crop/scale and native size; inspect actual video and encode logs.
3. Prototype portal CreateSession → SelectSources → Start → OpenPipeWireRemote,
   including request cancellation and FD/session cleanup. A PipeWire node alone
   is not sufficient authorization: retain the portal-provided remote FD.
4. Warm up; measure three 60-second samples each at 1080p30, 1080p60 and native
   size with the same scrolling-text/motion workload and comparable quality for
   GPU Screen Recorder, wl-screenrec and the embedded candidate. Record source
   dimensions/transform/scale, codec settings, frame timestamps/drops and visual
   acceptance. Collect helper/system/GPU cost separately; record AC/battery,
   brightness/workload and reliable power measurements only when available.
5. Add comparable audio-on samples once those paths exist. Choose an embedded
   Rust pipeline only after the measurements support it. No silent software
   fallback; reject unavailable hardware paths with actionable errors.

Direct Wayland is a candidate for the custom region UX and DMA-BUF path. Portal
capture provides a compositor-mediated picker/authorization flow but requires
session management and may add selection UI. GStreamer's VA encoder can accept
system-memory frames as well as VA surfaces, so its presence does not establish
zero-copy. Negotiated buffers and GPU crop/scale remain unverified for both paths.

No third-party code has been copied. Before borrowing wl-screenrec code, review
the exact revision's LICENSE and dependency licenses (not resolved here). An
FFmpeg build's configure flags/providers affect licensing; Rust bindings do not
remove that obligation. GStreamer core/plugin licensing and redistributed
dependencies also need an exact inventory. Complexity, portability and fallback
policy remain decision inputs, with no assumed winner or performance claims.

## Sources

- [wl-screenrec source and CLI options](https://github.com/rosalyntg/wl-screenrec/blob/main/src/main.rs)
- [wl-screenrec architecture, protocols and build dependencies](https://github.com/rosalyntg/wl-screenrec)
- [Portal ScreenCast contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [GStreamer VA H.264 encoder and memory formats](https://gstreamer.freedesktop.org/documentation/va/vah264enc.html)
- [Fedora GStreamer development package](https://packages.fedoraproject.org/pkgs/gstreamer1/gstreamer1-devel/)
- [Arch FFmpeg package and dependencies](https://archlinux.org/packages/extra/x86_64/ffmpeg/)
- [DNF5 transaction history and undo](https://dnf5.readthedocs.io/en/latest/commands/history.8.html)
- [Wayland Rust connection API](https://docs.rs/wayland-client/0.31.15/wayland_client/struct.Connection.html)
- [wlr-screencopy protocol](https://wayland.app/protocols/wlr-screencopy-unstable-v1)
- [FFmpeg library muxing example/API usage](https://github.com/FFmpeg/FFmpeg/blob/n8.0/doc/examples/mux.c)

## Validation and recovery

- `python3 -B -m unittest discover -s tests -p 'test_*.py' -v`: five tests pass,
  covering no-write dry runs, destination preservation, prerequisite failures,
  early recorder failure and kill/reap escalation.
- `cargo fmt --check`: pass.
- `CARGO_TARGET_DIR=/tmp/hyprrecorder-target cargo test --offline --locked`:
  all 12 existing Rust tests pass.
- `CARGO_TARGET_DIR=/tmp/hyprrecorder-target cargo clippy --offline --locked --all-targets -- -D warnings`: pass.
- Actual dry run emits structured arguments and prerequisite failures; `--run`
  exits 2 before creating its destination because wl-screenrec/GPU access is absent.
- Scope review and `git diff --check`: pass. Only this checkout changed.

These were the initial external-harness checks. The later owned-capture samples,
visual acceptance, exploratory timings and outstanding live checks are recorded
above; no completed controlled comparison exists.

Checkpoint: `/home/ramlej/.local/state/hyprshell/agent-backups/20261021T131910Z-zdfcxni_`.
Recovery (no desktop reload needed):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261021T131910Z-zdfcxni_ --apply
```

This phase stays uncommitted until its live exit criteria are satisfied.

## GPU crop/scale continuation — 2026-10-22

`spike-gpu` now accepts optional `--crop X,Y,W,H` and `--size WIDTHxHEIGHT`.
Coordinates are physical pixels in the selected unrotated scale-1 monitor.
The full RGB DMA-BUF stays imported at source dimensions. Its mapped VA frame
carries crop margins; `scale_vaapi` receives explicit output dimensions and
square-pixel SAR, crops/converts/scales through GPU VPP, then feeds H.264.
Synchronous conversion completion before reuse remains unchanged. No third-party
implementation was copied, CPU fallback added, or software performance claimed.

The implementation follows FFmpeg's documented source behavior:
[VA-API VPP surface-region handling](https://github.com/FFmpeg/FFmpeg/blob/n8.0/libavfilter/vaapi_vpp.c)
and [scale_vaapi output geometry](https://github.com/FFmpeg/FFmpeg/blob/n8.0/libavfilter/vf_scale_vaapi.c).
The local filter exposes explicit width/height and `reset_sar` options.

The experimental policy rejects odd dimensions, empty/out-of-bounds crops,
overflow and output aspect ratios different from the crop. It does not silently
stretch, clamp, or add padding. Defaults preserve full-source/native output.
Upscaling is labeled in the countdown; `report.json` adds resolved `geometry`.
`--no-cursor` retains the existing compositor capture-session exclusion request.
The README contains the two live checks for crop+downscale+cursor-off and
full-source downscale+cursor-on. The user ran the first sample in the graphical
session; both short crop/scale and cursor-mode samples are now accepted. Phase 2 and its
backend decision gate remain open and uncommitted.

Offline validation for this continuation: 42 unit + 7 CLI tests with all
features, 11 unit + 6 CLI tests without features, 37 unit + 6 CLI tests with
`dmabuf-spike`, and five Python harness tests pass. Formatting, default/all-feature
Clippy with warnings denied, and diff checks pass. Tests cover geometry bounds,
overflow, aspect ratios, invalid dimensions, parser composition and failure
before session access/output creation, plus existing codec/finalization checks.
A fresh session probe reports `session_reachable: false`; it acquired no buffers.
These checks do not exercise GPU VPP output or prove cursor exclusion.

Checkpoint (preserves prior dirty Phase 2 source and docs):
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T001027Z-842kmojx`.
Recovery, without a desktop reload:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T001027Z-842kmojx --apply
```

### Accepted crop/downscale/cursor-off sample

User-session evidence: `/tmp/hyprrecorder-gpu-crop-01/report.json` and
`capture.mkv`. The requested physical crop was (320,180), 1280×720 inside a
1920×1080 source, scaled to 640×360 with cursor exclusion. The run completed
and finalized without interruption or report error: 282 submitted and decoded
H.264 frames, 28.276 acquired FPS against the requested 30, 3,106,212 media bytes.
ffprobe confirms 640×360, SAR 1:1 and DAR 16:9. Packet PTS/DTS and decoded PTS
are strictly increasing. Full decode with `-fps_mode passthrough
-enc_time_base demux -f null -` emits no errors. A default null-output decode
rounded timestamps into its output timebase and warned about duplicate DTS;
the file's own packet timestamps remain strictly increasing.
The user explicitly accepted the central crop, correct proportions and absent
cursor. This validates the short sample, not sustained 30 FPS, arbitrary output
geometry, interactive selection or a backend decision.

Documentation checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T005523Z-yphej_zl`.
Rollback only this evidence update (no desktop reload):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T005523Z-yphej_zl --apply
```

### Accepted full-source downscale/cursor-on sample

User-session evidence: `/tmp/hyprrecorder-gpu-scale-01/report.json` and
`capture.mkv`. Full-source (0,0), 1920×1080 capture scaled to 1280×720 with
cursor enabled. Completed/finalized without interruption or report error:
293 submitted and decoded H.264 frames at 29.2917 acquired FPS against 30.
ffprobe confirms 1280×720, SAR 1:1, DAR 16:9. All 293 packet PTS, packet DTS
and decoded PTS increase strictly. Full decode with passthrough and demuxer
timebase exits zero without diagnostics. The user explicitly accepted the
whole-screen image, proportions and visible cursor.

Both short crop/scale and cursor-mode checks passed on this monitor. These
samples do not validate arbitrary-aspect padding, interactive selection,
rotated/fractionally scaled outputs, sustained 30 FPS or a backend decision.
Next work is overlap/performance investigation and portal capture; Phase 2
remains open, with external-recorder installation deferred.

Documentation checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T005756Z-hi_wx53w`.
Rollback this evidence update (no desktop reload):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T005756Z-hi_wx53w --apply
```


## Portal property investigation — 2026-10-22

Added `probe-portal --json` without changing the existing direct capture paths.
Three bounded shell-free busctl property queries report interface reachability,
raw uint32 values, known source/cursor bits and per-property errors. Missing
properties stay null. Unknown bits remain in raw masks without inventing support.
The command never creates a session, shows a picker or acquires buffers; it may
activate the portal service through D-Bus. It requires busctl at runtime only.

The [ScreenCast contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
defines source and cursor masks. Actual capture requires CreateSession,
SelectSources, Start, asynchronous request responses and OpenPipeWireRemote's
Unix FD. A future spike must retain one D-Bus connection throughout that lifecycle,
close requests/sessions on cancellation, and consume PipeWire buffers before
claiming capture. The property probe is a prerequisite, not evidence for choosing
portal capture. The [busctl reference](https://www.freedesktop.org/software/systemd/man/latest/busctl.html)
documents the property query and typed terse output used here.

Installed Hyprland portal metadata advertises ScreenCast. The agent's user-bus
connection is denied with Operation not permitted; no live property or capture
result is available from the agent. The user's graphical-terminal result is
recorded below. Portal acquisition, GPU buffer negotiation,
comparison under matching conditions, and overlap/performance investigation
remain outstanding. No backend selected and Phase 2 remains uncommitted.

Checkpoint preserving all earlier Phase 2 edits:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T010021Z-lywf2q5u`.
Rollback this continuation (no desktop reload):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T010021Z-lywf2q5u --apply
```

Offline checks passed: default 13 unit + 7 CLI tests; all features 44 unit + 8 CLI tests; five Python harness tests; default/all-feature Clippy with warnings denied; formatting and diff whitespace checks. Actual portal probe returned null properties with user-bus permission errors and capture_verified false. No capture performed.


### Live portal property result — 2026-10-22

The user ran `cargo run --offline --locked -- probe-portal --json` in the graphical
terminal. ScreenCast interface reachable: true. All three properties returned
without errors: version 6, AvailableSourceTypes 7 (monitor, window, virtual),
AvailableCursorModes 3 (hidden, embedded). Metadata cursor mode is not advertised.
The report correctly retains capture_verified false. This establishes property
reachability and advertisements only; no session, picker, PipeWire stream,
GPU buffer or encoded output was tested. It does not select a backend.

Next portal step: implement a persistent D-Bus session/request lifecycle and
actual PipeWire buffer acquisition, including cancellation and cleanup. Phase 2
remains open and uncommitted. Only PLAN.md and this evidence document changed;
reviewed against checkpoint baseline and checked with git diff --check.

Evidence-update checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T010300Z-s0mttlan`.
Recovery (no desktop reload):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T010300Z-s0mttlan --apply
```


## Owned portal/PipeWire source spike — 2026-10-22

`portal-spike` adds a separate Rust CLI/report module and a native bridge using
GIO and PipeWire. Existing direct SHM/DMA-BUF/GPU paths and default feature
behavior are preserved. The installed development libraries resolve through
pkg-config (GIO 2.88.4, PipeWire 1.6.9); no packages/configs were changed and no
Rust dependency or lockfile change was needed.

The bridge owns one dedicated D-Bus connection, subscribes before submitting
portal methods, consumes asynchronous Response signals, retains the session,
requests one monitor with embedded/hidden cursor and obtains the restricted
PipeWire remote FD. It prefers the stream serial for targeting when supplied.
Mapped raw RGB SHM buffers are negotiated explicitly, checked for valid bounds,
stride and corruption flags, counted with optional header PTS, then returned.
No pixels are inspected, media encoded/saved or audio opened. DMA-BUF transport
and GPU conversion remain outside this portal spike. The report distinguishes
session start, remote open, valid/rejected delivery, interruption, cleanup,
timestamp counts and errors; image correctness remains false.

Requests time out after 120 seconds, synchronous D-Bus methods after five;
Ctrl+C/SIGTERM are observed during request/countdown/acquisition waits.
A pending request is closed on cancellation/timeout. Cleanup destroys the
PipeWire stream/context, disconnects the core, closes descriptors/session and
the dedicated D-Bus connection. Unconfirmed explicit closes set cleanup_ok false;
connection closure additionally releases connection-owned portal state.
No frames after ten startup seconds, source closure, bus/core/stream errors,
geometry/format changes and failed negotiation cannot masquerade as success.
The requested measurement interval begins at the first valid frame; damage-driven
idle delivery can be sparse. This is source delivery evidence only.

API references used: [ScreenCast contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html),
[PipeWire input stream API example](https://docs.pipewire.org/video-play_8c-example.html),
and installed GIO/PipeWire/SPA headers. This implementation adds no copied
third-party implementation. Live source selection, cancellation, FD transfer,
actual delivery, cleanup and visual checks remain unverified in the agent's
restricted session. Phase 2 remains open and uncommitted; no backend selected.

Next user-session check (keep the fixture moving on the selected monitor):

```sh
cargo run --offline --locked --features portal-spike -- probe-portal-source --json --seconds 10 --fps 60
```

Checkpoint, including previous dirty Phase 2 contents:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T010434Z-ahvzwnft`.
Rollback this continuation, without a desktop reload:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T010434Z-ahvzwnft --apply
```

Offline validation: default 13 unit + 8 CLI tests; portal-spike 14 unit + 8
CLI tests; all features 45 unit + 9 CLI tests; five Python harness tests passed.
Default, portal-feature and all-feature Clippy passed with warnings denied;
formatting and diff whitespace checks passed. The native SHM guard self-test
compiled with -Wall -Wextra -Werror and passed valid-buffer, missing data/chunk,
empty/corrupt, out-of-bounds, short/negative stride and unsupported DMA-BUF tests.
These checks cover argument/failure reporting and buffer guards, not portal
request success or live frame/cursor correctness. Fedora provider ownership was
verified locally; Arch header providers checked via official package file lists
(glib2 and libpipewire), without building on Arch.


### Portal CreateSession token correction — 2026-10-22

The user's first portal source run returned NoReply/Remote peer disconnected
before session start or remote open, with zero frames. Local user-service logs
identify the portal 1.22.1 abort during CreateSession: assertion
`session->token != NULL`. The spike sent an empty CreateSession dictionary,
omitting session_handle_token. This omission triggered the observed portal
assertion; no PipeWire stream or media buffers were acquired.

The native bridge now constructs CreateSession with fresh session_handle_token
and handle_token strings. UUID hyphens are normalized to underscores for valid
object-path elements. SelectSources and Start also receive fresh request tokens.
Method-call errors now identify the failing portal method. The exact constructor
used in production has a native self-test checking the D-Bus tuple/dictionary,
both nonempty string tokens, valid path elements and uniqueness across calls.
Existing native buffer guards still pass. Compilation with -Wall -Wextra -Werror,
14 unit/8 CLI portal-feature tests and portal-feature Clippy pass. Formatting and
diff whitespace checks pass. No actual portal session success is established.

The [installed-version portal implementation](https://github.com/flatpak/xdg-desktop-portal/blob/1.22.1/src/xdp-session.c)
looks up session_handle_token and asserts a non-null session token. The fix
supplies it directly; no desktop configuration, package or service changes were
made. Logs show the portal exited with ABRT; the agent cannot control or query
its current state via the restricted user bus. From the graphical terminal,
start the portal service then rerun the README probe. Verify successful live
selection, delivery and cleanup before continuing performance comparisons.

Correction checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T011135Z-ebonpqwd`.
Rollback this patch (this reinstates the broken token omission):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T011135Z-ebonpqwd --apply
```


### First portal delivery and teardown reporting correction — 2026-10-22

The user started the portal service successfully and ran the token-corrected
10-second, 60-FPS source probe. Session start and restricted remote open both
succeeded. The report contains 226 valid 1920x1080 buffers, zero rejections,
SPA format 12/memory type 2, 226 PTS samples and no nonincreasing timestamps.
Elapsed 10.004109 seconds, delivery 22.4907585 FPS. The supplied first/last PTS
are separated by about 10.015643 seconds. This establishes live source delivery;
image quality/cursor behavior, sustained 60 FPS and a backend ranking remain
unverified. Workload visibility was not supplied for this run.

The report then incorrectly failed with PipeWire stream disconnected despite
cleanup_ok true. The state callback unconditionally treated UNCONNECTED as an
error, including the transition emitted by our own pw_stream_destroy. Cleanup
now marks intentional teardown before destroying the stream; only UNCONNECTED
during that teardown is ignored. Unexpected disconnects still fail, genuine
ERROR states still fail even during teardown, and prior acquisition errors are
preserved. Native callback regression checks exercise all of these cases along
with existing token/buffer checks. Native -Wall -Wextra -Werror, 14 unit/8 CLI
portal-feature tests, Clippy with warnings denied and formatting checks pass.
The corrected full live completion remains pending. No desktop, package or
service settings changed; only native code and evidence/plan documents changed.

Rerun the same README probe, keeping motion visible throughout, and inspect
error/null, cleanup_ok and interruption fields before claiming clean completion.
Phase 2 remains open and uncommitted.

Checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T014439Z-n1w5qkd8`.
Rollback this reporting correction (no desktop reload):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T014439Z-n1w5qkd8 --apply
```


### Clean portal source completion — 2026-10-22

The user reran the teardown-corrected 10-second, 60-FPS probe. Session start
and remote open both succeeded, with no interruption, cleanup_ok true and
error null. It delivered 230 valid 1920x1080 buffers, rejected none, and recorded
230 strictly increasing PTS samples. Elapsed 10.018044 seconds, delivery rate
22.8587536649 FPS; first/last PTS span 10.000354292 seconds. SPA format remains
12 and memory type 2. buffer_delivery_verified is true; image_correctness_verified
remains false. This confirms one clean source-only acquisition/cleanup run.
It does not verify pixels/cursor, sustained 60 FPS or interactive cancellation.
No service restart was reported as part of this retry.

The rate remains below the requested 60 FPS. This report does not include
negotiated frame-rate/delivery timing detail, and the user did not explicitly
confirm fixture visibility. Do not diagnose a compositor fault or rank backends
from these two short portal samples. Next investigation should retain negotiated
format/rate and timing evidence under confirmed visible motion, then compare
with direct capture under matching conditions. Backend selection and Phase 2
remain open and uncommitted.

Only documentation changed for this result. Figures and task diff reviewed;
git diff --check passed. No code tests rerun for this evidence-only update.
Checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T015524Z-o1nrj4v0`.
Rollback this evidence update (no desktop reload):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T015524Z-o1nrj4v0 --apply
```


## Portal negotiated-rate/timing investigation — 2026-10-22

Added diagnostics to the same portal source path without changing format offers,
FPS requests, poll timeout, buffering or capture policy. The native/Rust report
now includes the latest negotiated raw-video frame/max-rate fractions, number
of format events, process callbacks and empty dequeues. The
[SPA raw-video contract](https://docs.pipewire.org/structspa__video__info__raw.html)
defines 0/1 as variable frame rate, with max_framerate valid only then; the report
preserves raw fractions and nulls FPS for a zero denominator. It does not claim
negotiated rates equal actual source presentation.

Constant-size summaries retain min/mean/max and counts strictly over 25/50/100 ms
for valid-buffer arrival intervals, strictly positive successive valid PTS
intervals, and acquisition-loop iterations (D-Bus dispatch plus PipeWire poll
and callbacks, including stream startup). No timing vector or pixels are stored.
Nonincreasing PTS remain separately counted. Missing header PTS can span samples.
The original elapsed-based delivery_fps remains; delivery_span_fps additionally
uses the first-to-last valid arrival span. Empty timing summaries have null
min/mean/max. Format-event count makes renegotiation visible, though only the
latest rates are retained. These counters cannot alone establish dropped frames,
a backend ranking or a compositor/driver fault.

Native synthetic tests cover zero/positive interval bounds and strict threshold
counts, alongside existing token/buffer/teardown checks. Rust tests cover empty
summaries, undefined vs variable rates, fractional rates and mean calculation.
The portal-feature suite passes 15 unit + 8 CLI tests and Clippy with warnings
denied. Native self-tests build with -Wall -Wextra -Werror. Live timing data
requires another graphical-session run with continuous motion explicitly
confirmed visible. No capture session is started from the restricted agent.
No packages or desktop configs changed; Phase 2 remains open and uncommitted.

Run the same ten-second/60-FPS README command, share JSON and confirm fixture
visibility. Interpret negotiated rates and client/PTS/loop timing together
before changing scheduling or comparing with direct capture.

Checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T015712Z-7yraeml9`.
Rollback this diagnostics addition (no desktop reload):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T015712Z-7yraeml9 --apply
```

Final continuation checks: all-feature 46 unit + 9 CLI tests and all-feature
Clippy also passed; formatting, whitespace and checkpoint-relative scope diff
review passed. Live timing evidence remains pending.


### First instrumented portal timing result — 2026-10-22

User-supplied JSON: session/remote open true, interruption false, cleanup_ok true,
error null. 292 valid 1920x1080 buffers, no rejected buffers, 292 PTS samples,
no nonincreasing PTS and 292 process callbacks with no empty dequeues. Elapsed
10.008173 s; arrival span 10.008153 s; rates 29.076236 and 29.076294 FPS.
One format event: negotiated frame rate 0/1 (variable), max rate 120/1.

291 arrival intervals: min/mean/max 19.524/34.392278/62.431 ms;
279 over 25 ms, 9 over 50 ms, none over 100 ms.
291 positive PTS intervals: 21.933293/34.418851/69.646460 ms;
278 over 25 ms, 8 over 50 ms, none over 100 ms.
621 loop iterations: 0.198/16.206787/20.330 ms, none over 25 ms.
The report retains SPA format 12 and memory type 2.

Interpretation: this negotiation does not advertise a fixed 30-FPS cap. Client
arrival cadence broadly matches source PTS cadence; no measured acquisition-loop
iteration exceeded 20.330 ms. This does not identify whether source production,
SHM transport, scheduling outside the measured loop, or workload behavior limits
delivery. It does not prove that changing the poll timeout cannot affect it.
Neither portal GPU performance nor encoder behavior was tested. No backend
ranking follows. The user has not explicitly confirmed the fixture stayed
visible throughout, so a controlled workload comparison is still needed.

Next: confirm visible continuous motion, then compare source-only direct and
portal capture under matching current driver/workload conditions. Preserve the
existing capture policy until evidence supports a focused change. Phase 2 and
backend selection remain open. Only documentation changed; supplied values and
scope diff reviewed, git diff --check passed; code tests were not rerun.

Checkpoint:
`/home/ramlej/.local/state/hyprshell/agent-backups/20261022T020159Z-rmpfsa8y`.
Rollback this evidence update (no desktop reload):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T020159Z-rmpfsa8y --apply
```

## Matched SHM source comparison harness — 2026-10-22

Added `tools/compare_sources.py` to collect current wlr SHM, ext SHM and portal
SHM source-only evidence in sequence with matching requested duration, FPS and
hidden cursor. No encoder/audio is active and no image correctness is claimed.
The runner uses argument arrays without a shell, defaults to a no-write dry run,
requires a workload note for acquisition and refuses existing destinations.
Raw stdout/stderr and direct JSON reports are retained alongside comparison.json.
The summary normalizes first-to-last arrival rates without treating portal upper
rate negotiation as equivalent to the direct client's pacing policy.

Failed, interrupted or timed-out probes stop the sequence and retain evidence.
Timeout first requests cancellation, then kills/reaps an unresponsive child.
The requested monitor is recorded, but the portal picker choice and visibility
of the same moving workload require separate user confirmation. The tool never
marks those acceptance checks passed automatically. Samples remain sequential;
workload, driver and thermal conditions still need review before interpretation.

Only checkout tooling/tests/docs changed. No package, desktop or capture-policy
changes. Phase 2 remains open and uncommitted pending controlled live evidence.
Validation: all 10 Python tests passed, including a simulated three-source
sequence retaining raw evidence and pending visibility acceptance. Tests also
cover no-write planning, preserving existing destinations,
failed-report rejection, early stopping/evidence retention and timeout reaping.
Live acquisition cannot run in this restricted session; use the README workflow.

Checkpoint: `/home/ramlej/.local/state/hyprshell/agent-backups/20261022T021100Z-sfw4fgzc`.
Rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T021100Z-sfw4fgzc --apply
```

## First matched SHM source run — 2026-10-22

Inspected `/tmp/hyprrecorder-source-comparison-01/comparison.json`, both direct
reports and the portal stdout JSON. Each requested ten seconds at 60 FPS with
cursor hidden. All completed cleanly at 1920x1080 without timeout/interruption.
No media/audio was captured and no image inspection was performed.

| Source | Frames | First-to-last arrival FPS |
| --- | ---: | ---: |
| wlr SHM | 369 | 36.895807 |
| ext SHM | 392 | 39.218318 |
| Portal/PipeWire SHM | 292 | 29.121769 |

The direct reports identify a 60.007-Hz output. Their completed flags are true,
with no error; source-only finalized=false is expected. Direct client CPU totals
are 0.160632 and 0.167294 seconds; peak RSS 45,400 and 45,052 KiB. These exclude
compositor/portal work, so do not imply whole-system efficiency. Direct pacing
reported 269 and 235 late iterations; those are not measured dropped frames.

Portal: session/remote open, cleanup_ok true, zero rejected buffers, 292 PTS
samples with none nonincreasing, 292 process callbacks and no empty dequeues.
One format event, variable rate 0/1, max 120/1, SPA format 12/memory type 2.
Elapsed 10.012789 seconds, arrival span 9.992525 seconds. Arrival mean/max
34.338574/213.638 ms; positive PTS mean/max 34.362710/214.212897 ms. Each has
one interval over 100 ms. Acquisition-loop mean/max 16.318495/20.312 ms, none
over 25 ms. This sample shows a long arrival gap also present in source PTS,
without a corresponding long measured client-loop iteration. It does not
identify the bottleneck or establish a fixed 30-FPS portal limit.

Correction after user clarification: no browser or animated test page was used
for these tests. The agent omitted opening the workload page from its runnable
instructions, then incorrectly interpreted the earlier confirmation as evidence
of that workload. The user followed the supplied commands. eDP-1 selection was
confirmed; continuous motion and matching content across runs were not established.
No browser-rate history exists. The command's workload note was copied from the
agent's example and is not evidence that the browser page was running. Raw logs
and JSON are retained unchanged; this correction supersedes the earlier workload
acceptance claim.

Direct requests are paced; portal negotiates an upper rate. These measurements
verify clean source delivery for the desktop content present, without identifying
that content's update cadence. They cannot establish a backend winner, a fixed FPS
ceiling or a performance regression. Phase 2 remains open and uncommitted; code
and capture policy are unchanged.

Validation: checked raw reports against supplied output, recomputed first-to-last
arrival rates, and reviewed the documentation-only scope and whitespace diff.
Code tests were not rerun for this evidence-only update. Only checkout docs changed.

Checkpoint: `/home/ramlej/.local/state/hyprshell/agent-backups/20261022T021700Z-ugacz4q0`.
Rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T021700Z-ugacz4q0 --apply
```

## Linear DMA-BUF source follow-up — 2026-10-22

Inspected `/tmp/hyprrecorder-matched-dmabuf-01/report.json`. The ten-second probe
completed cleanly: 441 frames at 44.144334 FPS, 1920x1080 on a reported 60.007-Hz
output, cursor off, unpaced acquisition with no client FPS-cap sleep. A single
linear XRGB8888 plane (modifier zero, stride 7680, offset zero) was imported and
reused. cpu_pixel_access=false, no encoder/audio/media, completed=true,
interrupted=false, error=null. Source-only finalized=false is expected.
Elapsed 10.012753 seconds, acquisition span 9.967304 seconds; all 441 arrival
and presentation timestamps increase strictly. Setup took 0.025627 seconds.

Derived interval statistics over 440 intervals (milliseconds):

| Timing | Minimum | Median | Mean | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Client arrival | 10.164 | 16.860 | 22.652964 | 59.816 | 190.070 |
| Source presentation | 10.321872 | 16.803629 | 22.666725 | 59.702446 | 190.203398 |

p95 uses sorted element floor((n-1)*0.95), without interpolation. Typical cadence
is near the display interval but longer gaps reduce the average. The similar
source/arrival interval distributions do not locate the cause. No encode work
is active in this sample, so this result alone does not motivate encoding overlap.
Earlier accepted GPU minute 01 delivered 59.719 FPS under historical conditions;
that is not a controlled regression comparison. This run also differs from the
new direct SHM probes in pacing policy. No backend winner or efficiency claim
follows from these short sequential source runs.

The user clarified that no browser or animated test page was used for these
tests; actual desktop motion is unspecified, and no browser-rate history exists.
The agent's assumption of a controlled moving workload was unsupported. Preserve
the successful delivery/timestamp results, but do not use the observed mean FPS
as a throughput ceiling or diagnose a scheduling fault from it. Any future
controlled motion experiment must provide the complete setup explicitly before
acquisition. Phase 2 remains open and uncommitted. Code, raw evidence, packages
and desktop configuration are unchanged.

Validation: recomputed FPS, checked all timestamps/counts and interval summaries,
reviewed task-only documentation changes and ran git diff --check. No code tests
rerun for this evidence-only update. Only checkout PLAN.md and this document changed.

Checkpoint: `/home/ramlej/.local/state/hyprshell/agent-backups/20261022T021936Z-o_lv08h0`.
Rollback:

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T021936Z-o_lv08h0 --apply
```


### Workload correction checkpoint — 2026-10-22

Corrected the SHM and DMA-BUF workload descriptions after the user clarified no
browser/test page was used. Only checkout PLAN.md and this document changed;
raw measurements are retained. Reviewed the focused correction and whitespace;
no code tests rerun for documentation-only edits.

Checkpoint: `/home/ramlej/.local/state/hyprshell/agent-backups/20261022T022119Z-357a87fx`.
Rollback (reinstates the incorrect workload interpretation):

```sh
python3 /home/ramlej/.agents/skills/hyprskill/scripts/checkpoint.py restore /home/ramlej/.local/state/hyprshell/agent-backups/20261022T022119Z-357a87fx --apply
```
