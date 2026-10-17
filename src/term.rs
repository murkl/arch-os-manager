//! The interface in the terminal it was started from.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};

use crate::ui::{App, FRAME, Key, Msg, Target};

pub fn run(start: Option<Target>) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let console = std::env::var("TERM").is_ok_and(|t| t == "linux");
    let keys = tx.clone();
    std::thread::spawn(move || {
        let mut last = None;
        while let Ok(e) = event::read() {
            let msg = match e {
                Event::Key(k) if k.kind != KeyEventKind::Release => key(k).map(|key| {
                    let again = held(&mut last, key, Instant::now());
                    if again || k.kind == KeyEventKind::Repeat {
                        Msg::Held(key)
                    } else {
                        Msg::Key(key)
                    }
                }),
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => Some(Msg::Key(Key::WheelUp)),
                    MouseEventKind::ScrollDown => Some(Msg::Key(Key::WheelDown)),
                    _ => None,
                },
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
    if let Some(aspect) = aspect() {
        app.ctx.aspect = aspect;
    }
    let mut terminal = ratatui::init();
    mouse(true);
    let restore = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        mouse(false);
        restore(info);
    }));
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
    mouse(false);
    ratatui::restore();
    result
}

/// The wheel as scrolling: without it, a terminal sends the arrow keys instead. A text is then
/// selected with shift held.
fn mouse(capture: bool) {
    let mut out = std::io::stdout();
    let done = match capture {
        true => ratatui::crossterm::execute!(out, EnableMouseCapture),
        false => ratatui::crossterm::execute!(out, DisableMouseCapture),
    };
    if let Err(e) = done {
        crate::warn!("mouse capture {capture}: {e}");
    }
}

/// How many times a cell is higher than wide, where the terminal says its size in pixels.
fn aspect() -> Option<f32> {
    let s = ratatui::crossterm::terminal::window_size().ok()?;
    let cell = |pixels: u16, cells: u16| {
        (pixels > 0 && cells > 0).then(|| f32::from(pixels) / f32::from(cells))
    };
    Some(cell(s.height, s.rows)? / cell(s.width, s.columns)?)
}

/// Faster than a hand presses a key twice, slower than a keyboard repeats it.
const REPEAT: Duration = Duration::from_millis(150);

/// Whether a key is held: a terminal sends its repeats as presses, so the same key again within
/// `REPEAT` of its last one counts as held.
fn held(last: &mut Option<(Key, Instant)>, key: Key, now: Instant) -> bool {
    let held = last.is_some_and(|(k, at)| k == key && now.duration_since(at) < REPEAT);
    *last = Some((key, now));
    held
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_coming_again_at_the_rate_of_a_keyboard_is_held() {
        let (mut last, start) = (None, Instant::now());
        let at = |ms| start + Duration::from_millis(ms);
        assert!(!held(&mut last, Key::Char('r'), at(0)));
        assert!(held(&mut last, Key::Char('r'), at(30)));
        assert!(held(&mut last, Key::Char('r'), at(60)));
        assert!(!held(&mut last, Key::Char('j'), at(90)), "another key");
        assert!(!held(&mut last, Key::Char('j'), at(400)), "pressed again");
    }
}
