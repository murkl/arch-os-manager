//! One log for the runtime and every script it runs.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

static FILE: Mutex<Option<File>> = Mutex::new(None);

const ROTATE_AT: u64 = 2 * 1024 * 1024;

pub fn init(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > ROTATE_AT) {
        std::fs::rename(path, path.with_extension("log.1"))?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    *FILE.lock().unwrap() = Some(file);
    Ok(())
}

pub fn write(level: &str, msg: &str) {
    let mut guard = FILE.lock().unwrap();
    let Some(file) = guard.as_mut() else { return };
    let now = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S");
    for line in msg.lines() {
        let _ = writeln!(file, "{now} {level:<5} {line}");
    }
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::write("INFO", &format!($($arg)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::log::write("WARN", &format!($($arg)*)) };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { $crate::log::write("ERROR", &format!($($arg)*)) };
}
