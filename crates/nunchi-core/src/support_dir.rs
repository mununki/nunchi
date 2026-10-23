//! `%APPDATA%\Nunchi`: settings, per-app rules and learned words. `NUNCHI_SUPPORT_DIR` points a
//! development copy (or a test) at another folder.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

pub fn url() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NUNCHI_SUPPORT_DIR") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            static DIR: OnceLock<PathBuf> = OnceLock::new();
            return Some(DIR.get_or_init(|| moved_in(Path::new(&appdata))).clone());
        }
    }
    std::env::var("HOME").ok().map(|home| Path::new(&home).join(".config").join("nunchi"))
}

/// The folder in `appdata`. Nunchi was called Araso before and kept its files in `Araso`: the
/// first process to look moves that over, so the settings and the words learned carry on. While it
/// can't (a file in it is open), the old folder is used.
fn moved_in(appdata: &Path) -> PathBuf {
    let dir = appdata.join("Nunchi");
    let old = appdata.join("Araso");
    if !dir.exists() && old.is_dir() && std::fs::rename(&old, &dir).is_err() && !dir.exists() {
        return old;
    }
    dir
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

/// What tells a file changed, to notice another process writing it: when it was last written, and
/// how long it is. Two writes can fall in one tick of the file's time (another app's input method
/// writing right after this one); the length tells most of them apart still.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stamp {
    modified: SystemTime,
    length: u64,
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Stamp { modified: metadata.modified().ok()?, length: metadata.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_the_folder_from_the_old_name() {
        let appdata = std::env::temp_dir().join(format!("nunchi-appdata-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&appdata);
        std::fs::create_dir_all(appdata.join("Araso")).unwrap();
        std::fs::write(appdata.join("Araso").join("dictionary.json"), "{}").unwrap();

        assert_eq!(moved_in(&appdata), appdata.join("Nunchi"));
        assert!(appdata.join("Nunchi").join("dictionary.json").exists());
        assert!(!appdata.join("Araso").exists());

        // Once moved, a folder of the old name made again (an app still on the old version) stays.
        std::fs::create_dir_all(appdata.join("Araso")).unwrap();
        assert_eq!(moved_in(&appdata), appdata.join("Nunchi"));
        assert!(appdata.join("Araso").exists());
        let _ = std::fs::remove_dir_all(&appdata);
    }
}
