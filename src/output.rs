//! Exclusive partial creation and no-replace publication. Never infer success from existence.
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Output {
    pub partial: PathBuf,
    pub final_path: PathBuf,
}
impl Output {
    pub fn create(directory: &Path) -> Result<(Self, File), String> {
        fs::create_dir_all(directory).map_err(|e| format!("Cannot create destination: {e}"))?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis();
        for suffix in 0..100 {
            let name = format!("Hyprrecorder-{stamp}-{}-{suffix}", std::process::id());
            let partial = directory.join(format!("{name}.partial.mkv"));
            let final_path = directory.join(format!("{name}.mkv"));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&partial)
            {
                Ok(file) => {
                    return Ok((
                        Self {
                            partial,
                            final_path,
                        },
                        file,
                    ))
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("Cannot create recording: {e}")),
            }
        }
        Err("Cannot allocate a unique recording name".into())
    }
    pub fn publish(&self) -> Result<(), String> {
        File::open(&self.partial)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("Cannot sync partial: {e}"))?;
        fs::hard_link(&self.partial, &self.final_path)
            .map_err(|e| format!("Cannot publish without overwrite: {e}"))?;
        fs::remove_file(&self.partial)
            .map_err(|e| format!("Saved link exists but partial cleanup failed: {e}"))?;
        File::open(
            self.final_path
                .parent()
                .ok_or("Missing destination parent")?,
        )
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("Cannot sync destination: {e}"))
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing settings parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut created = None;
    for suffix in 0..100 {
        let temp = parent.join(format!(".settings-{}-{suffix}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
        {
            Ok(file) => {
                created = Some((temp, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    let (temp, mut file) = created.ok_or("Cannot allocate settings temporary file")?;
    let result = (|| {
        use std::io::Write;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(temp); // This call exclusively created it.
    result
}

pub fn private_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err("Runtime directory must be a private directory owned by this user".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn publication_cannot_replace_existing_content_and_partials_are_private() {
        let root = std::env::temp_dir().join(format!("hyprrecorder-output-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let (out, f) = Output::create(&root).unwrap();
        assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        fs::write(&out.final_path, b"previous").unwrap();
        assert!(out.publish().is_err());
        assert_eq!(fs::read(&out.final_path).unwrap(), b"previous");
        assert!(out.partial.exists());
        fs::remove_file(&out.final_path).unwrap();
        out.publish().unwrap();
        assert!(!out.partial.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
