//! Application capture engine. Uses the same negotiated GPU primitives as the
//! measured experiment, with independent lifecycle/output/audio ownership.
use super::*;
use crate::{
    audio::Audio,
    config::Settings,
    ipc::{Instance, Status},
    monitors::Monitor,
    output::Output as RecordingOutput,
    video_geometry::Geometry,
};

pub(crate) fn capture(
    settings: &Settings,
    monitor: &Monitor,
    geometry: Geometry,
    instance: &Instance,
    seconds: Option<u32>,
    countdown: u32,
) -> Result<Option<std::path::PathBuf>, String> {
    let _signals = Signals::install();
    let cancelled = || instance.stop.load(Ordering::Relaxed) || STOP.load(Ordering::Relaxed);
    let connection =
        Connection::connect_to_env().map_err(|e| format!("Cannot reach Wayland display: {e}"))?;
    let mut queue = connection.new_event_queue::<super::super::State>();
    let mut state = super::super::State::default();
    state.gpu.enabled = true;
    connection.display().get_registry(&queue.handle(), ());
    sync(&mut queue, &mut state, &connection)?;
    sync(&mut queue, &mut state, &connection)?;
    let (&id, output) = state
        .outputs
        .iter()
        .find(|(_, o)| o.name == monitor.name)
        .ok_or("Selected monitor disappeared")?;
    if !output.normal {
        return Err("Rotated or flipped capture is unsupported by this build".into());
    }
    let output = output.proxy.clone().ok_or("Selected output disappeared")?;
    state.selected = Some(id);
    state.active = true;
    let session =
        ext_capture::Session::start(&state, &output, settings.video.cursor, &queue.handle())?;
    wait(
        &mut queue,
        &mut state,
        &connection,
        ext_capture::Session::negotiated,
    )?;
    let constraints = state
        .gpu
        .constraints
        .clone()
        .ok_or("Incomplete GPU constraints")?;
    if (constraints.width, constraints.height) != (monitor.width, monitor.height) {
        return Err("Monitor geometry changed during preparation; select it again".into());
    }
    let device = Device::open(constraints.device, None)?;
    let mut allocation = None;
    let mut failures = Vec::new();
    for (format, modifiers) in candidates(&constraints, &state.gpu.imports, Some(0)) {
        match device.allocate(&constraints, format, &modifiers) {
            Ok(b) => {
                allocation = Some(b);
                break;
            }
            Err(e) => failures.push(e),
        }
    }
    let backing = allocation.ok_or_else(|| {
        format!(
            "No shared linear GPU buffer; no software fallback. {}",
            failures.join("; ")
        )
    })?;
    if backing.desc.planes != 1 || backing.desc.modifier != 0 {
        return Err("Encoder needs a single-plane linear GPU buffer".into());
    }
    let imported = backing.import(&mut state, &mut queue, &connection)?;
    if session_locked()? {
        return Err("Session is locked; unlock it before recording".into());
    }
    let mut audio = Audio::open(&settings.audio)?;
    if cancelled() {
        return Ok(None);
    }
    let (destination, file) = RecordingOutput::create(&settings.output_directory()?)?;
    let result = (|| -> Result<bool, String> {
        let input = crate::embedded_video::GpuInput {
            fd: unsafe { BorrowedFd::borrow_raw(backing.desc.fds[0]) },
            width: constraints.width,
            height: constraints.height,
            format: backing.desc.format,
            pitch: backing.desc.strides[0],
            offset: backing.desc.offsets[0],
            modifier: 0,
            geometry,
        };
        let mut video = Video::open_gpu_with_audio(
            file,
            input,
            u32::from(settings.video.fps),
            &device.node(),
            audio.is_some(),
        )?;
        instance.update(Status::Countdown);
        let until = Instant::now() + Duration::from_secs(countdown as u64);
        while Instant::now() < until {
            if cancelled() {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if cancelled() {
            return Ok(false);
        }
        // A single CLOCK_MONOTONIC epoch covers audio and video, including the
        // first acquisition's delay. Sources remain corked through countdown.
        let epoch = crate::audio::now_us();
        if let Some(audio) = &mut audio {
            audio.begin(epoch)?;
        }
        instance.update(Status::Recording { frames: 0 });
        let begin = Instant::now();
        let deadline = seconds.map(|n| begin + Duration::from_secs(n as u64));
        let mut frames = 0u64;
        let mut pending = None;
        let mut last_progress = Instant::now();
        let mut last_pts = -1i64;
        let mut next_tick = Instant::now();
        let mut last_monitor_check = Instant::now();
        let boot_offset = boot_us() - crate::audio::now_us();
        loop {
            if cancelled() || deadline.is_some_and(|t| Instant::now() >= t) {
                break;
            }
            if (boot_us() - crate::audio::now_us() - boot_offset).abs() > 500_000 {
                return Err("System suspended during recording; partial retained".into());
            }
            if last_monitor_check.elapsed() >= Duration::from_secs(1) {
                if session_locked()? {
                    return Err("Session locked during recording; partial retained".into());
                }
                let current = crate::monitors::select(settings)?;
                if current.name != monitor.name
                    || current.width != monitor.width
                    || current.height != monitor.height
                    || current.x != monitor.x
                    || current.y != monitor.y
                    || current.scale != monitor.scale
                    || current.transform != monitor.transform
                {
                    return Err("Selected monitor changed; partial retained".into());
                }
                last_monitor_check = Instant::now();
            }
            if pending.is_none() && state.ready.is_none() {
                pending = Some(session.capture_into(&mut state, &imported.0, &queue.handle())?);
            }
            if pending.is_some() {
                // A pending RGB copy and a repeat of the last converted NV12
                // frame use distinct GPU storage. No access to in-flight RGB.
                let mut until = Instant::now() + Duration::from_millis(30);
                if frames > 0 {
                    until = until.min(next_tick);
                }
                if let Some(end) = deadline {
                    until = until.min(end);
                }
                let ready = wait_until(
                    &mut queue,
                    &mut state,
                    &connection,
                    |s| s.ready.is_some(),
                    until,
                );
                if cancelled() {
                    break;
                }
                if ready? {
                    drop(pending.take());
                }
            }
            if Instant::now() >= next_tick && (frames > 0 || state.ready.is_some()) {
                let pts = crate::audio::now_us() - epoch;
                if pts <= last_pts {
                    return Err("Capture clock moved backwards".into());
                }
                if state.ready.take().is_some() {
                    video.push_gpu(pts)?;
                } else {
                    video.repeat_gpu(pts)?;
                }
                frames += 1;
                last_pts = pts;
                next_tick += Duration::from_secs_f64(1.0 / f64::from(settings.video.fps));
                // Drop scheduling debt; never produce an unbounded catch-up burst.
                if next_tick < Instant::now() {
                    next_tick = Instant::now();
                }
            } else if pending.is_none() {
                std::thread::sleep(Duration::from_millis(2));
            }
            if frames == 0 && begin.elapsed() > Duration::from_secs(3) {
                return Err("Source produced no initial frame within three seconds".into());
            }
            if let Some(audio) = &mut audio {
                audio.drain(&mut video)?;
            }
            if last_progress.elapsed() >= Duration::from_secs(1) {
                instance.update(Status::Recording { frames });
                last_progress = Instant::now();
            }
        }
        drop(pending.take());
        instance.update(Status::Stopping);
        if frames == 0 {
            return Ok(false);
        }
        if let Some(audio) = &mut audio {
            audio.drain(&mut video)?;
        }
        // Set the final packet duration without reading an in-flight RGB
        // buffer. Static-screen recordings extend through the stop instant.
        video.end_at(crate::audio::now_us() - epoch)?;
        video.finish()?;
        Ok(true)
    })();
    drop(audio);
    match result {
        Ok(true) => {
            destination.publish()?;
            Ok(Some(destination.final_path))
        }
        Ok(false) => {
            fs::remove_file(&destination.partial)
                .map_err(|e| format!("Cancelled; cannot remove empty partial: {e}"))?;
            Ok(None)
        }
        Err(error) => Err(format!(
            "{error}. Partial: {}",
            destination.partial.display()
        )),
    }
}
fn boot_us() -> i64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut t);
    }
    t.tv_sec * 1_000_000 + t.tv_nsec / 1000
}

fn session_locked() -> Result<bool, String> {
    let session = std::env::var("XDG_SESSION_ID").unwrap_or_else(|_| "self".into());
    let result = crate::process::run(
        "loginctl",
        &["show-session", &session, "--property=LockedHint", "--value"],
        Duration::from_secs(1),
    )?;
    if !result.success {
        return Err(
            "Cannot check session lock state through logind; recording did not continue".into(),
        );
    }
    match result.stdout.trim() {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err("Unrecognized session lock state".into()),
    }
}
