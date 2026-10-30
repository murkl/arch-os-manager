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

use super::{Op, attach, command, pipe};

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
        let (cmd, given) = run_command(op, args)?;
        Session::spawn_with(cmd, size, sink, given)
    }

    /// As `spawn_with`, given nothing.
    #[cfg(test)]
    pub(super) fn spawn(
        cmd: std::process::Command,
        size: (u16, u16),
        sink: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Session, String> {
        Session::spawn_with(cmd, size, sink, Vec::new())
    }

    /// Runs `cmd` on a new pseudo terminal, with fd 3 for lib.sh's events and what `given` holds
    /// at fd 4 and on.
    fn spawn_with(
        mut cmd: std::process::Command,
        (rows, cols): (u16, u16),
        sink: impl Fn(Event) + Send + Sync + 'static,
        given: Vec<OwnedFd>,
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
        let mut fds = vec![(events_w.as_raw_fd(), 3)];
        fds.extend(given.iter().map(AsRawFd::as_raw_fd).zip(4..));
        cmd.env("TERM", "xterm-256color")
            .stdin(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stdout(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut cmd, move || {
                rustix::process::setsid()?;
                rustix::process::ioctl_tiocsctty(BorrowedFd::borrow_raw(0))?;
                attach(&fds)
            });
        }
        let mut child = cmd.spawn().map_err(|e| format!("bash: {e}"))?;
        drop(cmd);
        drop((events_w, given));

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

/// The command of a run, and what it is given at fd 4 and on. Only what elevates holds shutdown
/// off: systemctl refuses a reboot while its caller's own lock blocks it. With the password held,
/// sudo takes it from fd 4 and this program's renewal from fd 5, never from the terminal, where
/// any program of the run could ask for it in sudo's words.
fn run_command(op: Op, args: &[String]) -> Result<(std::process::Command, Vec<OwnedFd>), String> {
    let mut cmd = command(op, args)?;
    let mut given = Vec::new();
    if op.meta().root {
        cmd.env("SM_RUN", op.title());
        if let Some(password) = super::sudo::get() {
            cmd.env("SM_PASSWORD", "on");
            if let Ok(program) = crate::paths::program() {
                cmd.env("SM_PROGRAM", program);
            }
            given = vec![holding(&password)?, holding(&password)?];
        }
    }
    if super::terminal() {
        cmd.env("SM_TERMINAL", "on");
    }
    Ok((cmd, given))
}

/// The end of a pipe that holds `password` for one read.
fn holding(password: &str) -> Result<OwnedFd, String> {
    use std::io::Write;
    let (read, write) = pipe().map_err(|e| e.to_string())?;
    std::fs::File::from(write)
        .write_all(format!("{password}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(read)
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
            .0
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

    /// What a run is given waits at fd 4 and on, beside lib.sh's events at fd 3, and lies in
    /// none of its output unasked.
    #[test]
    fn a_run_reads_what_it_is_given_at_fd_4_and_on() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cmd = std::process::Command::new("bash");
        cmd.arg("-c")
            .arg(r#"read -r a <&4; read -r b <&5; [ "$a $b" = "one two" ] && echo given >&3"#);
        let given = vec![holding("one").unwrap(), holding("two").unwrap()];
        let _session =
            Session::spawn_with(cmd, (24, 80), move |e| drop(tx.send(e)), given).unwrap();
        // The end and the events come on threads of their own, in either order.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let (mut read, mut code) = (false, None);
        while !(read && code.is_some()) && std::time::Instant::now() < deadline {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Event::Script(s)) if s == "given" => read = true,
                Ok(Event::Exit(c)) => code = Some(c),
                _ => {}
            }
        }
        assert!(read && code == Some(0), "{read} {code:?}");
    }

    /// Only a run that elevates gets the password held, on two pipes, never one that does not.
    #[test]
    fn only_a_run_that_elevates_gets_the_password_held() {
        let _held = crate::script::sudo::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let gets = |op: Op| {
            let (cmd, given) = run_command(op, &[]).unwrap();
            let on = cmd
                .get_envs()
                .any(|(k, v)| k == "SM_PASSWORD" && v == Some("on".as_ref()));
            (on, given.len())
        };
        crate::script::sudo::set("pw".into());
        assert_eq!(gets(op::UPGRADE_RUN), (true, 2));
        assert_eq!(gets(op::SYSTEM_REBOOT), (false, 0));
        crate::script::sudo::forget();
        assert_eq!(gets(op::UPGRADE_RUN), (false, 0));
    }
}
