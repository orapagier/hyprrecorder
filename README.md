# Hyprrecorder

A compact native screen recorder for Hyprland: full screen or drag region,
resolution and FPS controls, cursor, default system audio and microphone.

The application layers are implemented behind `desktop`/`recorder` features.
**Live acceptance is still pending; this is not a certified release.** The new
application/audio path must pass [the release checklist](docs/release-checklist.md).
Earlier accepted prototype videos do not certify it. See [implementation and
limits](docs/implementation.md) and the full [plan](PLAN.md).

## Desktop application

Review [Fedora](packaging/fedora.txt) or [Arch](packaging/arch.txt) build inputs;
keep a consistent FFmpeg provider and hardware-matching VA-API driver. This build
uses Qt 6 Widgets, libpulse and embedded FFmpeg/GBM/DRM. No external recorder is a
production dependency. The default diagnostic build remains header-light.

```sh
cargo build --locked --release --features desktop
python3 tools/validate.py
python3 tools/install.py            # review local install paths
python3 tools/install.py --apply
hyprrecorder                        # open the window inside Hyprland
hyprrecorder stop                   # request stop, flush and save
hyprrecorder status                 # JSON state while the worker runs
```

The window offers Screen/Region, monitor selection, Native/720p/1080p/1440p/2160p
or custom even dimensions, 1–240 FPS, cursor, independent system/microphone
switches, destination and countdown. It hides before capture and returns after
finalization. Region selection uses `slurp`; Escape cancels. Output fits with black
padding and marks upscaling. Rotation/flips and odd physical source sizes currently
fail explicitly. Fractional region geometry has unit coverage; live acceptance
remains pending. No media pipeline opens before Record.

Audio sources are resolved/pinned at start; missing/muted requested sources fail
clearly so their switches can be turned off. H.264 video and one mixed 48 kHz
stereo PCM track are saved as MKV, without silent software encoding fallback.
Long-run A/V sync remains unmeasured. Saved preferences use XDG config paths,
atomic replacement and a last-good copy; malformed preferences are preserved.
New installations discover the XDG Videos directory with a safe fallback.

For a terminal capture with explicit preferences:

```sh
hyprrecorder record --config /absolute/path/settings.toml --seconds 10
# --silent disables both audio sources for this run.
# --countdown 0..30 overrides the remembered countdown.
```

Use nonprivate content for tests. The app hides its own view; hide any unrelated
terminal before the countdown ends. Repeated launches during recording do not
reveal controls. No startup service, default audio-route changes, uploads or
shortcut changes are installed. Verify shortcut conflicts before optional
Hyprshell integration; proposed open/stop bindings are in the release checklist.

`python3 tools/install.py --uninstall` previews removal; add `--apply` to perform
it. Modified or unowned managed files are preserved and block the operation.
Recordings, preferences and verified installer backup history survive uninstall.

`python3 tools/validate.py --session` also runs the Unix listener integration test
inside the desktop terminal. The restricted agent cannot bind/send Unix sockets
or access the compositor/GPU/audio session. Offscreen UI and synthetic media
checks do not substitute for actual recordings.

## Build and run

Rust 1.86+ and Cargo are required. The foundation uses Serde/JSON/TOML and a
Rust Wayland client for protocol discovery. Default builds need no multimedia
development libraries. The optional capture spike needs FFmpeg development
packages, pkg-config and a C compiler. Only Rust 1.99 has been tested.

```sh
cargo build --locked
cargo run --locked -- --help
cargo run --locked -- config --print-default
cargo run --locked -- config --check config.example.toml
cargo run --locked -- doctor
cargo run --locked -- doctor --json
cargo run --locked -- probe-wayland --json
```

Use `--offline` when these locked crates are already cached. Cargo does not
install system packages. No command above creates configuration, modifies audio
routing, records content, or creates a video directory.

An optional hardware probe encodes 30 synthetic black frames through VA-API:

```sh
cargo run --locked -- doctor --probe-encoder
```

This requires FFmpeg with `h264_vaapi`, an accessible render device and a working
driver. It records no screen/audio and saves no video file. The current probe
uses the first visible render node; multi-GPU selection is a later capability.
A successful probe proves only that synthetic encode path, not capture readiness,
quality, cursor correctness, or A/V sync. It is not a performance benchmark.

