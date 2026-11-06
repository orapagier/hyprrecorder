use crate::{
    config::{parse_resolution, CaptureMode, Region, Resolution, Settings},
    process,
    video_geometry::{Geometry, Request},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Monitor {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    #[serde(default)]
    pub transform: u32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(rename = "mirrorOf", default)]
    pub mirror_of: String,
}
pub fn discover() -> Result<Vec<Monitor>, String> {
    let result = process::run("hyprctl", &["-j", "monitors"], Duration::from_secs(2))?;
    if !result.success {
        return Err("Cannot query Hyprland monitors; launch inside the desktop session".into());
    }
    parse(&result.stdout)
}
pub(crate) fn parse(text: &str) -> Result<Vec<Monitor>, String> {
    let monitors: Vec<Monitor> =
        serde_json::from_str(text).map_err(|e| format!("Invalid monitor response: {e}"))?;
    Ok(monitors
        .into_iter()
        // Hyprland uses "none" for an ordinary, non-mirrored output.
        .filter(|m| !m.disabled && (m.mirror_of.is_empty() || m.mirror_of == "none"))
        .collect())
}
pub fn select(settings: &Settings) -> Result<Monitor, String> {
    let monitors = discover()?;
    monitors
        .iter()
        .find(|m| match &settings.capture.monitor {
            Some(name) => &m.name == name,
            None => m.focused,
        })
        .or_else(|| {
            settings
                .capture
                .monitor
                .is_none()
                .then(|| monitors.first())
                .flatten()
        })
        .cloned()
        .ok_or("Selected monitor is unavailable".into())
}
impl Monitor {
    pub fn geometry(&self, settings: &Settings) -> Result<Geometry, String> {
        if self.transform != 0 || !self.scale.is_finite() || self.scale <= 0.0 {
            return Err(
                "This build supports unrotated outputs; unsupported transform/scale".into(),
            );
        }
        let crop = if settings.capture.mode == CaptureMode::Region {
            let Region {
                x,
                y,
                width,
                height,
            } = settings
                .capture
                .region
                .ok_or("Select a region before recording")?;
            let left = (f64::from(x) - f64::from(self.x)) * self.scale;
            let top = (f64::from(y) - f64::from(self.y)) * self.scale;
            let right = left + f64::from(width) * self.scale;
            let bottom = top + f64::from(height) * self.scale;
            if left < 0.0
                || top < 0.0
                || right > f64::from(self.width) + 0.01
                || bottom > f64::from(self.height) + 0.01
            {
                return Err("Region must stay inside the selected monitor; cross-monitor capture is unsupported".into());
            }
            // Inward rounding, then even crop extents, never captures outside selection.
            let x = left.ceil() as u32;
            let y = top.ceil() as u32;
            let w = (right.floor() as u32).saturating_sub(x) & !1;
            let h = (bottom.floor() as u32).saturating_sub(y) & !1;
            Some([x, y, w, h])
        } else {
            None
        };
        let size = match parse_resolution(&settings.video.resolution)? {
            Resolution::Native => None,
            Resolution::Sized { width, height } => Some([width, height]),
        };
        Request { crop, size }.resolve_fit(self.width, self.height)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hyprland_non_mirrored_outputs_remain_eligible() {
        let base = serde_json::json!({"name":"eDP-1","width":1920,"height":1080,"x":0,"y":0,"scale":1.0,"focused":true,"disabled":false});
        let mut current = base.clone();
        current["mirrorOf"] = "none".into();
        let mut legacy = base.clone();
        legacy["name"] = "DP-1".into();
        legacy["mirrorOf"] = "".into();
        let mut missing = base.clone();
        missing["name"] = "DP-2".into();
        let mut mirrored = base.clone();
        mirrored["name"] = "HDMI-A-1".into();
        mirrored["mirrorOf"] = "eDP-1".into();
        let mut disabled = current.clone();
        disabled["name"] = "DP-3".into();
        disabled["disabled"] = true.into();
        let result =
            parse(&serde_json::json!([current, legacy, missing, mirrored, disabled]).to_string())
                .unwrap();
        assert_eq!(
            result.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["eDP-1", "DP-1", "DP-2"]
        );
        assert!(result[0].focused);
        assert!(parse("[]").unwrap().is_empty());
        assert!(parse("invalid IPC response").is_err());
    }
    #[test]
    fn negative_origin_fractional_region_is_inward_rounded_and_cross_monitor_rejected() {
        let monitor = Monitor {
            name: "test".into(),
            width: 1920,
            height: 1080,
            x: -1280,
            y: 30,
            scale: 1.5,
            transform: 0,
            focused: true,
            disabled: false,
            mirror_of: String::new(),
        };
        let mut s = Settings::default();
        s.capture.mode = CaptureMode::Region;
        s.capture.region = Some(Region {
            x: -1279,
            y: 31,
            width: 101,
            height: 81,
        });
        let g = monitor.geometry(&s).unwrap();
        assert_eq!((g.x, g.y, g.width, g.height), (2, 2, 150, 120));
        s.capture.region.as_mut().unwrap().x = -1281;
        assert!(monitor.geometry(&s).is_err());
    }
}
