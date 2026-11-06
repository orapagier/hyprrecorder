//! Minimal owned FFmpeg writer for the experimental CPU-copy capture path.
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    fs::File,
    os::fd::AsRawFd,
    ptr::NonNull,
};

extern "C" {
    #[cfg(feature = "gpu-encode-spike")]
    fn hr_video_open_gpu_audio(
        out: *mut *mut c_void,
        fd: c_int,
        width: c_int,
        height: c_int,
        fps: c_int,
        device: *const c_char,
        dma_fd: c_int,
        pitch: u32,
        offset: u32,
        format: u32,
        geometry: *const crate::video_geometry::Geometry,
        audio: c_int,
    ) -> c_int;
    #[cfg(feature = "gpu-encode-spike")]
    #[cfg(feature = "recorder")]
    fn hr_video_repeat_gpu(video: *mut c_void, pts_us: i64) -> c_int;
    #[cfg(feature = "gpu-encode-spike")]
    fn hr_video_push_gpu(video: *mut c_void, pts_us: i64) -> c_int;
    fn hr_video_open(
        out: *mut *mut c_void,
        fd: c_int,
        width: c_int,
        height: c_int,
        fps: c_int,
        device: *const c_char,
    ) -> c_int;
    fn hr_video_push(
        video: *mut c_void,
        pixels: *const u8,
        stride: c_int,
        inverted: c_int,
        pts_us: i64,
    ) -> c_int;
    #[cfg(feature = "recorder")]
    fn hr_video_push_audio(
        video: *mut c_void,
        samples: *const i16,
        frames: c_int,
        position: i64,
    ) -> c_int;
    #[cfg(all(test, feature = "recorder"))]
    fn hr_video_open_fixture_audio(
        out: *mut *mut c_void,
        fd: c_int,
        width: c_int,
        height: c_int,
        fps: c_int,
    ) -> c_int;
    #[cfg(feature = "recorder")]
    fn hr_video_end_at(video: *mut c_void, end_us: i64) -> c_int;
    fn hr_video_finish(video: *mut c_void) -> c_int;
    fn hr_video_free(video: *mut c_void);
    fn hr_video_error(code: c_int, text: *mut c_char, capacity: usize);
}

fn checked(result: c_int) -> Result<(), String> {
    if result >= 0 {
        return Ok(());
    }
    let mut text = [0 as c_char; 256];
    // The C bridge writes at most this buffer's length and NUL terminates it.
    unsafe {
        hr_video_error(result, text.as_mut_ptr(), text.len());
        Err(format!(
            "FFmpeg: {}",
            CStr::from_ptr(text.as_ptr()).to_string_lossy()
        ))
    }
}

pub struct Video {
    native: NonNull<c_void>,
    file: File,
    width: u32,
    height: u32,
    finished: bool,
    gpu_input: bool,
}

#[cfg(feature = "gpu-encode-spike")]
pub(crate) struct GpuInput<'a> {
    pub fd: std::os::fd::BorrowedFd<'a>,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub pitch: u32,
    pub offset: u32,
    pub modifier: u64,
    pub geometry: crate::video_geometry::Geometry,
}

impl Video {
    #[cfg(feature = "gpu-encode-spike")]
    pub(crate) fn open_gpu(
        file: File,
        input: GpuInput<'_>,
        fps: u32,
        device: &str,
    ) -> Result<Self, String> {
        Self::open_gpu_with_audio(file, input, fps, device, false)
    }

    #[cfg(feature = "gpu-encode-spike")]
    pub(crate) fn open_gpu_with_audio(
        file: File,
        input: GpuInput<'_>,
        fps: u32,
        device: &str,
        audio: bool,
    ) -> Result<Self, String> {
        if !(2..=8192).contains(&input.width)
            || !(2..=8192).contains(&input.height)
            || input.width % 2 != 0
            || input.height % 2 != 0
            || !(1..=240).contains(&fps)
            || input.pitch < input.width * 4
            || input.pitch > i32::MAX as u32
            || input.modifier != 0
            || ![0x34325258, 0x34325241].contains(&input.format)
        {
            return Err(
                "GPU writer requires even 2–8192 pixel linear XRGB/ARGB, valid pitch and FPS"
                    .into(),
            );
        }
        input.geometry.validate(input.width, input.height)?;
        let device = CString::new(device).map_err(|_| "Render device contains NUL")?;
        let mut pointer = std::ptr::null_mut();
        unsafe {
            // The bridge duplicates the input DMA fd; File retains the borrowed output fd.
            checked(hr_video_open_gpu_audio(
                &mut pointer,
                file.as_raw_fd(),
                input.width as c_int,
                input.height as c_int,
                fps as c_int,
                device.as_ptr(),
                input.fd.as_raw_fd(),
                input.pitch,
                input.offset,
                input.format,
                &input.geometry,
                audio as c_int,
            ))?;
        }
        Ok(Self {
            native: NonNull::new(pointer).ok_or("FFmpeg returned no GPU writer")?,
            file,
            width: input.width,
            height: input.height,
            finished: false,
            gpu_input: true,
        })
    }

