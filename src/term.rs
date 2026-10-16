//! The interface in the terminal it was started from.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Instant;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::ui::{App, FRAME, Key, Msg, Target};

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
            let drawn = Instant::now();
            let wake = app.wake(drawn);
            match rx.recv_timeout(wake.saturating_duration_since(drawn)) {
                Ok(msg) => {
                    app.handle(msg);
                    // What follows within a frame is drawn with it; a flood still gets drawn.
                    let (due, budget) = (drawn + FRAME, Instant::now() + FRAME);
                    while !app.done() && Instant::now() < budget {
                        match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                            Ok(msg) => app.handle(msg),
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return Ok(()),
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
            if !app.done() && Instant::now() >= wake {
                app.handle(Msg::Tick);
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
