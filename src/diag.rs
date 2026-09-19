//! Minimal diagnostic log at `%APPDATA%\deskpulse\diag.log`.
//!
//! Each line is prefixed with milliseconds since process start, so timing
//! between events (e.g. a menu click and the actual exit) can be measured
//! without a console.

use std::io::Write as _;
use std::sync::OnceLock;
use std::time::Instant;

fn start() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

fn path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(
        std::path::PathBuf::from(base)
            .join("deskpulse")
            .join("diag.log"),
    )
}

/// Truncates the log and writes the first line.
pub fn reset(message: &str) {
    let Some(path) = path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("[0 ms] {message}\n"));
}

/// Appends a timestamped line.
pub fn log(message: &str) {
    let Some(path) = path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "[{} ms] {message}", start().elapsed().as_millis());
    }
}
