//! A script that only reads: no terminal, no questions, a time limit.

use std::io::{BufRead, BufReader, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{Failure, Op, attach_fd3, command, pipe};

const LIMIT: Duration = Duration::from_secs(120);

/// The nice value of a query: it reads in the background, a run is what the person waits for.
const NICE: i32 = 10;

/// Runs a script and returns what it printed. A script that elevates is refused.
pub fn query(op: Op, args: &[&str]) -> Result<String, Failure> {
    query_within(op, args, LIMIT)
}

/// As query, for a script that waits on a person, such as a notification.
pub fn query_within(op: Op, args: &[&str], limit: Duration) -> Result<String, Failure> {
    query_lines(op, args, limit, |_| {})
}

/// As query_within, telling each line of the output as it comes, before the script ends.
pub fn query_lines(
    op: Op,
    args: &[&str],
    limit: Duration,
    mut each: impl FnMut(&str),
) -> Result<String, Failure> {
    assert!(!op.meta().root, "{op} elevates and runs only as a run");
    let started = Instant::now();
    let result = run(op, args, limit, &mut each);
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

fn run(
    op: Op,
    args: &[&str],
    limit: Duration,
    each: &mut dyn FnMut(&str),
) -> Result<String, Failure> {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let cmd = command(op, &args).map_err(|e| Failure::new(op, e))?;
    collect(op, cmd, limit, each)
}

/// What the command of a query printed, within the limit, each line told as it comes.
fn collect(
    op: Op,
    mut cmd: Command,
    limit: Duration,
    each: &mut dyn FnMut(&str),
) -> Result<String, Failure> {
    let (events, events_w) = pipe().map_err(|e| Failure::new(op, e.to_string()))?;
    let raw = events_w.as_raw_fd();
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A session of its own: no terminal to stop at or write into, one group the limit stops whole,
    // and behind the interface for the processors and the disk, whose priority Linux takes from the
    // nice value.
    // SAFETY: setsid, setpriority, dup2 and fcntl are async-signal-safe.
    unsafe {
        cmd.pre_exec(move || {
            rustix::process::setsid()?;
            let _ = rustix::process::setpriority_process(None, NICE);
            attach_fd3(raw)
        })
    };
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
    if let Some(stdout) = child.stdout.take() {
        let mut lines = BufReader::new(stdout);
        loop {
            let start = out.len();
            match lines.read_until(b'\n', &mut out) {
                Ok(0) | Err(_) => break,
                Ok(_) => each(String::from_utf8_lossy(&out[start..]).trim_end_matches('\n')),
            }
        }
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
        let out = run(op::SYSTEM_STATE, &[], LIMIT, &mut |_| {}).expect("system state runs");
        assert!(out.contains("hostname\t"));
    }

    #[test]
    fn stops_a_script_at_the_limit() {
        let failure = run(op::SYSTEM_STATE, &[], Duration::ZERO, &mut |_| {}).unwrap_err();
        assert!(failure.detail.contains("no answer"), "{failure:?}");
    }

    #[test]
    fn tells_each_line_before_the_script_ends() {
        let started = Instant::now();
        let mut seen = Vec::new();
        let mut cmd = Command::new("bash");
        cmd.args(["-c", "echo first; sleep 1; printf last"]);
        let out = collect(op::SYSTEM_STATE, cmd, LIMIT, &mut |line| {
            seen.push((line.to_string(), started.elapsed()))
        })
        .unwrap();
        assert_eq!(out, "first\nlast");
        assert_eq!(seen[0].0, "first");
        assert!(seen[0].1 < Duration::from_millis(900), "{seen:?}");
        assert_eq!(seen[1].0, "last");
    }

    #[test]
    fn a_query_reads_behind_the_interface() {
        let ours = rustix::process::getpriority_process(None).unwrap();
        let out = collect(op::SYSTEM_STATE, Command::new("nice"), LIMIT, &mut |_| {}).unwrap();
        assert_eq!(out.trim(), (ours + NICE).min(19).to_string());
    }
}
