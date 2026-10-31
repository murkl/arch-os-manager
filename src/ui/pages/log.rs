//! The errors of this boot, without the ones the person chose to hide.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::data::journal::Entry;
use crate::i18n::{t, tf};
use crate::ui::detail::{Act, Block, Detail, done, loaded, show};
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

fn hide(message: &str, ctx: &mut Ctx) {
    let pattern = crate::data::journal::pattern(message);
    if !ctx.settings.hidden_log.contains(&pattern) {
        ctx.settings.hidden_log.push(pattern);
        ctx.save_settings();
    }
}

/// A row of the log: an entry by its place in the journal, or the count of the hidden messages.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Pick {
    Entry(usize),
    Hidden,
}

pub struct LogPage {
    list: List<Pick>,
    /// Where the cursor goes once the rows are read anew after a hide: the next error left.
    next: Option<Pick>,
}

impl LogPage {
    pub fn new() -> LogPage {
        LogPage {
            list: List::filterable(),
            next: None,
        }
    }

    fn entry(&self, ctx: &Ctx, i: usize) -> Option<Entry> {
        ctx.shared.journal.ready()?.get(i).cloned()
    }

    /// The row the cursor takes once the kind of `e` is hidden: the next of another kind below,
    /// else above.
    fn after(&self, e: &Entry, ctx: &Ctx) -> Option<Pick> {
        let rows = self.list.rows();
        let chosen = self.list.selected()?.id.clone();
        let at = rows.iter().position(|r| r.id == chosen)?;
        let gone = crate::data::journal::kinds(std::slice::from_ref(&e.message));
        let other = |r: &Row<Pick>| match r.id {
            Some(Pick::Entry(i)) => self.entry(ctx, i).is_some_and(|o| !o.hidden(&gone)),
            _ => false,
        };
        let below = rows[at + 1..].iter().find(|r| other(r));
        below
            .or_else(|| rows[..at].iter().rev().find(|r| other(r)))
            .and_then(|r| r.id.clone())
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
        let kinds = crate::data::journal::kinds(&ctx.settings.hidden_log);
        let hidden = all.iter().filter(|e| e.hidden(&kinds)).count();
        let errors: Vec<Row<Pick>> = all
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, e)| !e.hidden(&kinds))
            .map(|(i, e)| {
                let first = e.message.lines().next().unwrap_or_default();
                Row::new(Pick::Entry(i), format!("{}  {first}", e.ident))
                    .dot(Tone::Warn)
                    .detail(e.clock(), Tone::Muted)
            })
            .collect();
        let mut area = area;
        if errors.is_empty() {
            let text = match hidden {
                0 => t("No errors since boot."),
                _ => t("No other errors since boot."),
            };
            let mut none = Detail::default();
            none.set(vec![done(text)], Vec::new());
            // The count follows a line below the text, however it wraps.
            let used = none.height(area.width, ctx) + 1;
            none.draw(area, buf, ctx);
            area = Rect::new(
                area.x,
                area.y + used,
                area.width,
                area.height.saturating_sub(used),
            );
        }
        let mut rows = errors;
        // The count is that of the rules the page behind it lists, also those matching nothing of
        // this boot.
        if !ctx.settings.hidden_log.is_empty() {
            // A blank line between the errors and the count.
            if !rows.is_empty() {
                rows.push(Row::heading(""));
            }
            let count = tf("{n} hidden", &[("n", &ctx.settings.hidden_log.len())]);
            rows.push(Row::new(Pick::Hidden, count).icon(icon::HIDDEN, Tone::Muted));
        }
        self.list.set(rows);
        if let Some(pick) = self.next.take() {
            self.list.focus(&pick);
        }
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        let mut h = super::list_hints(self.list.typing());
        let entry = matches!(
            self.list.selected().and_then(|r| r.id.as_ref()),
            Some(Pick::Entry(_))
        );
        if entry && !self.list.typing() {
            h.insert(2, ("x", Does::Hide));
        }
        h
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if key == Key::Char('x') && !self.list.typing() {
            if let Some(Pick::Entry(i)) = self.list.selected().and_then(|r| r.id.clone())
                && let Some(e) = self.entry(ctx, i)
            {
                self.next = self.after(&e, ctx);
                hide(&e.message, ctx);
            }
            return Flow::Stay;
        }
        match self.list.key(key) {
            Picked::Open(Pick::Entry(i)) => match self.entry(ctx, i) {
                Some(e) => Flow::Push(Box::new(EntryPage::new(e))),
                None => Flow::Stay,
            },
            Picked::Open(Pick::Hidden) => Flow::Push(Box::new(HiddenPage::new())),
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
            .icon(crate::ui::icon::HIDDEN)
            .does(Does::Hide),
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
    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        self.detail.hints()
    }
    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }
}

