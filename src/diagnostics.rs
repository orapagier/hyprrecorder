use crate::process;
use serde::Serialize;
use std::{env, fs, path::Path, time::Duration};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Available,
    Unavailable,
    Unverified,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub application_version: &'static str,
    pub phase: u8,
    pub recording_implemented: bool,
    pub checks: Vec<Check>,
}

impl Report {
    fn add(&mut self, name: &str, status: Status, detail: impl Into<String>) {
        self.checks.push(Check {
            name: name.into(),
            status,
            detail: detail.into(),
        });
    }

    pub fn print(&self) {
        println!(
            "Hyprrecorder {} — Phase 1 diagnostics",
            self.application_version
        );
        println!("Recording is not implemented yet. Available tools do not establish capture readiness.\n");
        for check in &self.checks {
            let label = match check.status {
                Status::Available => "available",
                Status::Unavailable => "unavailable",
                Status::Unverified => "unverified",
            };
            println!("[{label}] {}: {}", check.name, check.detail);
        }
    }
}

fn compact(text: &str) -> String {
    text.lines()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(400)
        .collect()
}

fn failure_detail(result: &process::Probe) -> String {
    let detail = if result.stderr.trim().is_empty() {
        compact(&result.stdout)
    } else {
        compact(&result.stderr)
    };
    if detail.is_empty() {
        "Command returned a nonzero exit status without diagnostic output".into()
    } else {
        detail
    }
}

fn command(report: &mut Report, name: &str, program: &str, args: &[&str]) {
    match process::run(program, args, Duration::from_secs(3)) {
        Ok(result) if result.success => {
            report.add(name, Status::Available, compact(&result.stdout))
        }
        Ok(result) => report.add(name, Status::Unverified, failure_detail(&result)),
        Err(error) => report.add(name, Status::Unverified, error),
    }
}

fn has_encoder(list: &str, name: &str) -> bool {
    list.lines()
        .any(|line| line.split_whitespace().nth(1) == Some(name))
}

