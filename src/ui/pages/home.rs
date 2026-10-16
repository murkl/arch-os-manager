//! The start page: everything there is, each with what it currently says.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::data::{ago, format_size, journal};
use crate::i18n::{t, tf, tn};
use crate::ui::list::{List, Picked, Row};
use crate::ui::target::{ALL, HOME, Target};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

pub struct HomePage {
    list: List,
    /// The person moved the cursor, so arriving data no longer moves it.
    touched: bool,
}

impl HomePage {
    pub fn new() -> HomePage {
        HomePage {
            list: List::default(),
            touched: false,
        }
    }

    fn rows(&self, ctx: &Ctx) -> Vec<Row> {
        if self.list.typing() {
            return ALL
                .iter()
                .filter(|t| visible(**t, ctx))
                .map(|t| t.search_row())
                .collect();
        }
        let mut rows = Vec::new();
        for (i, group) in HOME.iter().enumerate() {
            if i > 0 {
                rows.push(Row::heading(""));
            }
            for &target in group.iter().filter(|t| visible(**t, ctx)) {
                let (detail, tone, dim) = status(target, ctx);
                rows.push(
                    Row::new(target.id(), target.label())
                        .icon(target.icon(), Tone::Normal)
                        .detail(detail, tone)
                        .dim(dim),
                );
            }
        }
        rows
    }

    /// The row that is probably meant: a pending restart, then pending updates.
    fn suggested(ctx: &Ctx) -> Option<Target> {
        if ctx.shared.system.ready().is_some_and(|s| s.reboot) {
            return Some(Target::Reboot);
        }
        ctx.shared
            .updates
            .ready()
            .is_some_and(|u| !u.is_empty())
            .then_some(Target::Upgrade)
    }
}

fn visible(target: Target, ctx: &Ctx) -> bool {
    target != Target::Reboot || ctx.shared.system.ready().is_some_and(|s| s.reboot)
}

fn count<T>(
    value: &Loaded<Vec<T>>,
    some: impl Fn(usize) -> (String, Tone),
    none: (String, Tone),
) -> (String, Tone, bool) {
    match value {
        Loaded::Loading => ("…".into(), Tone::Muted, false),
        Loaded::Ready(v) if v.is_empty() => (none.0, none.1, false),
        Loaded::Ready(v) => {
            let (text, tone) = some(v.len());
            (text, tone, false)
        }
        Loaded::Failed(_) => (t("unknown").into(), Tone::Muted, false),
        Loaded::Missing(p) => (tf("needs {package}", &[("package", p)]), Tone::Muted, true),
    }
}

/// What a row says about its part of the system right now.
fn status(target: Target, ctx: &Ctx) -> (String, Tone, bool) {
    let s = &ctx.shared;
    let nothing = (String::new(), Tone::Muted);
    match target {
        Target::Upgrade => count(
            &s.updates,
            |n| (tn("{n} update", "{n} updates", n, &[]), Tone::Accent),
            (t("up to date").into(), Tone::Ok),
        ),
        Target::Reboot => (t("required").into(), Tone::Warn, false),
        Target::News => match &s.news {
            Loaded::Ready((items, offline)) => {
                let read = ctx.state.news_read.as_deref().unwrap_or_default();
                let unread = items.iter().filter(|i| !read.contains(&i.link)).count();
                match (unread, offline) {
                    (0, true) => (t("offline").into(), Tone::Muted, false),
                    (0, false) => (String::new(), Tone::Muted, false),
                    (n, _) => (tf("{n} unread", &[("n", &n)]), Tone::Warn, false),
                }
            }
            Loaded::Loading => ("…".into(), Tone::Muted, false),
            _ => (t("offline").into(), Tone::Muted, false),
        },
        Target::Packages => count(&s.installed, |n| (n.to_string(), Tone::Muted), nothing),
        Target::Services => match &s.services {
            Loaded::Ready(all) => match all.iter().filter(|x| x.failed()).count() {
                0 => (String::new(), Tone::Muted, false),
                n => (tf("{n} failed", &[("n", &n)]), Tone::Error, false),
            },
            other => count(other, |_| nothing.clone(), nothing.clone()),
        },
        Target::Log => match &s.journal {
            Loaded::Ready(all) => match journal::visible(all, &ctx.settings.hidden_log).count() {
                0 => (String::new(), Tone::Muted, false),
                n => (tn("{n} error", "{n} errors", n, &[]), Tone::Warn, false),
            },
            other => count(other, |_| nothing.clone(), nothing.clone()),
        },
        Target::Cleanup => match &s.cleanup {
            Loaded::Ready(plan) if plan.bytes() > 0 => {
                (format_size(plan.bytes()), Tone::Muted, false)
            }
            Loaded::Ready(_) => (String::new(), Tone::Muted, false),
            Loaded::Loading => ("…".into(), Tone::Muted, false),
            Loaded::Failed(_) => (t("unknown").into(), Tone::Muted, false),
            Loaded::Missing(p) => (tf("needs {package}", &[("package", p)]), Tone::Muted, true),
        },
        Target::Pacnew => count(
            &s.pacnew,
            |n| (tf("{n} open", &[("n", &n)]), Tone::Warn),
            nothing,
        ),
        Target::Mirrors => match s.mirrors.ready().and_then(|m| m.changed) {
            Some(changed) => (ago(changed), Tone::Muted, false),
            None => (String::new(), Tone::Muted, false),
        },
        Target::Firmware => count(
            &s.firmware,
            |n| (tn("{n} update", "{n} updates", n, &[]), Tone::Accent),
            (t("up to date").into(), Tone::Ok),
        ),
        _ => (String::new(), Tone::Muted, false),
    }
}

impl Page for HomePage {
    fn title(&self) -> String {
        String::new()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let rows = self.rows(ctx);
        self.list.set(rows);
        if !self.touched
            && let Some(target) = HomePage::suggested(ctx)
        {
            self.list.focus(target.id());
        }
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _ctx: &Ctx) -> Vec<(&'static str, String)> {
        if self.list.typing() {
            return super::list_hints(true);
        }
        vec![
            ("↑↓", t("choose").into()),
            ("⏎", t("open").into()),
            ("f", t("search").into()),
            ("r", t("reload").into()),
            ("q", t("quit").into()),
        ]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if key == Key::Char('r') && !self.list.typing() {
            // One reading at a time: the spinner in the header shows the last is under way.
            if ctx.busy() {
                return Flow::Stay;
            }
            ctx.reload();
            return Flow::Push(Box::new(super::splash::SplashPage::new()));
        }
        let was_typing = self.list.typing();
        let picked = self.list.key(key);
        if was_typing != self.list.typing() {
            let rows = self.rows(ctx);
            self.list.set(rows);
        }
        match picked {
            Picked::Open(id) => {
                let Some(target) = Target::from_id(&id) else {
                    return Flow::Stay;
                };
                Flow::PushAll(target.open(ctx))
            }
            Picked::Used => {
                self.touched = true;
                Flow::Stay
            }
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.updates, Loaded::Loading)
            || matches!(ctx.shared.features, Loaded::Loading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_reloads_behind_the_splash() {
        let (mut app, _) = crate::ui::app::tests::app();
        let flow = HomePage::new().key(Key::Char('r'), &mut app.ctx);
        assert!(matches!(flow, Flow::Push(page) if page.bare()));
        assert!(app.ctx.busy());
        let again = HomePage::new().key(Key::Char('r'), &mut app.ctx);
        assert!(
            matches!(again, Flow::Stay),
            "a second reading waits for the first"
        );
    }
}
