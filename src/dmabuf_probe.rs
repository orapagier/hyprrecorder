#[cfg(feature = "recorder")]
#[path = "recorder_gpu.rs"]
mod recorder_gpu;
#[cfg(feature = "recorder")]
pub(crate) use recorder_gpu::capture as record_application;
// GPU transport primitives and retained source/encoding experiments.
use super::*;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    os::fd::BorrowedFd,
    ptr::NonNull,
};
use wayland_protocols::{
    ext::image_copy_capture::v1::client::ext_image_copy_capture_session_v1 as session,
    wp::linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1 as params, zwp_linux_dmabuf_v1 as manager,
    },
};

const XRGB: u32 = u32::from_le_bytes(*b"XR24");
const ARGB: u32 = u32::from_le_bytes(*b"AR24");
const INVALID_MODIFIER: u64 = 0x00ff_ffff_ffff_ffff;

#[derive(Default)]
struct Pending {
    size: Option<(u32, u32)>,
    device: Option<u64>,
    formats: BTreeMap<u32, Vec<u64>>,
}
#[derive(Clone, PartialEq)]
struct Constraints {
    width: u32,
    height: u32,
    device: u64,
    formats: BTreeMap<u32, Vec<u64>>,
}
impl Pending {
    fn finish(mut self) -> Result<Constraints, String> {
        let (width, height) = self.size.ok_or("No DMA-BUF buffer size")?;
        geometry(width, height).bytes()?;
        for modifiers in self.formats.values_mut() {
            modifiers.sort_unstable();
            modifiers.dedup();
        }
        if !self.formats.values().any(|values| !values.is_empty()) {
            return Err("No supported explicit XRGB8888/ARGB8888 DMA-BUF modifiers".into());
        }
        Ok(Constraints {
            width,
            height,
            device: self.device.ok_or("No compositor DMA-BUF device")?,
            formats: self.formats,
        })
    }
}
fn geometry(width: u32, height: u32) -> Layout {
    Layout {
        width,
        height,
        stride: width.saturating_mul(4),
        format: wl_shm::Format::Xrgb8888,
    }
}
fn parse_device(bytes: &[u8]) -> Result<u64, String> {
    // This Linux prototype supports the platform's 64-bit dev_t ABI only.
    if std::mem::size_of::<libc::dev_t>() != 8 {
        return Err("Unsupported dev_t ABI".into());
    }
    Ok(u64::from_ne_bytes(
        bytes
            .try_into()
            .map_err(|_| "Malformed DMA-BUF device ID")?,
    ))
}
fn parse_modifiers(bytes: &[u8]) -> Result<Vec<u64>, String> {
    if bytes.len() % 8 != 0 || bytes.len() > 256 * 8 {
        return Err("Malformed or excessive DMA-BUF modifier array".into());
    }
    Ok(bytes
        .chunks_exact(8)
        .map(|v| u64::from_ne_bytes(v.try_into().expect("eight-byte chunk")))
        .filter(|&value| value != INVALID_MODIFIER)
        .collect())
}
fn candidates(
    constraints: &Constraints,
    imports: &std::collections::BTreeSet<(u32, u64)>,
    requested_modifier: Option<u64>,
) -> Vec<(u32, Vec<u64>)> {
    [XRGB, ARGB]
        .into_iter()
        .filter_map(|format| {
            let modifiers: Vec<_> = constraints
                .formats
                .get(&format)?
                .iter()
                .copied()
                .filter(|modifier| imports.contains(&(format, *modifier)))
                .filter(|modifier| {
                    requested_modifier.is_none_or(|requested| requested == *modifier)
                })
                .collect();
            (!modifiers.is_empty()).then_some((format, modifiers))
        })
        .collect()
}

