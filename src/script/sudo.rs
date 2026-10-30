//! The sudo password: asked once, held in memory for the session, never written anywhere and
//! handed to sudo through pipes only.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rustix::process::{DumpableBehavior, Pid, getppid, set_dumpable_behavior};

/// What sudo prints when asking, through `$SUDO_PROMPT`, recognisable and readable in the details.
pub const PROMPT: &str = "[sudo] password (system-manager): ";

/// What sudo says after a wrong password.
pub const REJECTED: &str = "Sorry, try again.";

static PASSWORD: Mutex<Option<String>> = Mutex::new(None);

/// Held by tests that depend on the password held, which is one for the process.
#[cfg(test)]
pub static LOCK: Mutex<()> = Mutex::new(());

pub fn get() -> Option<String> {
    PASSWORD.lock().unwrap().clone()
}

/// Holds the password, an empty one never, since sudo would count it as a failed attempt.
pub fn set(password: String) {
    forget();
    if !password.is_empty() {
        // No core dump holds it, and no other process of the user reads it, as ssh-agent does.
        let _ = set_dumpable_behavior(DumpableBehavior::NotDumpable);
        *PASSWORD.lock().unwrap() = Some(password);
    }
}

pub fn forget() {
    if let Some(mut old) = PASSWORD.lock().unwrap().take() {
        // SAFETY: zero bytes are valid UTF-8.
        unsafe { old.as_bytes_mut().fill(0) };
    }
}

/// How often the renewal looks whether the shell that started it still lives.
const TICK: Duration = Duration::from_secs(1);

/// Keeps sudo's time stamp of a run fresh while `shell`, which started this, lives: `script
/// renew` takes the password every `every`, read once from stdin. The first refusal ends it, as
/// each counts towards the lock of pam_faillock.
pub fn renew(shell: i32, every: Duration, script: &Path) -> ExitCode {
    // No other process of the user reads the password out of this one, no core dump holds it.
    let _ = set_dumpable_behavior(DumpableBehavior::NotDumpable);
    let mut held = String::new();
    if std::io::stdin().read_to_string(&mut held).is_err() {
        return ExitCode::FAILURE;
    }
    if held.ends_with('\n') {
        held.pop();
    }
    let (started, mut since) = (Pid::from_raw(shell), Instant::now());
    while getppid() == started {
        std::thread::sleep(TICK.min(every));
        if since.elapsed() >= every {
            since = Instant::now();
            if !renewed(&held, script) {
                break;
            }
        }
    }
    // SAFETY: zero bytes are valid UTF-8.
    unsafe { held.as_bytes_mut().fill(0) };
    ExitCode::SUCCESS
}

/// Whether `script renew` renewed sudo's time stamp, given the password on stdin.
fn renewed(password: &str, script: &Path) -> bool {
    let child = Command::new("bash")
        .arg("--noprofile")
        .arg("--norc")
        .arg(script)
        .arg("renew")
        .env_remove("SM_RUN")
        .env_remove("SM_TERMINAL")
        .env_remove("BASH_ENV")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{password}");
    }
    child.wait().is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::process::dumpable_behavior;

    /// While held, the password is in no core dump and out of reach of the user's other
    /// processes.
    #[test]
    fn a_held_password_leaves_the_process_undumpable() {
        let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set("secret".into());
        assert_eq!(dumpable_behavior().unwrap(), DumpableBehavior::NotDumpable);
        forget();
    }
}
