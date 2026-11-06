#![cfg(feature = "recorder")]
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
struct Fixture(PathBuf);
impl Fixture {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("hyprrecorder-{label}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_hyprrecorder"));
        c.env("XDG_RUNTIME_DIR", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env_remove("WAYLAND_SOCKET")
            .env("WAYLAND_DISPLAY", "/nonexistent/hyprrecorder-test");
        c
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn monitor_command_accepts_hyprlands_none_mirror_marker() {
    let f = Fixture::new("monitor-response");
    let helper = f.0.join("hyprctl");
    fs::write(&helper, r#"#!/bin/sh
printf '%s\n' '[{"name":"eDP-1","width":1920,"height":1080,"x":0,"y":0,"scale":1.0,"focused":true,"disabled":false,"mirrorOf":"none"},{"name":"HDMI-A-1","width":1920,"height":1080,"x":1920,"y":0,"scale":1.0,"disabled":false,"mirrorOf":"eDP-1"}]'
"#).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let out = f
        .command()
        .arg("monitors")
        .env("PATH", &f.0)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let monitors: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(monitors.as_array().unwrap().len(), 1);
    assert_eq!(monitors[0]["name"], "eDP-1");
    assert_eq!(monitors[0]["focused"], true);
}
#[test]
fn malformed_preferences_are_preserved_and_never_replaced_by_defaults() {
    let f = Fixture::new("malformed");
    let path = f.0.join("config/hyprrecorder/settings.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"broken = [").unwrap();
    let out = f.command().arg("record").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Invalid settings"));
    assert_eq!(fs::read(path).unwrap(), b"broken = [");
    assert!(!f.0.join("hyprrecorder").exists());
}
#[test]
fn unavailable_session_never_produces_a_saved_event_or_output_directory() {
    let f = Fixture::new("unavailable");
    let destination = f.0.join("never-created");
    let config = f.0.join("settings.toml");
    fs::write(
        &config,
        format!(
            "[output]\ndirectory = {:?}\n[audio]\nsystem=false\nmicrophone=false\n",
            destination.to_str().unwrap()
        ),
    )
    .unwrap();
    let out = f
        .command()
        .args(["record", "--seconds", "1", "--config"])
        .arg(config)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("\"state\":\"saved\""));
    assert!(!destination.exists());
}
#[cfg(feature = "desktop")]
#[test]
fn offscreen_window_smoke_writes_no_preferences_or_media() {
    let f = Fixture::new("ui");
    let out = f
        .command()
        .args(["ui", "--smoke"])
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QPA_PLATFORMTHEME", "")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!f.0.join("config/hyprrecorder/settings.toml").exists());
}
#[test]
fn invalid_duration_countdown_and_unknown_options_fail_before_runtime_writes() {
    let f = Fixture::new("options");
    for args in [
        ["record", "--seconds", "0"],
        ["record", "--countdown", "31"],
        ["record", "--shell", "true"],
    ] {
        let out = f.command().args(args).output().unwrap();
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
    }
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
}
