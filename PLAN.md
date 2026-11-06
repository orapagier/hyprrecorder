# Hyprrecorder implementation plan

Created: 2026-10-21. This is a standalone app in `/home/ramlej/repos/hyprrecorder`.

## Product contract

Build a small Rust screen recorder for the user's Fedora/Arch Hyprland desktop.
Opening `hyprrecorder` eventually presents one compact, modern window matching
Hyprshell. Choose full screen or draw a rectangle, choose output resolution/FPS,
toggle cursor/system audio/microphone, and record. Use the system's default audio
output and input without requiring a device picker. Hide the app before capture
begins. A global shortcut stops and saves without bringing controls into the video.

The project owns its UI, lifecycle, settings, capture orchestration, and error
handling. Existing codecs/drivers/multimedia libraries remain dependencies.
Reimplementing H.264, audio codecs, GPU drivers, or a general multimedia framework
is outside scope. An external recorder may serve as a benchmark or temporary
prototype, but it must not silently become the permanent architecture.

### Defaults and behavior

- Full screen means one selected monitor, initially the focused monitor; a small
  monitor selector appears when more than one eligible monitor exists. Combining
  multiple monitors into one video is outside the first release.
- Native output size means the selected monitor's physical pixel dimensions after
  accounting for transform, or the selected rectangle converted from compositor
  logical coordinates into pixels. Re-evaluate at recording start.
- Region selection is live drag selection with Escape to cancel. Initially keep
  the rectangle within one monitor; clearly reject cross-monitor selections.
- Video starts at 30 FPS, cursor enabled, hardware H.264 where verified. Offer
  24/30/60 presets and custom integer 1–240 FPS; hardware/source limits may reject
  choices. FPS above the display refresh rate does not create new visual detail.
- Offer Native, 720p/1080p/1440p/2160p, and custom output dimensions. Preserve
  aspect ratio using an explicitly defined fit/padding policy, never silent
  stretching. Higher output resolution is marked as upscaling where applicable.
- Encoder-compatible dimensions are even for initial 4:2:0 output. Explicit odd
  dimensions are rejected. Odd native dimensions are padded by at most one pixel,
  with the exact output size shown before recording.
- System audio and microphone default on, with independent off switches. Resolve
  default device identities at start; pin sources for that recording in v1.
  Changing a system default affects the next recording, not the current one.
- When a requested source is unavailable, report it and let the user choose how
  to continue. Never silently produce a silent track or substitute another mic.
- Default saved directory is `~/Videos/Hyprrecorder`, configurable. Phase 7 adds
  XDG Videos-directory discovery. Timestamped names never overwrite old recordings.
- Initially write Matroska (MKV), H.264 plus a mixed audio track. MP4 export/remux
  can be a later explicit feature. A crash can still damage the tail; do not claim
  recording files are corruption-proof.
- The window unmaps, region overlays disappear, then a short configurable
  countdown completes before acquisition begins. No app UI is intentionally
  present in captured frames. Verify this with actual recordings.
- Stop finalizes the file before presenting controls or a saved notification.
  A recording indicator must not appear in captured content unless opted in.
- One recording instance per user; repeated launches communicate with it.
- No background recording, startup service, capture at login, or resident polling
  daemon. When idle/hidden, no video/audio capture pipelines remain active.

### Visual direction

Match Hyprshell's current theme: dark `#202020` background, `#303030` surfaces,
`#252525` fields, `#f5f5f5` text, `#b8b8b8` secondary text, blue `#2874cc` accent,
12 px outer radii, 8 px control radii, Adwaita Sans. These are initial reference
tokens, not a reason to read or rewrite all desktop preferences. Respect reduced
motion and text scale. Keep the main view focused on capture mode, source/region,
resolution, FPS, three toggles, save location, and a primary Record action.

## Engineering decisions and boundaries

### Backend decision gate

Do not choose a winner based on language, package size, or unverified marketing.
Compare GPU Screen Recorder as an external baseline, Rust `wl-screenrec`, and an
embedded Rust pipeline using GStreamer or FFmpeg libraries. The preferred final
architecture is an app-owned Rust backend with existing library codecs. Phase 2
must demonstrate the capture-to-encoder path on this machine before committing.

