//! Optional session-based SHM comparison; still a CPU-copy capture path.
use super::*;
use wayland_protocols::ext::{
    image_capture_source::v1::client::{
        ext_image_capture_source_v1 as source,
        ext_output_image_capture_source_manager_v1 as source_manager,
    },
    image_copy_capture::v1::client::{
        ext_image_copy_capture_frame_v1 as frame, ext_image_copy_capture_manager_v1 as manager,
        ext_image_copy_capture_session_v1 as session,
    },
};

#[derive(Default)]
struct Constraints {
    size: Option<(u32, u32)>,
    format: Option<wl_shm::Format>,
}
impl Constraints {
    fn layout(self) -> Result<Layout, String> {
        let (width, height) = self.size.ok_or("ext capture omitted buffer dimensions")?;
        let layout = Layout {
            width,
            height,
            stride: width.checked_mul(4).ok_or("ext stride overflow")?,
            format: self
                .format
                .ok_or("ext capture offers no supported SHM format")?,
        };
        layout.bytes()?;
        Ok(layout)
    }
}

#[derive(Default)]
struct Metadata {
    timestamp: Option<u64>,
    normal: bool,
}
impl Metadata {
    fn timestamp(&mut self, hi: u32, lo: u32, ns: u32) -> Result<(), String> {
        let seconds = ((hi as u64) << 32) | lo as u64;
        self.timestamp = seconds
            .checked_mul(1_000_000_000)
            .and_then(|s| s.checked_add(ns as u64));
        if ns >= 1_000_000_000 || self.timestamp.is_none() {
            return Err("Invalid ext presentation timestamp".into());
        }
        Ok(())
    }
    fn ready(&self) -> Result<u64, String> {
        if !self.normal {
            return Err("ext frame requires a normal buffer transform".into());
        }
        self.timestamp
            .ok_or("ext Ready omitted presentation timestamp".into())
    }
}

#[derive(Default)]
pub(super) struct State {
    pub(super) manager: Option<manager::ExtImageCopyCaptureManagerV1>,
    pub(super) source_manager: Option<source_manager::ExtOutputImageCaptureSourceManagerV1>,
    pending: Constraints,
    pub(super) layout: Option<Layout>,
    metadata: Metadata,
}

pub(super) struct Session(session::ExtImageCopyCaptureSessionV1);
impl Drop for Session {
    fn drop(&mut self) {
        self.0.destroy();
    }
}
pub(super) struct Frame(frame::ExtImageCopyCaptureFrameV1);
impl Drop for Frame {
    fn drop(&mut self) {
        self.0.destroy();
    }
}
impl Session {
    pub(super) fn start(
        state: &super::State,
        output: &wl_output::WlOutput,
        cursor: bool,
        qh: &QueueHandle<super::State>,
    ) -> Result<Self, String> {
        let manager = state
            .ext
            .manager
            .as_ref()
            .ok_or("ext image copy capture unavailable; no fallback")?;
        let source_manager = state
            .ext
            .source_manager
            .as_ref()
            .ok_or("ext output capture source unavailable; no fallback")?;
        let source = source_manager.create_source(output, qh, ());
        let options = if cursor {
            manager::Options::PaintCursors
        } else {
            manager::Options::empty()
        };
        let session = manager.create_session(&source, options, qh, ());
        source.destroy();
        Ok(Self(session))
    }
    pub(super) fn negotiated(state: &super::State) -> bool {
        state.ext.layout.is_some()
    }
    pub(super) fn capture(
        &self,
        state: &mut super::State,
        qh: &QueueHandle<super::State>,
    ) -> Result<Frame, String> {
        let layout = state
            .ext
            .layout
            .ok_or("ext buffer constraints not complete")?;
        let buffer = buffer(layout, state.shm.as_ref().ok_or("wl_shm unavailable")?, qh)?;
        let frame =
            self.capture_into(state, buffer.proxy.as_ref().ok_or("Missing SHM proxy")?, qh)?;
        state.buffer = Some(buffer);
        Ok(frame)
    }
    pub(super) fn capture_into(
        &self,
        state: &mut super::State,
        buffer: &wl_buffer::WlBuffer,
        qh: &QueueHandle<super::State>,
    ) -> Result<Frame, String> {
        let layout = state
            .ext
            .layout
            .ok_or("ext buffer constraints not complete")?;
        let frame = Frame(self.0.create_frame(qh, ()));
        frame.0.attach_buffer(buffer);
        // Full damage also permits buffer reuse without tracking content damage.
        frame
            .0
            .damage_buffer(0, 0, layout.width as i32, layout.height as i32);
        state.layout = Some(layout);
        state.ext.metadata = Metadata::default();
        frame.0.capture();
        Ok(frame)
    }
}

