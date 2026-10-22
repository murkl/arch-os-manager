//! A script on its own pseudo terminal, so pacman and its kin behave as at a console.

use std::io::{BufRead, BufReader};
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::sync::Arc;

use rustix::fs::{Mode, OFlags};
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
        let mut cmd = command(op, args)?;
        cmd.env("SM_RUN", op.title());
        if super::terminal() {
            cmd.env("SM_TERMINAL", "on");
        }
        Session::spawn(cmd, size, sink)
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
        let (reader, out) = (session.master.clone(), sink.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            // Ends with EIO once no process holds the terminal any more.
            loop {
                match rustix::io::read(&*reader, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => out(Event::Output(buf[..n].to_vec())),
                    Err(rustix::io::Errno::INTR) => {}
                    Err(_) => break,
                }
            }
        });
        let script = sink.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(std::fs::File::from(events))
                .lines()
                .map_while(Result::ok)
            {
                script(Event::Script(line));
            }
        });
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

fn err(e: rustix::io::Errno) -> String {
    format!("pseudo terminal: {e}")
}
