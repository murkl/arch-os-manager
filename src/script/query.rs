//! A script that only reads: no terminal, no questions, a time limit.

use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{Failure, Op, attach_fd3, command, pipe};

const LIMIT: Duration = Duration::from_secs(120);

/// Runs a script and returns what it printed. A script that elevates is refused.
pub fn query(op: Op, args: &[&str]) -> Result<String, Failure> {
    query_within(op, args, LIMIT)
}

/// As query, for a script that waits on a person, such as a notification.
pub fn query_within(op: Op, args: &[&str], limit: Duration) -> Result<String, Failure> {
    assert!(!op.meta().root, "{op} elevates and runs only as a run");
    let started = Instant::now();
    let result = run(op, args, limit);
    let millis = started.elapsed().as_millis();
    match &result {
        Ok(out) => crate::info!(
            "{op} {args:?}: {} lines in {millis} ms",
            out.lines().count()
        ),
        Err(f) => crate::error!(
            "{op} {args:?}: {} after {millis} ms\n{}",
            f.detail,
            f.tail.join("\n")
        ),
    }
    result
}

fn run(op: Op, args: &[&str], limit: Duration) -> Result<String, Failure> {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut cmd = command(op, &args).map_err(|e| Failure::new(op, e))?;
    let (events, events_w) = pipe().map_err(|e| Failure::new(op, e.to_string()))?;
    let raw = events_w.as_raw_fd();
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // SAFETY: attach_fd3 only calls async-signal-safe dup2/fcntl.
    unsafe { cmd.pre_exec(move || attach_fd3(raw)) };
    let mut child = cmd
        .spawn()
        .map_err(|e| Failure::new(op, format!("bash: {e}")))?;
    drop(events_w);

    let stderr = child.stderr.take().map(read_thread);
    let events = read_thread(std::fs::File::from(events));
    let pid = child.id() as i32;
    let timed_out = Arc::new(AtomicBool::new(false));
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let flag = timed_out.clone();
    std::thread::spawn(move || {
        if wait.recv_timeout(limit).is_err() {
            flag.store(true, Ordering::SeqCst);
            if let Some(group) = rustix::process::Pid::from_raw(pid) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
        }
    });

    let mut out = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_end(&mut out);
    }
    let status = child.wait().map_err(|e| Failure::new(op, e.to_string()))?;
    let _ = done.send(());
    let stderr = stderr
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let events = events.join().unwrap_or_default();

    if timed_out.load(Ordering::SeqCst) {
        let mut f = Failure::new(op, format!("{op}: no answer within {} s", limit.as_secs()));
        f.tail = stderr.lines().map(str::to_string).collect();
        return Err(f);
    }
    if status.success() {
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    let fail = events.lines().find(|l| l.starts_with("fail\t"));
    let code = status.code().or(status.signal().map(|s| 128 + s));
    let lines: Vec<&str> = stderr.lines().collect();
    let tail = lines[lines.len().saturating_sub(20)..]
        .iter()
        .map(|l| l.to_string())
        .collect();
    Err(Failure::classify(op, code, fail, tail))
}

fn read_thread(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::op;

    #[test]
    fn returns_what_the_script_printed() {
        let out = run(op::SYSTEM_STATE, &[], LIMIT).expect("system state runs");
        assert!(out.contains("hostname\t"));
    }

    #[test]
    fn stops_a_script_at_the_limit() {
        let failure = run(op::SYSTEM_STATE, &[], Duration::ZERO).unwrap_err();
        assert!(failure.detail.contains("no answer"), "{failure:?}");
    }
}
