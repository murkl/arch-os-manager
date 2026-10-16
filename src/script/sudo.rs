//! The sudo password: asked once, held in memory for the session, never written anywhere.

use std::sync::Mutex;

/// What sudo prints when asking, through `$SUDO_PROMPT`; recognisable, and readable in the details.
pub const PROMPT: &str = "[sudo] password (system-manager): ";

/// What sudo says after a wrong password.
pub const REJECTED: &str = "Sorry, try again.";

static PASSWORD: Mutex<Option<String>> = Mutex::new(None);

pub fn get() -> Option<String> {
    PASSWORD.lock().unwrap().clone()
}

pub fn set(password: String) {
    forget();
    *PASSWORD.lock().unwrap() = Some(password);
}

pub fn forget() {
    if let Some(mut old) = PASSWORD.lock().unwrap().take() {
        // SAFETY: zero bytes are valid UTF-8.
        unsafe { old.as_bytes_mut().fill(0) };
    }
}
