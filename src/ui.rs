use crate::{config::Settings, ipc, monitors};
use std::ffi::{c_char, CStr, CString};
unsafe extern "C" {
    fn hr_ui_run(
        initial: *const c_char,
        runtime: *const c_char,
        binary: *const c_char,
        settings_path: *const c_char,
        smoke: i32,
        save: extern "C" fn(*const c_char, *mut c_char, usize) -> i32,
        active: extern "C" fn() -> i32,
    ) -> i32;
}
extern "C" fn active() -> i32 {
    i32::from(!matches!(ipc::request(false), Ok(ipc::Status::Idle)))
}
extern "C" fn save(json: *const c_char, error: *mut c_char, capacity: usize) -> i32 {
    let result = std::panic::catch_unwind(|| -> Result<(), String> {
        let text = unsafe { CStr::from_ptr(json) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let settings: Settings =
            serde_json::from_str(text).map_err(|e| format!("Invalid controls: {e}"))?;
        settings.validate()?;
        let monitor = monitors::select(&settings)?;
        monitor.geometry(&settings)?;
        settings.save_user()
    })
    .unwrap_or_else(|_| Err("Settings operation failed".into()));
    match result {
        Ok(()) => 0,
        Err(text) => {
            if capacity > 0 {
                let bytes = text.as_bytes();
                let count = bytes.len().min(capacity - 1);
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), error.cast(), count);
                    *error.add(count) = 0;
                }
            }
            1
        }
    }
}
pub fn run(args: &[String]) -> Result<(), String> {
    let smoke = args == ["--smoke"];
    if !args.is_empty() && !smoke {
        return Err("ui accepts only --smoke (offscreen validation)".into());
    }
    if !smoke && !matches!(ipc::request(false)?, ipc::Status::Idle) {
        eprintln!("A recording is active; controls remain hidden. Use hyprrecorder stop to save.");
        return Ok(());
    }
    let (settings, settings_error) = if smoke {
        (Settings::default(), None)
    } else {
        match Settings::load_user(){Ok(settings)=>(settings,None),Err(error)=>(Settings::default(),Some(format!("{error}. Saved preferences were preserved. Fix settings.toml or restore settings.last-good.toml in {}.",Settings::user_path()?.parent().ok_or("Missing config directory")?.display())))}
    };
    let monitors = if smoke {
        // Exercise production discovery parsing rather than bypassing it with
        // an already-filtered synthetic UI list.
        monitors::parse(
            r#"[{"name":"Preview","width":1920,"height":1080,"x":0,"y":0,"scale":1.0,"focused":true,"transform":0,"mirrorOf":"none"}]"#,
        )?
    } else {
        monitors::discover()?
    };
    let json =
        CString::new(serde_json::json!({"settings":settings,"monitors":monitors,"settings_error":settings_error}).to_string())
            .map_err(|e| e.to_string())?;
    let root =
        CString::new(ipc::runtime()?.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let exe = CString::new(
        std::env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    let path = CString::new(Settings::user_path()?.to_string_lossy().as_bytes())
        .map_err(|e| e.to_string())?;
    let code = unsafe {
        hr_ui_run(
            json.as_ptr(),
            root.as_ptr(),
            exe.as_ptr(),
            path.as_ptr(),
            i32::from(smoke),
            save,
            active,
        )
    };
    if code == 0 {
        Ok(())
    } else {
        Err(format!("Desktop window exited with status {code}"))
    }
}
