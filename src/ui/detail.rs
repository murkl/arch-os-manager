//! A page of facts with its actions underneath; the facts scroll, the actions stay.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::draw::{cut, put, waiting, width, wrap};
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
    /// A row as lists draw it: the lead column, the label, the detail on the right.
    Row(Lead, String, String, Tone),
    /// A row whose detail is a change: what it was, muted, then what it becomes.
    Change(Lead, String, (String, String)),
    Blank,
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
    acts: Vec<Act>,
    list: List,
    scroll: usize,
    page: usize,
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

    /// Puts the cursor on an action, by its position.
    pub fn focus(&mut self, index: usize) {
        self.list.focus(&index.to_string());
    }

    pub fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        let scroll_only = self.acts.is_empty();
        match key.nav(false) {
            Some(Nav::PageUp) => self.scroll = self.scroll.saturating_sub(self.page.max(1)),
            Some(Nav::PageDown) => self.scroll += self.page.max(1),
            Some(Nav::Up) if scroll_only => self.scroll = self.scroll.saturating_sub(1),
            Some(Nav::Down) if scroll_only => self.scroll += 1,
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
            h.push(("↑↓", t("scroll").to_string()));
        } else {
            h.push(("↑↓", t("choose").to_string()));
            h.push(("⏎", t("run").to_string()));
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
        let lines = render(&self.body, area.width as usize, ctx);
        self.page = body_height.saturating_sub(1);
        self.scroll = self.scroll.min(lines.len().saturating_sub(body_height));
        for (n, line) in lines.iter().skip(self.scroll).take(body_height).enumerate() {
            let y = area.y + n as u16;
            match line {
                Line::Spans(spans) => {
                    let mut x = area.x;
                    for (text, tone, bold) in spans {
                        let style = if *bold { s.bold(*tone) } else { s.tone(*tone) };
                        x = put(buf, x, y, text, area.right().saturating_sub(x), style);
                    }
                }
                Line::Row(lead, label, detail) => {
                    let pieces: Vec<_> = detail
                        .iter()
                        .map(|(text, tone)| (text.as_str(), s.tone(*tone)))
                        .collect();
                    draw_row(
                        buf,
                        area,
                        y,
                        *lead,
                        (label, s.tone(Tone::Normal)),
                        &pieces,
                        ctx,
                    )
                }
            }
        }
        if lines.len() > body_height + self.scroll && body_height > 0 {
            let more = format!("{} {}", s.glyphs.ellipsis, t("more"));
            put(
                buf,
                area.right().saturating_sub(width(&more) as u16),
                area.y + body_height as u16 - 1,
                &more,
                area.width,
                s.tone(Tone::Muted),
            );
        }
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
    Row(Lead, String, Vec<(String, Tone)>),
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
            Block::Heading(text) => {
                out.push(Line::Spans(vec![(cut(text, w, mark), Tone::Normal, true)]))
            }
            Block::Code(lines) => out.extend(
                lines
                    .iter()
                    .map(|(l, tone)| Line::Spans(vec![(cut(l, w, mark), *tone, false)])),
            ),
            Block::Styled(lines) => {
                out.extend(lines.iter().map(|spans| Line::Spans(spans.clone())))
            }
            Block::Row(lead, label, detail, tone) => out.push(Line::Row(
                *lead,
                label.clone(),
                vec![(detail.clone(), *tone)],
            )),
            Block::Change(lead, label, (was, becomes)) => out.push(Line::Row(
                *lead,
                label.clone(),
                vec![(was.clone(), Tone::Muted), (becomes.clone(), Tone::Accent)],
            )),
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
            let mut d = Detail::default();
            d.set(failure(f), Vec::new());
            d.draw(area, buf, ctx);
            None
        }
        Loaded::Missing(package) => {
            let text = crate::i18n::tf(
                "This needs {package}, see Settings, Optional packages.",
                &[("package", package)],
            );
            put(
                buf,
                area.x,
                area.y,
                &text,
                area.width,
                ctx.style.tone(Tone::Muted),
            );
            None
        }
    }
}
