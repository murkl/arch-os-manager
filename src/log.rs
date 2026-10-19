//! One log for the runtime and every script it runs, rotated at 2 MiB. A thread of its own
//! writes it: a run's output, line by line, never waits on the disk in the interface's thread.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

static LOG: Mutex<Option<Writer>> = Mutex::new(None);

const ROTATE_AT: u64 = 2 * 1024 * 1024;

/// What a write sends at most at once, so a flood still reaches the disk in steps.
const BATCH: usize = 64 * 1024;

/// Messages waiting for the disk at most. Past it, as while the disk hangs, a message is counted
/// instead of kept, and the count is written once the disk answers.
const QUEUE: usize = 16 * 1024;

struct Log {
    path: PathBuf,
    file: File,
}

/// A message as it was sent, stamped then.
struct Entry {
    at: jiff::Timestamp,
    level: &'static str,
    msg: String,
}

struct Writer {
    tx: SyncSender<Entry>,
    /// Messages the full queue turned away since the last write.
    dropped: Arc<AtomicUsize>,
    thread: JoinHandle<()>,
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

    /// Whole lines in one write, so the lines of the interface and the timer do not interleave.
    /// Past the limit, also in a long session, the log starts anew.
    fn write(&mut self, text: &str) {
        let _ = self.file.write_all(text.as_bytes());
        if self.file.metadata().is_ok_and(|m| m.len() > ROTATE_AT)
            && let Ok(fresh) = Log::open(&self.path)
        {
            *self = fresh;
        }
    }
}

/// A message as lines of the log, each with the time and the level.
fn lines(entry: &Entry, zone: &jiff::tz::TimeZone) -> String {
    let at = entry.at.to_zoned(zone.clone());
    let now = at.strftime("%Y-%m-%d %H:%M:%S");
    entry
        .msg
        .lines()
        .map(|line| format!("{now} {:<5} {line}\n", entry.level))
        .collect()
}

/// Writes what arrives until the last sender is gone, each write with what arrived meanwhile.
fn write_out(rx: Receiver<Entry>, dropped: &AtomicUsize, mut log: Log, zone: jiff::tz::TimeZone) {
    while let Ok(first) = rx.recv() {
        let mut text = lines(&first, &zone);
        while text.len() < BATCH
            && let Ok(next) = rx.try_recv()
        {
            text.push_str(&lines(&next, &zone));
        }
        let lost = dropped.swap(0, Ordering::Relaxed);
        if lost > 0 {
            let entry = Entry {
                at: jiff::Timestamp::now(),
                level: "WARN",
                msg: format!("{lost} messages dropped, the disk was too slow"),
            };
            text.push_str(&lines(&entry, &zone));
        }
        log.write(&text);
    }
}

/// Queues a message for the disk, or counts it where the queue is full.
fn offer(tx: &SyncSender<Entry>, dropped: &AtomicUsize, entry: Entry) {
    if let Err(TrySendError::Full(_)) = tx.try_send(entry) {
        dropped.fetch_add(1, Ordering::Relaxed);
    }
}

impl Writer {
    /// The thread that owns the log.
    fn spawn(log: Log, zone: jiff::tz::TimeZone) -> Writer {
        let (tx, rx) = mpsc::sync_channel::<Entry>(QUEUE);
        let dropped = Arc::new(AtomicUsize::new(0));
        let counted = dropped.clone();
        let thread = std::thread::spawn(move || write_out(rx, &counted, log, zone));
        Writer {
            tx,
            dropped,
            thread,
        }
    }

    fn finish(self) {
        drop(self.tx);
        let _ = self.thread.join();
    }
}

pub fn init(path: &Path) -> std::io::Result<()> {
    let writer = Writer::spawn(Log::open(path)?, jiff::tz::TimeZone::system());
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(writer);
    Ok(())
}

pub fn write(level: &'static str, msg: &str) {
    if let Some(w) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let entry = Entry {
            at: jiff::Timestamp::now(),
            level,
            msg: msg.to_string(),
        };
        offer(&w.tx, &w.dropped, entry);
    }
}

/// Writes what is still on its way, before the program ends.
pub fn finish() {
    let writer = LOG.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(writer) = writer {
        writer.finish();
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

    fn entry(level: &'static str, msg: &str) -> Entry {
        Entry {
            at: jiff::Timestamp::now(),
            level,
            msg: msg.to_string(),
        }
    }

    #[test]
    fn stamps_every_line_and_starts_anew_past_the_limit() {
        let dir = std::env::temp_dir().join(format!("sm-log-{}", std::process::id()));
        let path = dir.join("logs/system-manager.log");
        let mut log = Log::open(&path).unwrap();
        let zone = jiff::tz::TimeZone::UTC;
        log.write(&lines(&entry("ERROR", "first\nsecond"), &zone));
        let text = std::fs::read_to_string(&path).unwrap();
        let written: Vec<&str> = text.lines().collect();
        assert_eq!(written.len(), 2, "{text}");
        assert!(written[0].ends_with(" ERROR first") && written[1].ends_with(" ERROR second"));
        assert_eq!(written[0].len(), "2026-10-18 12:00:00 ERROR first".len());

        let long = "x".repeat(1024);
        for _ in 0..2100 {
            log.write(&lines(&entry("OUT", &long), &zone));
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
        other.write(&lines(&entry("INFO", "from the timer"), &zone));
        other.write(&lines(&entry("INFO", "next"), &zone));
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

    /// The thread writes what was sent, in order, once the program finishes.
    #[test]
    fn what_was_sent_is_written_in_order_by_the_end() {
        let dir = std::env::temp_dir().join(format!("sm-log-thread-{}", std::process::id()));
        let path = dir.join("system-manager.log");
        let writer = Writer::spawn(Log::open(&path).unwrap(), jiff::tz::TimeZone::UTC);
        for n in 0..1000 {
            let _ = writer.tx.send(entry("OUT", &format!("line {n}")));
        }
        writer.finish();
        let text = std::fs::read_to_string(&path).unwrap();
        let numbers: Vec<usize> = text
            .lines()
            .filter_map(|l| l.rsplit(' ').next()?.parse().ok())
            .collect();
        assert_eq!(numbers, (0..1000).collect::<Vec<_>>());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A full queue keeps the interface going: the message is counted, and the count written.
    #[test]
    fn a_full_queue_counts_what_it_drops_and_says_so() {
        let (tx, rx) = mpsc::sync_channel(1);
        let dropped = AtomicUsize::new(0);
        for n in 0..3 {
            offer(&tx, &dropped, entry("INFO", &n.to_string()));
        }
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
        drop(tx);

        let dir = std::env::temp_dir().join(format!("sm-log-full-{}", std::process::id()));
        let path = dir.join("system-manager.log");
        write_out(
            rx,
            &dropped,
            Log::open(&path).unwrap(),
            jiff::tz::TimeZone::UTC,
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("INFO  0\n") && text.contains("WARN  2 messages dropped"),
            "{text}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