#[derive(Default)]
pub(super) struct State {
    pub(super) enabled: bool,
    pub(super) manager: Option<manager::ZwpLinuxDmabufV1>,
    imports: std::collections::BTreeSet<(u32, u64)>,
    pending: Pending,
    constraints: Option<Constraints>,
    imported: Option<wl_buffer::WlBuffer>,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(buffer) = self.imported.take() {
            buffer.destroy();
        }
    }
}
pub(super) fn constraints(state: &mut super::State, event: session::Event) {
    let result = (|| -> Result<(), String> {
        match event {
            session::Event::BufferSize { width, height } => {
                state.gpu.pending.size = Some((width, height))
            }
            session::Event::DmabufDevice { device } => {
                state.gpu.pending.device = Some(parse_device(&device)?)
            }
            session::Event::DmabufFormat { format, modifiers }
                if [XRGB, ARGB].contains(&format) =>
            {
                let values = state.gpu.pending.formats.entry(format).or_default();
                let offered = parse_modifiers(&modifiers)?;
                if values.len() + offered.len() > 256 {
                    return Err("Excessive DMA-BUF modifier offers".into());
                }
                values.extend(offered);
            }
            session::Event::Done => {
                let constraints = std::mem::take(&mut state.gpu.pending).finish()?;
                if state
                    .gpu
                    .constraints
                    .as_ref()
                    .is_some_and(|prior| prior != &constraints)
                {
                    return Err("DMA-BUF capture constraints changed; stopping safely".into());
                }
                state.ext.layout = Some(geometry(constraints.width, constraints.height));
                state.gpu.constraints = Some(constraints);
            }
            session::Event::Stopped => return Err("DMA-BUF capture session stopped".into()),
            _ => {}
        }
        Ok(())
    })();
    if let Err(error) = result {
        state.error = Some(error);
    }
}
impl Dispatch<manager::ZwpLinuxDmabufV1, ()> for super::State {
    fn event(
        state: &mut Self,
        _: &manager::ZwpLinuxDmabufV1,
        event: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let manager::Event::Modifier {
            format,
            modifier_hi,
            modifier_lo,
        } = event
        {
            if [XRGB, ARGB].contains(&format) {
                if state.gpu.imports.len() >= 512 {
                    state.error = Some("Excessive DMA-BUF import offers".into());
                    return;
                }
                state.gpu.imports.insert((
                    format,
                    (u64::from(modifier_hi) << 32) | u64::from(modifier_lo),
                ));
            }
        }
    }
}
impl Dispatch<params::ZwpLinuxBufferParamsV1, ()> for super::State {
    fn event(
        state: &mut Self,
        _: &params::ZwpLinuxBufferParamsV1,
        event: params::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            params::Event::Created { buffer } => state.gpu.imported = Some(buffer),
            params::Event::Failed => {
                state.error = Some("Compositor rejected DMA-BUF import; no SHM fallback".into())
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(super::State, params::ZwpLinuxBufferParamsV1,
        [params::EVT_CREATED_OPCODE => (wl_buffer::WlBuffer, ())]);
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Descriptor {
    width: u32,
    height: u32,
    format: u32,
    planes: u32,
    modifier: u64,
    fds: [i32; 4],
    strides: [u32; 4],
    offsets: [u32; 4],
}
unsafe extern "C" {
    fn hr_gpu_open(device: u64, path: *const c_char, error: *mut c_char) -> *mut c_void;
    fn hr_gpu_close(device: *mut c_void);
    fn hr_gpu_node(device: *const c_void) -> *const c_char;
    fn hr_gpu_buffer(
        device: *mut c_void,
        width: u32,
        height: u32,
        format: u32,
        modifiers: *const u64,
        count: u32,
        out: *mut Descriptor,
        error: *mut c_char,
    ) -> *mut c_void;
    fn hr_gpu_free_buffer(buffer: *mut c_void);
}
struct Device(NonNull<c_void>);
impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            hr_gpu_close(self.0.as_ptr());
        }
    }
}
fn native_error(error: &[c_char; 512]) -> String {
    // The bridge always writes a NUL-terminated error to the 512-byte array.
    unsafe {
        CStr::from_ptr(error.as_ptr())
            .to_string_lossy()
            .into_owned()
    }
}
impl Device {
    fn open(device: u64, path: Option<&str>) -> Result<Self, String> {
        let path = path
            .map(CString::new)
            .transpose()
            .map_err(|_| "NUL in render path")?;
        let mut error = [0; 512];
        let ptr = unsafe {
            hr_gpu_open(
                device,
                path.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                error.as_mut_ptr(),
            )
        };
        NonNull::new(ptr)
            .map(Self)
            .ok_or_else(|| native_error(&error))
    }
    fn node(&self) -> String {
        unsafe {
            CStr::from_ptr(hr_gpu_node(self.0.as_ptr()))
                .to_string_lossy()
                .into_owned()
        }
    }
    fn allocate(
        &self,
        constraints: &Constraints,
        format: u32,
        modifiers: &[u64],
    ) -> Result<Backing<'_>, String> {
        let mut error = [0; 512];
        let mut desc = Descriptor::default();
        let ptr = unsafe {
            hr_gpu_buffer(
                self.0.as_ptr(),
                constraints.width,
                constraints.height,
                format,
                modifiers.as_ptr(),
                modifiers.len() as u32,
                &mut desc,
                error.as_mut_ptr(),
            )
        };
        let ptr = NonNull::new(ptr).ok_or_else(|| native_error(&error))?;
        Ok(Backing {
            ptr,
            desc,
            _device: self,
        })
    }
}
struct Backing<'a> {
    ptr: NonNull<c_void>,
    desc: Descriptor,
    _device: &'a Device,
}
impl Drop for Backing<'_> {
    fn drop(&mut self) {
        unsafe {
            hr_gpu_free_buffer(self.ptr.as_ptr());
        }
    }
}
struct Imported(wl_buffer::WlBuffer);
impl Drop for Imported {
    fn drop(&mut self) {
        self.0.destroy();
    }
}
struct Params(params::ZwpLinuxBufferParamsV1);
impl Drop for Params {
    fn drop(&mut self) {
        self.0.destroy();
    }
}
impl Backing<'_> {
    fn import(
        &self,
        state: &mut super::State,
        queue: &mut EventQueue<super::State>,
        connection: &Connection,
    ) -> Result<Imported, String> {
        let manager = state
            .gpu
            .manager
            .as_ref()
            .ok_or("linux-dmabuf v3 unavailable")?;
        let params = Params(manager.create_params(&queue.handle(), ()));
        for i in 0..self.desc.planes as usize {
            // The native backing owns these CLOEXEC fds through import and capture.
            let fd = unsafe { BorrowedFd::borrow_raw(self.desc.fds[i]) };
            params.0.add(
                fd,
                i as u32,
                self.desc.offsets[i],
                self.desc.strides[i],
                (self.desc.modifier >> 32) as u32,
                self.desc.modifier as u32,
            );
        }
        params.0.create(
            self.desc.width as i32,
            self.desc.height as i32,
            self.desc.format,
            params::Flags::empty(),
        );
        wait(queue, state, connection, |state| {
            state.gpu.imported.is_some()
        })?;
        Ok(Imported(
            state.gpu.imported.take().ok_or("No imported DMA-BUF")?,
        ))
    }
}

