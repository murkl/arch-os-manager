//! The start page: everything there is, each with what it currently says.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::data::{ago, format_size, journal};
use crate::i18n::{t, tf, tn};
use crate::ui::gauge;
use crate::ui::list::{List, Picked, Row};
use crate::ui::target::{ALL, HOME, Target};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

pub struct HomePage {
    list: List<Target>,
    /// The person moved the cursor, so arriving data no longer moves it until they open a row.
    touched: bool,
    /// The row last suggested, which the cursor was put on.
    suggested: Option<Target>,
}

impl HomePage {
    pub fn new() -> HomePage {
        HomePage {
            list: List::filterable(),
            touched: false,
            suggested: None,
        }
    }

    fn rows(&self, ctx: &Ctx) -> Vec<Row<Target>> {
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
                    Row::new(target, target.label())
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
            .is_some_and(|p| !p.updates.is_empty())
            .then_some(Target::Upgrade)
    }
}

/// What the machine uses, unless switched off, on the first row and a row clear of the menu, only
/// where all of the menu still fits under it, which says more. Returns the area left for the menu.
fn instruments(area: Rect, menu: usize, buf: &mut Buffer, ctx: &Ctx) -> Rect {
    if !ctx.settings.usage.unwrap_or(true)
        || usize::from(area.height) <= menu + 1
        || !gauge::draw(buf, area, ctx)
    {
        return area;
    }
    Rect::new(area.x, area.y + 2, area.width, area.height - 2)
}

fn visible(target: Target, ctx: &Ctx) -> bool {
    target != Target::Reboot || ctx.shared.system.ready().is_some_and(|s| s.reboot)
}

