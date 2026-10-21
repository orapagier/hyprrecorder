//! Bounded, shell-free probes. Drain both pipes concurrently to avoid deadlock.
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_OUTPUT: usize = 256 * 1024;

#[derive(Debug)]
pub struct Probe {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

fn drain(mut reader: impl Read) -> String {
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let remaining = MAX_OUTPUT.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..count.min(remaining)]);
            }
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

pub fn run(program: &str, args: &[&str], timeout: Duration) -> Result<Probe, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot start {program}: {e}"))?;
    let out = child.stdout.take().ok_or("Cannot open probe stdout")?;
    let err = child.stderr.take().ok_or("Cannot open probe stderr")?;
    let stdout = thread::spawn(move || drain(out));
    let stderr = thread::spawn(move || drain(err));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "{program} timed out after {} seconds",
                    timeout.as_secs_f32()
                ));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("Cannot wait for {program}: {e}"));
            }
        }
    };
    // Probes are direct executables, never shells or process trees.
    let stdout = stdout.join().map_err(|_| "Probe stdout reader failed")?;
    let stderr = stderr.join().map_err(|_| "Probe stderr reader failed")?;
    Ok(Probe {
        success: status?.success(),
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subprocess_timeout_returns_instead_of_hanging() {
        let started = Instant::now();
        assert!(run("sleep", &["2"], Duration::from_millis(50))
            .unwrap_err()
            .contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn nonzero_exit_is_not_a_successful_probe() {
        assert!(!run("false", &[], Duration::from_secs(1)).unwrap().success);
    }
}