/// The hidden messages, to show again.
struct HiddenPage {
    list: List<String>,
}

impl HiddenPage {
    fn new() -> HiddenPage {
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
        let rows: Vec<Row<String>> = ctx
            .settings
            .hidden_log
            .iter()
            .map(|h| Row::new(h.clone(), h.clone()).icon(crate::ui::icon::HIDDEN, Tone::Muted))
            .collect();
        if rows.is_empty() {
            show(
                vec![Block::Text(t("Nothing hidden.").into(), Tone::Muted)],
                area,
                buf,
                ctx,
            );
        }
        self.list.set(rows);
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, Does)> {
        if ctx.settings.hidden_log.is_empty() {
            return vec![("esc", Does::Back)];
        }
        vec![("↑↓", Does::Choose), ("⏎", Does::Show), ("esc", Does::Back)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::tests::{app, example, open, press, screen};

    /// Hidden with x, a kind leaves its count below the errors and the cursor on the next error of
    /// another kind. The count opens the hidden messages, and x hides nothing there.
    #[test]
    fn the_hidden_ones_are_counted_below_and_open_from_there() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        app.ctx.shared.journal = Loaded::Ready(vec![
            Entry::new(1, "kernel", "fan stopped"),
            Entry::new(2, "kernel", "example 1"),
            Entry::new(3, "kernel", "example 2"),
            Entry::new(4, "sshd", "auth failed"),
        ]);
        open(&mut app, Box::new(LogPage::new()));
        let cursor = app.ctx.style.glyphs.cursor;
        let chosen = |shown: &str| {
            shown
                .lines()
                .find(|l| l.contains(cursor))
                .unwrap_or("")
                .to_string()
        };
        let shown = screen(&mut app, &mut term);
        assert!(
            !shown.contains("hidden") && chosen(&shown).contains("auth failed"),
            "{shown}"
        );
        press(&mut app, &[Key::Down, Key::Char('x')]);
        let shown = screen(&mut app, &mut term);
        let count = shown
            .lines()
            .position(|l| l.contains("1 hidden"))
            .unwrap_or(0);
        let error = shown
            .lines()
            .position(|l| l.contains("auth failed"))
            .unwrap_or(usize::MAX);
        assert!(count > error, "the count stands below: {shown}");
        assert!(
            shown
                .lines()
                .nth(count - 1)
                .is_some_and(|l| l.trim().is_empty()),
            "a blank line before the count: {shown}"
        );
        assert!(chosen(&shown).contains("fan stopped"), "{shown}");
        press(&mut app, &[Key::Down]);
        let shown = screen(&mut app, &mut term);
        assert!(chosen(&shown).contains("1 hidden"), "{shown}");
        assert!(
            !shown.lines().last().unwrap_or("").contains("x hide"),
            "{shown}"
        );
        press(&mut app, &[Key::Char('x')]);
        assert!(
            screen(&mut app, &mut term).contains("1 hidden"),
            "x hides nothing there"
        );
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Hidden messages") && shown.contains("example #"),
            "{shown}"
        );
    }

    /// With every error hidden the log claims no more than that, without any it says none. A rule
    /// that hides nothing of this boot keeps its count.
    #[test]
    fn no_error_is_claimed_while_some_are_hidden() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        app.ctx.settings.hidden_log = vec!["example 0".into()];
        open(&mut app, Box::new(LogPage::new()));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("No other errors since boot.") && shown.contains("1 hidden"),
            "{shown}"
        );
        app.ctx.shared.journal = Loaded::Ready(Vec::new());
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("No errors since boot.") && shown.contains("1 hidden"),
            "the rule stays in reach: {shown}"
        );
        app.ctx.settings.hidden_log.clear();
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("No errors since boot.") && !shown.contains("hidden"),
            "{shown}"
        );
    }
}
