#[cfg(feature = "recorder")]
mod audio;
#[cfg(feature = "capture-spike")]
mod capture_spike;
mod config;
mod diagnostics;
#[cfg(feature = "capture-spike")]
mod embedded_video;
#[cfg(feature = "recorder")]
mod ipc;
#[cfg(feature = "recorder")]
mod monitors;
#[cfg(feature = "recorder")]
mod output;
mod portal_probe;
#[cfg(feature = "portal-spike")]
mod portal_source;
mod process;
#[cfg(feature = "recorder")]
mod recording;
#[cfg(feature = "desktop")]
mod ui;
#[cfg(feature = "gpu-encode-spike")]
mod video_geometry;
mod wayland_probe;

use config::Settings;
use std::{env, path::Path};

const HELP: &str = "hyprrecorder — a focused Wayland screen recorder

Usage:
  hyprrecorder                         Open the desktop window (desktop build)
  hyprrecorder record [--config PATH] [--seconds N] [--countdown 0..30] [--silent]
  hyprrecorder stop                    Request stop and finalization (idempotent)
  hyprrecorder status                  Print current state as JSON
  hyprrecorder ui [--smoke]
  hyprrecorder monitors                List eligible monitors as JSON
  hyprrecorder doctor [--json] [--probe-encoder]
  hyprrecorder probe-wayland --json
  hyprrecorder probe-portal --json
  hyprrecorder probe-portal-source --json [--seconds 10] [--fps 30|60] [--no-cursor]
  hyprrecorder spike --monitor NAME --directory NEW_ABSOLUTE_PATH [--seconds 10] [--fps 30|60] [--no-cursor] [--render-device PATH]
  hyprrecorder probe-source --monitor NAME --directory NEW_ABSOLUTE_PATH [--seconds 10] [--fps 30|60] [--no-cursor]
  hyprrecorder probe-dmabuf --monitor NAME --directory NEW_ABSOLUTE_PATH [--seconds 10] [--fps 30|60] [--unpaced] [--linear] [--no-cursor] [--render-device PATH]
  hyprrecorder spike-gpu --monitor NAME --directory NEW_ABSOLUTE_PATH [--seconds 10] [--fps 30|60] [--unpaced] [--crop X,Y,W,H] [--size WIDTHxHEIGHT] [--no-cursor] [--render-device PATH]
  hyprrecorder config --print-default
  hyprrecorder config --check PATH [--json]
  hyprrecorder --version
  hyprrecorder --build-info             Print compiled feature availability
  hyprrecorder --help

Build with --features desktop for the native window and app-owned recording engine.
The default build provides diagnostics; probes remain experimental.
spike requires the capture-spike build feature; it explicitly captures video only
through a CPU-copy path and embedded VA-API encoder. It is experimental.
spike and probe-source accept --capture-backend wlr|ext (default wlr).
probe-source requires the same feature; it measures capture without encoding,
audio or a saved video. It writes only a diagnostic report.
probe-dmabuf requires dmabuf-spike; it tests ext capture into GBM GPU buffers,
without CPU pixel access, encoding, audio or a saved video.
Its --unpaced option removes the FPS-cap sleep for source timing experiments.
Its --linear option requires an explicitly negotiated linear DMA-BUF modifier.
spike-gpu requires gpu-encode-spike; it tests linear DMA-BUF capture, VA-API GPU
conversion and H.264 recording without audio. Live acceptance is experimental.
doctor checks capabilities without changing settings or installing packages.
probe-wayland queries advertised interfaces without acquiring screen/audio buffers.
probe-portal reads ScreenCast properties through busctl without starting capture.
probe-portal-source requires portal-spike; it opens the portal picker and measures
PipeWire SHM video delivery without encoding, audio or saved media.
--probe-encoder optionally encodes synthetic frames without recording your desktop
or microphone and without saving a video file. It is not a capture benchmark.
config commands validate settings without creating the output directory.
See PLAN.md for implementation phases and completion criteria.
";