Candidate A: Rust Wayland capture through supported compositor protocols,
DMA-BUF where negotiated, GPU conversion/crop/scale, FFmpeg libraries/VA-API,
audio capture through PipeWire or its PulseAudio compatibility service, shared
timestamps, encoding, and muxing. `wl-screenrec` is useful implementation evidence;
respect its license before borrowing code.

Candidate B: Rust portal/PipeWire capture with an embedded GStreamer pipeline,
hardware encoding and audio mixing. GStreamer provides pipeline clocks, queues,
encoders, and muxers; that does not establish zero-copy behavior. Validate GPU
buffer negotiation and crop/scale support on the installed plugins.

Portal use offers compositor-mediated authorization but can introduce an extra
source picker; direct Wayland capture can support the desired custom selection
where protocols permit it. Record the UX/security/compatibility tradeoff. Neither
source is assumed to be supported just because a Wayland environment variable is
set. Cursor modes and native region support must be negotiated and tested.

### Proposed modules

- `config`: versioned settings and validation; atomic saves arrive with persistence.
- `diagnostics`: capability reports; separate installed, reachable, and verified.
- `capture`: source discovery, Wayland/portal session, buffers and cursor mode.
- `video`: conversion, crop, scaling, frame pacing, hardware encoder selection.
- `audio`: default output monitor/default input, mixing, timestamps and resampling.
- `recording`: lifecycle, resource ownership, finalization and failure recovery.
- `ipc`: single instance, local control, stop/status commands.
- `ui`: themed controls, region overlay, accessible keyboard navigation.
- `output`: unique file creation, disk errors, muxing and saved-file metadata.

Internal lifecycle: Idle → Selecting (when needed) → Preparing → Countdown →
Recording → Stopping → Saved. Selection/countdown may return to Idle. Every
active state can fail with a specific reason. Preparing validates sources,
dimensions, file creation, and encoder before starting the capture clock.
Repeated stop requests are idempotent. A partial file is never announced as a
successful finalized recording. UI and media work run independently.

Use bounded frame/audio queues with explicit backpressure/drop policy. Prefer
hardware conversion/encoding and shared GPU buffers when actually supported.
Avoid shell command construction. Never accept a socket request that can execute
an arbitrary command; use a private user runtime directory and typed messages.
Dispose capture sessions, descriptors and worker threads on cancellation/failure.

## Implementation phases

Each phase ends with a small reviewable result, relevant checks, an updated status
record, known limitations, and a local commit when applicable. Do not claim a
phase complete when a required live acceptance check remains unperformed. Keep
unrelated Hyprshell functionality and user edits unchanged.

### Phase 1 — Foundation and honest capability diagnostics

Deliverables:

- Standalone Rust crate, locked dependencies, README and this plan.
- Versioned typed settings: screen/region, optional monitor/rectangle, native or
  explicit resolution, FPS, cursor, both audio switches, output directory.
- Shell-free CLI: help/version, print defaults, validate a supplied TOML file,
  human/JSON diagnostics. No implicit configuration or output-directory writes.
- Probe session reachability, monitor dimensions, render-device visibility,
  default audio identities, encoder catalog and development-library availability.
- Optional synthetic VA-API H.264 probe; no screen/audio capture or video file.
- Meaningful validation and CLI tests; bounded probe execution and output.

Exit criteria: offline build/test, formatting and lint checks pass; default file
validates; JSON parses; invalid settings fail clearly; unsupported recording
commands cannot masquerade as success. Doctor must distinguish an FFmpeg-listed
encoder from one actually exercised. Save a dated local validation report without
private device names or captured content. Live capture is explicitly not part of
Phase 1. See `docs/phase-1.md`.

### Phase 2 — Capture spike, measurements, and architecture decision

Deliverables:

- Inventory exact runtime/development dependencies on Fedora and Arch; document
  codec/provider constraints separately from app code. Install only what is
  required, using a recoverable package workflow when machine changes are needed.
- A small command-line prototype for actual video capture and hardware encoding.
  Prototype direct Wayland and portal/PipeWire paths as feasible.
- Test full-monitor acquisition, buffer formats, cursor visibility, crop/scale,
  and source cancellation. Reject unsupported paths with actionable errors.
