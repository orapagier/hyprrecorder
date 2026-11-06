//! Experimental full-output acquisition. Explicit CPU-copy path, no UI/audio.
use crate::embedded_video::Video;
#[cfg(feature = "dmabuf-spike")]
#[path = "dmabuf_probe.rs"]
pub(crate) mod dmabuf_probe;
#[path = "ext_capture.rs"]
mod ext_capture;
use memmap2::{Mmap, MmapOptions};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd},
        unix::fs::DirBuilderExt,
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc,
    },
    time::{Duration, Instant},
};
use wayland_client::{
    backend::WaylandError,
    delegate_noop,
    protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool},
    Connection, Dispatch, EventQueue, QueueHandle, WEnum,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1 as frame, zwlr_screencopy_manager_v1 as manager,
};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stopped(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
struct Signals {
    interrupt: libc::sighandler_t,
    terminate: libc::sighandler_t,
}
impl Signals {
    fn install() -> Self {
        STOP.store(false, Ordering::Relaxed);
        // Handler only stores to a lock-free atomic; no allocation or I/O.
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

#[derive(Debug)]
struct Options {
    monitor: String,
    directory: PathBuf,
    seconds: u32,
    fps: u32,
    cursor: bool,
    device: String,
    ext: bool,
}
impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            monitor: String::new(),
            directory: PathBuf::new(),
            seconds: 10,
            fps: 30,
            cursor: true,
            device: "/dev/dri/renderD128".into(),
            ext: false,
        };
        let mut seen = std::collections::HashSet::new();
        let mut args = args.iter();
        while let Some(key) = args.next() {
            if !seen.insert(key) {
                return Err(format!("Repeated spike option: {key}"));
            }
            if key == "--no-cursor" {
                options.cursor = false;
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| format!("Missing value for {key}"))?;
            match key.as_str() {
                "--monitor" => options.monitor = value.clone(),
                "--directory" => options.directory = value.into(),
                "--seconds" => options.seconds = value.parse().map_err(|_| "Invalid seconds")?,
                "--fps" => options.fps = value.parse().map_err(|_| "Invalid FPS")?,
                "--render-device" => options.device = value.clone(),
                "--capture-backend" => {
                    options.ext = match value.as_str() {
                        "wlr" => false,
                        "ext" => true,
                        _ => return Err("Capture backend must be wlr or ext".into()),
                    }
                }
                _ => return Err(format!("Unknown spike option: {key}")),
            }
        }
        if options.monitor.is_empty()
            || !options.directory.is_absolute()
            || !(1..=300).contains(&options.seconds)
            || ![30, 60].contains(&options.fps)
            || !options.device.starts_with('/')
        {
            return Err("spike needs --monitor NAME --directory NEW_ABSOLUTE_PATH, 1–300 seconds, 30/60 FPS and an absolute render-device path".into());
        }
        Ok(options)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    width: u32,
    height: u32,
    stride: u32,
    format: wl_shm::Format,
}
impl Layout {
    fn bytes(self) -> Result<usize, String> {
        if !matches!(
            self.format,
            wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888
        ) || !(2..=8192).contains(&self.width)
            || !(2..=8192).contains(&self.height)
            || self.width % 2 != 0
            || self.height % 2 != 0
            || self.stride < self.width * 4
        {
            return Err("Spike only supports even 2–8192 pixel XRGB8888/ARGB8888 frames; no silent format conversion or padding".into());
        }
        let bytes = (self.stride as usize)
            .checked_mul(self.height as usize)
            .ok_or("Capture buffer size overflow")?;
        if bytes > 256 * 1024 * 1024 {
            return Err("Capture buffer exceeds spike's 256 MiB limit".into());
        }
        Ok(bytes)
    }
}

struct Buffer {
    proxy: Option<wl_buffer::WlBuffer>,
    pixels: Mmap,
    _file: File,
}
impl Drop for Buffer {
    fn drop(&mut self) {
        if let Some(proxy) = &self.proxy {
            proxy.destroy();
        }
    }
}

fn buffer(layout: Layout, shm: &wl_shm::WlShm, qh: &QueueHandle<State>) -> Result<Buffer, String> {
    let bytes = layout.bytes()?;
    // Anonymous memory only; no screenshots or raw frames are written to disk.
    let fd = unsafe { libc::memfd_create(c"hyprrecorder-spike".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // Ownership of this newly created descriptor transfers exactly once.
    let file = unsafe { File::from_raw_fd(fd) };
    // Extra tail padding also permits SIMD reads at the final row boundary.
    file.set_len((bytes + 64) as u64)
        .map_err(|e| e.to_string())?;
    let pool = shm.create_pool(file.as_fd(), (bytes + 64) as i32, qh, ());
    let proxy = pool.create_buffer(
        0,
        layout.width as i32,
        layout.height as i32,
        layout.stride as i32,
        layout.format,
        qh,
        (),
    );
    pool.destroy();
    // Read only after Ready. Every copy gets a distinct memfd so a new capture
    // cannot overwrite pixels still owned by the encoder thread.
    let pixels = unsafe { MmapOptions::new().map(&file).map_err(|e| e.to_string())? };
    Ok(Buffer {
        proxy: Some(proxy),
        pixels,
        _file: file,
    })
}

#[derive(Default)]
struct Output {
    proxy: Option<wl_output::WlOutput>,
    name: String,
    width: i32,
    height: i32,
    refresh_millihz: i32,
    scale: i32,
    normal: bool,
}
#[derive(Default)]
struct State {
    #[cfg(feature = "dmabuf-spike")]
    gpu: dmabuf_probe::State,
    ext: ext_capture::State,
    outputs: BTreeMap<u32, Output>,
    manager: Option<manager::ZwlrScreencopyManagerV1>,
    shm: Option<wl_shm::WlShm>,
    selected: Option<u32>,
    active: bool,
    layout: Option<Layout>,
    buffer: Option<Buffer>,
    ready: Option<u64>,
    inverted: bool,
    error: Option<String>,
}
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_output" if version >= 4 => {
                    let output = registry.bind::<wl_output::WlOutput, _, _>(name, 4, qh, name);
                    state.outputs.insert(
                        name,
                        Output {
                            proxy: Some(output),
                            scale: 1,
                            ..Output::default()
                        },
                    );
                }
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                #[cfg(feature = "dmabuf-spike")]
                "zwp_linux_dmabuf_v1" if version >= 3 => {
                    state.gpu.manager = Some(registry.bind(name, 3, qh, ()))
                }
                "ext_image_copy_capture_manager_v1" => {
                    state.ext.manager = Some(registry.bind(name, 1, qh, ()))
                }
                "ext_output_image_capture_source_manager_v1" => {
                    state.ext.source_manager = Some(registry.bind(name, 1, qh, ()))
                }
                "zwlr_screencopy_manager_v1" if version >= 3 => {
                    state.manager = Some(registry.bind(name, 3, qh, ()))
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.remove(&name);
                if state.selected == Some(name) {
                    state.error = Some("Selected output disappeared".into());
                }
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(id) else {
            return;
        };
        let before = (
            output.width,
            output.height,
            output.refresh_millihz,
            output.scale,
            output.normal,
        );
        match event {
            wl_output::Event::Name { name } => output.name = name,
            wl_output::Event::Geometry { transform, .. } => {
                output.normal = transform == WEnum::Value(wl_output::Transform::Normal)
            }
            wl_output::Event::Scale { factor } => output.scale = factor,
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                refresh,
            } if flags.contains(wl_output::Mode::Current) => {
                output.width = width;
                output.height = height;
                output.refresh_millihz = refresh;
            }
            _ => {}
        }
        if state.active
            && state.selected == Some(*id)
            && before
                != (
                    output.width,
                    output.height,
                    output.refresh_millihz,
                    output.scale,
                    output.normal,
                )
        {
            state.error =
                Some("Selected output mode/scale/transform changed; stopping safely".into());
        }
    }
}
impl Dispatch<frame::ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        frame: &frame::ZwlrScreencopyFrameV1,
        event: frame::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            frame::Event::Buffer {
                format: WEnum::Value(format),
                width,
                height,
                stride,
            } => {
                state.layout = Some(Layout {
                    width,
                    height,
                    stride,
                    format,
                })
            }
            frame::Event::BufferDone => {
                let result = state
                    .layout
                    .ok_or("No supported shared-memory buffer offered".into())
                    .and_then(|layout| {
                        buffer(layout, state.shm.as_ref().ok_or("wl_shm unavailable")?, qh)
                    });
                match result {
                    Ok(buffer) => {
                        if let Some(proxy) = &buffer.proxy {
                            frame.copy(proxy);
                        }
                        state.buffer = Some(buffer);
                    }
                    Err(error) => state.error = Some(error),
                }
            }
            frame::Event::Flags {
                flags: WEnum::Value(flags),
            } => state.inverted = flags.contains(frame::Flags::YInvert),
            frame::Event::Ready {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
            } => {
                let seconds = ((tv_sec_hi as u64) << 32) | tv_sec_lo as u64;
                state.ready = seconds
                    .checked_mul(1_000_000_000)
                    .and_then(|s| s.checked_add(tv_nsec as u64));
                if tv_nsec >= 1_000_000_000 || state.ready.is_none() {
                    state.error = Some("Invalid presentation timestamp".into());
                }
            }
            frame::Event::Failed => {
                state.error = Some("Compositor refused or lost the capture source".into())
            }
            _ => {}
        }
    }
}
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore manager::ZwlrScreencopyManagerV1);

