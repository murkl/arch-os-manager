//! A page of facts with its actions underneath; the facts scroll, the actions stay.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::draw::{cut, put, scrollbar, waiting, width, wrap};
use super::icon;
use super::list::{Lead, List, Picked, Row, draw_row};
use super::{Ctx, Flow, Key, Loaded, Nav, Tone};
use crate::i18n::t;
use crate::script::Failure;

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Field(String, String, Tone),
    Text(String, Tone),
    Heading(String),
    /// Lines kept as they are, each in its tone.
    Code(Vec<(String, Tone)>),
    /// Lines kept as they are, each in pieces of their own tone, bold or not.
    Styled(Vec<Vec<Span>>),
    /// A row as lists draw it: the lead column, the label, the detail on the right in pieces.
    Row(Lead, String, Vec<(String, Tone)>),
    /// Text beside the lead column, wrapped under itself.
    Note(Lead, String, Tone),
    Blank,
}

/// What is done or in order, the same on every page: a check, then the sentence.
pub fn done(text: impl Into<String>) -> Block {
    Block::Note(Lead::Icon(icon::DONE, Tone::Ok), text.into(), Tone::Normal)
}

/// Draws facts without actions, for a page that has nothing else to show.
pub fn show(body: Vec<Block>, area: Rect, buf: &mut Buffer, ctx: &Ctx) {
    let mut d = Detail::default();
    d.set(body, Vec::new());
    d.draw(area, buf, ctx);
}

pub struct Act {
    pub label: String,
    pub icon: Option<&'static str>,
    /// What the action is about, on the right: a count, a size.
    pub note: Option<(String, Tone)>,
    /// Shown, but not available, and the reason next to it.
    pub unavailable: Option<String>,
    pub go: Box<dyn FnMut(&mut Ctx) -> Flow>,
}

impl Act {
    pub fn new(label: impl Into<String>, go: impl FnMut(&mut Ctx) -> Flow + 'static) -> Act {
        Act {
            label: label.into(),
            icon: None,
            note: None,
            unavailable: None,
            go: Box::new(go),
        }
    }

    pub fn icon(mut self, icon: &'static str) -> Act {
        self.icon = Some(icon);
        self
    }

    pub fn note(mut self, text: impl Into<String>, tone: Tone) -> Act {
        self.note = Some((text.into(), tone));
        self
    }

    pub fn unless(mut self, reason: Option<String>) -> Act {
        self.unavailable = reason;
        self
    }
}

#[derive(Default)]
pub struct Detail {
    pub body: Vec<Block>,
    /// The leading blocks that stay while the rest scrolls.
    pinned: usize,
    acts: Vec<Act>,
    list: List,
    scroll: usize,
    page: usize,
    /// The facts are longer than their room, as last drawn.
    overflow: bool,
}