pub fn collect(probe_encoder: bool) -> Report {
    let mut report = Report {
        schema_version: 1,
        application_version: env!("CARGO_PKG_VERSION"),
        phase: 1,
        recording_implemented: false,
        checks: Vec::new(),
    };
    let wayland = env::var_os("WAYLAND_DISPLAY").is_some();
    report.add(
        "wayland_environment",
        if wayland {
            Status::Available
        } else {
            Status::Unverified
        },
        if wayland {
            "WAYLAND_DISPLAY is set; connection and capture protocol support are not tested"
        } else {
            "WAYLAND_DISPLAY is unset; run diagnostics from the graphical session"
        },
    );

    match process::run("hyprctl", &["-j", "monitors"], Duration::from_secs(3)) {
        Ok(result) if result.success => {
            match serde_json::from_str::<Vec<serde_json::Value>>(&result.stdout) {
                Ok(monitors) if !monitors.is_empty() => {
                    let screens: Vec<_> = monitors
                        .iter()
                        .map(|m| {
                            format!(
                                "{}: {}x{}, scale {}{}",
                                m["name"].as_str().unwrap_or("unknown"),
                                m["width"],
                                m["height"],
                                m["scale"],
                                if m["focused"].as_bool() == Some(true) {
                                    " (focused)"
                                } else {
                                    ""
                                }
                            )
                        })
                        .collect();
                    report.add("hyprland_monitors", Status::Available, screens.join("; "));
                }
                _ => report.add(
                    "hyprland_monitors",
                    Status::Unverified,
                    "No usable monitor list returned",
                ),
            }
        }
        Ok(result) => report.add(
            "hyprland_monitors",
            Status::Unverified,
            format!("Cannot query session IPC: {}", failure_detail(&result)),
        ),
        Err(error) => report.add("hyprland_monitors", Status::Unverified, error),
    }

    let render_nodes: Vec<_> = fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("renderD"))
        })
        .collect();
    let node = render_nodes.iter().min().cloned();
    if let Some(path) = &node {
        let opened = fs::OpenOptions::new().read(true).write(true).open(path);
        report.add(
            "gpu_render_device",
            if opened.is_ok() {
                Status::Available
            } else {
                Status::Unverified
            },
            match opened {
                Ok(_) => format!(
                    "{} opens; actual encoder support is still unverified",
                    path.display()
                ),
                Err(error) => format!("{}: {error}", path.display()),
            },
        );
    } else {
        report.add(
            "gpu_render_device",
            Status::Unverified,
            "No render device visible; sandbox restrictions do not establish a hardware fault",
        );
    }

    command(
        &mut report,
        "default_system_output",
        "pactl",
        &["get-default-sink"],
    );
    command(
        &mut report,
        "default_microphone",
        "pactl",
        &["get-default-source"],
    );
    match process::run(
        "ffmpeg",
        &["-hide_banner", "-encoders"],
        Duration::from_secs(3),
    ) {
        Ok(result) if result.success => {
            let encoders: Vec<_> = ["h264_vaapi", "h264_qsv", "h264_nvenc", "aac", "libopus"]
                .into_iter()
                .filter(|name| has_encoder(&result.stdout, name))
                .collect();
            report.add(
                "ffmpeg_encoder_catalog",
                Status::Available,
                format!(
                    "Listed: {}. This proves build support only, not working hardware",
                    encoders.join(", ")
                ),
            );
        }
        Ok(result) => report.add(
            "ffmpeg_encoder_catalog",
            Status::Unverified,
            compact(&result.stderr),
        ),
        Err(error) => report.add("ffmpeg_encoder_catalog", Status::Unverified, error),
    }

    for (name, package) in [
        ("ffmpeg_development", "libavcodec"),
        ("pipewire_development", "libpipewire-0.3"),
        ("gstreamer_development", "gstreamer-1.0"),
    ] {
        match process::run("pkg-config", &["--modversion", package], Duration::from_secs(3)) {
            Ok(result) if result.success => report.add(name, Status::Available, compact(&result.stdout)),
            Ok(_) => report.add(name, Status::Unavailable,
                format!("{package} not found by pkg-config; future backend builds may need development packages")),
            Err(error) => report.add(name, Status::Unverified, error),
        }
    }

    if probe_encoder {
        if let Some(path) = node {
            probe_vaapi(&mut report, &path);
        } else {
            report.add(
                "vaapi_synthetic_encode",
                Status::Unverified,
                "Skipped: no visible render device",
            );
        }
    } else {
        report.add("vaapi_synthetic_encode", Status::Unverified,
            "Not requested; --probe-encoder encodes 30 synthetic frames, captures no desktop/audio, and saves no file");
    }
    report
}

fn probe_vaapi(report: &mut Report, node: &Path) {
    let path = node.to_string_lossy();
    // Deliberately synthetic: no display, audio source, or output file is opened.
    let args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-vaapi_device",
        &path,
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=1280x720:r=30",
        "-vf",
        "format=nv12,hwupload",
        "-frames:v",
        "30",
        "-an",
        "-c:v",
        "h264_vaapi",
        "-f",
        "null",
        "-",
    ];
    match process::run("ffmpeg", &args, Duration::from_secs(10)) {
        Ok(result) if result.success => report.add("vaapi_synthetic_encode", Status::Available,
            format!("Encoded 30 synthetic H.264 frames through {}; this does not test screen capture, scaling, quality, or audio", node.display())),
        Ok(result) => report.add("vaapi_synthetic_encode", Status::Unverified, compact(&result.stderr)),
        Err(error) => report.add("vaapi_synthetic_encode", Status::Unverified, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_detection_requires_an_exact_catalog_entry() {
        let catalog = " V....D h264_vaapi VAAPI H.264\n A..... aac AAC\n";
        assert!(has_encoder(catalog, "h264_vaapi"));
        assert!(!has_encoder(catalog, "h264"));
        assert!(!has_encoder(
            "Description mentions h264_vaapi",
            "h264_vaapi"
        ));
    }

    #[test]
    fn errors_written_to_stdout_remain_actionable() {
        let result = process::Probe {
            success: false,
            stdout: "Couldn't set socket timeout (2)".into(),
            stderr: String::new(),
        };
        assert!(failure_detail(&result).contains("socket timeout"));
    }
}
