//! A one-line text box.

use ratatui::buffer::Buffer;

use super::draw::{put, width};
use super::{Ctx, Key, Tone};

#[derive(Default)]
pub struct Input {
    pub text: String,
    pub masked: bool,
    pub placeholder: String,
    /// Letters after the cursor, so a text set from outside leaves it at the end.
    after: usize,
}

impl Input {
    pub fn new(placeholder: &str) -> Input {
        Input {
            placeholder: placeholder.to_string(),
            ..Input::default()
        }
    }

    pub fn masked(placeholder: &str) -> Input {
        Input {
            masked: true,
            ..Input::new(placeholder)
        }
    }

    /// Takes a key that edits the text or moves the cursor. Returns whether it did.
    pub fn key(&mut self, key: Key) -> bool {
        let len = self.text.chars().count();
        self.after = self.after.min(len);
        let at = len - self.after;
        match key {
            Key::Char(c) if !c.is_control() => {
                let byte = self.byte(at);
                self.text.insert(byte, c);
            }
            Key::Backspace if at > 0 => self.remove(at - 1, at),
            Key::Delete if self.after > 0 => {
                self.remove(at, at + 1);
                self.after -= 1;
            }
            Key::Backspace | Key::Delete => {}
            Key::Left => self.after = (self.after + 1).min(len),
            Key::Right => self.after = self.after.saturating_sub(1),
            Key::Home => self.after = len,
            Key::End => self.after = 0,
            Key::CtrlLeft => self.after = len - self.word_start(at),
            Key::CtrlRight => self.after = len - self.word_end(at),
            Key::CtrlBackspace | Key::Ctrl('w') => self.remove(self.word_start(at), at),
            Key::Ctrl('u') => self.remove(0, at),
            _ => return false,
        }
        true
    }

    pub fn take(&mut self) -> String {
        self.after = 0;
        std::mem::take(&mut self.text)
    }

    /// The byte where letter `at` begins.
    fn byte(&self, at: usize) -> usize {
        self.text
            .char_indices()
            .nth(at)
            .map_or(self.text.len(), |(i, _)| i)
    }

    fn remove(&mut self, from: usize, to: usize) {
        let range = self.byte(from)..self.byte(to);
        self.text.replace_range(range, "");
    }

    /// Where the word left of `at` begins, past the spaces before it. A hidden text is one word,
    /// so its spaces stay hidden.
    fn word_start(&self, at: usize) -> usize {
        if self.masked {
            return 0;
        }
        let letters: Vec<char> = self.text.chars().take(at).collect();
        let words = letters.iter().rposition(|c| *c != ' ').map_or(0, |i| i + 1);
        letters[..words]
            .iter()
            .rposition(|c| *c == ' ')
            .map_or(0, |i| i + 1)
    }

    /// Where the word right of `at` ends, past the spaces before it.
    fn word_end(&self, at: usize) -> usize {
        let len = self.text.chars().count();
        if self.masked {
            return len;
        }
        let rest: Vec<char> = self.text.chars().skip(at).collect();
        let word = rest.iter().position(|c| *c != ' ').unwrap_or(rest.len());
        at + rest[word..]
            .iter()
            .position(|c| *c == ' ')
            .map_or(rest.len(), |i| word + i)
    }

    pub fn draw(&self, buf: &mut Buffer, x: u16, y: u16, w: u16, ctx: &Ctx) {
        debug_assert!(
            !self.placeholder.is_empty(),
            "every box names what it takes"
        );
        let s = &ctx.style;
        let prompt = format!("{} ", s.glyphs.path);
        let after = put(buf, x, y, &prompt, w, s.bold(Tone::Accent));
        // One cell stays free for the cursor, which stands where the next letter goes.
        let room = w.saturating_sub(after - x + 1) as usize;
        if self.text.is_empty() {
            put(
                buf,
                after,
                y,
                &self.placeholder,
                room as u16,
                s.tone(Tone::Muted),
            );
            ctx.cursor.set(Some((after, y)));
            return;
        }
        let letters: Vec<String> = match self.masked {
            true => self
                .text
                .chars()
                .map(|_| s.glyphs.mask.to_string())
                .collect(),
            false => self.text.chars().map(String::from).collect(),
        };
        let widths: Vec<usize> = letters.iter().map(|l| width(l)).collect();
        let len = letters.len();
        let at = len - self.after.min(len);
        let (shown, column) = window(&letters, &widths, at, room, s.glyphs.ellipsis);
        put(buf, after, y, &shown, room as u16, s.tone(Tone::Normal));
        if column <= room {
            ctx.cursor.set(Some((after + column as u16, y)));
        }
    }
}

