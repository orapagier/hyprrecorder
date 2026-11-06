use crate::{config::Audio as Choices, process};
use std::{
    ffi::{c_char, c_void, CStr, CString},
    ptr::NonNull,
    time::Duration,
};

unsafe extern "C" {
    fn hr_audio_open(
        system: *const c_char,
        microphone: *const c_char,
        error: *mut c_char,
    ) -> *mut c_void;
    fn hr_audio_begin(audio: *mut c_void, epoch: i64) -> i32;
    fn hr_audio_read(
        audio: *mut c_void,
        samples: *mut i16,
        capacity: i32,
        position: *mut i64,
        error: *mut c_char,
    ) -> i32;
    fn hr_audio_free(audio: *mut c_void);
}
pub fn now_us() -> i64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    ts.tv_sec * 1_000_000 + ts.tv_nsec / 1000
}
fn pactl(args: &[&str]) -> Result<String, String> {
    let p = process::run("pactl", args, Duration::from_secs(2))?;
    if !p.success {
        return Err("Cannot resolve default audio device; turn off that source to continue".into());
    }
    Ok(p.stdout.trim().to_owned())
}
fn source(name: &str, microphone: bool) -> Result<CString, String> {
    let all: serde_json::Value = serde_json::from_str(&pactl(&["-f", "json", "list", "sources"])?)
        .map_err(|e| e.to_string())?;
    let found = all
        .as_array()
        .and_then(|v| v.iter().find(|v| v["name"].as_str() == Some(name)))
        .ok_or("Requested audio source is unavailable; turn it off to continue")?;
    if found["mute"] == true {
        return Err("Requested audio source is muted; unmute it or turn it off to continue".into());
    }
    if microphone
        && (found["monitor_of_sink"].is_string()
            || name.ends_with(".monitor")
            || (found["monitor_of_sink"].is_u64()
                && found["monitor_of_sink"].as_u64() != Some(u32::MAX as u64)))
    {
        return Err(
            "Default input is a playback monitor; select a microphone in system sound settings"
                .into(),
        );
    }
    CString::new(name).map_err(|_| "NUL in audio source".into())
}
pub struct Audio(NonNull<c_void>);
impl Drop for Audio {
    fn drop(&mut self) {
        unsafe {
            hr_audio_free(self.0.as_ptr());
        }
    }
}
impl Audio {
    pub fn open(choices: &Choices) -> Result<Option<Self>, String> {
        if !choices.system && !choices.microphone {
            return Ok(None);
        }
        let system = if choices.system {
            let sink = pactl(&["get-default-sink"])?;
            let all: serde_json::Value =
                serde_json::from_str(&pactl(&["-f", "json", "list", "sinks"])?)
                    .map_err(|e| e.to_string())?;
            let name = all
                .as_array()
                .and_then(|v| v.iter().find(|v| v["name"].as_str() == Some(&sink)))
                .and_then(|v| v["monitor_source"].as_str())
                .ok_or("Default output has no monitor source; turn off system audio to continue")?;
            Some(source(name, false)?)
        } else {
            None
        };
        let microphone = if choices.microphone {
            Some(source(&pactl(&["get-default-source"])?, true)?)
        } else {
            None
        };
        let mut error = [0 as c_char; 512];
        let pointer = unsafe {
            hr_audio_open(
                system.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                microphone.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                error.as_mut_ptr(),
            )
        };
        NonNull::new(pointer)
            .map(|p| Some(Self(p)))
            .ok_or_else(|| unsafe {
                CStr::from_ptr(error.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            })
    }
    pub fn begin(&mut self, epoch: i64) -> Result<(), String> {
        if unsafe { hr_audio_begin(self.0.as_ptr(), epoch) } < 0 {
            Err("Cannot start audio streams".into())
        } else {
            Ok(())
        }
    }
    pub fn drain(&mut self, video: &mut crate::embedded_video::Video) -> Result<(), String> {
        for _ in 0..200 {
            // <= two seconds of queued stereo, constant memory.
            let mut samples = [0i16; 960];
            let mut position = 0;
            let mut error = [0 as c_char; 512];
            let count = unsafe {
                hr_audio_read(
                    self.0.as_ptr(),
                    samples.as_mut_ptr(),
                    480,
                    &mut position,
                    error.as_mut_ptr(),
                )
            };
            if count < 0 {
                return Err(unsafe {
                    CStr::from_ptr(error.as_ptr())
                        .to_string_lossy()
                        .into_owned()
                });
            }
            if count == 0 {
                return Ok(());
            }
            video.push_audio(&samples[..count as usize * 2], position)?;
        }
        Err("Audio queue did not drain within its bound".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" {
        fn hr_audio_fixture(enabled: i32, out: *mut i16, missing: i32, error: *mut c_char) -> i32;
    }
    #[test]
    fn actual_mixer_handles_four_combinations_headroom_and_timing_loss() {
        for (enabled, expected) in [(0, 0), (1, 8192), (2, 16384), (3, 12288)] {
            let mut samples = [0i16; 960];
            let mut error = [0 as c_char; 512];
            let frames =
                unsafe { hr_audio_fixture(enabled, samples.as_mut_ptr(), 0, error.as_mut_ptr()) };
            assert_eq!(frames, if enabled == 0 { 0 } else { 480 });
            if enabled != 0 {
                assert!(samples
                    .iter()
                    .all(|v| (i32::from(*v) - expected).abs() <= 1));
            }
        }
        let mut samples = [0i16; 960];
        let mut error = [0 as c_char; 512];
        assert_eq!(
            unsafe { hr_audio_fixture(1, samples.as_mut_ptr(), 1, error.as_mut_ptr()) },
            -1
        );
        assert!(unsafe { CStr::from_ptr(error.as_ptr()) }
            .to_string_lossy()
            .contains("timing gap"));
    }
}
