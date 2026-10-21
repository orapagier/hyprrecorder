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

- Phase 1: implemented; validation evidence tracked in `docs/phase-1.md`.
- Phases 2–8: not started.
- Next: satisfy Phase 2's build prerequisites, run the first real capture spike
  in an accessible graphical session, then record the backend decision.
- No existing desktop configuration or package installation is part of Phase 1.

## Primary references

- [wl-screenrec: Rust/GPU capture architecture](https://github.com/rosalyntg/wl-screenrec)
- [GPU Screen Recorder: CLI baseline and hardware support](https://git.dec05eba.com/gpu-screen-recorder/about/)
- [wf-recorder: VA-API capture comparison](https://github.com/ammen99/wf-recorder)
- [GStreamer Rust bindings](https://github.com/GStreamer/gstreamer-rs)
- [GStreamer VA hardware encoders](https://gstreamer.freedesktop.org/documentation/va/)
- [ScreenCast portal contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
