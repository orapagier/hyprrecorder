use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hyprrecorder"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn public_config_contract_preserves_capture_and_audio_defaults() {
    let output = cli(&["config", "--check", "config.example.toml", "--json"]);
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["settings"]["video"]["resolution"], "native");
    assert_eq!(value["settings"]["audio"]["system"], true);
    assert_eq!(value["settings"]["audio"]["microphone"], true);
    assert_eq!(value["recording_implemented"], cfg!(feature = "recorder"));
}

#[test]
fn unsupported_recording_and_bad_arguments_fail_without_side_effects() {
    for args in [
        vec!["record", "--invalid"],
        vec!["doctor", "--json", "--json"],
        vec!["--version", "unexpected"],
        vec!["probe-portal"],
        vec!["probe-portal", "--json", "--json"],
        vec![
            "config",
            "--check",
            "/nonexistent/hyprrecorder-settings.toml",
        ],
    ] {
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn printed_defaults_are_valid_toml() {
    let output = cli(&["config", "--print-default"]);
    assert!(output.status.success());
    let parsed: toml::Value = toml::from_str(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
    assert_eq!(parsed["video"]["fps"].as_integer(), Some(30));
}

#[test]
fn portal_probe_reports_inaccessible_bus_without_claiming_capture() {
    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_hyprrecorder"))
        .args(["probe-portal", "--json"])
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/hyprrecorder-test-bus",
        )
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["screencast_interface_reachable"], false);
    assert_eq!(report["capture_verified"], false);
    for property in report["properties"].as_array().unwrap() {
        assert!(property["value"].is_null());
        assert!(!property["error"].as_str().unwrap().is_empty());
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(11));
}

#[test]
fn portal_source_requires_feature_or_reports_bus_failure_without_claiming_frames() {
    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_hyprrecorder"))
        .args(["probe-portal-source", "--json", "--seconds", "1"])
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/hyprrecorder-test-bus",
        )
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    #[cfg(feature = "portal-spike")]
    {
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["session_started"], false);
        assert_eq!(report["remote_opened"], false);
        assert_eq!(report["valid_frames"], 0);
        assert_eq!(report["buffer_delivery_verified"], false);
        assert_eq!(report["image_correctness_verified"], false);
        assert_eq!(report["cleanup_ok"], true);
        assert!(!report["error"].as_str().unwrap().is_empty());
    }
    #[cfg(not(feature = "portal-spike"))]
    assert!(String::from_utf8_lossy(&output.stderr).contains("--features portal-spike"));
    assert!(started.elapsed() < std::time::Duration::from_secs(7));
}

#[test]
fn wayland_probe_reports_inaccessible_session_without_claiming_capture() {
    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_hyprrecorder"))
        .args(["probe-wayland", "--json"])
        .env_remove("WAYLAND_SOCKET")
        .env("WAYLAND_DISPLAY", "/nonexistent/hyprrecorder-test-display")
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["session_reachable"], false);
    assert_eq!(report["capture_verified"], false);
    assert!(report["interfaces"].as_array().unwrap().is_empty());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn dmabuf_probe_requires_its_feature_or_an_accessible_session_without_creating_files() {
    let directory = std::env::temp_dir().join(format!(
        "hyprrecorder-no-dmabuf-session-{}",
        std::process::id()
    ));
    assert!(!directory.exists());
    let output = Command::new(env!("CARGO_BIN_EXE_hyprrecorder"))
        .args([
            "probe-dmabuf",
            "--monitor",
            "eDP-1",
            "--directory",
            directory.to_str().unwrap(),
        ])
        .env_remove("WAYLAND_SOCKET")
        .env("WAYLAND_DISPLAY", "/nonexistent/hyprrecorder-test-display")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr);
    #[cfg(feature = "dmabuf-spike")]
    assert!(error.contains("Cannot reach the session Wayland display"));
    #[cfg(not(feature = "dmabuf-spike"))]
    assert!(error.contains("--features dmabuf-spike"));
    assert!(!directory.exists());
}

#[test]
fn gpu_recording_spike_requires_feature_or_session_before_output_creation() {
    let directory = std::env::temp_dir().join(format!(
        "hyprrecorder-no-gpu-session-{}",
        std::process::id()
    ));
    assert!(!directory.exists());
    let output = Command::new(env!("CARGO_BIN_EXE_hyprrecorder"))
        .args([
            "spike-gpu",
            "--monitor",
            "eDP-1",
            "--directory",
            directory.to_str().unwrap(),
        ])
        .env_remove("WAYLAND_SOCKET")
        .env("WAYLAND_DISPLAY", "/nonexistent/hyprrecorder-test-display")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr);
    #[cfg(feature = "gpu-encode-spike")]
    assert!(error.contains("Cannot reach the session Wayland display"));
    #[cfg(not(feature = "gpu-encode-spike"))]
    assert!(error.contains("--features gpu-encode-spike"));
    assert!(!directory.exists());
}

#[cfg(feature = "gpu-encode-spike")]
#[test]
fn invalid_gpu_geometry_fails_before_session_or_output_creation() {
    let directory = std::env::temp_dir().join(format!(
        "hyprrecorder-invalid-geometry-{}",
        std::process::id()
    ));
    assert!(!directory.exists());
    let output = Command::new(env!("CARGO_BIN_EXE_hyprrecorder"))
        .args([
            "spike-gpu",
            "--monitor",
            "eDP-1",
            "--directory",
            directory.to_str().unwrap(),
            "--crop",
            "0,0,1280,720",
            "--size",
            "640x480",
        ])
        .env_remove("WAYLAND_SOCKET")
        .env("WAYLAND_DISPLAY", "/nonexistent/hyprrecorder-test-display")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("aspect ratio"));
    assert!(!directory.exists());
}

#[test]
fn build_info_distinguishes_diagnostic_and_desktop_installations() {
    let output = cli(&["--build-info"]);
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["recorder"], cfg!(feature = "recorder"));
    assert_eq!(value["desktop"], cfg!(feature = "desktop"));
}