/// The letters that fit `room`, the one under the cursor at `at` always among them, a cut marked
/// at its side. Returns them with the column of the cursor.
fn window(
    letters: &[String],
    widths: &[usize],
    at: usize,
    room: usize,
    mark: &str,
) -> (String, usize) {
    let len = letters.len();
    let sum = |from: usize, to: usize| widths[from..to].iter().sum::<usize>();
    let front = |start: usize| usize::from(start > 0);
    // From `start` the rest fits whole, or up to the letter under the cursor with a cell for the
    // mark behind it.
    let fits = |start: usize| {
        front(start) + sum(start, len) <= room
            || at < len && front(start) + sum(start, at + 1) < room
    };
    let start = (0..at).find(|s| fits(*s)).unwrap_or(at);
    let mut shown = String::new();
    let mut used = front(start);
    if used > 0 {
        shown.push_str(mark);
    }
    let column = used + sum(start, at);
    let mut end = start;
    while end < len && used + widths[end] <= room {
        shown.push_str(&letters[end]);
        used += widths[end];
        end += 1;
    }
    if end < len {
        while used + 1 > room && end > at + 1 {
            end -= 1;
            used -= widths[end];
            shown.truncate(shown.len() - letters[end].len());
        }
        shown.push_str(mark);
    }
    (shown, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_leaves_navigation_alone() {
        let mut i = Input::new("");
        for c in "pacman -S".chars() {
            assert!(i.key(Key::Char(c)));
        }
        assert!(i.key(Key::Ctrl('w')));
        assert_eq!(i.text, "pacman ");
        assert!(i.key(Key::Backspace));
        assert!(!i.key(Key::Up) && !i.key(Key::Enter) && !i.key(Key::Esc));
        assert_eq!(i.take(), "pacman");
    }

    fn typed(text: &str) -> Input {
        let mut i = Input::new("x");
        for c in text.chars() {
            i.key(Key::Char(c));
        }
        i
    }

    #[test]
    fn the_cursor_moves_and_letters_go_where_it_stands() {
        let mut i = typed("pacman");
        for k in [
            Key::Left,
            Key::Left,
            Key::Left,
            Key::Char('X'),
            Key::Home,
            Key::Char('>'),
            Key::End,
            Key::Char('<'),
        ] {
            assert!(i.key(k), "{k:?}");
        }
        assert_eq!(i.text, ">pacXman<");
        i.key(Key::Home);
        i.key(Key::Delete);
        i.key(Key::Right);
        i.key(Key::Backspace);
        assert_eq!(i.text, "acXman<");
        i.key(Key::End);
        assert!(i.key(Key::Right) && i.key(Key::Delete), "at the end, taken");
        assert_eq!(i.text, "acXman<");
    }

    #[test]
    fn words_are_jumped_and_deleted_before_the_cursor() {
        let mut i = typed("sudo pacman -Syu");
        i.key(Key::CtrlLeft);
        i.key(Key::CtrlBackspace);
        assert_eq!(i.text, "sudo -Syu");
        i.key(Key::CtrlRight);
        i.key(Key::Ctrl('w'));
        assert_eq!(i.text, "sudo ");
        let mut j = typed("sudo pacman");
        j.key(Key::CtrlLeft);
        j.key(Key::Ctrl('u'));
        assert_eq!(j.text, "pacman");
    }

    #[test]
    fn a_hidden_text_is_one_word() {
        let mut i = Input::masked("x");
        for c in "two words".chars() {
            i.key(Key::Char(c));
        }
        i.key(Key::CtrlLeft);
        i.key(Key::Char('>'));
        assert_eq!(i.text, ">two words");
        i.key(Key::End);
        i.key(Key::CtrlBackspace);
        assert_eq!(i.text, "");
    }

    #[test]
    fn a_text_set_from_outside_keeps_the_cursor_in_it() {
        let mut i = typed("firefox");
        i.key(Key::Home);
        i.text.clear();
        assert!(i.key(Key::Char('a')));
        assert_eq!(i.text, "a");
        i.text = "prefilled".into();
        i.key(Key::Char('!'));
        assert_eq!(i.text, "prefilled!", "the cursor stands at the end");
    }

    #[test]
    fn a_long_text_keeps_the_cursor_in_sight() {
        let (app, _) = crate::ui::app::tests::app();
        let cell = |i: &Input| {
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 12, 1));
            i.draw(&mut buf, 0, 0, 12, &app.ctx);
            let (x, _) = app.ctx.cursor.get().unwrap();
            let line: String = (0..12).map(|x| buf[(x, 0)].symbol().to_string()).collect();
            (line, x)
        };
        let mut i = typed("abcdefghijklmnopqrstuvwxyz");
        let (line, x) = cell(&i);
        assert!(line.ends_with("…stuvwxyz ") && x == 11, "{line} {x}");
        for _ in 0..20 {
            i.key(Key::Left);
        }
        let (line, x) = cell(&i);
        assert!(line.contains("abcdefg"), "{line}");
        assert_eq!(line.chars().nth(x as usize), Some('g'), "{line} {x}");
        for _ in 0..3 {
            i.key(Key::Right);
        }
        let (line, x) = cell(&i);
        assert_eq!(
            line.chars().nth(x as usize),
            Some('j'),
            "under the cursor: {line} {x}"
        );
    }
}
