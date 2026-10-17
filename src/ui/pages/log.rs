//! The errors of this boot, without the ones the person chose to hide.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::data::journal::Entry;
use crate::i18n::t;
use crate::ui::detail::{Act, Block, Detail, done, loaded, show};
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

fn hide(message: &str, ctx: &mut Ctx) {
    let pattern = crate::data::journal::pattern(message);
    if !ctx.settings.hidden_log.contains(&pattern) {
        ctx.settings.hidden_log.push(pattern);
        ctx.save_settings();
    }
}

pub struct LogPage {
    list: List,
}

impl LogPage {
    pub fn new() -> LogPage {
        LogPage {
            list: List::default(),
        }
    }

    fn entry(&self, ctx: &Ctx, id: &str) -> Option<Entry> {
        ctx.shared
            .journal
            .ready()?
            .get(id.parse::<usize>().ok()?)
            .cloned()
    }
}

impl Page for LogPage {
    fn title(&self) -> String {
        t("System log").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(all) = loaded(&ctx.shared.journal, area, buf, ctx) else {
            return;
        };
        let rows: Vec<Row> = all
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, e)| !e.hidden(&ctx.settings.hidden_log))
            .map(|(i, e)| {
                let first = e.message.lines().next().unwrap_or_default();
                Row::new(i.to_string(), format!("{}  {first}", e.ident))
                    .dot(Tone::Warn)
                    .detail(e.clock(), Tone::Muted)
            })
            .collect();
        if rows.is_empty() {
            show(vec![done(t("No errors since the start."))], area, buf, ctx);
        }
        self.list.set(rows);
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        let mut h = super::list_hints(self.list.typing());
        if !self.list.typing() {
            h.insert(2, ("x", t("hide").into()));
        }
        h
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if key == Key::Char('x') && !self.list.typing() {
            if let Some(e) = self.list.selected().and_then(|r| self.entry(ctx, &r.id)) {
                hide(&e.message, ctx);
            }
            return Flow::Stay;
        }
        match self.list.key(key) {
            Picked::Open(id) => match self.entry(ctx, &id) {
                Some(e) => Flow::Push(Box::new(EntryPage::new(e))),
                None => Flow::Stay,
            },
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.journal, Loaded::Loading)
    }
}

struct EntryPage {
    detail: Detail,
}

impl EntryPage {
    fn new(e: Entry) -> EntryPage {
        let body = vec![
            Block::Field(t("Time").into(), e.clock(), Tone::Normal),
            Block::Field(t("From").into(), e.ident.clone(), Tone::Normal),
            Block::Blank,
            Block::Text(e.message.clone(), Tone::Normal),
        ];
        let message = e.message.clone();
        let acts = vec![
            Act::new(t("Hide messages like this"), move |ctx: &mut Ctx| {
                hide(&message, ctx);
                Flow::Pop
            })
            .icon(crate::ui::icon::HIDDEN),
        ];
        let mut detail = Detail::default();
        detail.set(body, acts);
        // The time and the source stay while the message scrolls.
        detail.pin_head();
        EntryPage { detail }
    }
}

impl Page for EntryPage {
    fn title(&self) -> String {
        t("Entry").into()
    }
    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.detail.draw(area, buf, ctx);
    }
    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        self.detail.hints()
    }
    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }
}

/// The hidden messages, to show again.
pub struct HiddenPage {
    list: List,
}

impl HiddenPage {
    pub fn new() -> HiddenPage {
        HiddenPage {
            list: List::default(),
        }
    }
}

impl Page for HiddenPage {
    fn title(&self) -> String {
        t("Hidden messages").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let rows: Vec<Row> = ctx
            .settings
            .hidden_log
            .iter()
            .map(|h| Row::new(h.clone(), h.clone()).icon(crate::ui::icon::HIDDEN, Tone::Muted))
            .collect();
        if rows.is_empty() {
            show(
                vec![Block::Text(
                    t("Nothing is hidden. Hide a message on the system log with x.").into(),
                    Tone::Muted,
                )],
                area,
                buf,
                ctx,
            );
        }
        self.list.set(rows);
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, String)> {
        if ctx.settings.hidden_log.is_empty() {
            return vec![("esc", t("back").into())];
        }
        vec![
            ("↑↓", t("choose").into()),
            ("⏎", t("show again").into()),
            ("esc", t("back").into()),
        ]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(id) => {
                ctx.settings.hidden_log.retain(|h| *h != id);
                ctx.save_settings();
                Flow::Stay
            }
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }
}
