//! A script on its own pseudo terminal, so pacman and its kin behave as at a console.

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::sync::Arc;

use rustix::event::{PollFd, PollFlags, poll};
use rustix::fs::{Mode, OFlags};
use rustix::io::Errno;
use rustix::pty::OpenptFlags;
use rustix::termios::{LocalModes, Winsize};

use super::{Op, attach_fd3, command, pipe};

#[derive(Debug)]
pub enum Event {
    Output(Vec<u8>),
    /// A line lib.sh wrote to fd 3.
    Script(String),
    /// The exit code. A signal counts as 128 plus its number.
    Exit(i32),
}

pub struct Session {
    master: Arc<OwnedFd>,
}

impl Session {
    pub fn start(
        op: Op,
        args: &[String],
        size: (u16, u16),
        sink: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Session, String> {
        Session::spawn(run_command(op, args)?, size, sink)
    }

    /// Runs `cmd` on a new pseudo terminal, with fd 3 for lib.sh's events.
    pub(super) fn spawn(
        mut cmd: std::process::Command,
        (rows, cols): (u16, u16),
        sink: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Session, String> {
        let master =
            rustix::pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)
                .map_err(err)?;
        rustix::pty::grantpt(&master).map_err(err)?;
        rustix::pty::unlockpt(&master).map_err(err)?;
        let path = rustix::pty::ptsname(&master, Vec::new()).map_err(err)?;
        let slave = rustix::fs::open(
            path.as_c_str(),
            OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(err)?;
        let session = Session {
            master: Arc::new(master),
        };
        session.resize(rows, cols);

        let (events, events_w) = pipe().map_err(|e| e.to_string())?;
        // The reader takes events between reads of the terminal without waiting for more.
        rustix::io::ioctl_fionbio(&events, true).map_err(err)?;
        let raw = events_w.as_raw_fd();
        cmd.env("TERM", "xterm-256color")
            .stdin(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stdout(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut cmd, move || {
                rustix::process::setsid()?;
                rustix::process::ioctl_tiocsctty(BorrowedFd::borrow_raw(0))?;
                attach_fd3(raw)
            });
        }
        let mut child = cmd.spawn().map_err(|e| format!("bash: {e}"))?;
        drop(cmd);
        drop(events_w);

        let sink = Arc::new(sink);
        let (terminal, out) = (session.master.clone(), sink.clone());
        std::thread::spawn(move || relay(terminal.as_fd(), events, &*out));
        std::thread::spawn(move || {
            let code = match child.wait() {
                Ok(s) => s.code().or(s.signal().map(|n| 128 + n)).unwrap_or(1),
                Err(_) => 1,
            };
            sink(Event::Exit(code));
        });
        Ok(session)
    }

    pub fn write(&self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            match rustix::io::write(&*self.master, bytes) {
                Ok(n) => bytes = &bytes[n..],
                Err(rustix::io::Errno::INTR) => {}
                Err(_) => return,
            }
        }
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        let size = Winsize {
            ws_row: rows.max(1),
            ws_col: cols.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let _ = rustix::termios::tcsetwinsize(&*self.master, size);
    }

    /// Whether the terminal echoes. On Linux the master reports its slave's modes.
    pub fn echoes(&self) -> bool {
        rustix::termios::tcgetattr(&*self.master)
            .is_ok_and(|t| t.local_modes.contains(LocalModes::ECHO))
    }
}

/// Reads the terminal and lib.sh's events, which never block, in one place. After each read of
/// the terminal the events written meanwhile go first, so an event reaches the runtime before any
/// output written after it, as the hand-over before the AUR helper's first question.
fn relay(terminal: BorrowedFd<'_>, events: OwnedFd, sink: &dyn Fn(Event)) {
    let mut events = Some(events);
    let (mut open, mut pending, mut buf) = (true, Vec::new(), [0u8; 8192]);
    while open || events.is_some() {
        let mut fds = Vec::with_capacity(2);
        if open {
            fds.push(PollFd::new(&terminal, PollFlags::IN));
        }
        if let Some(e) = &events {
            fds.push(PollFd::new(e, PollFlags::IN));
        }
        match poll(&mut fds, None) {
            Ok(_) | Err(Errno::INTR) => {}
            Err(_) => break,
        }
        let output = match open && !fds[0].revents().is_empty() {
            false => None,
            // Ends with EIO once no process holds the terminal any more.
            true => match rustix::io::read(terminal, &mut buf) {
                Ok(n) if n > 0 => Some(buf[..n].to_vec()),
                Err(Errno::INTR) => None,
                _ => {
                    open = false;
                    None
                }
            },
        };
        if events.as_ref().is_some_and(|e| !drain(e, &mut pending)) {
            events = None;
        }
        while let Some(at) = pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = pending.drain(..=at).collect();
            sink(Event::Script(
                String::from_utf8_lossy(&line[..at]).into_owned(),
            ));
        }
        if let Some(bytes) = output {
            sink(Event::Output(bytes));
        }
    }
    if !pending.is_empty() {
        sink(Event::Script(
            String::from_utf8_lossy(&pending).into_owned(),
        ));
    }
}

/// Takes what lib.sh wrote, without waiting. False once its end is closed.
fn drain(events: &OwnedFd, pending: &mut Vec<u8>) -> bool {
    let mut buf = [0u8; 4096];
    loop {
        match rustix::io::read(events, &mut buf) {
            Ok(0) => return false,
            Ok(n) => pending.extend_from_slice(&buf[..n]),
            Err(Errno::INTR) => {}
            Err(e) => return e == Errno::AGAIN,
        }
    }
}

/// The command of a run. Only what elevates holds shutdown off: systemctl refuses a reboot while
/// its caller's own lock blocks it.
fn run_command(op: Op, args: &[String]) -> Result<std::process::Command, String> {
    let mut cmd = command(op, args)?;
    if op.meta().root {
        cmd.env("SM_RUN", op.title());
    }
    if super::terminal() {
        cmd.env("SM_TERMINAL", "on");
    }
    Ok(cmd)
}

fn err(e: rustix::io::Errno) -> String {
    format!("pseudo terminal: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::op;

    fn holds(op: Op) -> bool {
        run_command(op, &[])
            .unwrap()
            .get_envs()
            .any(|(k, v)| k == "SM_RUN" && v.is_some())
    }

    /// Both written before the reader looks: the event comes first, as it was written first.
    #[test]
    fn an_event_arrives_before_the_output_written_after_it() {
        let (terminal, output) = crate::script::pipe().unwrap();
        let (events, script) = crate::script::pipe().unwrap();
        rustix::io::ioctl_fionbio(&events, true).unwrap();
        rustix::io::write(&script, b"terminal\ton\n").unwrap();
        rustix::io::write(&output, b":: Proceed with installation? [Y/n] ").unwrap();
        drop((output, script));
        let seen = std::sync::Mutex::new(Vec::new());
        relay(terminal.as_fd(), events, &|e| seen.lock().unwrap().push(e));
        let seen = seen.into_inner().unwrap();
        assert!(
            matches!(&seen[..], [Event::Script(s), Event::Output(o)]
                if s == "terminal\ton" && o.starts_with(b":: Proceed")),
            "{seen:?}"
        );
    }

    #[test]
    fn only_a_run_that_elevates_holds_shutdown_off() {
        assert!(holds(op::UPGRADE_RUN));
        assert!(!holds(op::SYSTEM_REBOOT));
    }
}