#[derive(Serialize)]
struct GpuReport {
    #[serde(flatten)]
    measurement: Report,
    cpu_pixel_access: bool,
    buffer_reused: bool,
    render_device: Option<String>,
    compositor_device_id: Option<u64>,
    drm_format_hex: Option<String>,
    modifier_hex: Option<String>,
    plane_strides: Vec<u32>,
    plane_offsets: Vec<u32>,
    setup_seconds: f64,
    capture_offers: Vec<(String, Vec<String>)>,
    import_offers: Vec<(String, String)>,
    pacing: &'static str,
    frame_rate_limit: Option<u32>,
    requested_modifier_hex: Option<String>,
    #[cfg(feature = "gpu-encode-spike")]
    geometry: Option<crate::video_geometry::Geometry>,
}

fn parse_options(args: &[String]) -> Result<(Options, bool, bool), String> {
    if args.iter().any(|arg| arg == "--capture-backend") {
        return Err("probe-dmabuf selects ext DMA-BUF explicitly; omit --capture-backend".into());
    }
    let mut filtered = Vec::new();
    let mut unpaced = false;
    let mut linear = false;
    let mut args = args.iter();
    while let Some(key) = args.next() {
        if key == "--unpaced" || key == "--linear" {
            let selected = if key == "--unpaced" {
                &mut unpaced
            } else {
                &mut linear
            };
            if *selected {
                return Err(format!("Repeated probe option: {key}"));
            }
            *selected = true;
        } else {
            filtered.push(key.clone());
            // Consume values as values, including literal strings matching flags.
            if key != "--no-cursor" {
                if let Some(value) = args.next() {
                    filtered.push(value.clone());
                }
            }
        }
    }
    Ok((Options::parse(&filtered)?, unpaced, linear))
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    execute(args, false)
}

#[cfg(feature = "gpu-encode-spike")]
pub(crate) fn record_gpu(args: &[String]) -> Result<(), String> {
    execute(args, true)
}

