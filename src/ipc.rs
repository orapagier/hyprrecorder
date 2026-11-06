//! Same-user typed control on a private Unix socket; flock owns stale cleanup.
use crate::output::private_directory;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    Preparing,
    Countdown,
    Recording { frames: u64 },
    Stopping,
    Saved { path: PathBuf },
    Cancelled,
    Failed { error: String },
    Idle,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Request {
    Stop,
    Status,
}

pub fn runtime() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or("XDG_RUNTIME_DIR is unset; run in your desktop session")?;
    let base = PathBuf::from(base);
    let meta = fs::symlink_metadata(&base).map_err(|e| e.to_string())?;
    if !base.is_absolute()
        || !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err("XDG_RUNTIME_DIR must be absolute, private and owned by this user".into());
    }
    let path = base.join("hyprrecorder");
    private_directory(&path)?;
    Ok(path)
}

pub struct Instance {
    _lock: File,
    socket: PathBuf,
    done: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    pub status: Arc<Mutex<Status>>,
    pub stop: Arc<AtomicBool>,
}
impl Instance {
    pub fn acquire() -> Result<Self, String> {
        Self::at(runtime()?)
    }
    fn at(root: PathBuf) -> Result<Self, String> {
        private_directory(&root)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root.join("record.lock"))
            .map_err(|e| e.to_string())?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(
                "A recording instance is already active; use hyprrecorder status or stop".into(),
            );
        }
        let socket = root.join("control.sock");
        if let Ok(meta) = fs::symlink_metadata(&socket) {
            use std::os::unix::fs::FileTypeExt;
            if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
                return Err("Refusing unexpected control socket path".into());
            }
            fs::remove_file(&socket).map_err(|e| e.to_string())?;
        }
        let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(Status::Preparing));
        let (thread_done, thread_stop, thread_status) =
            (done.clone(), stop.clone(), status.clone());
        let worker = std::thread::spawn(move || {
            while !thread_done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = serve(&mut stream, &thread_stop, &thread_status);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            _lock: lock,
            socket,
            done,
            worker: Some(worker),
            status,
            stop,
        })
    }
    pub fn update(&self, status: Status) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = status.clone();
        if let Ok(json) = serde_json::to_string(&status) {
            println!("{json}");
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.socket);
    }
}
fn serve(stream: &mut UnixStream, stop: &AtomicBool, status: &Mutex<Status>) -> Result<(), String> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&cred) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut size,
        )
    } != 0
        || cred.uid != unsafe { libc::geteuid() }
    {
        return Err("Different-user IPC rejected".into());
    }
    stream
        .set_write_timeout(Some(Duration::from_millis(200)))
        .map_err(|e| e.to_string())?;
    let line = read_message(stream, 256, Duration::from_millis(200))?;
    let response = respond(&line, stop, status)?;
    writeln!(stream, "{response}").map_err(|e| e.to_string())
}
// Socket timeouts apply per read. Shrink the remaining timeout so a client
// trickling bytes cannot monopolize the single control worker or delay shutdown.
fn read_message(
    stream: &mut UnixStream,
    limit: usize,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    while bytes.len() < limit {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or("Control message timed out")?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 256];
        let capacity = buffer.len().min(limit - bytes.len());
        let count = match stream.read(&mut buffer[..capacity]) {
            Ok(0) => return Err("Incomplete control message".into()),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.to_string()),
        };
        if let Some(end) = buffer[..count].iter().position(|byte| *byte == b'\n') {
            bytes.extend_from_slice(&buffer[..=end]);
            return String::from_utf8(bytes).map_err(|e| e.to_string());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Err("Control message exceeds size limit".into())
}

fn respond(line: &str, stop: &AtomicBool, status: &Mutex<Status>) -> Result<String, String> {
    if line.len() > 256 || !line.ends_with('\n') {
        return Err("Invalid control message".into());
    }
    let request: Request =
        serde_json::from_str(line.trim()).map_err(|_| "Expected stop or status")?;
    if matches!(request, Request::Stop) {
        stop.store(true, Ordering::Relaxed);
    }
    serde_json::to_string(&*status.lock().unwrap_or_else(|e| e.into_inner()))
        .map_err(|e| e.to_string())
}

