# Hyprrecorder

A focused Rust screen recorder being built for Hyprland and the Hyprshell desktop.
The intended app provides full-screen/drag-region capture, native or custom output
resolution, FPS and cursor controls, plus default system audio and microphone.

**Current status: Phase 1 foundation. There is no recording engine or GUI yet.**
See [the implementation plan](PLAN.md) for eight phases and their acceptance criteria,
and [Phase 1 validation](docs/phase-1.md) for what has actually been checked.

## Build and run

Rust 1.86+ and Cargo are required. The current foundation uses Serde/JSON/TOML;
it does not require multimedia development libraries yet.

```sh
cargo build --locked
cargo run --locked -- --help
cargo run --locked -- config --print-default
cargo run --locked -- config --check config.example.toml
cargo run --locked -- doctor
cargo run --locked -- doctor --json
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
```

Phase 2 will choose an embedded library-backed Rust capture pipeline after real
hardware measurements. No claim is made that Rust alone makes encoding faster,
or that Hyprrecorder is officially packaged/supported by Fedora or Arch.