impl Detail {
    pub fn set(&mut self, body: Vec<Block>, acts: Vec<Act>) {
        self.body = body;
        let rows = acts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let row = Row::new(i.to_string(), a.label.clone());
                let row = match a.icon {
                    Some(icon) => row.icon(icon, Tone::Normal),
                    None => row,
                };
                match (&a.unavailable, &a.note) {
                    (Some(reason), _) => row.detail(reason.clone(), Tone::Muted),
                    (None, Some((text, tone))) => row.detail(text.clone(), *tone),
                    (None, None) => row,
                }
                .dim(a.unavailable.is_some())
            })
            .collect();
        self.list.set(rows);
        self.acts = acts;
    }

    /// Keeps the first `blocks` at the top while the rest scrolls.
    pub fn pin(&mut self, blocks: usize) {
        self.pinned = blocks;
    }

    /// Puts the cursor on an action, by its position.
    pub fn focus(&mut self, index: usize) {
        self.list.focus(&index.to_string());
    }

    pub fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        let scroll_only = self.acts.is_empty();
        match key.nav(false) {
            Some(Nav::PageUp) => self.scroll = self.scroll.saturating_sub(self.page.max(1)),
            Some(Nav::PageDown) => self.scroll += self.page.max(1),
            // The wheel scrolls the facts also where the arrows choose an action.
            Some(Nav::Up) if scroll_only || key == Key::WheelUp => {
                self.scroll = self.scroll.saturating_sub(1)
            }
            Some(Nav::Down) if scroll_only || key == Key::WheelDown => self.scroll += 1,
            Some(Nav::Top) if scroll_only => self.scroll = 0,
            Some(Nav::Bottom) if scroll_only => self.scroll = usize::MAX / 2,
            _ => {
                return match self.list.key(key) {
                    Picked::Open(id) => {
                        let act = &mut self.acts[id.parse::<usize>().unwrap_or(0)];
                        if act.unavailable.is_some() {
                            Flow::Stay
                        } else {
                            (act.go)(ctx)
                        }
                    }
                    Picked::Used => Flow::Stay,
                    Picked::Ignored => Flow::Ignored,
                };
            }
        }
        Flow::Stay
    }

    /// The lines the facts take at this width.
    pub fn height(&self, width: u16, ctx: &Ctx) -> u16 {
        render(&self.body, width as usize, ctx).len() as u16
    }

    pub fn hints(&self) -> Vec<(&'static str, String)> {
        let mut h = Vec::new();
        if self.acts.is_empty() {
            if self.overflow {
                h.push(("↑↓", t("scroll").to_string()));
            }
        } else {
            h.push(("↑↓", t("choose").to_string()));
            h.push(("⏎", t("run").to_string()));
            if self.overflow {
                h.push(("⇞⇟", t("scroll").to_string()));
            }
        }
        h.push(("esc", t("back").to_string()));
        h
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &Ctx) {
        let s = &ctx.style;
        let acts_height = if self.acts.is_empty() {
            0
        } else {
            self.acts.len() as u16 + 1
        };
        let body_height = area.height.saturating_sub(acts_height) as usize;
        let pinned = self.pinned.min(self.body.len());
        let head = render(&self.body[..pinned], area.width as usize, ctx);
        let lines = render(&self.body[pinned..], area.width as usize, ctx);
        let head_height = head.len().min(body_height);
        let rest_height = body_height - head_height;
        self.page = rest_height.saturating_sub(1);
        self.scroll = self.scroll.min(lines.len().saturating_sub(rest_height));
        self.overflow = lines.len() > rest_height;
        let column = head
            .iter()
            .chain(&lines)
            .any(|l| matches!(l, Line::Row(lead, ..) if lead.shows(s)));
        let shown = head
            .iter()
            .chain(lines.iter().skip(self.scroll).take(rest_height));
        for (n, line) in shown.enumerate() {
            let y = area.y + n as u16;
            match line {
                Line::Spans(spans) => {
                    let mut x = area.x;
                    for (text, tone, bold) in spans {
                        let style = if *bold { s.bold(*tone) } else { s.tone(*tone) };
                        x = put(buf, x, y, text, area.right().saturating_sub(x), style);
                    }
                }
                Line::Row(lead, (label, tone), detail) => {
                    let pieces: Vec<_> = detail
                        .iter()
                        .map(|(text, tone)| (text.as_str(), s.tone(*tone)))
                        .collect();
                    draw_row(
                        buf,
                        area,
                        y,
                        (*lead, column),
                        (label, s.tone(*tone)),
                        &pieces,
                        ctx,
                    )
                }
            }
        }
        let rest = Rect::new(
            area.x,
            area.y + head_height as u16,
            area.width,
            rest_height as u16,
        );
        scrollbar(buf, rest, lines.len(), self.scroll, ctx);
        if !self.acts.is_empty() {
            let rows = (self.acts.len() as u16).min(area.height);
            self.list.draw(
                Rect::new(area.x, area.y + area.height - rows, area.width, rows),
                buf,
                ctx,
            );
        }
    }
}

pub type Span = (String, Tone, bool);

enum Line {
    Spans(Vec<Span>),
    Row(Lead, (String, Tone), Vec<(String, Tone)>),
}

