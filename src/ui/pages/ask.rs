//! What a page asks before it acts: a text.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::ui::draw::{put, wrap};
use crate::ui::input::Input;
use crate::ui::{Ctx, Does, Flow, Key, Page, Tone};

type Then<T> = Box<dyn FnMut(T, &mut Ctx) -> Flow>;

/// A text, with a sentence above it on what it is for and an example in the empty box. Whoever
/// takes it checks it.
pub struct TextPage {
    title: String,
    question: String,
    input: Input,
    then: Then<String>,
}

impl TextPage {
    pub fn new(
        title: &str,
        question: &str,
        example: &str,
        prefill: String,
        then: impl FnMut(String, &mut Ctx) -> Flow + 'static,
    ) -> TextPage {
        let mut input = Input::new(example);
        input.text = prefill;
        TextPage {
            title: title.to_string(),
            question: question.to_string(),
            input,
            then: Box::new(then),
        }
    }
}

impl Page for TextPage {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let s = ctx.style;
        let mut y = area.y;
        for line in wrap(&self.question, area.width as usize) {
            put(buf, area.x, y, &line, area.width, s.tone(Tone::Normal));
            y += 1;
        }
        self.input.draw(buf, area.x, y + 1, area.width, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        vec![("⏎", Does::Use), ("esc", Does::Back)]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match key {
            Key::Enter => (self.then)(self.input.text.trim().to_string(), ctx),
            Key::Esc => Flow::Pop,
            k => {
                self.input.key(k);
                Flow::Stay
            }
        }
    }

    fn typing(&self) -> bool {
        true
    }
}