- Reproducible comparison with available baseline recorders; same source content,
  dimensions, FPS, codec, and comparable output quality. Include audio-on tests
  once comparable audio paths are available; label video-only results.
- Write `docs/backend-decision.md`: measurements, chosen dependencies, licensing,
  fallback policy, portability, complexity, and unresolved risks.

Measurement protocol: warm up, then run at least three 60-second samples for
1080p30 and 1080p60 with text scrolling and motion; include native desktop size
when different. Record app and helper CPU/RSS, system/GPU behavior where observable,
encoded/dropped frames, frame timestamps, output size and quality. Measure power
only when comparable conditions and a reliable tool are available; record AC vs
battery, brightness and workload. Include an idle UI baseline later. No numerical
performance claims without this evidence. No universal CPU/RAM target invented
before measuring the baseline.

Exit criteria: a real playable hardware-encoded capture and reproducible evidence
support the chosen path. Check text legibility/motion, not just process exit code.
If direct capture cannot meet requirements, document the alternative before
building the rest around it. Sandbox failures do not diagnose the desktop.

### Phase 3 — Reliable video recording core

Deliverables:

- Implement the selected library-backed pipeline behind a Rust interface; no
  dependency on another recorder's graphical UI.
- Monitor selection, native size, transforms and fractional scale handling.
- FPS/frame pacing, cursor option, GPU crop/scale where supported; make any CPU
  fallback explicit and measurable. Preserve aspect ratio and show output size.
- Unique output creation, MKV muxing, stop/flush/finalize and disk-error reporting.
- Lifecycle ownership and bounded queues; repeated stop and startup failure cleanly
  release resources. Inspect actual encoder capability before recording.
- CLI `record`, `stop`, `status` and instance coordination sufficient for testing.

Exit criteria: full-screen recordings at native/720p and 30/60 FPS play correctly;
cursor on/off is verified; rotated/scaled output behavior is tested or explicitly
unsupported before release. Test cancellation, two launches, destination denial,
disk-full simulation, source disappearance and encoder failure. Never overwrite
existing files. No silent software encoding fallback.

### Phase 4 — Default system audio and microphone

Deliverables:

- Resolve the default output's monitor stream and default microphone separately
  at start. Do not equate a playback sink with a microphone source.
- Independent system/mic switches and one mixed track. Define gain/headroom;
  no playback/loopback from the recording pipeline and no changes to global routes.
- Shared monotonic timebase, resampling and bounded buffers. Handle absent source,
  muted source, unplug, and changing defaults with the documented pinned-source
  policy. Do not hide loss of a requested source behind a success message.

Exit criteria: all four combinations (silent/system/mic/both) produce expected
tracks. A repeated clap/beep test at beginning/end of a 30-minute recording targets
≤80 ms A/V offset and ≤40 ms drift under a representative load; record measured
results and revise implementation rather than claiming unmeasured precision.
Verify headset/speaker defaults and cleanup without modifying system defaults.
Warn users about acoustic echo with speakers; no automatic DSP in v1.

### Phase 5 — Region selection and invisible recording controls

Deliverables:

- Drag selection with dimensions and Escape cancellation, constrained to one
  output. Correctly convert logical coordinates, negative positions, scale and
  transform to capture pixels. Do not assume the primary output starts at (0,0).
- Hide/unmap controls and selection overlays before acquisition; countdown is
  outside the recorded interval. Prevent region selections from changing on
  accidental later UI events.
- Define monitor hotplug/mode-change handling: stop safely and explain, rather
  than silently changing the capture rectangle mid-file.
- Private IPC stop/status and graceful process termination without deadlocks.

Exit criteria: inspect the first/last frames and middle of actual captures; app
window, selection border and stop controls must not appear. Test negative output
positions, fractional scale, Escape, very small regions and repeated stop.

### Phase 6 — Minimal, smooth native UI

Deliverables:

- Choose a maintained Rust-accessible native UI toolkit after checking Wayland,
  keyboard accessibility, theme fidelity, idle cost and dependency footprint.
  Rust app logic does not require hand-writing every widget or renderer.
