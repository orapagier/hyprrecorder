# First release acceptance gate

Do not tag or advertise a release until these results are recorded and remaining
limits are explicitly accepted. Keep raw recordings and device identities local.
Maintainers are responsible for reproducing reported failures against the locked
source and reviewing native-library updates before releasing new binaries.

## Prepare the actual desktop

1. Use nonprivate screen content and a disposable destination with enough space.
   Build `cargo build --locked --release --features desktop` after reviewing the
   Fedora/Arch build manifests. Run `target/release/hyprrecorder doctor --probe-encoder`.
   A listed encoder alone is insufficient.
2. Run `python3 tools/validate.py --session`. This adds the listener lifecycle,
   competing instance, slow-client and stale-socket test that the sandbox cannot run.
3. Open `tools/capture-workload.html` in a browser **on the monitor being recorded**.
   Start animation, enter full screen, confirm continuous motion and scrolling
   text, and keep that workspace visible throughout each comparison. Record the
   fixture's retained callback history afterwards. Do not substitute an unspecified
   desktop workload or infer visibility from an unrelated confirmation.
4. Copy `config.example.toml` to a disposable absolute path, set the exact monitor
   connector and a new output directory. Do not change personal defaults just for
   a test. For video-only runs set both audio switches false; for audio tests use
   each requested combination. A config path is passed as one argument, including
   paths with spaces.

Example short capture, from the repository after preparing the fixture:

```sh
target/release/hyprrecorder record --config /absolute/path/test.toml --seconds 10
```

The three-second countdown is outside the media interval; hide the terminal and
keep the fixture visible. The desktop window unmaps itself. For manual stop, run
`hyprrecorder stop` from another terminal or your reviewed stop binding; wait for
a saved notification/the view before treating the file as complete. Inspect a
saved file using `ffprobe -v error -show_streams -show_format -of json FILE` and
`ffmpeg -v error -xerror -i FILE -f null -`, then play it.

## Required observations

- [ ] Native/720p, 30/60 FPS, cursor on/off, region and arbitrary-aspect padding:
  correct monitor, orientation, text, color, proportions and cursor. Inspect first,
  last and middle frames for app, selection or stop controls. Check static-screen
  duration and repeats; repeats are not newly acquired frames.
- [ ] Three warmed-up 60-second 1080p30 and 1080p60 runs, plus native size when
  different. Keep workload and quality settings comparable across backends.
  Retain app/helper CPU/RSS, frame/source timing, output bytes and errors. Compare
  external recorders when installed. Power comparisons require comparable AC/
  battery, brightness/workload and an actual measurement tool. The old accepted
  prototype samples do not certify the application pipeline.
- [ ] All four audio combinations with actual default devices. Verify source
  gain/headroom, no unintended playback, muted/missing-source errors, pinning when
  defaults change, unplug/server loss and no streams after stopping. Test speaker
  and headset defaults without changing global routes as part of app behavior.
- [ ] A 30-minute beginning/end clap/beep test under representative load: measure
  A/V offset (target <=80 ms) and drift (target <=40 ms). Retain actual numbers.
  Synthetic PCM decoding does not satisfy this test.
- [ ] Fractional scale, negative output positions, cross-monitor rejection,
  tiny-region errors, Escape and repeated selection/stop. Rotated/flipped and
  odd native-size sources remain unsupported until separately implemented/tested.
- [ ] Two simultaneous CLI launches, repeated UI launch while capturing, CLI
  start with an idle view, view crash/SIGTERM, countdown cancellation, source
  loss, locking, suspend/resume, hotplug and mode/scale changes. No successful
  saved message for failed/partial files; no media resources remain active.
- [ ] Denied destination, filesystem without hard links, quota/full disk,
  interrupted trailer/sync, missing encoder and slow/hung driver. Native calls
  have no proven watchdog; document recovery and ensure partial files survive.
- [ ] Keyboard/pointer flow, visible focus, increased text size, HiDPI, reduced
  motion, malformed preferences and restored last-good preferences. Measure
  startup/idle/recording CPU/RSS, descriptor/thread stability across repeated runs
  and absence of idle capture/redraw work. Do not label unmeasured cost as low.
- [ ] Fresh Fedora and Arch user installations: native Wayland launch, discovery,
  managed upgrade, modified-file protection and uninstall. Retain exact distro,
  toolkit, GPU, driver and library versions. Other GPUs/compositors need separate
  acceptance. Review actual shortcut conflicts before optional integration.

## Optional Hyprshell bindings

Verify both saved Lua and `hyprctl -j binds` before adding any binding. Proposed
open: Super+Shift+R; stop/save: Super+Ctrl+R. Use this machine's existing Lua
binding API and an explicit command. Preserve Alt+Z and all logout/power/screenshot
bindings. Checkpoint checkout and live copies separately, validate Hyprland config,
then reload and test. No binding was installed from the restricted agent session.

## Release artifacts and issue reports

- [ ] Run the validation tool; retain locked release binary SHA256, Rust license
  inventory, native library versions, dynamic dependencies and build configuration.
  Review native-library licensing and codec redistribution constraints before
  publishing. The app's MIT license does not license external libraries/drivers.
- [ ] Choose a maintainer/release owner; record tested compatibility and explicitly
  accepted limits. No automatic GitHub publication or upload is part of installation.
- [ ] Issue report: app/build version, distro/library/GPU-driver versions, steps,
  expected/actual result and redacted error. Attach sample media only by deliberate
  choice; omit personal paths, device identities, microphone/screen contents and
  environment dumps unless specifically needed and reviewed.
- [ ] Recovery: keep `.partial.mkv` and logs, stop the recorder, verify capture
  resources are released, and reopen. Never overwrite old media to retry. Revert
  installation using its verified backup/checkpoint. Preferences and recordings
  are retained by uninstall.
