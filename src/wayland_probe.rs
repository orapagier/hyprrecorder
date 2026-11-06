//! Read-only registry discovery, isolated in a bounded subprocess.
use crate::process;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, env, time::Duration};
use wayland_client::{protocol::wl_registry, Connection, Dispatch, QueueHandle};

#[derive(Debug, Serialize, Deserialize)]
pub struct Interface {
    pub name: String,
    pub version: u32,
}

#[derive(Default)]
struct Registry {
    globals: BTreeMap<u32, Interface>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Registry {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                state.globals.insert(
                    name,
                    Interface {
                        name: interface,
                        version,
                    },
                );
            }
            wl_registry::Event::GlobalRemove { name } => {
                state.globals.remove(&name);
            }
            _ => {}
        }
    }
}

pub fn query() -> Result<Vec<Interface>, String> {
    let connection = Connection::connect_to_env()
        .map_err(|e| format!("Cannot connect to the session Wayland display: {e}"))?;
    let mut queue = connection.new_event_queue::<Registry>();
    let _registry = connection.display().get_registry(&queue.handle(), ());
    let mut state = Registry::default();
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland registry query failed: {e}"))?;
    let mut interfaces: Vec<_> = state.globals.into_values().collect();
    interfaces.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
    Ok(interfaces)
}

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    session_reachable: bool,
    interfaces: Vec<Interface>,
    wlr_screencopy_advertised: bool,
    ext_output_capture_advertised: bool,
    dmabuf_advertised: bool,
    capture_verified: bool,
    detail: String,
}

fn report(interfaces: Vec<Interface>, error: Option<String>) -> Report {
    let has = |name: &str| interfaces.iter().any(|i| i.name == name && i.version > 0);
    Report {
        schema_version: 1,
        session_reachable: error.is_none(),
        wlr_screencopy_advertised: has("zwlr_screencopy_manager_v1"),
        ext_output_capture_advertised: has("ext_image_copy_capture_manager_v1")
            && has("ext_output_image_capture_source_manager_v1"),
        dmabuf_advertised: has("zwp_linux_dmabuf_v1"),
        capture_verified: false,
        interfaces,
        detail: error.unwrap_or_else(|| "Registry only: advertised protocols do not prove permission, buffer negotiation, cursor support or hardware encoding. No capture session was created.".into()),
    }
}

pub fn collect() -> Report {
    // A stalled compositor cannot block the CLI indefinitely. The worker never
    // spawns helpers, binds capture globals, acquires buffers or captures audio.
    let result = (|| {
        let executable = env::current_exe().map_err(|e| e.to_string())?;
        let executable = executable.to_str().ok_or("Executable path is not UTF-8")?;
        let probe = process::run(
            executable,
            &["__wayland-probe-worker"],
            Duration::from_secs(3),
        )?;
        if !probe.success {
            return Err(format!("Wayland access unverified: {}. Run from the graphical session; this does not diagnose the desktop.", probe.stderr.trim()));
        }
        serde_json::from_str::<Vec<Interface>>(&probe.stdout).map_err(|e| e.to_string())
    })();
    match result {
        Ok(interfaces) => report(interfaces, None),
        Err(error) => report(Vec::new(), Some(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interfaces(names: &[&str]) -> Vec<Interface> {
        names
            .iter()
            .map(|name| Interface {
                name: (*name).into(),
                version: 1,
            })
            .collect()
    }

    #[test]
    fn ext_capture_needs_both_manager_and_output_source() {
        let partial = report(interfaces(&["ext_image_copy_capture_manager_v1"]), None);
        assert!(partial.session_reachable);
        assert!(!partial.ext_output_capture_advertised);
        let complete = report(
            interfaces(&[
                "ext_image_copy_capture_manager_v1",
                "ext_output_image_capture_source_manager_v1",
            ]),
            None,
        );
        assert!(complete.ext_output_capture_advertised);
        assert!(!complete.capture_verified);
        assert!(!complete.dmabuf_advertised);
    }

    #[test]
    fn registry_does_not_imply_capture_or_hardware_success() {
        let supported = report(
            interfaces(&["zwlr_screencopy_manager_v1", "zwp_linux_dmabuf_v1"]),
            None,
        );
        assert!(supported.wlr_screencopy_advertised);
        assert!(supported.dmabuf_advertised);
        assert!(!supported.capture_verified);
        let unavailable = report(Vec::new(), Some("inaccessible".into()));
        assert!(!unavailable.session_reachable);
        assert!(!unavailable.capture_verified);
    }
}