pub fn request(stop: bool) -> Result<Status, String> {
    let root = runtime()?;
    let path = root.join("control.sock");
    let started = std::time::Instant::now();
    let mut stream = loop {
        match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                // The lock also covers the interval before bind/after unlink.
                let lock = match OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(root.join("record.lock"))
                {
                    Ok(lock) => lock,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Status::Idle),
                    Err(e) => return Err(e.to_string()),
                };
                if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0 {
                    return Ok(Status::Idle);
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::WouldBlock {
                    return Err(format!("Cannot confirm recording state: {error}"));
                }
                if !stop {
                    return Ok(Status::Preparing);
                }
                if started.elapsed() >= Duration::from_secs(1) {
                    return Err("Recorder control is unavailable; stop was not delivered".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("Cannot contact recorder: {e}")),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    writeln!(stream, "\"{}\"", if stop { "stop" } else { "status" }).map_err(|e| e.to_string())?;
    let line = read_message(&mut stream, 16384, Duration::from_secs(1))?;
    serde_json::from_str(&line).map_err(|e| format!("Invalid recorder response: {e}"))
}

/// Ask an existing view to unmap before *any* CLI acquisition. A stale or
/// unresponsive view socket fails closed rather than recording the controls.
pub fn hide_view() -> Result<(), String> {
    let path = runtime()?.join("ui.sock");
    let mut stream = match UnixStream::connect(path) {
        Ok(stream) => stream,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Cannot hide existing recorder window: {e}")),
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    stream.write_all(b"hide\n").map_err(|e| e.to_string())?;
    let reply = read_message(&mut stream, 16, Duration::from_secs(1))?;
    if reply != "hidden\n" {
        return Err("Recorder window is busy or could not hide; cancel selection and retry".into());
    }
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    #[test]
    fn control_message_requires_bounded_complete_utf8_line() {
        for (bytes, limit, valid) in [
            (b"ok\n".as_slice(), 3, true),
            (b"ok".as_slice(), 3, false),
            (b"long\n".as_slice(), 3, false),
            (b"\xff\n".as_slice(), 3, false),
        ] {
            let (mut reader, mut writer) = UnixStream::pair().unwrap();
            writer.write_all(bytes).unwrap();
            drop(writer);
            assert_eq!(
                read_message(&mut reader, limit, Duration::from_secs(1)).is_ok(),
                valid
            );
        }
    }

    #[test]
    fn trickling_client_cannot_extend_message_deadline() {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            for _ in 0..50 {
                if writer.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        let result = read_message(&mut reader, 256, Duration::from_millis(100));
        let elapsed = started.elapsed();
        drop(reader);
        worker.join().unwrap();
        assert!(result.is_err());
        assert!(elapsed < Duration::from_millis(700), "{elapsed:?}");
    }
    #[test]
    fn typed_control_rejects_arbitrary_messages_and_stop_is_idempotent() {
        let stop = AtomicBool::new(false);
        let status = Mutex::new(Status::Countdown);
        for invalid in ["{\"exec\":\"false\"}\n", "\"status\"", "\"show\"\n"] {
            assert!(respond(invalid, &stop, &status).is_err());
            assert!(!stop.load(Ordering::Relaxed));
        }
        assert!(respond(&format!("{}\n", "x".repeat(257)), &stop, &status).is_err());
        assert!(respond("\"status\"\n", &stop, &status)
            .unwrap()
            .contains("countdown"));
        for _ in 0..2 {
            assert!(respond("\"stop\"\n", &stop, &status).is_ok());
            assert!(stop.load(Ordering::Relaxed));
        }
    }
    #[test]
    fn lock_file_and_private_directory_reject_competitors_and_links() {
        let root = std::env::temp_dir().join(format!("hyprrecorder-lock-{}", std::process::id()));
        private_directory(&root).unwrap();
        let path = root.join("lock");
        let first = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(first.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert_ne!(
            unsafe { libc::flock(second.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let link = root.join("linked");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert!(private_directory(&link).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires Unix socket bind permission; run cargo test --all-features -- --ignored in the desktop session"]
    fn competing_instances_typed_requests_slow_client_and_stale_recovery() {
        let root = std::env::temp_dir().join(format!("hyprrecorder-ipc-{}", std::process::id()));
        let instance = Instance::at(root.clone()).unwrap();
        assert!(Instance::at(root.clone()).is_err());
        let send = |text: &str| {
            let mut socket = UnixStream::connect(root.join("control.sock")).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket.write_all(text.as_bytes()).unwrap();
            let mut reply = String::new();
            let _ = BufReader::new(socket).read_line(&mut reply);
            reply
        };
        assert!(send("\"status\"\n").contains("preparing"));
        assert!(send("{\"exec\":\"false\"}\n").is_empty());
        assert!(!instance.stop.load(Ordering::Relaxed));
        let slow = UnixStream::connect(root.join("control.sock")).unwrap();
        assert!(send("\"status\"\n").contains("preparing"));
        drop(slow);
        send("\"stop\"\n");
        send("\"stop\"\n");
        assert!(instance.stop.load(Ordering::Relaxed));
        drop(instance);
        assert!(!root.join("control.sock").exists());
        let stale = UnixListener::bind(root.join("control.sock")).unwrap();
        drop(stale);
        let again = Instance::at(root.clone()).unwrap();
        drop(again);
        fs::write(root.join("control.sock"), b"preserve").unwrap();
        assert!(Instance::at(root.clone()).is_err());
        assert_eq!(fs::read(root.join("control.sock")).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap();
    }
}
