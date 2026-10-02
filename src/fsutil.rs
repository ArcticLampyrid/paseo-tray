//! Crash-safe file replacement shared by every file the tray writes.

use anyhow::{Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU32, Ordering},
};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Replaces `path` with `contents` via a uniquely named temp file in the same directory:
/// created owner-only from the start (these files can hold credentials), flushed to disk,
/// given the old file's permissions, then renamed over the target. Never leaves a
/// truncated file, and concurrent writers cannot clobber each other's temp file.
/// A symlinked target is replaced through the link, not turned into a regular file.
pub fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = path.parent().context("path has no parent directory")?;
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .context("path has no file name")?
        .to_string_lossy();
    let temp = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = write_temp(&temp, &path, contents).and_then(|()| {
        fs::rename(&temp, &path).with_context(|| format!("replacing {}", path.display()))
    });
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_temp(temp: &Path, target: &Path, contents: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(temp)
        .with_context(|| format!("creating {}", temp.display()))?;
    if let Ok(meta) = fs::metadata(target) {
        file.set_permissions(meta.permissions())?;
    }
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", temp.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("paseo-tray-fsutil-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn replaces_contents_and_leaves_no_temp_files() {
        let dir = scratch("replace");
        let file = dir.join("a.json");
        write_atomically(&file, "one").unwrap();
        write_atomically(&file, "two").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "two");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn new_files_are_owner_only_and_existing_modes_are_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("mode");
        let file = dir.join("a.json");
        write_atomically(&file, "x").unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
        write_atomically(&file, "y").unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o640
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_a_symlink() {
        let dir = scratch("link");
        let real = dir.join("real.json");
        let link = dir.join("link.json");
        fs::write(&real, "old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomically(&link, "new").unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        fs::remove_dir_all(&dir).unwrap();
    }
}
