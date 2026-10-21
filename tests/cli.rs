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
    assert_eq!(value["recording_implemented"], false);
}

#[test]
fn unsupported_recording_and_bad_arguments_fail_without_side_effects() {
    for args in [
        vec!["record"],
        vec!["doctor", "--json", "--json"],
        vec!["--version", "unexpected"],
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
