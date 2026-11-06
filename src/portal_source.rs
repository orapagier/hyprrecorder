//! Explicit source-only portal experiment; Rust owns CLI, signals and reporting.
use serde::Serialize;
use std::{
    ffi::{c_char, CStr},
    sync::atomic::{AtomicBool, Ordering},
};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stopped(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
extern "C" fn should_stop() -> libc::c_int {
    STOP.load(Ordering::Relaxed).into()
}
struct Signals {
    interrupt: libc::sighandler_t,
    terminate: libc::sighandler_t,
}
impl Signals {
    fn install() -> Self {
        STOP.store(false, Ordering::Relaxed);
        // The handler only stores to a lock-free atomic.
        unsafe {
            Self {
                interrupt: libc::signal(libc::SIGINT, stopped as *const () as libc::sighandler_t),
                terminate: libc::signal(libc::SIGTERM, stopped as *const () as libc::sighandler_t),
            }
        }
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.interrupt);
            libc::signal(libc::SIGTERM, self.terminate);
        }
    }
}
#[repr(C)]
#[derive(Default)]
struct Timing {
    count: u64,
    total_ms: f64,
    min_ms: f64,
    max_ms: f64,
    over_25ms: u64,
    over_50ms: u64,
    over_100ms: u64,
}
#[derive(Debug, Serialize)]
struct TimingReport {
    count: u64,
    min_ms: Option<f64>,
    mean_ms: Option<f64>,
    max_ms: Option<f64>,
    over_25ms: u64,
    over_50ms: u64,
    over_100ms: u64,
}
impl Timing {
    fn report(&self) -> TimingReport {
        TimingReport {
            count: self.count,
            min_ms: (self.count > 0).then_some(self.min_ms),
            mean_ms: (self.count > 0).then(|| self.total_ms / self.count as f64),
            max_ms: (self.count > 0).then_some(self.max_ms),
            over_25ms: self.over_25ms,
            over_50ms: self.over_50ms,
            over_100ms: self.over_100ms,
        }
    }
}
#[derive(Serialize)]
struct Rate {
    numerator: u32,
    denominator: u32,
    fps: Option<f64>,
}
impl Rate {
    fn new(numerator: u32, denominator: u32) -> Self {
        Self {
            numerator,
            denominator,
            fps: (denominator > 0).then(|| numerator as f64 / denominator as f64),
        }
    }
}
#[repr(C)]
#[derive(Default)]
struct Native {
    frames: u64,
    rejected: u64,
    pts_samples: u64,
    nonincreasing_pts: u64,
    first_pts: i64,
    last_pts: i64,
    elapsed: f64,
    width: u32,
    height: u32,
    format: u32,
    memory_type: u32,
    session_started: u32,
    remote_opened: u32,
    interrupted: u32,
    cleanup_ok: u32,
    rate_num: u32,
    rate_denom: u32,
    max_rate_num: u32,
    max_rate_denom: u32,
    format_events: u64,
    process_callbacks: u64,
    empty_dequeues: u64,
    delivery_span_seconds: f64,
    delivery_intervals: Timing,
    pts_intervals: Timing,
    loop_iterations: Timing,
    error: ErrorBuffer,
}
#[repr(transparent)]
struct ErrorBuffer([c_char; 512]);
impl Default for ErrorBuffer {
    fn default() -> Self {
        Self([0; 512])
    }
}
extern "C" {
    fn hr_portal_source(
        seconds: u32,
        fps: u32,
        cursor: u32,
        stop: extern "C" fn() -> libc::c_int,
        result: *mut Native,
    ) -> libc::c_int;
}
#[derive(Debug, PartialEq)]
struct Options {
    seconds: u32,
    fps: u32,
    cursor: bool,
}
impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            seconds: 10,
            fps: 30,
            cursor: true,
        };
        let mut seen = std::collections::HashSet::new();
        let mut iter = args.iter();
        while let Some(key) = iter.next() {
            if !seen.insert(key.as_str()) {
                return Err(format!("Repeated portal option: {key}"));
            }
            match key.as_str() {
                "--json" => {}
                "--no-cursor" => options.cursor = false,
                "--seconds" | "--fps" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| format!("Missing value for {key}"))?
                        .parse()
                        .map_err(|_| format!("Invalid value for {key}"))?;
                    if key == "--seconds" {
                        options.seconds = value;
                    } else {
                        options.fps = value;
                    }
                }
                _ => return Err(format!("Unknown portal source option: {key}")),
            }
        }
        if !seen.contains("--json")
            || !(1..=60).contains(&options.seconds)
            || ![30, 60].contains(&options.fps)
        {
            return Err("probe-portal-source requires --json, 1–60 seconds and 30/60 FPS".into());
        }
        Ok(options)
    }
}
#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    backend: &'static str,
    requested_seconds: u32,
    requested_fps: u32,
    cursor_requested: &'static str,
    session_started: bool,
    remote_opened: bool,
    interrupted: bool,
    cleanup_ok: bool,
    valid_frames: u64,
    rejected_buffers: u64,
    elapsed_seconds: f64,
    delivery_fps: Option<f64>,
    delivery_span_seconds: f64,
    delivery_span_fps: Option<f64>,
    negotiated_rate: Rate,
    negotiated_max_rate: Rate,
    format_events: u64,
    process_callbacks: u64,
    empty_dequeues: u64,
    delivery_intervals: TimingReport,
    pts_intervals: TimingReport,
    loop_iterations: TimingReport,
    width: u32,
    height: u32,
    spa_video_format: u32,
    spa_memory_type: u32,
    pts_samples: u64,
    first_pts_ns: Option<i64>,
    last_pts_ns: Option<i64>,
    nonincreasing_pts: u64,
    buffer_delivery_verified: bool,
    image_correctness_verified: bool,
    error: Option<&'a str>,
    detail: &'static str,
}
pub fn run(args: &[String]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let _signals = Signals::install();
    eprintln!("Portal source probe: choose one monitor in the picker. Ctrl+C cancels. No audio or saved media.");
    let mut native = Native::default();
    // Native bridge writes only this repr(C) result and retains no Rust pointers.
    let result = unsafe {
        hr_portal_source(
            options.seconds,
            options.fps,
            options.cursor.into(),
            should_stop,
            &mut native,
        )
    };
    let error = unsafe { CStr::from_ptr(native.error.0.as_ptr()) }.to_string_lossy();
    let report = Report {
        schema_version: 1, backend: "portal-pipewire-shm",
        requested_seconds: options.seconds, requested_fps: options.fps,
        cursor_requested: if options.cursor { "embedded" } else { "hidden" },
        session_started: native.session_started != 0, remote_opened: native.remote_opened != 0,
        interrupted: native.interrupted != 0, cleanup_ok: native.cleanup_ok != 0,
        valid_frames: native.frames, rejected_buffers: native.rejected, elapsed_seconds: native.elapsed,
        delivery_fps: (native.frames > 1 && native.elapsed > 0.0).then(|| (native.frames - 1) as f64 / native.elapsed),
        delivery_span_seconds: native.delivery_span_seconds,
        delivery_span_fps: (native.frames > 1 && native.delivery_span_seconds > 0.0).then(|| (native.frames - 1) as f64 / native.delivery_span_seconds),
        negotiated_rate: Rate::new(native.rate_num, native.rate_denom),
        negotiated_max_rate: Rate::new(native.max_rate_num, native.max_rate_denom),
        format_events: native.format_events, process_callbacks: native.process_callbacks, empty_dequeues: native.empty_dequeues,
        delivery_intervals: native.delivery_intervals.report(), pts_intervals: native.pts_intervals.report(), loop_iterations: native.loop_iterations.report(),
        width: native.width, height: native.height,
        spa_video_format: native.format, spa_memory_type: native.memory_type,
        pts_samples: native.pts_samples, first_pts_ns: (native.pts_samples > 0).then_some(native.first_pts),
        last_pts_ns: (native.pts_samples > 0).then_some(native.last_pts), nonincreasing_pts: native.nonincreasing_pts,
        buffer_delivery_verified: native.frames > 0, image_correctness_verified: false,
        error: if error.is_empty() { None } else { Some(&error) },
        detail: "Source-only SHM delivery; frames are validated and returned without pixel inspection, encoding, audio or saved media. FPS is a negotiated upper request, not a compositor presentation measurement. No GPU/zero-copy or image-quality claim.",
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    if result < 0 {
        Err(if error.is_empty() {
            "Portal cleanup failed".into()
        } else {
            error.into_owned()
        })
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn missing_timing_and_unknown_rates_do_not_invent_measurements() {
        let timing = Timing::default().report();
        assert!(timing.min_ms.is_none() && timing.mean_ms.is_none() && timing.max_ms.is_none());
        assert_eq!(Rate::new(0, 1).fps, Some(0.0));
        assert_eq!(Rate::new(0, 0).fps, None);
        assert_eq!(Rate::new(60000, 1001).fps, Some(60000.0 / 1001.0));
        let timing = Timing {
            count: 2,
            total_ms: 60.0,
            min_ms: 20.0,
            max_ms: 40.0,
            over_25ms: 1,
            ..Timing::default()
        }
        .report();
        assert_eq!(timing.mean_ms, Some(30.0));
        assert_eq!(timing.over_25ms, 1);
    }
    #[test]
    fn requires_explicit_json_and_bounded_options_before_connecting() {
        for args in [
            vec![],
            vec!["--json", "--seconds", "0"],
            vec!["--json", "--seconds", "61"],
            vec!["--json", "--fps", "240"],
            vec!["--json", "--fps"],
            vec!["--json", "--json"],
            vec!["--json", "--no-cursor", "--no-cursor"],
            vec!["--json", "--directory", "/tmp/no"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        assert_eq!(
            parse(&["--json", "--no-cursor", "--fps", "60", "--seconds", "1"]).unwrap(),
            Options {
                seconds: 1,
                fps: 60,
                cursor: false
            }
        );
    }
}