impl Dispatch<session::ExtImageCopyCaptureSessionV1, ()> for super::State {
    fn event(
        state: &mut Self,
        _: &session::ExtImageCopyCaptureSessionV1,
        event: session::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        #[cfg(feature = "dmabuf-spike")]
        if state.gpu.enabled {
            super::dmabuf_probe::constraints(state, event);
            return;
        }
        match event {
            session::Event::BufferSize { width, height } => {
                state.ext.pending.size = Some((width, height))
            }
            session::Event::ShmFormat {
                format: WEnum::Value(format),
            } if matches!(format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888) => {
                if state.ext.pending.format != Some(wl_shm::Format::Xrgb8888) {
                    state.ext.pending.format = Some(format);
                }
            }
            session::Event::Done => match std::mem::take(&mut state.ext.pending).layout() {
                Ok(layout) if state.ext.layout.is_none_or(|prior| prior == layout) => {
                    state.ext.layout = Some(layout)
                }
                Ok(_) => {
                    state.error = Some("ext capture constraints changed; stopping safely".into())
                }
                Err(error) => state.error = Some(error),
            },
            session::Event::Stopped => state.error = Some("ext capture session stopped".into()),
            _ => {} // DMA-BUF offers are intentionally unused by this SHM experiment.
        }
    }
}

impl Dispatch<frame::ExtImageCopyCaptureFrameV1, ()> for super::State {
    fn event(
        state: &mut Self,
        _: &frame::ExtImageCopyCaptureFrameV1,
        event: frame::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        frame_event(state, event);
    }
}

fn frame_event(state: &mut super::State, event: frame::Event) {
    let result = match event {
        frame::Event::PresentationTime {
            tv_sec_hi,
            tv_sec_lo,
            tv_nsec,
        } => state.ext.metadata.timestamp(tv_sec_hi, tv_sec_lo, tv_nsec),
        frame::Event::Transform { transform } => {
            state.ext.metadata.normal = transform == WEnum::Value(wl_output::Transform::Normal);
            if state.ext.metadata.normal {
                Ok(())
            } else {
                Err("Unsupported ext buffer transform".into())
            }
        }
        frame::Event::Ready => state
            .ext
            .metadata
            .ready()
            .map(|timestamp| state.ready = Some(timestamp)),
        frame::Event::Failed { reason } => Err(format!("ext capture failed: {reason:?}")),
        _ => Ok(()),
    };
    if let Err(error) = result {
        state.error = Some(error);
    }
}

delegate_noop!(super::State: ignore source::ExtImageCaptureSourceV1);
delegate_noop!(super::State: ignore source_manager::ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(super::State: ignore manager::ExtImageCopyCaptureManagerV1);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constraints_require_complete_supported_even_sized_shm() {
        assert!(Constraints::default().layout().is_err());
        assert!(Constraints {
            size: Some((1920, 1080)),
            format: None
        }
        .layout()
        .is_err());
        assert!(Constraints {
            size: Some((1919, 1080)),
            format: Some(wl_shm::Format::Xrgb8888)
        }
        .layout()
        .is_err());
        let layout = Constraints {
            size: Some((1920, 1080)),
            format: Some(wl_shm::Format::Argb8888),
        }
        .layout()
        .unwrap();
        assert_eq!(layout.bytes().unwrap(), 1920 * 1080 * 4);
    }
    #[test]
    fn presentation_metadata_does_not_release_pixels_before_ready() {
        let mut state = super::super::State::default();
        frame_event(
            &mut state,
            frame::Event::PresentationTime {
                tv_sec_hi: 0,
                tv_sec_lo: 2,
                tv_nsec: 123,
            },
        );
        frame_event(
            &mut state,
            frame::Event::Transform {
                transform: WEnum::Value(wl_output::Transform::Normal),
            },
        );
        assert_eq!(state.ready, None);
        assert!(state.error.is_none());
        frame_event(&mut state, frame::Event::Ready);
        assert_eq!(state.ready, Some(2_000_000_123));
        let mut failed = super::super::State::default();
        frame_event(
            &mut failed,
            frame::Event::Failed {
                reason: WEnum::Value(frame::FailureReason::BufferConstraints),
            },
        );
        assert!(failed.error.as_ref().unwrap().contains("BufferConstraints"));
        assert_eq!(failed.ready, None);
    }
    #[test]
    fn readiness_requires_timestamp_and_supported_transform() {
        let mut metadata = Metadata::default();
        assert!(metadata.ready().is_err());
        metadata.timestamp(0, 2, 123).unwrap();
        assert!(metadata.ready().is_err());
        metadata.normal = true;
        assert_eq!(metadata.ready().unwrap(), 2_000_000_123);
        assert!(metadata.timestamp(0, 0, 1_000_000_000).is_err());
        assert!(metadata.timestamp(u32::MAX, u32::MAX, 0).is_err());
    }
}
