use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub capture: Capture,
    pub video: Video,
    pub audio: Audio,
    pub output: Output,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capture {
    pub mode: CaptureMode,
    pub monitor: Option<String>,
    pub region: Option<Region>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    #[default]
    Screen,
    Region,
}

/// Logical compositor coordinates, not physical pixels. Phase 3 resolves scale
/// and output transforms before submitting a capture request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Video {
    pub resolution: String,
    pub fps: u16,
    pub cursor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Audio {
    pub system: bool,
    pub microphone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    pub directory: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Native,
    Sized { width: u32, height: u32 },
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            capture: Capture::default(),
            video: Video::default(),
            audio: Audio::default(),
            output: Output::default(),
        }
    }
}

impl Default for Video {
    fn default() -> Self {
        Self {
            resolution: "native".into(),
            fps: 30,
            cursor: true,
        }
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self {
            system: true,
            microphone: true,
        }
    }
}

impl Default for Output {
    fn default() -> Self {
        Self {
            directory: "~/Videos/Hyprrecorder".into(),
        }
    }
}

pub fn parse_resolution(value: &str) -> Result<Resolution, String> {
    if value == "native" {
        return Ok(Resolution::Native);
    }
    let (width, height) = value
        .split_once('x')
        .ok_or("Resolution must be native or WIDTHxHEIGHT")?;
    let width = width
        .parse::<u32>()
        .map_err(|_| "Invalid resolution width")?;
    let height = height
        .parse::<u32>()
        .map_err(|_| "Invalid resolution height")?;
    if !(2..=16384).contains(&width) || !(2..=16384).contains(&height) {
        return Err("Output dimensions must each be 2–16384 pixels".into());
    }
    if width % 2 != 0 || height % 2 != 0 {
        return Err(
            "Explicit output dimensions must be even for the initial 4:2:0 recording profile"
                .into(),
        );
    }
    Ok(Resolution::Sized { width, height })
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let settings: Self = toml::from_str(text).map_err(|e| format!("Invalid settings: {e}"))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!(
                "Unsupported settings version {}; expected 1",
                self.version
            ));
        }
        if !(1..=240).contains(&self.video.fps) {
            return Err(
                "FPS must be 1–240; actual supported rates are checked before recording".into(),
            );
        }
        parse_resolution(&self.video.resolution)?;
        if let Some(monitor) = &self.capture.monitor {
            if monitor.trim().is_empty() || monitor.chars().any(char::is_control) {
                return Err(
                    "Monitor must be a nonempty connector name without control characters".into(),
                );
            }
        }
        if let Some(region) = self.capture.region {
            if self.capture.mode != CaptureMode::Region {
                return Err("A fixed region requires capture.mode = region".into());
            }
            if region.width == 0 || region.height == 0 {
                return Err("Region dimensions must be positive".into());
            }
            // Reject rectangles which cannot be represented in compositor coordinates.
            if i32::try_from(region.width)
                .ok()
                .and_then(|w| region.x.checked_add(w))
                .is_none()
                || i32::try_from(region.height)
                    .ok()
                    .and_then(|h| region.y.checked_add(h))
                    .is_none()
            {
                return Err("Region exceeds compositor coordinate bounds".into());
            }
        }
        if self.output.directory.trim().is_empty()
            || self.output.directory.chars().any(char::is_control)
        {
            return Err(
                "Output directory must be nonempty and contain no control characters".into(),
            );
        }
        if self.output.directory.starts_with('~') && !self.output.directory.starts_with("~/") {
            return Err("Only ~/ home expansion is supported in the output directory".into());
        }
        if !self.output.directory.starts_with("~/")
            && !Path::new(&self.output.directory).is_absolute()
        {
            return Err("Output directory must be absolute or start with ~/".into());
        }
        Ok(())
    }

    pub fn output_directory(&self) -> Result<PathBuf, String> {
        if let Some(rest) = self.output.directory.strip_prefix("~/") {
            let home = std::env::var_os("HOME")
                .ok_or("HOME is unset; use an absolute output directory")?;
            Ok(PathBuf::from(home).join(rest))
        } else {
            Ok(PathBuf::from(&self.output.directory))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_and_defaults_agree() {
        let example = Settings::parse(include_str!("../config.example.toml")).unwrap();
        assert_eq!(
            serde_json::to_value(example).unwrap(),
            serde_json::to_value(Settings::default()).unwrap()
        );
    }

    #[test]
    fn rejects_typos_and_unsupported_versions() {
        for text in [
            "version = 2",
            "[video]\nfsp = 60",
            "[audio]\nmic = true",
            "[capture]\nmode = 'window'",
        ] {
            assert!(Settings::parse(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn rejects_bad_rates_and_sizes() {
        for fps in [0, 241] {
            assert!(Settings::parse(&format!("[video]\nfps = {fps}")).is_err());
        }
        for size in [
            "0x1080",
            "1921x1080",
            "1920x0",
            "20000x1080",
            "1920x1080x2",
            "-2x1080",
        ] {
            assert!(parse_resolution(size).is_err(), "accepted {size}");
        }
        assert_eq!(
            parse_resolution("2560x1440").unwrap(),
            Resolution::Sized {
                width: 2560,
                height: 1440
            }
        );
    }

    #[test]
    fn negative_monitor_coordinates_are_valid_but_overflow_is_not() {
        assert!(Settings::parse("[capture]\nmode = 'region'\nregion = { x = -1920, y = 0, width = 1920, height = 1080 }").is_ok());
        assert!(Settings::parse(
            "[capture]\nmode = 'region'\nregion = { x = 2147483647, y = 0, width = 2, height = 2 }"
        )
        .is_err());
        assert!(
            Settings::parse("[capture]\nregion = { x = 0, y = 0, width = 2, height = 2 }").is_err()
        );
    }

    #[test]
    fn output_paths_are_not_shell_expressions() {
        assert!(Settings::parse("[output]\ndirectory = 'relative/videos'").is_err());
        assert!(Settings::parse("[output]\ndirectory = '~someone/Videos'").is_err());
        let s = Settings::parse("[output]\ndirectory = '/tmp/$(touch do-not-run)'").unwrap();
        assert_eq!(
            s.output_directory().unwrap(),
            PathBuf::from("/tmp/$(touch do-not-run)")
        );
    }
}