fn nonblocking<T>(result: Result<T, WaylandError>, operation: &str) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(format!("Wayland {operation} failed: {error}")),
    }
}

// One nonblocking I/O iteration. Keep the read guard alive across polling; when
// only writes are ready, dropping the unused read closure cancels its guard.
// Closures let regression tests replay socket backpressure without a compositor.
fn io_step(
    flush: impl FnOnce() -> Result<(), WaylandError>,
    poll: impl FnOnce(libc::c_short) -> std::io::Result<libc::c_short>,
    read: impl FnOnce() -> Result<usize, WaylandError>,
) -> Result<(), String> {
    let blocked_write = nonblocking(flush(), "flush")?.is_none();
    let events = libc::POLLIN | if blocked_write { libc::POLLOUT } else { 0 };
    let ready = match poll(events) {
        Ok(ready) => ready,
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => return Ok(()),
        Err(error) => return Err(format!("Wayland poll failed: {error}")),
    };
    if ready & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Err(format!(
            "Wayland socket disconnected or invalid (poll flags {ready:#x})"
        ));
    }
    if ready & libc::POLLIN != 0 {
        // Housekeeping-only messages and readiness races may yield WouldBlock.
        // The outer loop dispatches pending events and retains its deadline.
        nonblocking(read(), "read")?;
    }
    Ok(())
}

