//! One log for the runtime and every script it runs.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LOG: Mutex<Option<Log>> = Mutex::new(None);

const ROTATE_AT: u64 = 2 * 1024 * 1024;

struct Log {
    path: PathBuf,
    file: File,
}

impl Log {
    /// Moves a full log aside first, unless another process did so already.
    fn open(path: &Path) -> std::io::Result<Log> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::metadata(path).is_ok_and(|m| m.len() > ROTATE_AT) {
            let _ = std::fs::rename(path, path.with_extension("log.1"));
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Log {
            path: path.to_path_buf(),
            file,
        })
    }

    /// One write per message, so the lines of the interface and the timer do not interleave.
    /// Past the limit, also in a long session, the log starts anew.
    fn write(&mut self, level: &str, msg: &str) {
        let now = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S");
        let text: String = msg
            .lines()
            .map(|line| format!("{now} {level:<5} {line}\n"))
            .collect();
        let _ = self.file.write_all(text.as_bytes());
        if self.file.metadata().is_ok_and(|m| m.len() > ROTATE_AT)
            && let Ok(fresh) = Log::open(&self.path)
        {
            *self = fresh;
        }
    }
}

pub fn init(path: &Path) -> std::io::Result<()> {
    let log = Log::open(path)?;
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(log);
    Ok(())
}

pub fn write(level: &str, msg: &str) {
    if let Some(log) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        log.write(level, msg);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_every_line_and_starts_anew_past_the_limit() {
        let dir = std::env::temp_dir().join(format!("sm-log-{}", std::process::id()));
        let path = dir.join("logs/system-manager.log");
        let mut log = Log::open(&path).unwrap();
        log.write("ERROR", "first\nsecond");
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].ends_with(" ERROR first") && lines[1].ends_with(" ERROR second"));
        assert_eq!(lines[0].len(), "2026-10-18 12:00:00 ERROR first".len());

        let long = "x".repeat(1024);
        for _ in 0..2100 {
            log.write("OUT", &long);
        }
        let rotated = std::fs::metadata(path.with_extension("log.1"))
            .unwrap()
            .len();
        assert!(rotated > ROTATE_AT, "{rotated}");
        assert!(std::fs::metadata(&path).unwrap().len() < ROTATE_AT);
        // Another process with the old file open follows to the new one.
        let mut other = Log {
            path: path.clone(),
            file: OpenOptions::new()
                .append(true)
                .open(path.with_extension("log.1"))
                .unwrap(),
        };
        other.write("INFO", "from the timer");
        other.write("INFO", "next");
        let kept = std::fs::metadata(path.with_extension("log.1"))
            .unwrap()
            .len();
        assert!(kept > rotated, "the rotated log stays: {kept}");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .ends_with(" INFO  next\n")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
