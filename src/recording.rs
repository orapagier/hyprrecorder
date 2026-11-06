use crate::{
    config::{CaptureMode, Settings},
    ipc::{Instance, Status},
    monitors,
};
use std::path::Path;

pub fn run(args: &[String]) -> Result<(), String> {
    let mut path = None;
    let mut seconds = None;
    let mut countdown = None;
    let mut silent = false;
    let mut args = args.iter();
    let mut seen = std::collections::HashSet::new();
    while let Some(key) = args.next() {
        if !seen.insert(key) {
            return Err(format!("Repeated recording option: {key}"));
        }
        if key == "--silent" {
            silent = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("Missing value for {key}"))?;
        match key.as_str() {
            "--config" => path = Some(value),
            "--seconds" => seconds = Some(value.parse::<u32>().map_err(|_| "Invalid duration")?),
            "--countdown" => {
                countdown = Some(value.parse::<u32>().map_err(|_| "Invalid countdown")?)
            }
            _ => return Err(format!("Unknown recording option: {key}")),
        }
    }
    if countdown.is_some_and(|n| n > 30) || seconds.is_some_and(|s| s == 0 || s > 86400) {
        return Err("Countdown must be 0–30 seconds; duration 1–86400 seconds".into());
    }
    let mut settings = match path {
        Some(p) => Settings::load(Path::new(p))?,
        None => Settings::load_user()?,
    };
    if silent {
        settings.audio.system = false;
        settings.audio.microphone = false;
    }
    settings.validate()?;
    let countdown = countdown.unwrap_or(settings.recording.countdown_seconds);
    let instance = Instance::acquire()?;
    instance.update(Status::Preparing);
    let result = (|| {
        crate::ipc::hide_view()?;
        if settings.capture.mode == CaptureMode::Region && settings.capture.region.is_none() {
            return Err(
                "Select a region in the window or supply capture.region in settings".into(),
            );
        }
        let monitor = monitors::select(&settings)?;
        let geometry = monitor.geometry(&settings)?;
        eprintln!(
            "{}×{} output; content {}×{}, padding {},{}; {} FPS, cursor {}{}",
            geometry.output_width,
            geometry.output_height,
            geometry.content_width,
            geometry.content_height,
            geometry.pad_x,
            geometry.pad_y,
            settings.video.fps,
            settings.video.cursor,
            if geometry.content_width > geometry.width || geometry.content_height > geometry.height
            {
                " (upscaling)"
            } else {
                ""
            }
        );
        let mut pinned = settings.clone();
        pinned.capture.monitor = Some(monitor.name.clone());
        crate::capture_spike::dmabuf_probe::record_application(
            &pinned, &monitor, geometry, &instance, seconds, countdown,
        )
    })();
    match &result {
        Ok(Some(path)) => {
            instance.update(Status::Saved { path: path.clone() });
            // Notification only after media, audio streams and capture resources
            // are finalized/released. No UI opens during the recording.
            let _ = crate::process::run(
                "notify-send",
                &[
                    "--app-name=Hyprrecorder",
                    "Recording saved",
                    &path.to_string_lossy(),
                ],
                std::time::Duration::from_secs(2),
            );
        }
        Ok(None) => instance.update(Status::Cancelled),
        Err(error) => instance.update(Status::Failed {
            error: error.clone(),
        }),
    }
    result.map(|_| ())
}
