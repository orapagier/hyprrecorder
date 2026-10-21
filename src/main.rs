mod config;
mod diagnostics;
mod process;

use config::Settings;
use std::{env, path::Path};

const HELP: &str = "hyprrecorder — a focused Wayland screen recorder (Phase 1 foundation)

Usage:
  hyprrecorder doctor [--json] [--probe-encoder]
  hyprrecorder config --print-default
  hyprrecorder config --check PATH [--json]
  hyprrecorder --version
  hyprrecorder --help

Phase 1 does not record screens/audio or provide a graphical interface yet.
doctor checks capabilities without changing settings or installing packages.
--probe-encoder optionally encodes synthetic frames without recording your desktop
or microphone and without saving a video file. It is not a capture benchmark.
config commands validate settings without creating the output directory.
See PLAN.md for implementation phases and completion criteria.
";

fn run(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        print!("{HELP}");
        return Ok(());
    };
    match command {
        "--help" | "-h" if args.len() == 1 => print!("{HELP}"),
        "--version" | "-V" if args.len() == 1 => println!("hyprrecorder {}", env!("CARGO_PKG_VERSION")),
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
                println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
            } else {
                report.print();
            }
        }
        "config" if args.len() == 2 && args[1] == "--print-default" => {
            print!("{}", toml::to_string_pretty(&Settings::default()).map_err(|e| e.to_string())?);
        }
        "config" if (args.len() == 3 || args.len() == 4) && args[1] == "--check" => {
            if args.len() == 4 && args[3] != "--json" {
                return Err(format!("Unknown config option: {}", args[3]));
            }
            let settings = Settings::load(Path::new(&args[2]))?;
            let directory = settings.output_directory()?;
            if args.len() == 4 {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "valid": true,
                    "settings": settings,
                    "resolved_output_directory": directory,
                    "recording_implemented": false
                })).map_err(|e| e.to_string())?);
            } else {
                println!("Settings are valid. Output: {} (not created).", directory.display());
                println!("Recording is not implemented yet.");
            }
        }
        _ => return Err("Unsupported command or arguments; use --help. Recording and the UI are planned for later phases.".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run(&env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("hyprrecorder: {error}");
        std::process::exit(2);
    }
}
