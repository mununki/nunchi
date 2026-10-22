//! A log for diagnosing the input method in other apps: written only while the file
//! `%APPDATA%\Araso\logging` exists, to `%APPDATA%\Araso\logs\<exe>-<pid>.log`. Never typed text:
//! the kind of key and what happened to it, as upstream logs.

use std::io::Write;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| araso_core::support_dir::file("logging").is_some_and(|p| p.exists()))
}

pub fn write(message: &str) {
    if !enabled() {
        return;
    }
    let Some(dir) = araso_core::support_dir::file("logs") else { return };
    let _ = std::fs::create_dir_all(&dir);
    let exe = crate::runtime::app_name().unwrap_or_else(|| "unknown".into());
    let path = dir.join(format!("{exe}-{}.log", std::process::id()));
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let _ = writeln!(file, "{}.{:03} {message}", now.as_secs(), now.subsec_millis());
    }
}