- One compact floating window using the visual direction above. Main actions:
  Screen/Region, monitor/reselect, resolution, FPS, cursor, audio, mic, destination,
  Record. Hide advanced technical detail behind actionable error/help text.
- Smooth bounded transitions; honor reduced motion/text scale; tab navigation,
  keyboard activation, visible focus and clear recording/saving/error states.
- Remember settings atomically in XDG config paths, preserving last good settings
  on malformed files. Reopening during capture cannot accidentally expose controls
  in the video; provide status through an appropriate out-of-band mechanism.

Exit criteria: live keyboard/pointer acceptance, readable HiDPI layout and error
flows; no continuous redraw loop while idle, no media capture before Record, no
idle GPU/video work. Measure startup/idle/recording cost against Phase 2 baselines.
Keep backend diagnostics available separately from the main product flow.

### Phase 7 — Desktop integration and reproducible Fedora/Arch installation

Deliverables:

- User-local launcher and desktop entry/icon; build/install/uninstall instructions
  and dependency manifests for both distros. Document codec repositories honestly.
  Working on a distro is not the same as official distro packaging/support.
- Verify live bindings before adding shortcuts. Candidate: Super+Shift+R to open,
  Super+Ctrl+R to stop/save. Existing Alt+Z belongs to Hyprbar; preserve it.
- Keep desktop integration optional for standalone use. Preserve integration in
  Hyprshell's checkout and matching installed copies only when that phase is
  authorized; checkpoint before editing either. Update installer paths narrowly.
- XDG Videos discovery with safe fallback, saved-file notification after capture
  ends, and an explicit Open folder action. No automatic uploads.
- Single-instance behavior and clear saved-state UX. No auto-start/background
  recorder service. Never bind over logout/reboot/power or screenshot shortcuts.

Exit criteria: fresh user install on supported Fedora/Arch environments, native
Wayland launch, start/stop shortcuts, app discovery and uninstall are tested.
Record exact distro/toolkit/driver versions and limits. Validate Hyprland config
before reload and preserve unrelated shortcuts/settings. No release claim for
environments only inspected through mocks.

### Phase 8 — Hardening and first release candidate

Deliverables:

- Longer recordings, suspend/resume, locked session, permission denial, full disk,
  sudden source loss, interrupted finalization and display unplug/mode change.
- Memory/descriptor/thread leak checks across repeated recordings; bounded logs
  and no unrelated audio/device/user data in diagnostics or shared issue reports.
- Repeat comparable performance and audio sync runs, record quality settings and
  measurable regressions. Publish limitations instead of unsupported claims.
- Reproducible release builds, dependency/license inventory, documented release
  checklist, maintenance ownership, recovery and issue-report template.

Exit criteria: all required product behaviors validated live, known failures
recover without leaving capture active, compatibility matrix documented, and
remaining limitations accepted explicitly. A first release may target Intel on
Hyprland initially; AMD/NVIDIA/other compositors require separate validation.

## Risks and mitigations

| Risk | Response |
| --- | --- |
| GPU encoder listed but unavailable | Actual synthetic encode, then real capture tests |
| Extra copies introduced by buffer negotiation | Inspect negotiated formats/memory and benchmark |
| GStreamer/FFmpeg codec packaging differs by distro | Explicit provider requirements and clean-install checks |
| Audio clock drift or unbounded queues | Common timebase, resampling, bounded queues, long tests |
| Portal picker disrupts the simple flow | Compare direct capture protocols and state the tradeoff |
| Window/overlay leaks into recording | Unmap before capture and inspect first/last frames |
| Higher output size mistaken for extra detail | Mark upscaling and retain native default |
| Own backend takes more upkeep than a wrapper | Small scope, mature libraries, measured decision gate |
| Shortcut collides with a custom binding | Inspect saved and live bindings before installation |
| Capture API unavailable to the agent | Report unverified and provide user-session checks |

## Status and next action


### Application continuation — 2026-10-22

Implementation now includes a feature-gated native Qt window and app-owned
recording pipeline. This status supersedes the older continuation entries below;
it does not turn unperformed live checks into passes.