`doctor` exits successfully when it produces a report, even if capabilities are
missing/unverified. JSON includes per-check status and `recording_implemented`.
Invalid CLI/configuration input exits with code 2 and a message on stderr.
Probe commands have time limits and bounded retained stdout/stderr. Diagnostics
can contain default device/connector names; review reports before sharing them.

`probe-wayland --json` connects to the session display and reads the registry
under a three-second worker timeout. It reports direct-capture/DMA-BUF protocol
advertisements separately from successful acquisition (`capture_verified` remains
false). It creates no capture session or media buffers. An inaccessible session
produces a report with `session_reachable: false`, not a desktop failure diagnosis.

## Portal capability investigation

```sh
cargo run --offline --locked -- probe-portal --json
```

This bounded property-only probe uses systemd's `busctl` (no new Rust or multimedia
build dependencies). It queries ScreenCast `version`, `AvailableSourceTypes` and
`AvailableCursorModes` independently, preserving raw masks and individual errors.
Known source/cursor bits are decoded; missing properties remain null, not zero.
It may activate the desktop portal service through normal D-Bus activation, but
creates no capture session, opens no source picker, connects to no PipeWire stream
and writes no files. It never claims capture works from advertised capabilities.
The command exits successfully when a diagnostic report is produced, including
when the bus, service or busctl is unavailable. Invalid arguments exit with code 2.

Run this from a graphical terminal and share its JSON for the next portal step.
A sandbox connection denial does not establish a desktop or portal failure.
The separate portal-spike feature below implements one persistent D-Bus connection
for request responses/session ownership and Unix-FD transfer into PipeWire.
Its live capture acceptance and the backend decision remain open.

## Experimental portal/PipeWire source probe

