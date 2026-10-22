//! `%APPDATA%\Araso`: settings, per-app rules and learned words. `ARASO_SUPPORT_DIR` points a
//! development copy (or a test) at another folder.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn url() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ARASO_SUPPORT_DIR") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            return Some(Path::new(&appdata).join("Araso"));
        }
    }
    std::env::var("HOME").ok().map(|home| Path::new(&home).join(".config").join("araso"))
}

pub fn file(name: &str) -> Option<PathBuf> {
    url().map(|dir| dir.join(name))
}

/// Moves a file that can't be read (hand-edited, or from a newer version) to
/// `<name>.unreadable`, so the next save doesn't write over what the user had.
pub fn set_aside(path: &Path) {
    let mut aside = path.as_os_str().to_owned();
    aside.push(".unreadable");
    let _ = std::fs::remove_file(&aside);
    let _ = std::fs::rename(path, &aside);
}

/// Writes `data` to `path` through a temporary file, so a reader never sees half a file.
pub fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".{}.tmp", std::process::id()));
    std::fs::write(&temp, data)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// When the file last changed, to notice another process writing it.
pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