| Phase | Implementation / acceptance status |
| --- | --- |
| 1 | Foundation retained; capability reporting reflects the selected build |
| 2 | Direct ext/linear DMA-BUF/VA-API selected provisionally for implementation; matched external comparisons and portal acceptance remain open |
| 3 | Recording lifecycle, pinned monitor, bounded GPU acquisition/repeats, fit/padding, MKV finalization, no-overwrite outputs and stop/status implemented; new live video/error acceptance pending |
| 4 | Independent default-source capture, timestamp placement, mixed PCM track and explicit source errors implemented; actual devices and 30-minute offset/drift acceptance pending |
| 5 | Slurp drag/Escape integration, logical-to-pixel regions, view-hide acknowledgement, countdown, repeated-launch protection, lock/suspend/mode-change failure paths implemented; live invisibility/hotplug checks pending |
| 6 | Qt 6 Widgets window, native focus controls, theme, atomic preferences and last-good recovery implemented; offscreen checks available, physical keyboard/pointer/HiDPI/idle measurements pending |
| 7 | User-local installer, desktop entry/icon, Fedora/Arch inputs, XDG Videos, saved notification and Open folder implemented; shortcuts intentionally require live conflict review and fresh-distro acceptance remains pending |
| 8 | Synthetic media/mixer/failure checks, installer preservation, repeatable validation/license inventories and release checklist implemented; long-run/live hardening and release sign-off remain open |

Details: [application architecture and limits](docs/implementation.md),
[required live release checks](docs/release-checklist.md). Rotated/flipped and odd
native sources are explicitly unsupported; required automatic one-pixel native
padding remains open. No release is tagged or claimed. The sandbox denies GPU,
compositor and audio access plus Unix socket bind/send; listener acceptance runs
with `python3 tools/validate.py --session` in the desktop terminal. No desktop
shortcuts, packages or Hyprshell files changed.

Validation: formatting and strict default/all-feature Clippy passed. Default
build: 22 Rust tests passed. All features: 68 Rust tests passed, one real-listener
test explicitly ignored because the sandbox forbids sockets. Twelve Python
harness/installer tests passed. Synthetic codec-fixture/mixed-PCM mux decoding,
private output, persistence, geometry and offscreen Qt paint/focus checks passed.
The desktop release build passed with locked/offline dependencies. Evidence:
`/tmp/hyprrecorder-validation-20261022-release-candidate` (checks, logs, native
versions and Rust dependency licenses).

An isolated-prefix installation passed binary hash, desktop-file, icon and
window checks. Installed the matching release at `~/.local/bin/hyprrecorder`,
with `io.github.hyprrecorder.desktop`, SVG icon and installation receipt under
`~/.local/share`. The installed copy passed build-info, hash, desktop-file and
isolated offscreen window checks. No user preferences or recordings were created.
Launch `hyprrecorder` in the desktop; request save with `hyprrecorder stop`.

Task checkpoints are retained under
`~/.local/state/hyprshell/agent-backups/`: implementation `20261022T022726Z-2uij9uwo`,
capability reporting `20261022T024739Z-mhqhe8no`, installation
`20261022T030357Z-v7wfls1u`. The first contains `rollback.sh` restoring all three
in order, and a private task diff preserving the pre-existing Phase 2 work.
The local Git task commit remains deferred until required live acceptance is
complete; no existing staged or unstaged work was discarded.

### Earlier investigation history

- Phase 1: implemented; validation evidence tracked in `docs/phase-1.md`.
- Phase 2: in progress. Native protocol discovery, an owned Rust wl_shm capture
  spike with embedded FFmpeg/VA-API encoding, and external comparison tooling
  are implemented. Evidence/limits are in `docs/backend-decision.md`. No backend
  selected; a first real 1080p capture finalizes and decodes, and the user accepted
  its motion, colors and cursor. Controlled comparisons remain open.
- Phases 3–8: not started.
- User-session continuation: protocol advertisement and synthetic VA-API H.264
  encoding passed. The owned ten-second capture produced 287 matching decoded
  H.264 frames; actual acquisition was about 28.66 FPS, below the requested 30.
- FFmpeg build prerequisites: installed by the user in additions-only DNF
  transaction 32. Offline feature build and synthetic codec tests now pass.