fn wait(
    queue: &mut EventQueue<State>,
    state: &mut State,
    connection: &Connection,
    until: impl Fn(&State) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    if wait_until(queue, state, connection, until, deadline)? {
        Ok(())
    } else {
        Err("Wayland source did not respond within three seconds".into())
    }
}

// ext capture may legitimately wait for damage on an unchanged source.
// Its recording deadline ends that wait without classifying idle as failure.
fn wait_until(
    queue: &mut EventQueue<State>,
    state: &mut State,
    connection: &Connection,
    until: impl Fn(&State) -> bool,
    deadline: Instant,
) -> Result<bool, String> {
    loop {
        queue.dispatch_pending(state).map_err(|e| e.to_string())?;
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if STOP.load(Ordering::Relaxed) {
            return Err("Capture cancelled".into());
        }
        if until(state) {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        io_step(
            || connection.flush(),
            |events| {
                let mut poll = libc::pollfd {
                    fd: connection.as_fd().as_raw_fd(),
                    events,
                    revents: 0,
                };
                let timeout = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .clamp(1, 100) as i32;
                let result = unsafe { libc::poll(&mut poll, 1, timeout) };
                if result < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(poll.revents)
                }
            },
            || guard.read(),
        )?;
    }
}

// Sync through the same bounded event loop instead of an unbounded roundtrip.
impl Dispatch<wayland_client::protocol::wl_callback::WlCallback, std::sync::Arc<AtomicBool>>
    for State
{
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_callback::WlCallback,
        _: wayland_client::protocol::wl_callback::Event,
        done: &std::sync::Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        done.store(true, Ordering::Relaxed);
    }
}
fn sync(
    queue: &mut EventQueue<State>,
    state: &mut State,
    connection: &Connection,
) -> Result<(), String> {
    let done = std::sync::Arc::new(AtomicBool::new(false));
    connection.display().sync(&queue.handle(), done.clone());
    wait(queue, state, connection, |_| done.load(Ordering::Relaxed))
}

#[derive(Serialize)]
struct FrameTiming {
    presentation_ns: u64,
    acquisition_pts_us: u64,
    capture_ms: f64,
    queue_wait_ms: f64,
    encode_ms: f64,
}

fn measured_fps(timings: &[FrameTiming]) -> Option<f64> {
    let first = timings.first()?;
    let last = timings.last()?;
    let span = last
        .acquisition_pts_us
        .checked_sub(first.acquisition_pts_us)?;
    (timings.len() > 1 && span > 0).then(|| (timings.len() - 1) as f64 * 1_000_000.0 / span as f64)
}

fn destination(directory: &Path, source_only: bool) -> Result<Option<File>, String> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(directory)
        .map_err(|e| format!("Need a new sample directory; nothing overwritten: {e}"))?;
    if source_only {
        return Ok(None);
    }
    OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(directory.join("capture.partial.mkv"))
        .map(Some)
        .map_err(|e| e.to_string())
}

struct PendingFrame {
    buffer: Buffer,
    stride: u32,
    inverted: bool,
    timing: FrameTiming,
}

#[derive(Default)]
struct Encoded {
    timings: Vec<FrameTiming>,
    finalized: bool,
    error: Option<String>,
}

struct Encoder {
    sender: Option<SyncSender<PendingFrame>>,
    finalize: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<Encoded>>,
}