The optional `portal-spike` feature uses the installed GIO Unix-FD and PipeWire
libraries (`gio-unix-2.0`, `libpipewire-0.3` via pkg-config and a C compiler).
It does not require the FFmpeg/GBM capture features or add Rust dependencies.
Fedora development providers are `glib2-devel` and `pipewire-devel`; Arch uses
[glib2](https://archlinux.org/packages/core/x86_64/glib2/files/) and
[libpipewire](https://archlinux.org/packages/extra/x86_64/libpipewire/files/).
This machine already has the required development files;
no package installation was performed. Arch builds remain untested.

```sh
cargo run --offline --locked --features portal-spike -- probe-portal-source --json --seconds 10 --fps 60
```

Choose one monitor in the portal picker. After selection, switch to the motion
fixture during the three-second countdown and keep it visible throughout the
probe. The acquisition interval starts at the first valid buffer. Without a valid
buffer, startup fails after ten seconds. JSON is printed after cleanup; share it
with any picker/error behavior. Nothing is saved to disk. No audio is opened.

This uses one dedicated GIO D-Bus connection for CreateSession, SelectSources,
Start and Unix-FD transfer through OpenPipeWireRemote. PipeWire targets the
stream serial when supplied, otherwise its legacy node ID. The owned native
bridge requests raw 32-bit RGB over mapped shared-memory buffers, validates
bounds/strides/corruption flags, counts delivery and header timestamps, and
returns each buffer without reading pixels. No DMA-BUF, GPU encoding, image
correctness or performance comparison is established by this probe.

Cursor embedding is requested by default; `--no-cursor` requests hidden cursor.
Only monitor sources are requested, even when the portal offers other types.
`--fps 30|60` is a negotiated upper request, not proof of presentation rate.
`--seconds` accepts 1–60. `--json` is required. Duplicate/unknown options fail
before connecting. Existing capture commands and default builds remain available.

The JSON now includes negotiated raw-video `negotiated_rate` and
`negotiated_max_rate` fractions, format-event count, process callbacks and empty
dequeues. A 0/1 frame rate means variable rate; the maximum is meaningful only
for a variable rate. A zero denominator is unavailable, with FPS null. These are
negotiated values, not measured presentation rates.

`delivery_intervals` summarizes time between valid buffers at the client;
`pts_intervals` summarizes strictly positive consecutive valid PTS differences;
`loop_iterations` includes D-Bus dispatch and PipeWire poll/dispatch time during
startup and acquisition. Each uses fixed-size counters for min/mean/max and
intervals strictly over 25/50/100 ms, without retaining frames or a timing log.
Empty timing sets have null min/mean/max. Repeated/decreasing PTS are counted
separately. Missing PTS can leave gaps between observed timestamp samples.
The original `delivery_fps` still uses elapsed acquisition time; new
`delivery_span_fps` uses the first-to-last valid-buffer arrival span. Neither is
a count of distinct compositor presentations or proof of dropped frames.

For the next test, keep the moving fixture visible on the selected monitor for
the full interval, and explicitly confirm it stayed visible when sharing JSON.
No performance cause or backend ranking follows from counters alone.

Ctrl+C/SIGTERM cancels waits and acquisition; synchronous D-Bus calls may take
up to five seconds to return before cancellation is observed. Each asynchronous
portal request has a 120-second timeout. Cancellation closes a pending request;
cleanup destroys the stream, disconnects the remote, closes descriptors/session
and the dedicated connection. The report marks interruption and cleanup outcome.
Picker cancellation, source loss, negotiation failure and no valid frames report
an error with exit code 2. A Ctrl+C interruption is explicitly labeled and may
have zero frames. Source geometry/format changes stop with an error. Pixel quality,
interactive cancellation and resource release still require live acceptance.

## Experimental owned capture spike

This explicitly records the selected monitor with no audio, after a three-second
countdown. Hide the terminal during the countdown. It uses our own Rust Wayland
capture and embedded FFmpeg libraries; no external recorder is needed:

```sh
cargo run --locked --features capture-spike -- spike --monitor eDP-1 --seconds 10 --directory /tmp/hyprrecorder-owned-01
```

Use a new absolute directory each time. On success it contains `capture.mkv`
and `report.json`. Ctrl+C stops and finalizes any acquired frames. Failures retain
a partial MKV and an error report when the destination remains writable.
`--fps 60`, `--no-cursor` and `--render-device /dev/dri/renderD128` are supported.
This spike deliberately uses **CPU-copy capture and color conversion, followed
by VA-API H.264 upload/encoding**. It is not a DMA-BUF/zero-copy implementation.
Capture and encoding overlap with one queued frame. Completed mapped buffers
move to the encoder thread without another pixel copy; backpressure bounds memory.
There is no software encoding fallback. It requires wlr-screencopy v3, named
outputs, even 8-bit RGB dimensions and an unrotated monitor at scale 1. Region,
scaling, audio, UI and production stop/status coordination remain later work.
It ignores the product TOML configuration; its explicit CLI options define the
video-only experiment, including disabling both audio sources.

Inspect the video for motion, readable text, colors/orientation and cursor.
The JSON records timestamps, capture/queue/encode timings, process CPU/RSS, byte size and
finalization; visual acceptance remains unverified until a person inspects it.
The existing doctor/config `recording_implemented` field still describes the
production recorder, which is not implemented.

For capture-only timing, run the same acquisition path without an encoder or
video file (only `report.json` is written):

```sh
cargo run --locked --features capture-spike -- probe-source --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-01
```

This still acquires screen frames into anonymous shared memory. It discards
them without pixel processing and opens no microphone or encoder render device.
Close the previous video player and use comparable scrolling/motion during the
probe. Its result isolates source acquisition; it does not validate recording
quality, hardware encoding or a zero-copy path. Both commands now print the
measured acquisition FPS and include it plus monitor refresh in their reports.

An explicit session-based SHM comparison is also available:

```sh
cargo run --locked --features capture-spike -- probe-source --capture-backend ext --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-ext-01
```

Use continuous scrolling or animation during this comparison: ext capture may
wait for content changes on a static screen. Both `spike` and `probe-source`
accept `--capture-backend wlr|ext`; the default remains `wlr`, with no automatic
fallback. The ext path negotiates SHM constraints once per session but still
copies pixels through CPU memory. Its performance needs live verification.
Idle ext capture ends at the requested duration without inventing duplicate
frames; recordings do not yet extend the last frame to cover a trailing idle
period. A low acquisition rate on static content is not a dropped-frame count.

For the GPU buffer transport experiment (also report-only):

```sh
cargo run --locked --features dmabuf-spike -- probe-dmabuf --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-source-dmabuf-01
```

`dmabuf-spike` adds GBM/libdrm build requirements to `capture-spike`. This command
uses ext capture and linux-dmabuf v3, negotiates an explicit XRGB8888/ARGB8888
format/modifier combination, and reuses one imported GBM buffer. It selects the
render node belonging to the compositor's advertised DRM device. An optional
`--render-device` override must identify that same device. There is no SHM
fallback, CPU pixel mapping, audio, encoding or video file. Acquisition starts
after negotiation and the three-second countdown; keep visible content moving.

The report includes the selected render node, capture/import offers, format,
modifier, plane strides/offsets, setup duration and actual acquisition rate.
This checks buffer delivery, not image correctness or an end-to-end zero-copy
recording path. GPU-driver calls can still delay cancellation. Live performance
and compatibility require user-session measurements and visual validation.

To isolate the client's FPS-cap sleep, add `--unpaced` to `probe-dmabuf`.
Requests then follow each Ready event immediately, with one frame outstanding;
the duration, cancellation and damage-driven waits still apply. In this mode
`--fps` is a reference for timing counters, not an enforced rate limit. Reports
label `pacing: unpaced` and `frame_rate_limit: null`; default runs retain the cap.

For repeatable moving content, open the local synthetic fixture:

```sh
xdg-open "$PWD/tools/capture-workload.html"
```

Click **Full screen** and leave motion active on the captured monitor. Start the
probe from the terminal and switch back during its three-second countdown. The
page animates a repeating pattern and scrolling text without network requests
or manual scrolling. Note its browser update rate during capture alongside the
recorder result. That indicator measures requestAnimationFrame callbacks; it
does not independently verify compositor presentation or capture rate. Keep the
same browser, monitor, workload and Mesa version for comparisons. Pause motion
only when deliberately testing idle-content behavior.

The fixture retains its lowest sampled rate and last 20 timestamped FPS samples
until **Reset measurements** or a page reload. They survive pause/resume, hidden
periods and full-screen changes; partial measurement windows reset at those
boundaries. Click Reset before each run, then inspect the retained values after
the probe finishes. The minimum includes visible window-switch/system delays,
so it is evidence to compare with the sample timeline, not proof of a capture
fault. A steady live rate after capture ends does not describe the capture period.

### Matched SHM source comparison

Build both source features, then use the comparison runner from a graphical
terminal. Its default mode prints argument arrays and creates nothing:

```sh
cargo build --offline --locked --features capture-spike,portal-spike
python3 tools/compare_sources.py --monitor eDP-1 \
  --directory /tmp/hyprrecorder-source-comparison-01
```

To acquire, add `--run --workload-note "same browser, full-screen moving fixture"`.
It runs wlr SHM, ext SHM and portal SHM sequentially for ten seconds each,
requesting 60 FPS with cursor hidden. Keep the same continuous motion visible
on the same monitor throughout; choose that monitor in the portal picker, then
return to the fixture during its countdown. Each source has a three-second
countdown. Keep browser/driver/display settings fixed. Longer runs accept
`--seconds 60`; use a new directory for each repetition. No media/audio is saved.

The private destination keeps raw stdout/stderr, direct reports and
`comparison.json`. The runner stops on a failed, interrupted or timed-out probe,
retaining prior evidence. It gives each process duration plus 90 seconds for
setup/picker, then requests cancellation and finally kills/reaps if necessary.
This timeout is a harness limit, not a diagnosis of the desktop.

Share `comparison.json`, confirm the picker monitor and whether motion stayed
visible in **all three** runs, and note the fixture's retained browser rates.
The summary compares first-to-last arrival rates; raw reports preserve each
backend's timestamps and diagnostics. Direct requests are client-paced; portal
FPS is an upper negotiation request. Sequential samples can still differ in
workload or thermal state. This isolates SHM source delivery without encoding;
it does not rank GPU recording pipelines or prove image correctness. Visibility
and monitor acceptance remain pending in the report until reviewed separately.

Once DMA-BUF source delivery works, the separate GPU recording experiment is:

```sh
cargo run --locked --features gpu-encode-spike -- spike-gpu --monitor eDP-1 --fps 60 --seconds 10 --directory /tmp/hyprrecorder-gpu-video-01
```

This feature adds libavfilter development requirements (already installed on
the tested Fedora machine). It requires a single-plane linear RGB buffer,
imports it directly into VA-API, converts to NV12 with `scale_vaapi`, then
encodes H.264/MKV. There is no CPU pixel conversion/upload or software fallback.
Capture, GPU conversion and encoding are serialized in this initial experiment;
source-only rates do not predict its recording rate. No audio is implemented.
The output is staged as `capture.partial.mkv` and published as `capture.mkv`
only after checked finalization. Failures retain the partial file and report.

Keep the animation workspace visible throughout capture. A browser on an
inactive workspace can stop updating while the probe captures the visible
workspace. Start from the terminal, switch to the animation during the
three-second countdown, and return after the requested duration. Inspect the
saved video for colors, text, motion and cursor. The first live ten-second GPU
video finalized 555 matching decoded frames at 55.53 FPS, and the user accepted
its image/motion/cursor quality. Three unpaced minute-long 1080p60 samples
finalized matching decoded frames at 59.72, 59.05 and 59.79 FPS without
interruption or decode errors. Three paced minute-long 1080p30 samples also passed
at 29.84, 29.82 and 29.83 FPS. The user visually accepted all six completed
minute-long recordings. Comparison measurements remain outstanding. `spike-gpu` also accepts `--unpaced` to remove
the client rate-cap sleep while retaining the requested FPS as a reference.

For GPU crop/scale and cursor checks, `spike-gpu` now accepts:

```sh
cargo run --offline --locked --features gpu-encode-spike -- spike-gpu --monitor eDP-1 --fps 30 --seconds 10 --crop 320,180,1280,720 --size 640x360 --no-cursor --directory /tmp/hyprrecorder-gpu-crop-01
```

`--crop X,Y,W,H` uses physical pixels relative to the selected monitor's top-left;
this spike still requires an unrotated, scale-1 output. Omitting it captures the
whole monitor. `--size WIDTHxHEIGHT` sets encoded dimensions; omitting it keeps
the crop's native dimensions. Widths/heights must be even, 2–8192. Crop origins
may be odd; out-of-bounds rectangles fail rather than being clamped. The spike
requires exactly matching source/crop and output aspect ratios; it rejects
stretching and does not yet implement padding for arbitrary output shapes.
GPU VPP crops/converts/scales the imported surface, without CPU pixel access.
The countdown prints crop/output size, upscaling where applicable and cursor
on/off; `report.json` records `geometry` separately from full capture dimensions.
Cursor remains on by default; `--no-cursor` requests compositor-side exclusion.
The first crop/downscale/cursor-off sample finalized and decoded 282 frames at
28.28 FPS. The user accepted the central crop, proportions and absent pointer.
The full-source 1280×720 downscale/cursor-on sample finalized and decoded 293
frames at 29.29 FPS. The user accepted the whole-screen view, proportions and
visible pointer. Both short geometry/cursor checks passed; sustained performance
and other monitor transforms/scales still need separate validation.

Keep the motion fixture visible during the countdown and recording, and move
the pointer inside the crop. Inspect `capture.mkv`: only the central 1280×720
region should appear, scaled to 640×360, with no pointer. Compare with a second
run using `--size 1280x720` and no crop or `--no-cursor`, in a new directory:
that should show the full 1920×1080 source scaled to 720p with the pointer.
No audio is expected. Supply the reports and visual results before marking this
gate passed; source protocol support and unit tests cannot establish image quality.

To compare buffer layouts, add `--linear` to `probe-dmabuf`. It requires modifier
zero in both capture and import offers and passes only that layout to GBM; an
unsupported layout fails without substituting tiling or SHM. The report keeps
the requested modifier separately from the allocated modifier and plane layout.
Without this option the negotiated driver choice remains unchanged. Compare on
the same full-screen fixture and note the browser callback rate.

## Configuration contract

[config.example.toml](config.example.toml) documents all current settings. Unknown
fields and unsupported versions are rejected. Native dimensions are resolved in
a later recording phase. Custom output dimensions must be even and 2–16384;
FPS accepts 1–240, subject to hardware/source checks before capture is implemented.
Rectangle coordinates are compositor logical coordinates and may be negative.
An omitted rectangle in region mode means interactive selection is still needed.

Both audio switches default on. The eventual engine resolves system defaults at
start, and pins those sources for the recording. Explicit output paths must be
absolute or start with `~/`; no shell expansion or command execution is used.

## Development checks

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features capture-spike
cargo clippy --locked --features capture-spike --all-targets -- -D warnings
python3 -B -m unittest discover -s tests -p 'test_*.py' -v
```

Phase 2 will choose an embedded library-backed Rust capture pipeline after real
hardware measurements. No claim is made that Rust alone makes encoding faster,
or that Hyprrecorder is officially packaged/supported by Fedora or Arch.
