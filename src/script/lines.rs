//! Terminal output as plain lines: escapes dropped, a carriage return starting the line over.

/// The end of an unfinished line that is kept: a question stands at its end, and the line is
/// looked at after every chunk of output.
const KEEP: usize = 4096;

#[derive(Default)]
pub struct Lines {
    current: Vec<u8>,
    state: State,
    carriage: bool,
}

#[derive(Default, PartialEq)]
enum State {
    #[default]
    Text,
    Escape,
    Csi,
    Osc,
    OscEscape,
    /// A two-byte escape such as a character set selection.
    Skip,
}

impl Lines {
    /// Feeds bytes and returns the lines they completed.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut done = Vec::new();
        for &b in bytes {
            match self.state {
                State::Escape => {
                    self.state = match b {
                        b'[' => State::Csi,
                        b']' => State::Osc,
                        b'(' | b')' | b'#' | b'%' => State::Skip,
                        _ => State::Text,
                    };
                    continue;
                }
                State::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.state = State::Text;
                    }
                    continue;
                }
                State::Osc => {
                    match b {
                        0x07 => self.state = State::Text,
                        0x1b => self.state = State::OscEscape,
                        _ => {}
                    }
                    continue;
                }
                State::OscEscape | State::Skip => {
                    self.state = State::Text;
                    continue;
                }
                State::Text => {}
            }
            if self.carriage && b != b'\n' {
                self.current.clear();
            }
            self.carriage = false;
            match b {
                0x1b => self.state = State::Escape,
                b'\n' => done
                    .push(String::from_utf8_lossy(&std::mem::take(&mut self.current)).into_owned()),
                b'\r' => self.carriage = true,
                0x08 => {
                    self.current.pop();
                }
                b'\t' => self.current.push(b' '),
                0..0x20 | 0x7f => {}
                _ => self.current.push(b),
            }
        }
        if self.current.len() > 2 * KEEP {
            self.current.drain(..self.current.len() - KEEP);
        }
        done
    }

    /// The line not yet ended, where a question waits for its answer.
    pub fn current(&self) -> String {
        String::from_utf8_lossy(&self.current).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_colour_and_keeps_the_last_redraw() {
        let mut l = Lines::default();
        let done = l.feed(b"\x1b[1;32m:: \x1b[0mSynchronizing\r\n( 1/2) 50%\r( 2/2) 100%\r\nnext");
        assert_eq!(done, [":: Synchronizing", "( 2/2) 100%"]);
        assert_eq!(l.current(), "next");
    }

    #[test]
    fn keeps_the_end_of_an_endless_line() {
        let mut l = Lines::default();
        for _ in 0..1000 {
            l.feed(&[b'.'; 1000]);
        }
        l.feed(b"Proceed? [Y/n] ");
        assert!(l.current().len() <= 2 * KEEP);
        assert!(l.current().ends_with("Proceed? [Y/n] "));
    }

    #[test]
    fn keeps_utf8_split_across_chunks_and_drops_titles() {
        let mut l = Lines::default();
        let word = "Größe\n".as_bytes();
        let (a, b) = word.split_at(3);
        assert!(l.feed(b"\x1b]0;title\x07").is_empty());
        assert!(l.feed(a).is_empty());
        assert_eq!(l.feed(b), ["Größe"]);
    }
}