#[cfg(feature = "gpu-encode-spike")]
fn recording_options(
    args: &[String],
) -> Result<(Vec<String>, crate::video_geometry::Request), String> {
    use crate::video_geometry::{numbers, Request};
    let mut request = Request::default();
    let mut filtered = Vec::new();
    let mut args = args.iter();
    while let Some(key) = args.next() {
        if key == "--crop" || key == "--size" {
            let value = args
                .next()
                .ok_or_else(|| format!("Missing value for {key}"))?;
            if key == "--crop" {
                if request.crop.is_some() {
                    return Err("Repeated --crop".into());
                }
                request.crop = Some(numbers(value, ',')?);
            } else {
                if request.size.is_some() {
                    return Err("Repeated --size".into());
                }
                request.size = Some(numbers(value, 'x')?);
            }
        } else {
            filtered.push(key.clone());
            if !["--unpaced", "--linear", "--no-cursor"].contains(&key.as_str()) {
                if let Some(value) = args.next() {
                    filtered.push(value.clone());
                }
            }
        }
    }
    // Check known geometry before connecting or writing. With size alone, the
    // source aspect ratio is unknown until negotiation; validate dimensions now.
    if request.crop.is_some() {
        request.resolve(8192, 8192)?;
    } else if let Some([w, h]) = request.size {
        Request {
            crop: Some([0, 0, w, h]),
            size: None,
        }
        .resolve(8192, 8192)?;
    }
    Ok((filtered, request))
}