- The first 60-FPS attempt was interrupted at about 41.6 seconds and finalized
  1861 matching decoded frames at about 44.73 FPS. Capture/encode overlap with
  one queued frame is now implemented and tested synthetically; its live effect
  and visual behavior remain unverified.
- The first overlap attempt failed after 191 frames with a retryable Wayland
  `WouldBlock` result wrongly treated as fatal. Socket read/flush retry handling
  is corrected with regression tests. A subsequent full minute completed without
  interruption/error and decoded all 1809 reported frames, at about 30.15 FPS
  rather than the requested 60. This validates that run's completion, not 60-FPS
  performance or latest visual quality.
- User confirmed the selected monitor is at 60.008 Hz, scale 1. The ten-second
  wlr source-only probe acquired 287 frames at 28.687 FPS, with no encoding and
  mean acquisition latency 34.816 ms. This locates a limit in acquisition for
  that run; it does not identify a compositor or driver fault.
- An explicit ext-image-copy-capture SHM backend is implemented for comparison,
  retaining wlr as the default. Its first live source-only run completed with
  120 frames at 11.951 FPS: median acquisition 32.572 ms, p95 397.749 ms,
  maximum 499.887 ms. The user confirmed scrolling the conversation during the
  run; it must not be dismissed as an idle-screen measurement. Manual scrolling
  does not establish a controlled 60-Hz damage workload or backend ranking.
- The user installed mesa-libgbm-devel and upgraded six Mesa runtime packages to
  26.2.5-1.fc44. A separate dmabuf-spike feature now implements a report-only
  ext DMA-BUF source probe with GBM allocation, device matching, explicit
  format/modifier negotiation and one reused GPU buffer. Native/feature checks
  pass. The first live DMA-BUF probe imported/reused a three-plane XRGB8888
  buffer and acquired 398 frames at 39.80 FPS without error. Capture median
  17.018 ms/p95 43.242 ms; this proves delivery, not image correctness or 60 FPS.
- An explicit --unpaced probe mode now removes the client's FPS-cap sleep while
  retaining one outstanding frame and duration/cancellation checks. Default
  pacing is unchanged. The first unpaced run acquired 478 frames at 47.897 FPS,
  without error; median acquisition 17.449 ms, p95 35.604 ms. Sustained 60 FPS
  remains unproven, and manual scrolling does not isolate the cap's effect.
- A local continuous-motion/text fixture now displays browser callback rate for
  repeatable source tests. Its first unpaced run completed with 327 frames at
  32.750 FPS. A user screenshot shows browser callbacks at 60.0 FPS and largest
  interval 16.7 ms at that moment; it does not measure every source presentation.
- An explicit --linear probe option now restricts allocation to negotiated
  modifier zero, with no layout/SHM fallback. The prior three-plane layout remains
  the default driver-selected choice. Linear import succeeded with one plane:
  368 frames/36.799 FPS in the first run, whose animation was subsequently
  reported absent; 361 frames/36.146 FPS with the animation running in the next.
  The user reports browser rates sometimes below 10, and a steady 60 FPS observed
  after capture stops. Neither run proves sustained 60 FPS or a layout ranking.
- The fixture now retains timestamped callback samples and its lowest sampled
  rate until manual reset/reload. This makes capture-period behavior inspectable
  afterwards while preserving the existing scene, pause and full-screen controls.
- The browser was on workspace 2 and terminal on workspace 1. Keeping workspace
  2 visible for the next linear unpaced run delivered 595 frames at59.648FPS,
  without interruption/error; acquisition median16.653ms, p9518.333ms. This
  supports visibility as the earlier confounder, not a general layout ranking.
- A separate gpu-encode-spike feature now connects linear DMA-BUF import,
  VA-API GPU RGB-to-NV12 conversion and H.264/MKV via spike-gpu. It serializes
  capture/conversion/encode and explicitly synchronizes VPP before RGB buffer
  reuse. CPU capture/encoding paths and source probes remain available.
- The first GPU video completed ten seconds with 555 submitted/decoded frames
  at 55.528 FPS, no decode errors and strictly increasing PTS. The user accepted
  motion, text, colors, orientation and cursor. No audio was expected or present.
  This validates one short recording, not sustained 60 FPS or a backend winner.