fn count<T>(
    value: &Loaded<Vec<T>>,
    some: impl Fn(usize) -> (String, Tone),
    none: (String, Tone),
    ctx: &Ctx,
) -> (String, Tone, bool) {
    match value {
        Loaded::Loading => ctx.loading(),
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
        Target::Upgrade => match &s.updates {
            // Up to date only where every source said so.
            Loaded::Ready(p) if p.updates.is_empty() && !p.failed.is_empty() => {
                (t("unknown").into(), Tone::Muted, false)
            }
            Loaded::Ready(p) if p.updates.is_empty() => (t("up to date").into(), Tone::Ok, false),
            Loaded::Ready(p) => {
                let n = p.updates.len();
                (tn("{n} update", "{n} updates", n, &[]), Tone::Accent, false)
            }
            Loaded::Loading => ctx.loading(),
            Loaded::Failed(_) => (t("unknown").into(), Tone::Muted, false),
            Loaded::Missing(p) => (tf("needs {package}", &[("package", p)]), Tone::Muted, true),
        },
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
            Loaded::Loading => ctx.loading(),
            _ => (t("offline").into(), Tone::Muted, false),
        },
        Target::Packages => count(&s.installed, |n| (n.to_string(), Tone::Muted), nothing, ctx),
        Target::Services => match &s.services {
            Loaded::Ready(all) => match all.iter().filter(|x| x.failed()).count() {
                0 => (all.len().to_string(), Tone::Muted, false),
                n => (tf("{n} failed", &[("n", &n)]), Tone::Error, false),
            },
            other => count(other, |_| nothing.clone(), nothing.clone(), ctx),
        },
        Target::Log => match &s.journal {
            Loaded::Ready(all) => match journal::visible(all, &ctx.settings.hidden_log).count() {
                0 => (String::new(), Tone::Muted, false),
                n => (tn("{n} error", "{n} errors", n, &[]), Tone::Warn, false),
            },
            other => count(other, |_| nothing.clone(), nothing.clone(), ctx),
        },
        Target::Cleanup => match &s.cleanup {
            Loaded::Ready(plan) => (format_size(plan.bytes()), Tone::Muted, false),
            Loaded::Loading => ctx.loading(),
            Loaded::Failed(_) => (t("unknown").into(), Tone::Muted, false),
            Loaded::Missing(p) => (tf("needs {package}", &[("package", p)]), Tone::Muted, true),
        },
        Target::Pacnew => count(
            &s.pacnew,
            |n| (tf("{n} open", &[("n", &n)]), Tone::Warn),
            (t("up to date").into(), Tone::Ok),
            ctx,
        ),
        Target::Mirrors => match s.mirrors.ready().and_then(|m| m.changed) {
            Some(changed) => (ago(changed), Tone::Muted, false),
            None => (String::new(), Tone::Muted, false),
        },
        Target::Firmware => count(
            &s.firmware,
            |n| (tn("{n} update", "{n} updates", n, &[]), Tone::Accent),
            (t("up to date").into(), Tone::Ok),
            ctx,
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
        let area = match self.list.typing() {
            true => area,
            false => instruments(area, rows.len(), buf, ctx),
        };
        self.list.set(rows);
        // A row that comes to be meant takes the cursor, unless the person is moving it.
        let now = HomePage::suggested(ctx);
        if !self.touched && !self.list.typing() && now != self.suggested {
            if let Some(target) = now {
                self.list.focus(&target);
            }
            self.suggested = now;
        }
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _ctx: &Ctx) -> Vec<(&'static str, Does)> {
        if self.list.typing() {
            return vec![
                ("↑↓", Does::Choose),
                ("⏎", Does::Open),
                ("esc", Does::CloseSearch),
            ];
        }
        vec![
            ("↑↓", Does::Choose),
            ("⏎", Does::Open),
            ("f", Does::Search),
            ("r", Does::Reload),
            ("q", Does::Quit),
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
            Picked::Open(target) => {
                self.touched = false;
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

    /// Every row still reading, and the usage before its first reading, shows the spinner.
    fn animating(&self, ctx: &Ctx) -> bool {
        ctx.busy() || ctx.usage.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row still reading shows the spinner, and the page keeps drawing while anything reads.
    #[test]
    fn a_row_still_reading_shows_the_spinner_until_it_is_in() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, _) = crate::ui::app::tests::app();
        let frames = app.ctx.style.glyphs.spinner;
        app.ctx.shared.services = Loaded::Loading;
        let (text, tone, dim) = status(Target::Services, &app.ctx);
        assert!(frames.contains(&text.as_str()) && tone == Tone::Accent && !dim);
        app.ctx.shared.services = Loaded::Ready(Vec::new());
        let (text, ..) = status(Target::Services, &app.ctx);
        assert!(!frames.contains(&text.as_str()));

        let page = HomePage::new();
        app.ctx.usage = Some(crate::data::usage::Usage::default());
        assert!(!page.animating(&app.ctx));
        app.ctx.pending.set(1);
        assert!(page.animating(&app.ctx));
        app.ctx.pending.set(0);
        app.ctx.usage = None;
        assert!(page.animating(&app.ctx));
    }

    #[test]
    fn services_count_until_one_fails_and_no_configuration_update_is_up_to_date() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, _) = crate::ui::app::tests::app();
        let service = |active: &str| crate::data::services::Service {
            user: false,
            unit: "x.service".into(),
            description: String::new(),
            active: active.into(),
            enabled: "enabled".into(),
        };
        app.ctx.shared.services = Loaded::Ready(vec![service("active"), service("active")]);
        assert_eq!(
            status(Target::Services, &app.ctx),
            ("2".into(), Tone::Muted, false)
        );
        app.ctx.shared.services = Loaded::Ready(vec![service("failed"), service("active")]);
        assert_eq!(
            status(Target::Services, &app.ctx),
            ("1 failed".into(), Tone::Error, false)
        );
        app.ctx.shared.pacnew = Loaded::Ready(Vec::new());
        assert_eq!(
            status(Target::Pacnew, &app.ctx),
            ("up to date".into(), Tone::Ok, false)
        );
    }

    #[test]
    fn the_usage_stands_under_the_header_with_as_much_as_fits() {
        use crate::data::usage::Usage;
        use crate::ui::app::tests::screen;
        use crate::ui::icon;
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = crate::ui::app::tests::app();
        app.ctx.usage = Some(Usage {
            cpu: Some(0.23),
            memory: Some(0.387),
            disk: Some(1.0),
        });
        type Term = ratatui::Terminal<ratatui::backend::TestBackend>;
        let row = |term: &Term, y| -> String {
            let buf = term.backend().buffer();
            (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
        };
        // The bar's cells in a colour.
        let lit = |term: &Term, fg| {
            let buf = term.backend().buffer();
            buf.content()
                .iter()
                .filter(|c| c.fg == fg && c.symbol() == app.ctx.style.glyphs.bar)
                .count()
        };
        let style = app.ctx.style;
        let shown = screen(&mut app, &mut term);
        let usage = row(&term, 4);
        assert!(
            [
                icon::CPU,
                "CPU",
                "23%",
                icon::MEMORY,
                "39%",
                icon::DISK,
                "Disk",
                "100%"
            ]
            .iter()
            .all(|v| usage.contains(v)),
            "{shown}"
        );
        assert!(
            usage.trim_end().ends_with("100%") && row(&term, 6).contains("Update"),
            "at the right edge, a row clear of the menu: {shown}"
        );
        assert_eq!(
            (lit(&term, style.success), lit(&term, style.error)),
            (2 + 3, 7),
            "{shown}"
        );
        let buf = term.backend().buffer();
        assert!(
            buf.content()
                .iter()
                .filter(|c| [icon::CPU, icon::MEMORY, icon::DISK].contains(&c.symbol()))
                .all(|c| c.fg == style.muted),
            "the icons stay calm whatever the state: {shown}"
        );

        let mut narrow = Term::new(ratatui::backend::TestBackend::new(60, 30)).unwrap();
        let shown = screen(&mut app, &mut narrow);
        let usage = row(&narrow, 4);
        assert!(usage.contains("RAM") && usage.contains("39%"), "{shown}");
        assert_eq!(lit(&narrow, style.success), 0, "{shown}");

        let mut small = Term::new(ratatui::backend::TestBackend::new(40, 30)).unwrap();
        let shown = screen(&mut app, &mut small);
        let usage = row(&small, 4);
        assert!(
            usage.contains(icon::MEMORY) && usage.contains("39%") && !usage.contains("RAM"),
            "{shown}"
        );

        let colors = app.ctx.theme().colors;
        app.ctx.style = crate::ui::Style::new(&colors, true);
        let shown = screen(&mut app, &mut term);
        let usage = row(&term, 4);
        assert!(
            usage.contains("CPU") && !usage.contains(icon::CPU),
            "the console has no icons: {shown}"
        );
        app.ctx.style = style;

        let mut short = Term::new(ratatui::backend::TestBackend::new(80, 22)).unwrap();
        let shown = screen(&mut app, &mut short);
        assert!(
            !shown.contains("CPU") && shown.contains("Settings"),
            "{shown}"
        );
    }

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

    /// The row meant takes the cursor until the person moves it, and again once they opened one.
    #[test]
    fn a_restart_that_comes_up_takes_the_cursor_again_after_a_row_was_opened() {
        use crate::ui::app::tests::{app, example, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        let mut cursor = |app: &mut crate::ui::App| {
            let shown = screen(app, &mut term);
            let row = shown.lines().find(|l| l.contains('▶')).unwrap_or_default();
            row.to_string()
        };
        assert!(cursor(&mut app).contains("Update"), "updates wait");
        press(&mut app, &[Key::Down]);
        assert!(cursor(&mut app).contains("News"));
        if let Loaded::Ready(system) = &mut app.ctx.shared.system {
            system.reboot = true;
        }
        assert!(cursor(&mut app).contains("News"), "the person is moving it");
        press(&mut app, &[Key::Enter, Key::Esc]);
        assert!(cursor(&mut app).contains("Restart"), "back from the news");
    }
}