fn execute(args: &[String], recording: bool) -> Result<(), String> {
    #[cfg(feature = "gpu-encode-spike")]
    let (filtered, geometry_request) = if recording {
        recording_options(args)?
    } else {
        (args.to_vec(), crate::video_geometry::Request::default())
    };
    #[cfg(feature = "gpu-encode-spike")]
    let parse_args = &filtered;
    #[cfg(not(feature = "gpu-encode-spike"))]
    let parse_args = args;
    let (options, unpaced, linear) = parse_options(parse_args)?;
    let requested_modifier = (linear || recording).then_some(0);
    let explicit_device = args.iter().any(|arg| arg == "--render-device");
    let _signals = Signals::install();
    let connection = Connection::connect_to_env()
        .map_err(|e| format!("Cannot reach the session Wayland display: {e}"))?;
    let mut queue = connection.new_event_queue::<super::State>();
    let mut state = super::State::default();
    state.gpu.enabled = true;
    connection.display().get_registry(&queue.handle(), ());
    sync(&mut queue, &mut state, &connection)?;
    sync(&mut queue, &mut state, &connection)?;
    let (&id, output) = state
        .outputs
        .iter()
        .find(|(_, output)| output.name == options.monitor)
        .ok_or("Monitor not found; probe needs wl_output v4 names")?;
    if output.scale != 1 || !output.normal {
        return Err("DMA-BUF probe needs unrotated output at scale 1".into());
    }
    if state.gpu.manager.is_none() {
        return Err("linux-dmabuf v3 unavailable; no SHM fallback".into());
    }
    let refresh = (output.refresh_millihz > 0).then(|| output.refresh_millihz as f64 / 1000.0);
    let output = output.proxy.as_ref().ok_or("Output disappeared")?.clone();
    let mut output_file = destination(&options.directory, !recording)?;
    let partial = options.directory.join("capture.partial.mkv");
    state.selected = Some(id);
    state.active = true;
    let mut report = GpuReport {
        measurement: Report {
            schema_version: 1,
            mode: if recording {
                "recording"
            } else {
                "source_only"
            },
            backend: if recording {
                "ext-dmabuf-vaapi"
            } else {
                "ext-dmabuf-gbm"
            },
            pipeline: if recording {
                "serialized-gpu-convert-encode"
            } else {
                "gpu-source-only"
            },
            queued_frame_capacity: 0,
            encoder: if recording { "h264_vaapi" } else { "none" },
            target_bitrate_bits_per_second: if recording { 12_000_000 } else { 0 },
            gop_frames: if recording { options.fps * 2 } else { 0 },
            audio: false,
            cursor_requested: options.cursor,
            requested_fps: options.fps,
            requested_seconds: options.seconds,
            monitor_refresh_hz: refresh,
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
            visual_acceptance: if recording {
                "unverified"
            } else {
                "not_applicable"
            },
            error: None,
        },
        cpu_pixel_access: false,
        buffer_reused: false,
        render_device: None,
        compositor_device_id: None,
        drm_format_hex: None,
        modifier_hex: None,
        plane_strides: Vec::new(),
        plane_offsets: Vec::new(),
        setup_seconds: 0.0,
        capture_offers: Vec::new(),
        import_offers: Vec::new(),
        pacing: if unpaced {
            "unpaced"
        } else {
            "requested_fps_cap"
        },
        frame_rate_limit: (!unpaced).then_some(options.fps),
        requested_modifier_hex: requested_modifier.map(|value| format!("{value:#018x}")),
        #[cfg(feature = "gpu-encode-spike")]
        geometry: None,
    };
    let setup = Instant::now();
    let mut started = None;
    let initial_usage = usage();
    let result = (|| -> Result<(), String> {
        let session =
            ext_capture::Session::start(&state, &output, options.cursor, &queue.handle())?;
        wait(
            &mut queue,
            &mut state,
            &connection,
            ext_capture::Session::negotiated,
        )?;
        let constraints = state
            .gpu
            .constraints
            .as_ref()
            .ok_or("Incomplete DMA-BUF constraints")?
            .clone();
        #[cfg(feature = "gpu-encode-spike")]
        if recording {
            report.geometry =
                Some(geometry_request.resolve(constraints.width, constraints.height)?);
        }
        report.compositor_device_id = Some(constraints.device);
        report.capture_offers = constraints
            .formats
            .iter()
            .map(|(format, modifiers)| {
                (
                    format!("{format:#010x}"),
                    modifiers.iter().map(|m| format!("{m:#018x}")).collect(),
                )
            })
            .collect();
        report.import_offers = state
            .gpu
            .imports
            .iter()
            .map(|(f, m)| (format!("{f:#010x}"), format!("{m:#018x}")))
            .collect();
        let candidates = candidates(&constraints, &state.gpu.imports, requested_modifier);
        if candidates.is_empty() {
            return Err(if linear || recording {
                "Linear modifier is not shared by capture and linux-dmabuf import; no layout fallback"
            } else {
                "No explicit modifier shared by capture and linux-dmabuf import"
            }.into());
        }
        let device = Device::open(
            constraints.device,
            explicit_device.then_some(options.device.as_str()),
        )?;
        report.render_device = Some(device.node());
        let mut allocated = None;
        let mut errors = Vec::new();
        for (format, modifiers) in candidates {
            match device.allocate(&constraints, format, &modifiers) {
                Ok(buffer) => {
                    allocated = Some(buffer);
                    break;
                }
                Err(error) => errors.push(format!("{format:#x}: {error}")),
            }
        }
        let backing = allocated.ok_or_else(|| {
            format!(
                "No negotiated GBM allocation succeeded: {}",
                errors.join("; ")
            )
        })?;
        report.drm_format_hex = Some(format!("{:#010x}", backing.desc.format));
        report.modifier_hex = Some(format!("{:#018x}", backing.desc.modifier));
        report.plane_strides = backing.desc.strides[..backing.desc.planes as usize].to_vec();
        report.plane_offsets = backing.desc.offsets[..backing.desc.planes as usize].to_vec();
        let imported = backing.import(&mut state, &mut queue, &connection)?;
        if linear {
            eprintln!(
                "Linear DMA-BUF imported: modifier {:#018x}, {} plane(s).",
                backing.desc.modifier, backing.desc.planes
            );
        }
        report.measurement.source_width = constraints.width;
        report.measurement.source_height = constraints.height;
        report.measurement.source_stride = backing.desc.strides[0];
        #[cfg(feature = "gpu-encode-spike")]
        let mut video = if recording {
            if backing.desc.modifier != 0 || backing.desc.planes != 1 {
                return Err(
                    "GPU recording requires a single-plane linear RGB buffer; no layout fallback"
                        .into(),
                );
            }
            use crate::embedded_video::GpuInput;
            let input = GpuInput {
                fd: unsafe { BorrowedFd::borrow_raw(backing.desc.fds[0]) },
                width: backing.desc.width,
                height: backing.desc.height,
                format: backing.desc.format,
                pitch: backing.desc.strides[0],
                offset: backing.desc.offsets[0],
                modifier: backing.desc.modifier,
                geometry: report.geometry.ok_or("No recording geometry")?,
            };
            Some(Video::open_gpu(
                output_file.take().ok_or("No recording output file")?,
                input,
                options.fps,
                &device.node(),
            )?)
        } else {
            None
        };
        #[cfg(not(feature = "gpu-encode-spike"))]
        let _ = &mut output_file;
        report.setup_seconds = setup.elapsed().as_secs_f64();
        #[cfg(feature = "gpu-encode-spike")]
        if let Some(g) = report.geometry {
            eprintln!(
                "Crop {},{} {}×{} → {}×{}{}; cursor {}.",
                g.x,
                g.y,
                g.width,
                g.height,
                g.output_width,
                g.output_height,
                if g.output_width > g.width {
                    " (upscaling)"
                } else {
                    ""
                },
                if options.cursor { "on" } else { "off" }
            );
        }
        if recording {
            eprintln!("GPU recording spike: linear DMA-BUF → VA-API RGB import → GPU NV12 conversion → H.264. No audio.\nCapture starts in 3 seconds; switch to the animation workspace. Ctrl+C stops and finalizes.");
        } else {
            eprintln!("DMA-BUF source probe: one GBM GPU buffer; no CPU pixel access, encoding, audio or video file.\nCapture starts in 3 seconds; hide this terminal. Ctrl+C stops the probe.");
        }
        if unpaced {
            eprintln!("Unpaced acquisition: requests follow Ready without the FPS-cap sleep; source delivery may still wait for damage.");
        }
        let countdown = Instant::now() + Duration::from_secs(3);
        while Instant::now() < countdown && !STOP.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let begin = Instant::now();
        started = Some(begin);
        let deadline = begin + Duration::from_secs(options.seconds as u64);
        let mut first_acquisition = None;
        while Instant::now() < deadline && !STOP.load(Ordering::Relaxed) {
            let tick = Instant::now();
            state.ready = None;
            let frame = session.capture_into(&mut state, &imported.0, &queue.handle())?;
            let ready = wait_until(
                &mut queue,
                &mut state,
                &connection,
                |state| state.ready.is_some(),
                deadline,
            );
            drop(frame);
            if STOP.load(Ordering::Relaxed) {
                break;
            }
            if !ready? {
                break;
            }
            let acquired = Instant::now();
            let timestamp = state.ready.ok_or("No frame timestamp")?;
            let m = &mut report.measurement;
            if m.last_presentation_ns.is_some_and(|last| timestamp < last) {
                return Err("Presentation timestamp moved backwards".into());
            }
            let first = *first_acquisition.get_or_insert(acquired);
            let pts_us = acquired.duration_since(first).as_micros() as u64;
            let mut encode_ms = 0.0;
            #[cfg(feature = "gpu-encode-spike")]
            if let Some(video) = &mut video {
                let encoded = Instant::now();
                video.push_gpu(pts_us as i64)?;
                encode_ms = encoded.elapsed().as_secs_f64() * 1000.0;
            }
            #[cfg(not(feature = "gpu-encode-spike"))]
            let _ = &mut encode_ms;
            m.first_presentation_ns.get_or_insert(timestamp);
            m.last_presentation_ns = Some(timestamp);
            m.timings.push(FrameTiming {
                presentation_ns: timestamp,
                acquisition_pts_us: pts_us,
                capture_ms: acquired.duration_since(tick).as_secs_f64() * 1000.0,
                queue_wait_ms: 0.0,
                encode_ms,
            });
            m.frames += 1;
            if let Some(rest) =
                Duration::from_secs_f64(1.0 / options.fps as f64).checked_sub(tick.elapsed())
            {
                if !unpaced {
                    std::thread::sleep(
                        rest.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
            } else {
                m.late_iterations += 1;
            }
        }
        if report.measurement.frames == 0 {
            return Err("No DMA-BUF frames acquired".into());
        }
        #[cfg(feature = "gpu-encode-spike")]
        if let Some(video) = &mut video {
            video.finish()?;
        }
        Ok(())
    })();
    let result = result.and_then(|()| {
        if recording {
            fs::hard_link(&partial, options.directory.join("capture.mkv"))
                .map_err(|e| e.to_string())?;
            fs::remove_file(&partial).map_err(|e| e.to_string())?;
            File::open(&options.directory)
                .and_then(|file| file.sync_all())
                .map_err(|e| e.to_string())?;
            report.measurement.finalized = true;
        }
        Ok(())
    });
    if recording {
        report.measurement.media_bytes = fs::metadata(if report.measurement.finalized {
            options.directory.join("capture.mkv")
        } else {
            partial
        })
        .map(|m| m.len())
        .unwrap_or(0);
    }
    if report.setup_seconds == 0.0 {
        report.setup_seconds = setup.elapsed().as_secs_f64();
    }
    let final_usage = usage();
    let m = &mut report.measurement;
    m.cpu_user_seconds = final_usage.0 - initial_usage.0;
    m.cpu_system_seconds = final_usage.1 - initial_usage.1;
    m.process_peak_rss_kib = final_usage.2;
    m.elapsed_seconds = started.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
    m.actual_acquisition_fps = measured_fps(&m.timings);
    m.completed = result.is_ok();
    m.interrupted = STOP.load(Ordering::Relaxed);
    m.error = result.as_ref().err().cloned();
    report.buffer_reused = m.frames > 1;
    let file =
        File::create_new(options.directory.join("report.json")).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&file, &report).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    eprintln!(
        "Evidence: {}",
        options.directory.join("report.json").display()
    );
    result?;
    let fps = report
        .measurement
        .actual_acquisition_fps
        .map(|fps| format!("{fps:.2}"))
        .unwrap_or_else(|| "unavailable".into());
    if recording {
        println!("Finalized {} GPU-encoded frames at {fps} acquired FPS: {}. Visual acceptance remains unverified.",
            report.measurement.frames, options.directory.join("capture.mkv").display());
    } else {
        println!(
        "Acquired {} DMA-BUF source frames at {fps} FPS (requested {}; pacing: {}). No video was written.",
        report.measurement.frames, options.fps, report.pacing
    );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "gpu-encode-spike")]
    #[test]
    fn recording_geometry_preserves_flags_and_literal_values() {
        let args: Vec<String> = [
            "--monitor",
            "--crop",
            "--directory",
            "/tmp/example",
            "--crop",
            "321,181,1280,720",
            "--size",
            "640x360",
            "--no-cursor",
            "--unpaced",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let (filtered, request) = recording_options(&args).unwrap();
        let (options, unpaced, _) = parse_options(&filtered).unwrap();
        assert_eq!(options.monitor, "--crop");
        assert!(!options.cursor);
        assert!(unpaced);
        assert_eq!(request.resolve(1920, 1080).unwrap().output_width, 640);
        for invalid in [
            vec!["--crop", "0,0,1280,720", "--crop", "0,0,64,64"],
            vec!["--size", "1280x720", "--size", "640x360"],
            vec!["--size"],
            vec!["--size", "1279x720"],
            vec!["--crop", "0,0,1280,720", "--size", "640x480"],
        ] {
            assert!(
                recording_options(&invalid.into_iter().map(String::from).collect::<Vec<_>>())
                    .is_err()
            );
        }
        assert!(recording_options(&["--size".into(), "1280x720".into()]).is_ok());
        // Source-only probes must reject recording geometry, even in all-feature builds.
        assert!(parse_options(&["--crop".into(), "0,0,64,64".into()]).is_err());
    }

    #[test]
    fn linear_selection_requires_both_offers_and_never_substitutes_tiling() {
        let mut state = super::super::State::default();
        batch(&mut state, 1920, 17, &[0, 2]);
        let c = state.gpu.constraints.as_ref().unwrap();
        assert_eq!(
            candidates(c, &[(XRGB, 0), (XRGB, 2)].into(), Some(0)),
            vec![(XRGB, vec![0])]
        );
        assert!(candidates(c, &[(XRGB, 2)].into(), Some(0)).is_empty());
        let mut tiled_only = super::super::State::default();
        batch(&mut tiled_only, 1920, 17, &[2]);
        assert!(candidates(
            tiled_only.gpu.constraints.as_ref().unwrap(),
            &[(XRGB, 0), (XRGB, 2)].into(),
            Some(0)
        )
        .is_empty());
    }

    #[test]
    fn linear_flag_is_explicit_composes_with_unpaced_and_rejects_duplicates() {
        let mut args: Vec<String> = ["--monitor", "eDP-1", "--directory", "/tmp/example"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(!parse_options(&args).unwrap().2);
        args.extend(["--linear".into(), "--unpaced".into()]);
        let (_, unpaced, linear) = parse_options(&args).unwrap();
        assert!(linear && unpaced);
        args.push("--linear".into());
        assert!(parse_options(&args).unwrap_err().contains("Repeated"));
    }

    #[test]
    fn unpaced_mode_is_explicit_and_does_not_swallow_option_values() {
        let mut args: Vec<String> = ["--monitor", "eDP-1", "--directory", "/tmp/example"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(!parse_options(&args).unwrap().1);
        args.push("--unpaced".into());
        assert!(parse_options(&args).unwrap().1);
        args.push("--unpaced".into());
        assert!(parse_options(&args).unwrap_err().contains("Repeated"));
        args = [
            "--monitor",
            "--unpaced",
            "--directory",
            "/tmp/example",
            "--no-cursor",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let (options, unpaced, linear) = parse_options(&args).unwrap();
        assert_eq!(options.monitor, "--unpaced");
        assert!(!unpaced);
        assert!(!linear);
        assert!(!options.cursor);
    }

    fn batch(state: &mut super::super::State, width: u32, device: u64, modifiers: &[u64]) {
        constraints(
            state,
            session::Event::BufferSize {
                width,
                height: 1080,
            },
        );
        constraints(
            state,
            session::Event::DmabufDevice {
                device: device.to_ne_bytes().to_vec(),
            },
        );
        constraints(
            state,
            session::Event::DmabufFormat {
                format: XRGB,
                modifiers: modifiers.iter().flat_map(|m| m.to_ne_bytes()).collect(),
            },
        );
        constraints(state, session::Event::Done);
    }

    #[test]
    fn malformed_device_and_modifiers_cannot_reach_allocation() {
        assert!(parse_device(&[0; 7]).is_err());
        assert!(parse_modifiers(&[0; 9]).is_err());
        assert!(parse_modifiers(&[0; 2056]).is_err());
        assert_eq!(parse_device(&17u64.to_ne_bytes()).unwrap(), 17);
        assert_eq!(
            parse_modifiers(&INVALID_MODIFIER.to_ne_bytes()).unwrap(),
            Vec::<u64>::new()
        );
        let mut state = super::super::State::default();
        batch(&mut state, 1919, 17, &[0]);
        assert!(state.error.is_some());
        assert!(state.ext.layout.is_none());
        assert!(Pending::default().finish().is_err());
    }

    #[test]
    fn intersection_never_substitutes_a_format_or_implicit_modifier() {
        let mut state = super::super::State::default();
        batch(&mut state, 1920, 17, &[0, 2, INVALID_MODIFIER]);
        assert!(state.error.is_none());
        let c = state.gpu.constraints.as_ref().unwrap();
        assert!(candidates(c, &[(ARGB, 0), (XRGB, 3)].into(), None).is_empty());
        assert_eq!(
            candidates(c, &[(XRGB, 2), (ARGB, 0)].into(), None),
            vec![(XRGB, vec![2])]
        );
    }

    #[test]
    fn reordered_constraints_are_accepted_but_device_and_size_changes_stop() {
        let mut state = super::super::State::default();
        batch(&mut state, 1920, 17, &[2, 0, 2]);
        batch(&mut state, 1920, 17, &[0, 2]);
        assert!(state.error.is_none());
        batch(&mut state, 1920, 18, &[0, 2]);
        assert!(state
            .error
            .as_ref()
            .unwrap()
            .contains("constraints changed"));
        assert_eq!(state.gpu.constraints.as_ref().unwrap().device, 17);
        let mut changed_size = super::super::State::default();
        batch(&mut changed_size, 1920, 17, &[0]);
        batch(&mut changed_size, 1280, 17, &[0]);
        assert!(changed_size.error.is_some());
    }

    #[test]
    fn missing_explicit_device_has_no_fallback() {
        let error = Device::open(0, Some("/nonexistent/hyprrecorder-render-node"))
            .err()
            .unwrap();
        assert!(error.contains("explicit render device"));
        assert!(Device::open(0, Some("/tmp/invalid\0path")).is_err());
    }

    #[test]
    fn probe_rejects_unused_backend_choice_before_connecting() {
        assert!(run(&["--capture-backend".into(), "wlr".into()])
            .unwrap_err()
            .contains("omit --capture-backend"));
    }
}
