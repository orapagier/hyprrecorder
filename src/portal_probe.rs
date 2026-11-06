//! Property-only portal diagnostics: never create a capture session or stream.
use crate::process;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Serialize)]
pub struct Property {
    name: &'static str,
    value: Option<u32>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    schema_version: u32,
    screencast_interface_reachable: bool,
    properties: Vec<Property>,
    source_types: Vec<&'static str>,
    cursor_modes: Vec<&'static str>,
    capture_verified: bool,
    detail: &'static str,
}

fn parse_uint(text: &str) -> Result<u32, String> {
    let mut words = text.split_whitespace();
    if words.next() != Some("u") {
        return Err("Expected a D-Bus uint32 property".into());
    }
    let value = words
        .next()
        .ok_or("Missing uint32 property value")?
        .parse::<u32>()
        .map_err(|_| "Invalid uint32 property value")?;
    if words.next().is_some() {
        return Err("Unexpected data after uint32 property".into());
    }
    Ok(value)
}

fn property(name: &'static str) -> Property {
    let result = process::run(
        "busctl",
        &[
            "--user",
            "--timeout=2",
            "--no-pager",
            "get-property",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.ScreenCast",
            name,
        ],
        Duration::from_secs(3),
    )
    .and_then(|probe| {
        if probe.success {
            parse_uint(&probe.stdout)
        } else {
            Err(format!(
                "Portal property query failed: {}",
                probe.stderr.trim()
            ))
        }
    });
    match result {
        Ok(value) => Property {
            name,
            value: Some(value),
            error: None,
        },
        Err(error) => Property {
            name,
            value: None,
            error: Some(error),
        },
    }
}

fn labels(value: Option<u32>, names: [&'static str; 3]) -> Vec<&'static str> {
    names
        .into_iter()
        .enumerate()
        .filter_map(|(bit, name)| value.filter(|v| v & (1 << bit) != 0).map(|_| name))
        .collect()
}

pub fn collect() -> Report {
    let properties: Vec<_> = ["version", "AvailableSourceTypes", "AvailableCursorModes"]
        .into_iter()
        .map(property)
        .collect();
    Report {
        schema_version: 1,
        screencast_interface_reachable: properties.iter().any(|p| p.value.is_some()),
        source_types: labels(properties[1].value, ["monitor", "window", "virtual"]),
        cursor_modes: labels(properties[2].value, ["hidden", "embedded", "metadata"]),
        properties,
        capture_verified: false,
        detail: "Properties only: no capture session, source picker, PipeWire connection or media buffers. A property error does not diagnose the desktop. Advertisements do not verify capture, cursor behavior, GPU transport or encoding.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_property_parser_rejects_malformed_and_wrong_typed_results() {
        assert_eq!(parse_uint("u 4294967295\n").unwrap(), u32::MAX);
        for text in [
            "",
            "u",
            "u -1",
            "u 4294967296",
            "s 5",
            "u 1 extra",
            "u 1\nu 2",
        ] {
            assert!(parse_uint(text).is_err(), "{text}");
        }
    }

    #[test]
    fn unavailable_is_distinct_from_empty_and_unknown_bits_are_not_known_modes() {
        assert!(labels(None, ["a", "b", "c"]).is_empty());
        assert!(labels(Some(0), ["a", "b", "c"]).is_empty());
        assert_eq!(labels(Some(0b1011), ["a", "b", "c"]), vec!["a", "b"]);
    }
}
