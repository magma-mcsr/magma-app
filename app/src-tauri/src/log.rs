//! A log of what held sync up (rate limits, network and server errors, updates), kept as
//! `magma.log` next to the settings, so a slow upload can be explained afterwards. It holds only
//! times, match numbers, player names from your list and error messages, and it is never sent
//! anywhere. Past 1 MB it is renamed to `magma.old.log` and a new one is started.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 1_000_000;

static FILE: OnceLock<PathBuf> = OnceLock::new();
static WRITING: Mutex<()> = Mutex::new(());

/// Start writing to `dir`. Lines from before this only go to the console.
pub fn init(dir: &Path) {
    let _ = FILE.set(dir.join("magma.log"));
}

pub fn write(line: &str) {
    eprintln!("{line}");
    let Some(path) = FILE.get() else { return };
    let _lock = WRITING.lock();
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("old.log"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

/// Like `eprintln!`, and also written to the log file.
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::log::write(&format!($($arg)*))
    };
}