impl Encoder {
    fn start(
        open: impl FnOnce() -> Result<Video, String> + Send + 'static,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel::<PendingFrame>(1);
        let finalize = Arc::new(AtomicBool::new(false));
        let completion = finalize.clone();
        let worker = std::thread::Builder::new()
            .name("hyprrecorder-encoder".into())
            .spawn(move || {
                let mut result = Encoded::default();
                let encoded = (|| -> Result<(), String> {
                    // Native codec/fd state is created, used and freed on this thread.
                    let mut video = open()?;
                    for mut frame in receiver {
                        let started = Instant::now();
                        video.push(
                            &frame.buffer.pixels,
                            frame.stride,
                            frame.inverted,
                            frame.timing.acquisition_pts_us as i64,
                        )?;
                        frame.timing.encode_ms = started.elapsed().as_secs_f64() * 1000.0;
                        result.timings.push(frame.timing);
                        // The mapped pixels remain alive through synchronous push.
                        // Releasing this buffer cannot affect the next capture's memfd.
                    }
                    if completion.load(Ordering::Acquire) {
                        video.finish()?;
                        result.finalized = true;
                    }
                    Ok(())
                })();
                result.error = encoded.err();
                result
            })
            .map_err(|e| format!("Cannot start encoder worker: {e}"))?;
        Ok(Self {
            sender: Some(sender),
            finalize,
            worker: Some(worker),
        })
    }