fn run(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        #[cfg(feature = "desktop")]
        return ui::run(&[]);
        #[cfg(not(feature = "desktop"))]
        {
            print!("{HELP}");
            return Ok(());
        }
    };
    match command {
        "record" => {
            #[cfg(feature = "recorder")]
            recording::run(&args[1..])?;
            #[cfg(not(feature = "recorder"))]
            return Err("Build with --features recorder or desktop to record".into());
        }
        "status" | "stop" if args.len() == 1 => {
            #[cfg(feature = "recorder")]
            println!(
                "{}",
                serde_json::to_string(&ipc::request(command == "stop")?)
                    .map_err(|e| e.to_string())?
            );
            #[cfg(not(feature = "recorder"))]
            return Err("Build with --features recorder or desktop for recording control".into());
        }
        "monitors" if args.len() == 1 => {
            #[cfg(feature = "recorder")]
            println!(
                "{}",
                serde_json::to_string(&monitors::discover()?).map_err(|e| e.to_string())?
            );
            #[cfg(not(feature = "recorder"))]
            return Err("Build with --features recorder or desktop for monitor selection".into());
        }
        "ui" => {
            #[cfg(feature = "desktop")]
            ui::run(&args[1..])?;
            #[cfg(not(feature = "desktop"))]
            return Err("Build with --features desktop for the window".into());
        }
        "--help" | "-h" if args.len() == 1 => print!("{HELP}"),
        "--build-info" if args.len() == 1 => println!(
            "{}",
            serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"recorder":cfg!(feature="recorder"),"desktop":cfg!(feature="desktop")})
        ),
        "--version" | "-V" if args.len() == 1 => {
            println!("hyprrecorder {}", env!("CARGO_PKG_VERSION"))
        }
        "spike" => {
            #[cfg(feature = "capture-spike")]
            capture_spike::run(&args[1..])?;
            #[cfg(not(feature = "capture-spike"))]
            return Err(
                "Build with --features capture-spike to run the experimental capture path".into(),
            );
        }
        "probe-source" => {
            #[cfg(feature = "capture-spike")]
            capture_spike::probe_source(&args[1..])?;
            #[cfg(not(feature = "capture-spike"))]
            return Err("Build with --features capture-spike to run the source-only probe".into());
        }
        "probe-dmabuf" => {
            #[cfg(feature = "dmabuf-spike")]
            capture_spike::dmabuf_probe::run(&args[1..])?;
            #[cfg(not(feature = "dmabuf-spike"))]
            return Err("Build with --features dmabuf-spike to run the DMA-BUF probe".into());
        }
        "spike-gpu" => {
            #[cfg(feature = "gpu-encode-spike")]
            capture_spike::dmabuf_probe::record_gpu(&args[1..])?;
            #[cfg(not(feature = "gpu-encode-spike"))]
            return Err(
                "Build with --features gpu-encode-spike to run the GPU recording spike".into(),
            );
        }
        "probe-portal-source" => {
            #[cfg(feature = "portal-spike")]
            portal_source::run(&args[1..])?;
            #[cfg(not(feature = "portal-spike"))]
            return Err("Build with --features portal-spike to run the portal source probe".into());
        }
        "probe-portal" if args.len() == 2 && args[1] == "--json" => {
            let report = portal_probe::collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
            );
        }
        "probe-wayland" if args.len() == 2 && args[1] == "--json" => {
            let report = wayland_probe::collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
            );
        }
        "__wayland-probe-worker" if args.len() == 1 => {
            let interfaces = wayland_probe::query()?;
            println!(
                "{}",
                serde_json::to_string(&interfaces).map_err(|e| e.to_string())?
            );
        }
        "doctor" => {
            let mut json = false;
            let mut probe_encoder = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--json" if !json => json = true,
                    "--probe-encoder" if !probe_encoder => probe_encoder = true,
                    _ => return Err(format!("Unknown or repeated doctor option: {arg}")),
                }
            }
            let report = diagnostics::collect(probe_encoder);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                );
            } else {
                report.print();
            }
        }
        "config" if args.len() == 2 && args[1] == "--print-default" => {
            print!(
                "{}",
                toml::to_string_pretty(&Settings::default()).map_err(|e| e.to_string())?
            );
        }
        "config" if (args.len() == 3 || args.len() == 4) && args[1] == "--check" => {
            if args.len() == 4 && args[3] != "--json" {
                return Err(format!("Unknown config option: {}", args[3]));
            }
            let settings = Settings::load(Path::new(&args[2]))?;
            let directory = settings.output_directory()?;
            if args.len() == 4 {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "valid": true,
                        "settings": settings,
                        "resolved_output_directory": directory,
                        "recording_implemented": cfg!(feature="recorder")
                    }))
                    .map_err(|e| e.to_string())?
                );
            } else {
                println!(
                    "Settings are valid. Output: {} (not created).",
                    directory.display()
                );
                println!(
                    "Recording engine in this build: {}.",
                    cfg!(feature = "recorder")
                );
            }
        }
        _ => return Err("Unsupported command or arguments; use --help.".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run(&env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("hyprrecorder: {error}");
        std::process::exit(2);
    }
}
