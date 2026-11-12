//! Save label projects without truncating an existing file in place.
//!
//! Staging and temporary recovery data stay in a new dedicated directory beside
//! the target, so replacement is on the same volume. The caller remains
//! responsible for warning about unsaved changes and concurrent edits.
use std::{fs, io::Write, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Write a new project, or replace an existing project when `overwrite` is true.
/// A failed replacement retains the previous file or identifies its recovery
/// copy in the returned error. This does not resolve concurrent external edits.
pub fn write_project(path: &Path, bytes: &[u8], overwrite: bool) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    if path.file_name().is_none() { return Err("Choose a project file name.".into()); }
    if overwrite {
        let meta = fs::symlink_metadata(path).map_err(|e| format!("Could not inspect the existing project: {e}"))?;
        if !meta.file_type().is_file() { return Err("The existing project is not a regular file.".into()); }
    }
    let staging = create_staging_dir(parent)?;
    let replacement = staging.join("new.420p");
    let backup = staging.join("previous.420p");
    let prepared = (|| -> Result<(), String> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&replacement)
            .map_err(|e| format!("Could not prepare the project: {e}"))?;
        file.write_all(bytes).map_err(|e| format!("Could not write the project: {e}"))?;
        file.sync_all().map_err(|e| format!("Could not finish writing the project: {e}"))?;
        Ok(())
    })();
    if let Err(error) = prepared {
        return Err(cleanup_error(error, &replacement, &staging));
    }

    if !overwrite {
        if let Err(error) = move_new(&replacement, path) {
            return Err(cleanup_error(format!("Could not create the project without overwriting an existing file: {error}"), &replacement, &staging));
        }
        return fs::remove_dir(&staging).map_err(|e| format!("Project saved, but the empty temporary folder could not be removed ({}): {e}", staging.display()));
    }

    if let Err(error) = replace_existing(path, &replacement, &backup) {
        // ReplaceFileW may move the old project to the backup even on failure.
        // Restore only if the original name is absent; never overwrite a file.
        if !path.exists() && backup.exists() {
            if let Err(restore_error) = move_new(&backup, path) {
                return Err(format!("Could not save the project: {error}. The original is retained at {}; recovery to {} failed: {restore_error}. The new version is at {}.", backup.display(), path.display(), replacement.display()));
            }
        }
        if backup.exists() || !path.exists() {
            return Err(format!("Could not save the project: {error}. Recovery files were retained in {}; check them before retrying.", staging.display()));
        }
        return Err(cleanup_error(format!("Could not save the project; the existing file was retained: {error}"), &replacement, &staging));
    }

    match fs::read(path) {
        Ok(saved) if saved == bytes => {}
        Ok(_) => return Err(format!("The saved project did not match the requested contents. The previous version is retained at {}.", backup.display())),
        Err(error) => return Err(format!("Could not verify the saved project: {error}. The previous version is retained at {}.", backup.display())),
    }
    fs::remove_file(&backup).map_err(|e| format!("The project was saved and verified, but its temporary previous copy could not be removed ({}): {e}", backup.display()))?;
    fs::remove_dir(&staging).map_err(|e| format!("The project was saved and verified, but its empty temporary folder could not be removed ({}): {e}", staging.display()))?;
    Ok(())
}

fn create_staging_dir(parent: &Path) -> Result<PathBuf, String> {
    for _ in 0..32 {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = parent.join(format!(".dymo-save-{}-{id}", std::process::id()));
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not create a temporary project folder: {e}")),
        }
    }
    Err("Could not allocate a unique temporary project folder.".into())
}

fn cleanup_error(error: String, replacement: &Path, staging: &Path) -> String {
    // Only these exact files/directories were created by this save operation.
    let file_error = match fs::remove_file(replacement) {
        Ok(()) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(e),
    };
    let directory_error = fs::remove_dir(staging).err();
    if file_error.is_some() || directory_error.is_some() {
        format!("{error}. Temporary recovery data may remain in {}.", staging.display())
    } else { error }
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn move_new(from: &Path, to: &Path) -> std::io::Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" { fn MoveFileW(existing: *const u16, new: *const u16) -> i32; }
    let (from, to) = (wide(from), wide(to));
    // SAFETY: Both pointers refer to live, NUL-terminated UTF-16 buffers.
    if unsafe { MoveFileW(from.as_ptr(), to.as_ptr()) } == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(windows)]
fn replace_existing(target: &Path, replacement: &Path, backup: &Path) -> std::io::Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" { fn ReplaceFileW(replaced: *const u16, replacement: *const u16, backup: *const u16, flags: u32, exclude: *mut std::ffi::c_void, reserved: *mut std::ffi::c_void) -> i32; }
    let (target, replacement, backup) = (wide(target), wide(replacement), wide(backup));
    // SAFETY: The three paths are valid, live, NUL-terminated UTF-16 buffers;
    // the final two reserved arguments must be null. The backup is on the same volume.
    let result = unsafe { ReplaceFileW(target.as_ptr(), replacement.as_ptr(), backup.as_ptr(), 0, std::ptr::null_mut(), std::ptr::null_mut()) };
    if result == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let base = std::env::temp_dir();
        create_staging_dir(&base).expect("create isolated test folder")
    }
    #[test]
    fn save_new_and_replace_verify_contents_and_clean_staging() {
        let dir = fixture();
        let path = dir.join("label.420p");
        write_project(&path, b"first", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert!(write_project(&path, b"should not overwrite", false).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"first");
        write_project(&path, b"second", true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn overwrite_requires_an_existing_regular_file() {
        let dir = fixture();
        assert!(write_project(&dir.join("missing.420p"), b"content", true).is_err());
        assert!(write_project(&dir, b"content", true).is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn locked_original_remains_intact_when_replacement_fails() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = fixture();
        let path = dir.join("label.420p");
        fs::write(&path, b"old").unwrap();
        let locked = fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        assert!(write_project(&path, b"new", true).is_err());
        drop(locked);
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