/// The blocks as screen lines of the given width.
fn render(body: &[Block], w: usize, ctx: &Ctx) -> Vec<Line> {
    let mark = ctx.style.glyphs.ellipsis;
    let label_width = body
        .iter()
        .filter_map(|b| {
            if let Block::Field(l, ..) = b {
                Some(width(l))
            } else {
                None
            }
        })
        .max()
        .unwrap_or(0)
        .min(w / 3);
    let mut out: Vec<Line> = Vec::new();
    for block in body {
        match block {
            Block::Field(label, value, tone) => {
                let room = w.saturating_sub(label_width + 2).max(1);
                for (i, line) in wrap(value, room).into_iter().enumerate() {
                    let head = if i == 0 {
                        cut(label, label_width, mark)
                    } else {
                        String::new()
                    };
                    let pad = " ".repeat(label_width - width(&head) + 2);
                    out.push(Line::Spans(vec![
                        (head + &pad, Tone::Muted, false),
                        (line, *tone, false),
                    ]));
                }
            }
            Block::Text(text, tone) => out.extend(
                wrap(text, w)
                    .into_iter()
                    .map(|l| Line::Spans(vec![(l, *tone, false)])),
            ),
            Block::Heading(text) => out.extend(
                wrap(text, w)
                    .into_iter()
                    .map(|l| Line::Spans(vec![(l, Tone::Normal, true)])),
            ),
            Block::Code(lines) => out.extend(
                lines
                    .iter()
                    .map(|(l, tone)| Line::Spans(vec![(cut(l, w, mark), *tone, false)])),
            ),
            Block::Styled(lines) => {
                out.extend(lines.iter().map(|spans| Line::Spans(spans.clone())))
            }
            Block::Row(lead, label, detail) => out.push(Line::Row(
                *lead,
                (label.clone(), Tone::Normal),
                detail.clone(),
            )),
            // The text starts where a row's label does, as list rows have it.
            Block::Note(lead, text, tone) => {
                for (i, line) in wrap(text, w.saturating_sub(3)).into_iter().enumerate() {
                    let lead = if i == 0 { *lead } else { Lead::None };
                    out.push(Line::Row(lead, (line, *tone), Vec::new()));
                }
            }
            Block::Blank => out.push(Line::Spans(Vec::new())),
        }
    }
    out
}

/// How a failure reads on any page: what happened, what to do, where exactly.
pub fn failure(f: &Failure) -> Vec<Block> {
    let mut b = vec![Block::Heading(f.headline())];
    let explanation = f.explanation();
    if !explanation.is_empty() {
        b.push(Block::Text(explanation, Tone::Normal));
    }
    b.push(Block::Blank);
    b.push(Block::Text(f.detail.clone(), Tone::Muted));
    if !f.tail.is_empty() {
        b.push(Block::Code(
            f.tail
                .iter()
                .rev()
                .take(8)
                .rev()
                .map(|l| (l.clone(), Tone::Muted))
                .collect(),
        ));
    }
    b.push(Block::Blank);
    b.push(Block::Text(
        crate::i18n::tf("Log: {path}", &[("path", &crate::paths::log().display())]),
        Tone::Muted,
    ));
    b
}