- The first unpaced GPU minute completed with 3,582 matching decoded frames at
  59.719 FPS and no interruption/capture/decode error. Packet PTS/DTS and decoded
  PTS increase strictly. This is one of the three required 1080p60 samples;
  that minute's visual acceptance remains pending.
- Three unpaced 1080p60 minute samples now complete: 3,582/3,542/3,586 matching
  decoded frames at 59.719/59.052/59.785 FPS. All finalized without interruption
  or errors and decode fully with increasing PTS. Peak RSS 119.6–119.8 MiB.
  Three paced 1080p30 samples also passed: 1,790/1,789/1,790 matching decoded
  frames at 29.838/29.820/29.834 FPS, with increasing PTS and no decode errors.
  Sample 03 failed on storage quota; sample 04 replaces it. The user visually
  accepted all six completed minute-long recordings.
- GPU spike crop/scale controls are implemented: `--crop X,Y,W,H` in monitor-local
  physical pixels and `--size WIDTHxHEIGHT`, with GPU VPP conversion/scaling,
  checked bounds, even dimensions and exact aspect-ratio validation. The report
  retains source size plus resolved crop/output geometry; the countdown prints
  upscaling and cursor state. Cursor remains on unless `--no-cursor` is given.
  Arbitrary-aspect padding and interactive region selection remain later work.
- The first crop/downscale/cursor-off sample finalized 282 matching decoded
  frames at 28.276 FPS, producing 640×360 H.264 with square pixels. Packet
  PTS/DTS and decoded PTS increase strictly; decoding with the demuxer timebase
  reports no errors. The user accepted the central crop, proportions and absent
  cursor. This verifies one short sample, not sustained 30-FPS performance.
- The full-source downscale/cursor-on sample finalized 293 matching decoded
  frames at 29.292 FPS, producing 1280×720 H.264, SAR 1:1 and DAR 16:9.
  Packet PTS/DTS and decoded PTS increase strictly; full decode is error-free.
  The user accepted the whole-screen view, proportions and visible cursor.
  Both short crop/scale and cursor-mode checks passed; arbitrary-aspect padding,
  other transforms/scales and sustained performance remain unverified.
- Portal continuation: `probe-portal --json` now queries ScreenCast version,
  source types and cursor modes through bounded shell-free busctl calls, without
  creating a session or capture buffers. The user's graphical-terminal run
  passed: version 6, source mask 7 (monitor/window/virtual), cursor mask 3
  (hidden/embedded). No property errors. Capture remains unverified; next portal
  work is a persistent D-Bus session and actual PipeWire buffer acquisition.
  The agent's own user-bus access remains restricted.
- An optional `portal-spike` feature now implements `probe-portal-source`:
  persistent GIO portal requests/session, monitor picker, Unix-FD transfer and
  PipeWire raw RGB SHM buffer delivery with timestamp counters and cleanup.
  It saves no media and performs no encoding/audio/pixel inspection. Offline
  validation is recorded in backend-decision.md; live session/delivery,
  cancellation and resource cleanup acceptance remain pending. Next: run the
  README command from the graphical terminal and share its JSON.
- The first portal source run failed before session start with NoReply.
  The local journal identifies a portal assertion on a missing session token
  during CreateSession. The spike now supplies fresh valid session/request
  tokens, with native argument regression checks and method-labeled errors.
  Live retry and portal service recovery verification remain pending.
- The token-corrected portal run started the session and opened its remote:
  226 valid 1920x1080 buffers, zero rejected buffers, 226 strictly increasing
  PTS samples over 10.004109 seconds (22.4908 delivery FPS). Its final error was
  consistent with intentional pw_stream_destroy emitting UNCONNECTED. The
  teardown callback now ignores only that intentional disconnect, retaining
  unexpected disconnects and genuine errors. Native regression checks pass;
  a clean live completion remains to be verified. Workload visibility during
  this run is unconfirmed, so no backend performance ranking follows.