    #[cfg(feature = "gpu-encode-spike")]
    pub(crate) fn push_gpu(&mut self, pts_us: i64) -> Result<(), String> {
        if self.finished || !self.gpu_input || pts_us < 0 {
            return Err("GPU writer mode, timestamp or finalization state is invalid".into());
        }
        // Synchronous VPP completion precedes return; the capture buffer may then be reused.
        unsafe { checked(hr_video_push_gpu(self.native.as_ptr(), pts_us)) }
    }
    #[cfg(feature = "recorder")]
    pub fn repeat_gpu(&mut self, pts_us: i64) -> Result<(), String> {
        if self.finished || !self.gpu_input || pts_us < 0 {
            return Err("Invalid repeat frame".into());
        }
        unsafe { checked(hr_video_repeat_gpu(self.native.as_ptr(), pts_us)) }
    }
    #[cfg(test)]
    pub(crate) fn software_fixture(
        file: File,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self, String> {
        Self::create(file, width, height, fps, None)
    }

    pub fn open(
        file: File,
        width: u32,
        height: u32,
        fps: u32,
        device: &str,
    ) -> Result<Self, String> {
        let device = CString::new(device).map_err(|_| "Render device contains NUL")?;
        Self::create(file, width, height, fps, Some(&device))
    }

    fn create(
        file: File,
        width: u32,
        height: u32,
        fps: u32,
        device: Option<&CStr>,
    ) -> Result<Self, String> {
        if !(2..=8192).contains(&width)
            || !(2..=8192).contains(&height)
            || width % 2 != 0
            || height % 2 != 0
            || !(1..=240).contains(&fps)
        {
            return Err("Spike needs even 2–8192 pixel dimensions and 1–240 FPS".into());
        }
        let mut pointer = std::ptr::null_mut();
        // The bridge borrows the fd; this object keeps it open until after free.
        // Only tests call create with None, enabling a software test fixture.
        unsafe {
            checked(hr_video_open(
                &mut pointer,
                file.as_raw_fd(),
                width as c_int,
                height as c_int,
                fps as c_int,
                device.map_or(std::ptr::null(), CStr::as_ptr),
            ))?;
        }
        Ok(Self {
            native: NonNull::new(pointer).ok_or("FFmpeg returned no writer")?,
            file,
            width,
            height,
            finished: false,
            gpu_input: false,
        })
    }

    pub fn push(
        &mut self,
        pixels: &[u8],
        stride: u32,
        inverted: bool,
        pts_us: i64,
    ) -> Result<(), String> {
        if self.finished
            || self.gpu_input
            || pts_us < 0
            || stride < self.width * 4
            || stride > i32::MAX as u32
            || (stride as usize)
                .checked_mul(self.height as usize)
                .is_none_or(|size| size > pixels.len())
        {
            return Err(
                "Invalid or undersized capture frame, timestamp, or finalized writer".into(),
            );
        }
        // Dimensions/stride and entire readable pixel span are checked above.
        // The C bridge consumes pixels synchronously; it never retains the slice.
        unsafe {
            checked(hr_video_push(
                self.native.as_ptr(),
                pixels.as_ptr(),
                stride as c_int,
                inverted as c_int,
                pts_us,
            ))
        }
    }

    #[cfg(feature = "recorder")]
    pub fn push_audio(&mut self, samples: &[i16], position: i64) -> Result<(), String> {
        if self.finished
            || samples.is_empty()
            || samples.len() % 2 != 0
            || samples.len() > 9600
            || position < 0
        {
            return Err("Invalid audio packet".into());
        }
        unsafe {
            checked(hr_video_push_audio(
                self.native.as_ptr(),
                samples.as_ptr(),
                (samples.len() / 2) as c_int,
                position,
            ))
        }
    }

    #[cfg(feature = "recorder")]
    pub fn end_at(&mut self, end_us: i64) -> Result<(), String> {
        if self.finished {
            return Err("Writer is finalized".into());
        }
        unsafe { checked(hr_video_end_at(self.native.as_ptr(), end_us)) }
    }

    pub fn finish(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        // A failed finalization cannot be retried or announced as successful.
        self.finished = true;
        unsafe {
            checked(hr_video_finish(self.native.as_ptr()))?;
        }
        self.file
            .sync_all()
            .map_err(|e| format!("Cannot sync recording: {e}"))
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        // No implicit success/finalization on failure; this only releases resources.
        unsafe {
            hr_video_free(self.native.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        process::Command,
        sync::atomic::{AtomicU32, Ordering},
    };
    static NEXT: AtomicU32 = AtomicU32::new(0);

    #[cfg(feature = "gpu-encode-spike")]
    #[test]
    fn gpu_entry_rejects_cpu_writer_and_invalid_layout_before_driver_access() {
        use std::os::fd::AsFd;
        let fixture = Fixture::new();
        let mut writer = Video::software_fixture(fixture.file(), 64, 48, 30).unwrap();
        assert!(writer.push_gpu(0).unwrap_err().contains("mode"));
        let input = File::open("/dev/null").unwrap();
        let other = Fixture::new();
        let error = Video::open_gpu(
            other.file(),
            GpuInput {
                fd: input.as_fd(),
                width: 64,
                height: 48,
                format: 0x34325258,
                pitch: 256,
                offset: 0,
                modifier: 2,
                geometry: crate::video_geometry::Request::default()
                    .resolve(64, 48)
                    .unwrap(),
            },
            30,
            "/nonexistent/hyprrecorder-node",
        )
        .err()
        .unwrap();
        assert!(error.contains("linear"));
    }

    #[cfg(feature = "gpu-encode-spike")]
    #[test]
    fn gpu_writer_missing_hardware_cannot_fall_back_to_software() {
        use std::os::fd::AsFd;
        let fixture = Fixture::new();
        let input = File::open("/dev/null").unwrap();
        assert!(Video::open_gpu(
            fixture.file(),
            GpuInput {
                fd: input.as_fd(),
                width: 64,
                height: 48,
                format: 0x34325258,
                pitch: 256,
                offset: 0,
                modifier: 0,
                geometry: crate::video_geometry::Request::default()
                    .resolve(64, 48)
                    .unwrap(),
            },
            30,
            "/nonexistent/hyprrecorder-node"
        )
        .is_err());
        assert_eq!(
            fs::metadata(fixture.0.join("fixture.mkv")).unwrap().len(),
            0
        );
    }

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "hyprrecorder-codec-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn file(&self) -> File {
            File::create_new(self.0.join("fixture.mkv")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn embedded_fixture_encodes_decodes_and_preserves_timestamps() {
        let fixture = Fixture::new();
        let mut writer = Video::create(fixture.file(), 64, 48, 30, None).unwrap();
        // Padded rows and inverted storage exercise real swscale input strides.
        let mut pixels = vec![0; 272 * 48];
        for row in 0..48 {
            for col in 0..64 {
                pixels[row * 272 + col * 4..row * 272 + col * 4 + 4].copy_from_slice(if row < 24 {
                    &[0, 0, 255, 255]
                } else {
                    &[255, 0, 0, 255]
                });
            }
        }
        assert!(writer.push(&pixels[..100], 272, true, 0).is_err());
        for frame in 0..12 {
            writer.push(&pixels, 272, true, frame * 33333).unwrap();
        }
        writer.finish().unwrap();
        writer.finish().unwrap();
        assert!(writer.push(&pixels, 272, false, 500000).is_err());
        drop(writer);
        let path = fixture.0.join("fixture.mkv");
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_frames",
                "-show_streams",
                "-show_frames",
                "-of",
                "json",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["streams"][0]["nb_read_frames"], "12");
        assert_eq!(report["streams"][0]["width"], 64);
        let frames = report["frames"].as_array().unwrap();
        let last = frames.last().unwrap()["pts_time"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!((last - 0.366).abs() < 0.003);
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args([
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        let top = &decoded.stdout[10 * 64 * 3 + 10 * 3..][..3];
        let bottom = &decoded.stdout[36 * 64 * 3 + 10 * 3..][..3];
        assert!(top[2] > 180 && top[0] < 80, "top must be blue: {top:?}");
        assert!(
            bottom[0] > 180 && bottom[2] < 80,
            "bottom must be red: {bottom:?}"
        );
    }

    #[test]
    fn hardware_failure_has_no_software_fallback() {
        let fixture = Fixture::new();
        assert!(Video::open(
            fixture.file(),
            64,
            48,
            30,
            "/nonexistent/hyprrecorder-render-node"
        )
        .is_err());
    }

    #[test]
    fn sdr_color_tags_and_decoded_color_patches_match_the_rgb_source() {
        let fixture = Fixture::new();
        let colors: [[u8; 3]; 12] = [
            [0, 0, 0],
            [255, 255, 255],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [0, 255, 255],
            [255, 0, 255],
            [255, 255, 0],
            [128, 128, 128],
            [180, 90, 60],
            [32, 48, 64],
            [80, 120, 200],
        ];
        let width = colors.len() * 32;
        let height = 64;
        let mut pixels = vec![0u8; width * height * 4];
        for row in pixels.chunks_exact_mut(width * 4) {
            for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
                let [r, g, b] = colors[x / 32];
                pixel.copy_from_slice(&[b, g, r, 255]);
            }
        }
        let mut writer =
            Video::software_fixture(fixture.file(), width as u32, height as u32, 30).unwrap();
        writer.push(&pixels, width as u32 * 4, false, 0).unwrap();
        writer.finish().unwrap();
        drop(writer);
        let path = fixture.0.join("fixture.mkv");
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(probe.status.success());
        let report: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let stream = &report["streams"][0];
        assert_eq!(stream["color_space"], "bt709");
        assert_eq!(stream["color_range"], "tv");
        assert_eq!(stream["color_primaries"], "bt709");
        assert_eq!(stream["color_transfer"], "iec61966-2-1");
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&path)
            .args([
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        assert_eq!(decoded.stdout.len(), width * height * 3);
        for (index, expected) in colors.iter().enumerate() {
            let offset = (32 * width + index * 32 + 16) * 3;
            let actual = &decoded.stdout[offset..offset + 3];
            for channel in 0..3 {
                assert!(
                    (i16::from(actual[channel]) - i16::from(expected[channel])).abs() <= 8,
                    "patch {index}: source {expected:?}, decoded {actual:?}"
                );
            }
        }
    }

    #[test]
    fn disk_write_error_is_not_success() {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap();
        let result = Video::create(file, 64, 48, 30, None).and_then(|mut video| {
            video.push(&vec![0; 64 * 48 * 4], 64 * 4, false, 0)?;
            video.finish()
        });
        assert!(result.is_err());
    }
}

#[cfg(all(test, feature = "recorder"))]
mod audio_tests {
    use super::*;
    #[test]
    fn timed_pcm_and_video_mux_decode_with_static_video_tail() {
        let root = std::env::temp_dir().join(format!("hyprrecorder-av-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("av.mkv");
        let file = File::create_new(&path).unwrap();
        let mut pointer = std::ptr::null_mut();
        unsafe {
            checked(hr_video_open_fixture_audio(
                &mut pointer,
                file.as_raw_fd(),
                64,
                48,
                30,
            ))
            .unwrap();
        }
        let mut video = Video {
            native: NonNull::new(pointer).unwrap(),
            file,
            width: 64,
            height: 48,
            finished: false,
            gpu_input: false,
        };
        let pixels = vec![128; 64 * 48 * 4];
        for n in 0..100 {
            if n == 0 || n == 33 {
                video.push(&pixels, 256, false, n * 10_000).unwrap();
            }
            let samples: Vec<i16> = (0..960)
                .map(|i| ((i as f64 * 0.1).sin() * 1000.0) as i16)
                .collect();
            video.push_audio(&samples, n * 480).unwrap();
        }
        assert!(video.push_audio(&[0; 960], 0).is_err());
        video.end_at(1_000_000).unwrap();
        video.finish().unwrap();
        drop(video);
        let probe = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(probe.status.success());
        let value: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        assert_eq!(value["streams"][1]["codec_name"], "pcm_s16le");
        let duration: f64 = value["format"]["duration"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((duration - 1.0).abs() < 0.05, "{value}");
        let decode = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&path)
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        assert!(
            decode.status.success(),
            "{}",
            String::from_utf8_lossy(&decode.stderr)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
