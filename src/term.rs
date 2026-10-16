//! The interface in the terminal it was started from.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::ui::{App, Key, Msg, Target};

pub fn run(start: Option<Target>) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let console = std::env::var("TERM").is_ok_and(|t| t == "linux");
    let keys = tx.clone();
    std::thread::spawn(move || {
        while let Ok(e) = event::read() {
            let msg = match e {
                Event::Key(k) if k.kind != KeyEventKind::Release => key(k).map(Msg::Key),
                Event::Resize(..) => Some(Msg::Resize),
                _ => None,
            };
            if let Some(msg) = msg
                && keys.send(msg).is_err()
            {
                break;
            }
        }
    });
    let mut app = App::new(tx, Arc::new(|| {}), console, start);
    let mut terminal = ratatui::init();
    let result = (|| {
        loop {
            terminal.draw(|f| app.draw(f))?;
            let wait = if app.ticking() {
                Duration::from_millis(80)
            } else {
                Duration::from_millis(500)
            };
            match rx.recv_timeout(wait) {
                Ok(msg) => {
                    app.handle(msg);
                    while let Ok(msg) = rx.try_recv() {
                        app.handle(msg);
                    }
                }
                Err(RecvTimeoutError::Timeout) => app.handle(Msg::Tick),
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
            if app.done() {
                return Ok(());
            }
        }
    })();
    ratatui::restore();
    result
}

fn key(k: KeyEvent) -> Option<Key> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    Some(match k.code {
        KeyCode::Char(c) if ctrl => Key::Ctrl(c.to_ascii_lowercase()),
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        _ => return None,
    })
}