    fn submit(&self, mut frame: PendingFrame) -> Result<bool, String> {
        let sender = self.sender.as_ref().ok_or("Encoder queue is closed")?;
        let waiting = Instant::now();
        loop {
            frame.timing.queue_wait_ms = waiting.elapsed().as_secs_f64() * 1000.0;
            match sender.try_send(frame) {
                Ok(()) => return Ok(true),
                Err(TrySendError::Disconnected(_)) => {
                    return Err("Encoder worker stopped; partial retained".into())
                }
                Err(TrySendError::Full(returned)) => frame = returned,
            }
            if STOP.load(Ordering::Relaxed) {
                return Ok(false);
            }
            // Explicit backpressure: one queued frame, no unbounded accumulation.
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn finish(mut self, finalize: bool) -> Result<Encoded, String> {
        self.finalize.store(finalize, Ordering::Release);
        self.sender.take(); // Drain queued frames, then let the worker finalize.
        self.worker
            .take()
            .ok_or("Encoder worker unavailable")?
            .join()
            .map_err(|_| "Encoder worker panicked; partial retained".into())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn usage() -> (f64, f64, i64) {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // libc initializes this entire value on successful getrusage.
    unsafe {
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return (0.0, 0.0, 0);
        }
        let usage = usage.assume_init();
        (
            usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1_000_000.0,
            usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1_000_000.0,
            usage.ru_maxrss,
        )
    }
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    mode: &'static str,
    backend: &'static str,
    pipeline: &'static str,
    queued_frame_capacity: u32,
    encoder: &'static str,
    target_bitrate_bits_per_second: u32,
    gop_frames: u32,
    audio: bool,
    cursor_requested: bool,
    requested_fps: u32,
    requested_seconds: u32,
    monitor_refresh_hz: Option<f64>,
    actual_acquisition_fps: Option<f64>,
    frames: u64,
    first_presentation_ns: Option<u64>,
    last_presentation_ns: Option<u64>,
    source_width: u32,
    source_height: u32,
    source_stride: u32,
    elapsed_seconds: f64,
    cpu_user_seconds: f64,
    cpu_system_seconds: f64,
    process_peak_rss_kib: i64,
    late_iterations: u64,
    media_bytes: u64,
    timings: Vec<FrameTiming>,
    finalized: bool,
    completed: bool,
    interrupted: bool,
    visual_acceptance: &'static str,
    error: Option<String>,
}

pub fn run(args: &[String]) -> Result<(), String> {
    execute(args, false)
}

pub fn probe_source(args: &[String]) -> Result<(), String> {
    execute(args, true)
}

fn execute(args: &[String], source_only: bool) -> Result<(), String> {
    if source_only && args.iter().any(|argument| argument == "--render-device") {
        return Err(
            "probe-source does not use an encoder render device; omit --render-device".into(),
        );
    }
    let options = Options::parse(args)?;
    let _signals = Signals::install();
    let connection = Connection::connect_to_env()
        .map_err(|e| format!("Cannot reach the session Wayland display: {e}"))?;
    let mut queue = connection.new_event_queue::<State>();
    let mut state = State::default();
    connection.display().get_registry(&queue.handle(), ());
    sync(&mut queue, &mut state, &connection)?;
    sync(&mut queue, &mut state, &connection)?;
    let (&id, output) = state
        .outputs
        .iter()
        .find(|(_, output)| output.name == options.monitor)
        .ok_or("Monitor not found; spike requires wl_output v4 names")?;
    if output.scale != 1 || !output.normal {
        return Err("Initial spike requires an unrotated output at integer scale 1".into());
    }
    let monitor_refresh_hz =
        (output.refresh_millihz > 0).then(|| output.refresh_millihz as f64 / 1000.0);
    let output = output.proxy.as_ref().ok_or("Output disappeared")?.clone();
    let manager = if options.ext {
        None
    } else {
        Some(
            state
                .manager
                .as_ref()
                .ok_or("Initial spike requires wlr-screencopy v3; no capture fallback")?
                .clone(),
        )
    };
    if state.shm.is_none() {
        return Err("Compositor does not advertise wl_shm".into());
    }
    if !source_only {
        // The source-only probe never initializes a GPU encoder/device.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&options.device)
            .map_err(|e| format!("Cannot open selected render device: {e}"))?;
    }
    let mut file = destination(&options.directory, source_only)?;
    let partial = options.directory.join("capture.partial.mkv");
    state.selected = Some(id);
    state.active = true;
    if source_only {
        eprintln!("Source-only probe: shared-memory acquisition; no encoding, audio or video file.\nCapture starts in 3 seconds; hide this terminal. Ctrl+C stops the probe.");
    } else {
        eprintln!("CPU-copy spike: shared-memory capture + CPU conversion + VA-API upload. No audio.\nCapture starts in 3 seconds; hide this terminal. Ctrl+C stops and finalizes.");
    }
    let countdown = Instant::now() + Duration::from_secs(3);
    while Instant::now() < countdown && !STOP.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let started = Instant::now();
    let initial_usage = usage();
    let mut report = Report {
        schema_version: 1,
        mode: if source_only {
            "source_only"
        } else {
            "recording"
        },
        backend: match (options.ext, source_only) {
            (true, true) => "ext-shm",
            (true, false) => "ext-shm-cpu-convert-gpu-upload",
            (false, true) => "wlr-shm",
            (false, false) => "wlr-shm-cpu-convert-gpu-upload",
        },
        pipeline: if source_only {
            "source-only"
        } else {
            "bounded-capture-encode-overlap"
        },
        queued_frame_capacity: if source_only { 0 } else { 1 },
        encoder: if source_only { "none" } else { "h264_vaapi" },
        target_bitrate_bits_per_second: if source_only { 0 } else { 12_000_000 },
        gop_frames: if source_only { 0 } else { options.fps * 2 },
        audio: false,
        cursor_requested: options.cursor,
        requested_fps: options.fps,
        requested_seconds: options.seconds,
        monitor_refresh_hz,
        actual_acquisition_fps: None,
        frames: 0,
        first_presentation_ns: None,
        last_presentation_ns: None,
        source_width: 0,
        source_height: 0,
        source_stride: 0,
        elapsed_seconds: 0.0,
        cpu_user_seconds: 0.0,
        cpu_system_seconds: 0.0,
        process_peak_rss_kib: 0,
        late_iterations: 0,
        media_bytes: 0,
        timings: Vec::new(),
        finalized: false,
        completed: false,
        interrupted: false,
        visual_acceptance: if source_only {
            "not_applicable"
        } else {
            "unverified"
        },
        error: None,
    };
    let mut encoder = None;
    let mut initial_layout = None;
    let mut first_acquisition = None;
    let mut result = (|| -> Result<(), String> {
        let session = if options.ext {
            let session =
                ext_capture::Session::start(&state, &output, options.cursor, &queue.handle())?;
            wait(
                &mut queue,
                &mut state,
                &connection,
                ext_capture::Session::negotiated,
            )?;
            Some(session)
        } else {
            None
        };
        let deadline = started + Duration::from_secs(options.seconds as u64);
        while started.elapsed() < Duration::from_secs(options.seconds as u64)
            && !STOP.load(Ordering::Relaxed)
        {
            let tick = Instant::now();
            state.layout = None;
            state.ready = None;
            state.inverted = false;
            state.buffer = None;
            let copied = if let Some(session) = &session {
                let frame = session.capture(&mut state, &queue.handle())?;
                let copied = wait_until(
                    &mut queue,
                    &mut state,
                    &connection,
                    |state| state.ready.is_some(),
                    deadline,
                );
                // The protocol permits only one frame object per session.
                // Release it before another request, independent of encoder ownership.
                drop(frame);
                copied
            } else {
                let frame = manager.as_ref().ok_or("No wlr manager")?.capture_output(
                    options.cursor as i32,
                    &output,
                    &queue.handle(),
                    (),
                );
                let copied = wait(&mut queue, &mut state, &connection, |state| {
                    state.ready.is_some()
                });
                frame.destroy();
                copied.map(|()| true)
            };
            if STOP.load(Ordering::Relaxed) {
                break;
            }
            if !copied? {
                break;
            } // An idle ext source reached the requested duration.
            let acquired = Instant::now();
            let layout = state.layout.ok_or("No frame layout")?;
            let timestamp = state.ready.ok_or("No frame timestamp")?;
            if let Some(expected) = initial_layout {
                if layout != expected {
                    return Err("Capture dimensions/format changed; partial retained".into());
                }
            } else {
                layout.bytes()?;
                initial_layout = Some(layout);
                report.source_width = layout.width;
                report.source_height = layout.height;
                report.source_stride = layout.stride;
                report.first_presentation_ns = Some(timestamp);
                first_acquisition = Some(acquired);
                if !source_only {
                    let file = file.take().ok_or("Output file already consumed")?;
                    let device = options.device.clone();
                    let fps = options.fps;
                    encoder = Some(Encoder::start(move || {
                        Video::open(file, layout.width, layout.height, fps, &device)
                    })?);
                }
            }
            if report
                .last_presentation_ns
                .is_some_and(|last| timestamp < last)
            {
                return Err("Compositor presentation timestamp moved backwards".into());
            }
            // A static compositor image may retain its presentation timestamp.
            // Video PTS use monotonic acquisition time; source times are retained
            // separately so duplicate presentations do not invent higher FPS.
            let elapsed = acquired
                .duration_since(first_acquisition.ok_or("No acquisition clock")?)
                .as_micros() as u64;
            let frame = PendingFrame {
                buffer: state.buffer.take().ok_or("No copied buffer")?,
                stride: layout.stride,
                inverted: state.inverted,
                timing: FrameTiming {
                    presentation_ns: timestamp,
                    acquisition_pts_us: elapsed,
                    capture_ms: acquired.duration_since(tick).as_secs_f64() * 1000.0,
                    queue_wait_ms: 0.0,
                    encode_ms: 0.0,
                },
            };
            if source_only {
                report.timings.push(frame.timing);
                report.frames += 1;
                drop(frame.buffer);
            } else if !encoder.as_ref().ok_or("No encoder")?.submit(frame)? {
                break;
            }
            report.last_presentation_ns = Some(timestamp);
            // Acquisition and encoding overlap without copying mapped pixels.
            // Backpressure lowers actual FPS once the single queue slot is full.
            let interval = Duration::from_secs_f64(1.0 / options.fps as f64);
            if let Some(rest) = interval.checked_sub(tick.elapsed()) {
                std::thread::sleep(rest);
            } else {
                report.late_iterations += 1;
            }
        }
        Ok(())
    })();
    state.buffer = None;
    if let Some(encoder) = encoder {
        match encoder.finish(result.is_ok()) {
            Ok(encoded) => {
                report.timings = encoded.timings;
                report.frames = report.timings.len() as u64;
                report.first_presentation_ns =
                    report.timings.first().map(|timing| timing.presentation_ns);
                report.last_presentation_ns =
                    report.timings.last().map(|timing| timing.presentation_ns);
                if let Some(error) = encoded.error {
                    result = Err(match result {
                        Err(capture) => format!("{capture}; encoder: {error}"),
                        Ok(()) => error,
                    });
                } else if result.is_ok() && (!encoded.finalized || report.frames == 0) {
                    result = Err("No finalized frames; partial retained".into());
                }
            }
            Err(error) => result = Err(error),
        }
    } else if result.is_ok() && (!source_only || report.frames == 0) {
        result = Err(if source_only {
            "No source frames acquired"
        } else {
            "No video frames acquired; no recording saved"
        }
        .into());
    }
    if result.is_ok() && !source_only {
        result = (|| -> Result<(), String> {
            // Publishing via hard link fails atomically if a destination already exists.
            // The source link is removed only after the new destination exists.
            fs::hard_link(&partial, options.directory.join("capture.mkv"))
                .map_err(|e| e.to_string())?;
            fs::remove_file(&partial).map_err(|e| e.to_string())?;
            File::open(&options.directory)
                .and_then(|file| file.sync_all())
                .map_err(|e| e.to_string())?;
            report.finalized = true;
            Ok(())
        })();
    }
    report.interrupted = STOP.load(Ordering::Relaxed);
    report.completed = result.is_ok();
    report.actual_acquisition_fps = measured_fps(&report.timings);
    report.elapsed_seconds = started.elapsed().as_secs_f64();
    let final_usage = usage();
    report.cpu_user_seconds = final_usage.0 - initial_usage.0;
    report.cpu_system_seconds = final_usage.1 - initial_usage.1;
    report.process_peak_rss_kib = final_usage.2;
    report.media_bytes = if source_only {
        0
    } else {
        fs::metadata(if report.finalized {
            options.directory.join("capture.mkv")
        } else {
            partial
        })
        .map(|metadata| metadata.len())
        .unwrap_or(0)
    };
    report.error = result.as_ref().err().cloned();
    let report_file =
        File::create_new(options.directory.join("report.json")).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&report_file, &report).map_err(|e| e.to_string())?;
    report_file.sync_all().map_err(|e| e.to_string())?;
    eprintln!(
        "Evidence: {}",
        options.directory.join("report.json").display()
    );
    result?;
    let fps = report
        .actual_acquisition_fps
        .map(|fps| format!("{fps:.2}"))
        .unwrap_or_else(|| "unavailable".into());
    if source_only {
        println!(
            "Acquired {} source frames at {fps} FPS (requested {}). No video was written.",
            report.frames, options.fps
        );
    } else {
        println!("Finalized {} frames at {fps} acquired FPS (requested {}): {}. Visual acceptance remains unverified.",
            report.frames, options.fps, options.directory.join("capture.mkv").display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    static NEXT_FIXTURE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    #[test]
    fn capture_backend_selection_is_explicit_and_preserves_default() {
        let mut args: Vec<String> = ["--monitor", "eDP-1", "--directory", "/tmp/example"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(!Options::parse(&args).unwrap().ext);
        args.extend(["--capture-backend".into(), "ext".into()]);
        assert!(Options::parse(&args).unwrap().ext);
        *args.last_mut().unwrap() = "wlr".into();
        assert!(!Options::parse(&args).unwrap().ext);
        *args.last_mut().unwrap() = "auto".into();
        assert!(Options::parse(&args).is_err());
    }

    #[test]
    fn source_only_destination_has_no_media_and_never_overwrites() {
        let fixture = Fixture::new();
        let source = fixture.0.join("source");
        assert!(destination(&source, true).unwrap().is_none());
        assert_eq!(fs::read_dir(&source).unwrap().count(), 0);
        assert!(destination(&source, false).is_err());
        assert_eq!(fs::read_dir(&source).unwrap().count(), 0);
        let recording = fixture.0.join("recording");
        let file = destination(&recording, false).unwrap().unwrap();
        assert_eq!(file.metadata().unwrap().len(), 0);
        assert!(recording.join("capture.partial.mkv").exists());
    }

    #[test]
    fn actual_rate_uses_acquisition_timestamps_instead_of_requested_fps() {
        let timing = |acquisition_pts_us| FrameTiming {
            presentation_ns: 0,
            acquisition_pts_us,
            capture_ms: 0.0,
            queue_wait_ms: 0.0,
            encode_ms: 0.0,
        };
        assert_eq!(measured_fps(&[]), None);
        assert_eq!(measured_fps(&[timing(0)]), None);
        assert_eq!(measured_fps(&[timing(10), timing(10)]), None);
        assert_eq!(measured_fps(&[timing(10), timing(0)]), None);
        let frames = [timing(1_000_000), timing(1_016_667), timing(1_060_000)];
        assert!((measured_fps(&frames).unwrap() - 33.333333).abs() < 0.00001);
    }

    #[test]
    fn source_probe_rejects_unused_encoder_options_before_session_access() {
        let error =
            probe_source(&["--render-device".into(), "/dev/dri/renderD128".into()]).unwrap_err();
        assert!(error.contains("omit --render-device"));
    }

    #[test]
    fn nonblocking_write_then_read_retries_reach_the_next_event() {
        let would_block = || WaylandError::Io(std::io::Error::from_raw_os_error(libc::EAGAIN));
        // The outgoing socket is full. Only writability wakes us, so reading
        // would be incorrect. The next outer iteration must retry the flush.
        io_step(
            || Err(would_block()),
            |events| {
                assert_eq!(events, libc::POLLIN | libc::POLLOUT);
                Ok(libc::POLLOUT)
            },
            || panic!("write-only readiness must not trigger a read"),
        )
        .unwrap();
        // A readable socket may contain only internal deletion notifications.
        // Backend.read can then report WouldBlock instead of a queued app event.
        io_step(
            || Ok(()),
            |events| {
                assert_eq!(events, libc::POLLIN);
                Ok(libc::POLLIN)
            },
            || Err(would_block()),
        )
        .unwrap();
        let received = std::cell::Cell::new(false);
        io_step(
            || Ok(()),
            |_| Ok(libc::POLLIN),
            || {
                received.set(true);
                Ok(1)
            },
        )
        .unwrap();
        assert!(received.get());
    }

    #[test]
    fn fatal_socket_and_protocol_errors_are_not_hidden_as_retries() {
        let flush = io_step(
            || Err(WaylandError::Io(std::io::ErrorKind::BrokenPipe.into())),
            |_| panic!("fatal flush must not poll"),
            || panic!("fatal flush must not read"),
        );
        assert!(flush.unwrap_err().contains("flush failed"));
        let protocol = io_step(
            || Ok(()),
            |_| Ok(libc::POLLIN),
            || {
                Err(WaylandError::Protocol(
                    wayland_client::backend::protocol::ProtocolError {
                        code: 1,
                        object_id: 3,
                        object_interface: "test_capture".into(),
                        message: "source rejected".into(),
                    },
                ))
            },
        );
        assert!(protocol.unwrap_err().contains("source rejected"));
        let disconnected = io_step(
            || Ok(()),
            |_| Ok(libc::POLLHUP),
            || panic!("hangup must not wait for another read"),
        );
        assert!(disconnected.unwrap_err().contains("disconnected"));
    }

    #[test]
    fn poll_timeout_and_interruption_leave_read_unattempted() {
        io_step(
            || Ok(()),
            |_| Ok(0),
            || panic!("timeout must cancel the prepared read"),
        )
        .unwrap();
        io_step(
            || Ok(()),
            |_| Err(std::io::ErrorKind::Interrupted.into()),
            || panic!("interruption must return to cancellation/deadline checks"),
        )
        .unwrap();
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "hyprrecorder-queue-test-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn file(&self) -> File {
            File::create_new(self.0.join("partial.mkv")).unwrap()
        }
        fn frame(&self, sequence: u64) -> PendingFrame {
            let path = self.0.join(format!("pixels-{sequence}"));
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            file.set_len(64 * 48 * 4 + 64).unwrap();
            let pixels = unsafe { MmapOptions::new().map(&file).unwrap() };
            PendingFrame {
                buffer: Buffer {
                    proxy: None,
                    pixels,
                    _file: file,
                },
                stride: 256,
                inverted: false,
                timing: FrameTiming {
                    presentation_ns: sequence * 33_333_333,
                    acquisition_pts_us: sequence * 33_333,
                    capture_ms: 1.0,
                    queue_wait_ms: 0.0,
                    encode_ms: 0.0,
                },
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn bounded_worker_drains_in_order_and_finalizes_on_stop() {
        let fixture = Fixture::new();
        let file = fixture.file();
        let (release, gate) = mpsc::channel();
        let encoder = Encoder::start(move || {
            gate.recv().map_err(|e| e.to_string())?;
            Video::software_fixture(file, 64, 48, 30)
        })
        .unwrap();
        encoder
            .sender
            .as_ref()
            .unwrap()
            .try_send(fixture.frame(0))
            .ok()
            .unwrap();
        // Hold initialization so only the single queue slot can accept a frame.
        assert!(matches!(
            encoder.sender.as_ref().unwrap().try_send(fixture.frame(1)),
            Err(TrySendError::Full(_))
        ));
        release.send(()).unwrap();
        assert!(encoder.submit(fixture.frame(2)).unwrap());
        let result = encoder.finish(true).unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.finalized);
        assert_eq!(result.timings.len(), 2);
        assert_eq!(result.timings[0].acquisition_pts_us, 0);
        assert_eq!(result.timings[1].acquisition_pts_us, 66_666);
    }

    #[test]
    fn capture_failure_releases_worker_without_reporting_finalization() {
        let fixture = Fixture::new();
        let file = fixture.file();
        let encoder = Encoder::start(move || Video::software_fixture(file, 64, 48, 30)).unwrap();
        assert!(encoder.submit(fixture.frame(0)).unwrap());
        let result = encoder.finish(false).unwrap();
        assert!(result.error.is_none());
        assert!(!result.finalized);
        assert_eq!(result.timings.len(), 1);
    }

    #[test]
    fn worker_startup_error_surfaces_and_releases_queued_frames() {
        let fixture = Fixture::new();
        let file = fixture.file();
        let encoder = Encoder::start(move || {
            Video::open(file, 64, 48, 30, "/nonexistent/hyprrecorder-render-node")
        })
        .unwrap();
        let _ = encoder.submit(fixture.frame(0));
        let result = encoder.finish(true).unwrap();
        assert!(result.error.is_some());
        assert!(!result.finalized);
        assert!(result.timings.is_empty());
    }

    #[test]
    fn worker_write_error_cannot_report_successful_finalization() {
        let fixture = Fixture::new();
        let file = OpenOptions::new().write(true).open("/dev/full").unwrap();
        let encoder = Encoder::start(move || Video::software_fixture(file, 64, 48, 30)).unwrap();
        let _ = encoder.submit(fixture.frame(0));
        let result = encoder.finish(true).unwrap();
        assert!(result.error.is_some());
        assert!(!result.finalized);
    }

    #[test]
    fn invalid_options_and_buffer_layouts_fail_before_capture() {
        assert!(Options::parse(&[]).is_err());
        let arguments = [
            "--monitor",
            "eDP-1",
            "--directory",
            "/tmp/test",
            "--fps",
            "60",
        ]
        .map(str::to_owned);
        assert_eq!(Options::parse(&arguments).unwrap().fps, 60);
        assert!(
            Options::parse(&[arguments.to_vec(), vec!["--fps".into(), "30".into()]].concat())
                .is_err()
        );
        let mut layout = Layout {
            width: 1920,
            height: 1080,
            stride: 7680,
            format: wl_shm::Format::Xrgb8888,
        };
        assert_eq!(layout.bytes().unwrap(), 1920 * 1080 * 4);
        layout.stride = 1;
        assert!(layout.bytes().is_err());
        layout.stride = u32::MAX;
        assert!(layout.bytes().is_err());
        layout.stride = 7680;
        layout.width = 1919;
        assert!(layout.bytes().is_err());
    }
}