/// Draws the waiting or the failure of a value, and returns the value once there.
pub fn loaded<'a, T>(
    value: &'a Loaded<T>,
    area: Rect,
    buf: &mut Buffer,
    ctx: &Ctx,
) -> Option<&'a T> {
    match value {
        Loaded::Ready(v) => Some(v),
        Loaded::Loading => {
            waiting(buf, area, t("Loading"), None, None, ctx);
            None
        }
        Loaded::Failed(f) => {
            show(failure(f), area, buf, ctx);
            None
        }
        Loaded::Missing(package) => {
            let text = crate::i18n::tf(
                "This needs {package}, see Settings, Optional packages.",
                &[("package", package)],
            );
            show(vec![Block::Text(text, Tone::Muted)], area, buf, ctx);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(d: &mut Detail, area: Rect, ctx: &Ctx) -> Vec<String> {
        let mut buf = Buffer::empty(Rect::new(0, 0, area.right() + 2, area.bottom()));
        d.draw(area, &mut buf, ctx);
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn facts_longer_than_their_room_scroll_by_wheel_and_page_while_the_actions_stay() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, _) = crate::ui::app::tests::app();
        let mut d = Detail::default();
        let body = (0..50)
            .map(|i| Block::Text(format!("line {i}"), Tone::Normal))
            .collect();
        d.set(
            body,
            vec![
                Act::new("Go", |_| Flow::Stay),
                Act::new("Other", |_| Flow::Stay),
            ],
        );
        // Seven lines of facts, a blank one, then the two actions.
        let area = Rect::new(2, 0, 30, 10);
        let rows = drawn(&mut d, area, &app.ctx);
        assert!(
            rows[0].contains("line 0") && rows[6].contains("line 6"),
            "{rows:#?}"
        );
        assert!(
            rows[0].ends_with("┃") && rows[6].ends_with("│"),
            "{rows:#?}"
        );
        assert!(d.hints().iter().any(|(k, _)| *k == "⇞⇟"));

        d.key(Key::WheelDown, &mut app.ctx);
        let rows = drawn(&mut d, area, &app.ctx);
        assert!(rows[0].contains("line 1"), "{rows:#?}");
        assert!(
            rows[8].starts_with("▶") && rows[8].contains("Go"),
            "the action stays chosen"
        );
        d.key(Key::PageDown, &mut app.ctx);
        assert!(drawn(&mut d, area, &app.ctx)[0].contains("line 7"));
        d.key(Key::Down, &mut app.ctx);
        let rows = drawn(&mut d, area, &app.ctx);
        assert!(rows[0].contains("line 7") && rows[9].starts_with("▶"));

        d.set(vec![Block::Text("short".into(), Tone::Normal)], Vec::new());
        drawn(&mut d, area, &app.ctx);
        assert_eq!(
            d.hints(),
            [("esc", "back".to_string())],
            "nothing to scroll"
        );
    }

    #[test]
    fn a_pinned_head_stays_while_the_rest_scrolls() {
        let (mut app, _) = crate::ui::app::tests::app();
        let mut d = Detail::default();
        let mut body = vec![
            Block::Field("Time".into(), "12:00".into(), Tone::Normal),
            Block::Blank,
        ];
        body.extend((0..30).map(|i| Block::Text(format!("line {i}"), Tone::Normal)));
        d.set(body, Vec::new());
        d.pin(2);
        let area = Rect::new(0, 0, 30, 8);
        d.key(Key::PageDown, &mut app.ctx);
        let rows = drawn(&mut d, area, &app.ctx);
        assert!(
            rows[0].contains("12:00") && rows[1].trim().is_empty(),
            "{rows:#?}"
        );
        assert!(!rows[2].contains("line 0"), "scrolled: {rows:#?}");
        assert!(
            rows[1].ends_with(' ') && rows[2].ends_with('┃'),
            "the bar spans the rest: {rows:#?}"
        );
    }

    #[test]
    fn a_heading_wraps_rather_than_losing_its_end() {
        let (app, _) = crate::ui::app::tests::app();
        let lines = render(
            &[Block::Heading("A manual step before the update".into())],
            16,
            &app.ctx,
        );
        let texts: Vec<String> = lines
            .iter()
            .filter_map(|l| match l {
                Line::Spans(spans) => Some(spans.iter().map(|(t, ..)| t.as_str()).collect()),
                Line::Row(..) => None,
            })
            .collect();
        assert_eq!(texts, ["A manual step", "before the", "update"]);
    }

    #[test]
    fn a_note_wraps_under_its_own_text_not_under_the_lead() {
        let (app, _) = crate::ui::app::tests::app();
        let lines = render(
            &[Block::Note(
                Lead::Bullet,
                "one two three".into(),
                Tone::Normal,
            )],
            10,
            &app.ctx,
        );
        let leads: Vec<(Lead, &str)> = lines
            .iter()
            .filter_map(|l| match l {
                Line::Row(lead, (text, _), _) => Some((*lead, text.as_str())),
                Line::Spans(_) => None,
            })
            .collect();
        assert_eq!(leads, [(Lead::Bullet, "one two"), (Lead::None, "three")]);
    }
}