- The teardown-corrected portal run completed cleanly: session and remote open,
  230 valid 1920x1080 buffers, zero rejected, 230 strictly increasing PTS samples,
  10.018044 seconds and 22.8588 delivery FPS. No interruption, cleanup_ok true,
  error null. This confirms one clean source-only run. Image/cursor correctness,
  interactive cancellation and controlled performance comparisons remain open.
  Next portal investigation: negotiated stream rate and delivery timing, with
  confirmed visible motion; this result alone does not identify the rate limit.
- Portal source diagnostics now retain negotiated frame/max rates, format-event
  count, process/empty-dequeue counts, valid-delivery and PTS interval summaries,
  and main-loop iteration timing. Counters are bounded and capture scheduling is
  unchanged. A new first-to-last arrival rate supplements the original elapsed
  rate. Synthetic checks pass; live negotiated/timing evidence is pending.
- First instrumented portal sample completed cleanly: 292 valid frames at
  29.0763 delivery FPS, no rejected buffers or nonincreasing PTS. Negotiated
  rate 0/1 (variable), max 120/1, one format event. Arrival/PTS means were
  34.392/34.419 ms, while loop iterations peaked at 20.330 ms with none over
  25 ms. No advertised 30-FPS cap or measured long client-loop stall is shown;
  the rate limit's cause remains unidentified. Fixture visibility has still
  not been explicitly confirmed. Controlled source comparison remains next.
- A matched SHM source comparison runner now sequences wlr, ext and portal
  probes with matching duration/FPS/cursor settings, retains raw reports/logs,
  and summarizes first-to-last arrival rates. It defaults to a no-write dry run,
  refuses existing destinations and stops on failed/interrupted/timed-out runs.
  Visibility/picker-monitor confirmation stays explicitly pending. Next: run
  the README comparison with the same visible fixture on the selected monitor;
  inspect browser rates and source timing before altering scheduling. This
  measures source delivery, not GPU encoding or a backend winner.
- First matched SHM comparison completed all three ten-second/60-FPS requests
  cleanly at 1920x1080 with cursor hidden: wlr 369 frames / 36.8958 FPS,
  ext 392 / 39.2183 FPS, portal 292 / 29.1218 FPS (arrival-span rates).
  Portal reports zero rejected buffers/nonincreasing PTS, cleanup success,
  variable rate 0/1 and max 120/1. A 213.638-ms delivery gap aligns with a
  214.213-ms PTS gap; measured client-loop iterations stay below 20.312 ms.
  Correction: the user did not open a browser or the animated workload page
  for these tests. The agent had omitted that setup from its runnable commands
  and incorrectly interpreted the earlier confirmation as workload acceptance.
  Monitor choice was confirmed; continuous-motion workload was not established.
  These remain valid source-delivery measurements of unspecified desktop content,
  not a controlled motion comparison or evidence of a fixed source FPS ceiling.
- Linear unpaced DMA-BUF source follow-up completed: 441 frames at 44.1443 FPS,
  1920x1080, cursor off, modifier zero, one plane and buffer reuse. No capture
  error/interruption or CPU pixel access. Arrival/source timestamps strictly
  increase. Median arrival/source intervals are 16.860/16.804 ms, with maxima
  190.070/190.203 ms; cadence is variable rather than a uniform 44-FPS interval.
  This run does not isolate a transport bottleneck or measure GPU encoding.
  The user used no browser/test page for this run; no browser-rate history
  exists. Actual desktop motion was unspecified. Preserve these raw results,
  but do not diagnose a performance regression or change scheduling from them.
  Any future controlled motion test must include explicit setup instructions.
- Next: investigate overlap/performance and
  portal capture. Historical SHM samples predate the Mesa upgrade and need
  matching conditions for comparison. External recorder installation is deferred.
- No existing desktop configuration or package installation is part of Phase 1.

## Primary references

- [wl-screenrec: Rust/GPU capture architecture](https://github.com/rosalyntg/wl-screenrec)
- [GPU Screen Recorder: CLI baseline and hardware support](https://git.dec05eba.com/gpu-screen-recorder/about/)
- [wf-recorder: VA-API capture comparison](https://github.com/ammen99/wf-recorder)
- [GStreamer Rust bindings](https://github.com/GStreamer/gstreamer-rs)
- [GStreamer VA hardware encoders](https://gstreamer.freedesktop.org/documentation/va/)
- [ScreenCast portal contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
