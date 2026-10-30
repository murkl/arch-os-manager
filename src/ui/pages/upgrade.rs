//! What an update would bring, the news to read first, and the update itself.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::news::{NewsPage, unread};
use super::run::{Job, start};
use crate::data::updates::{Source, Update};
use crate::i18n::{t, tf, tn};
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail, done};
use crate::ui::draw::waiting;
use crate::ui::icon;
use crate::ui::input::Input;
use crate::ui::list::{Lead, changes};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Nav, Page, Tone};

pub struct UpgradePage {
    detail: Detail,
    focused: bool,
    /// The box that narrows the updates to the names holding its text, and what it lets through,
    /// in a view of its own without the actions, so enter starts no update.
    filter: Option<(Input, Detail)>,
}

impl UpgradePage {
    pub fn new() -> UpgradePage {
        UpgradePage {
            detail: Detail::default(),
            focused: false,
            filter: None,
        }
    }

    /// Whether there are updates to filter.
    fn filterable(ctx: &Ctx) -> bool {
        ctx.shared
            .updates
            .ready()
            .is_some_and(|p| !p.updates.is_empty())
    }

    fn build(&mut self, ctx: &Ctx) {
        if let Some((input, shown)) = &mut self.filter {
            let mut body = sections(ctx, &input.text.trim().to_lowercase());
            if body.is_empty() {
                body.push(Block::Text(t("No matches").into(), Tone::Muted));
            }
            shown.set(body, Vec::new());
        }
        let mut body = Vec::new();
        let news = unread(ctx);
        if news > 0 {
            body.push(Block::Text(
                tn("1 unread news item", "{n} unread news", news, &[]),
                Tone::Warn,
            ));
            body.push(Block::Blank);
        }
        if let Some(dropped) = ctx.shared.dropped.ready().filter(|d| !d.is_empty()) {
            body.push(Block::Text(
                tn(
                    "{n} package without source: {names}.",
                    "{n} packages without source: {names}.",
                    dropped.len(),
                    &[("names", &dropped.join(", "))],
                ),
                Tone::Warn,
            ));
            body.push(Block::Blank);
        }
        let mut known_current = false;
        match &ctx.shared.updates {
            // The page waits as a whole, see draw.
            Loaded::Loading => {}
            Loaded::Missing(p) => body.push(Block::Text(
                tf("Requires {package}.", &[("package", p)]),
                Tone::Muted,
            )),
            Loaded::Failed(f) => {
                body.push(Block::Text(f.headline(), Tone::Warn));
                body.push(Block::Text(f.explanation(), Tone::Muted));
            }
            Loaded::Ready(pending) => {
                for source in &pending.failed {
                    body.push(Block::Text(
                        match source {
                            Source::Aur => t("AUR unreachable."),
                            _ => t("Flatpak remotes unreachable."),
                        }
                        .to_string(),
                        Tone::Warn,
                    ));
                    body.push(Block::Blank);
                }
                if pending.updates.is_empty() && pending.failed.is_empty() {
                    known_current = true;
                    body.push(done(t("The system is up to date.")));
                }
                body.extend(sections(ctx, ""));
            }
        }
        let mut acts = Vec::new();
        if news > 0 {
            acts.push(
                Act::new(t("Read the news"), |_| {
                    Flow::Push(Box::new(NewsPage::new()))
                })
                .icon(icon::NEWS)
                .does(Does::Open),
            );
        }
        if known_current {
            let checking = ctx.busy().then(|| t("checking").to_string());
            acts.push(
                Act::new(t("Check again"), |ctx: &mut Ctx| {
                    ctx.reload();
                    Flow::Stay
                })
                .icon(icon::UPDATE)
                .does(Does::Reload)
                .unless(checking),
            );
        } else {
            acts.push(
                Act::new(t("Install now"), |ctx: &mut Ctx| {
                    Flow::Push(start(Job::new(op::UPGRADE_RUN, Vec::new()), ctx))
                })
                .icon(icon::UPDATE),
            );
        }
        self.detail.set(body, acts);
        if !self.focused {
            self.focused = true;
            self.detail.focus(0);
        }
    }
}

impl Page for UpgradePage {
    fn title(&self) -> String {
        t("Update").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        if self.animating(ctx) {
            if ctx.page_waited() {
                waiting(buf, area, t("Looking for updates"), None, None, ctx);
            }
            return;
        }
        self.build(ctx);
        let Some((input, shown)) = &mut self.filter else {
            self.detail.draw(area, buf, ctx);
            return;
        };
        input.draw(buf, area.x, area.y, area.width, ctx);
        let below = Rect::new(
            area.x,
            area.y + 2,
            area.width,
            area.height.saturating_sub(2),
        );
        shown.draw(below, buf, ctx);
    }

    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, Does)> {
        if self.animating(ctx) {
            return vec![("esc", Does::Back)];
        }
        if let Some((_, shown)) = &self.filter {
            let mut h = shown.hints();
            let esc = h.len() - 1;
            h[esc].1 = Does::CloseFilter;
            return h;
        }
        let mut h = self.detail.hints();
        if UpgradePage::filterable(ctx) {
            h.insert(h.len() - 1, ("f", Does::Filter));
        }
        h
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if self.animating(ctx) {
            return Flow::Ignored;
        }
        self.build(ctx);
        if let Some((input, shown)) = &mut self.filter {
            match key.nav(true) {
                Some(Nav::Back) => self.filter = None,
                Some(Nav::Open) => {}
                Some(Nav::Quit) => return Flow::Ignored,
                Some(_) => return shown.key(key, ctx),
                None => {
                    input.key(key);
                }
            }
            return Flow::Stay;
        }
        if key.nav(false) == Some(Nav::Filter) && UpgradePage::filterable(ctx) {
            self.filter = Some((Input::new(t("Filter")), Detail::default()));
            return Flow::Stay;
        }
        self.detail.key(key, ctx)
    }

    fn typing(&self) -> bool {
        self.filter.is_some()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.updates, Loaded::Loading)
    }
}

/// The updates by source, each under its heading with its count, those holding `query` in what
/// the row shows or in their name only.
fn sections(ctx: &Ctx, query: &str) -> Vec<Block> {
    let Some(pending) = ctx.shared.updates.ready() else {
        return Vec::new();
    };
    let list = &pending.updates;
    // The arrows keep their column while the filter narrows the rows.
    let pairs: Vec<_> = list.iter().map(|u| (&*u.old, &*u.new)).collect();
    let change = changes(&pairs, ctx.style.glyphs.arrow);
    let shown = labels(list);
    let mut body = Vec::new();
    for (source, heading, glyph) in [
        (Source::Repo, t("Official packages"), icon::PACKAGE),
        (Source::Aur, "AUR", icon::ARCH),
        (Source::Flatpak, "Flatpak", icon::APP),
    ] {
        let part: Vec<_> = list
            .iter()
            .zip(&change)
            .zip(&shown)
            .filter(|((u, _), label)| {
                u.source == source
                    && [&u.name, *label]
                        .iter()
                        .any(|s| s.to_lowercase().contains(query))
            })
            .collect();
        if part.is_empty() {
            continue;
        }
        body.push(Block::Styled(vec![vec![
            (heading.to_string(), Tone::Normal, true),
            (format!(" ({})", part.len()), Tone::Muted, false),
        ]]));
        for ((_, change), label) in part {
            let (was, becomes) = change.clone();
            body.push(Block::Row(
                Lead::Icon(glyph, Tone::Muted),
                vec![(label.clone(), Tone::Normal)],
                vec![(was, Tone::Muted), (becomes, Tone::Accent)],
            ));
        }
        body.push(Block::Blank);
    }
    body
}

/// What each row shows: a package by its name, a Flatpak by the name Flatpak shows, with its
/// branch where two have that name, by its id where two have that too.
fn labels(list: &[Update]) -> Vec<String> {
    let named = |u: &Update| u.source == Source::Flatpak && !u.title.is_empty();
    let twice = |all: &[String], i: usize| {
        named(&list[i]) && (0..all.len()).any(|j| j != i && named(&list[j]) && all[j] == all[i])
    };
    let titles: Vec<String> = list
        .iter()
        .map(|u| match named(u) {
            true => u.title.clone(),
            false => u.name.clone(),
        })
        .collect();
    let branched: Vec<String> = (0..list.len())
        .map(|i| match twice(&titles, i) && !list[i].branch.is_empty() {
            true => format!("{} {}", list[i].title, list[i].branch),
            false => titles[i].clone(),
        })
        .collect();
    (0..list.len())
        .map(|i| match twice(&branched, i) {
            true => list[i].name.clone(),
            false => branched[i].clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::tests::{app, example, open, press, screen};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    #[test]
    fn a_heading_counts_its_updates_in_a_quiet_tone() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = app();
        example(&mut app);
        open(&mut app, Box::new(UpgradePage::new()));
        let shown = screen(&mut app, &mut term);
        let y = shown.lines().position(|l| l.contains("AUR (1)")).unwrap();
        let x = shown.lines().nth(y).unwrap().find("AUR").unwrap();
        let cell = |dx: usize| &term.backend().buffer()[((x + dx) as u16, y as u16)];
        assert!(cell(0).modifier.contains(Modifier::BOLD));
        let count = cell(4);
        assert_eq!(count.symbol(), "(");
        assert_eq!(count.fg, app.ctx.style.tone(Tone::Muted).fg.unwrap());
        assert!(!count.modifier.contains(Modifier::BOLD));
    }

    /// Off until f, the filter narrows the updates by name while the actions wait, so enter
    /// starts nothing. esc brings everything back.
    #[test]
    fn the_filter_narrows_the_updates_to_the_names_holding_its_text() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        open(&mut app, Box::new(UpgradePage::new()));
        let footer = |shown: &str| {
            shown
                .lines()
                .rfind(|l| !l.trim().is_empty())
                .unwrap_or("")
                .to_string()
        };
        let shown = screen(&mut app, &mut term);
        assert!(
            !shown.contains("Filter") && footer(&shown).contains("f filter"),
            "{shown}"
        );
        press(&mut app, &[Key::Char('f')]);
        assert!(screen(&mut app, &mut term).contains("Filter"));
        press(&mut app, &[Key::Char('P'), Key::Char('a'), Key::Char('r')]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("paru") && shown.contains("AUR (1)"),
            "{shown}"
        );
        assert!(
            !shown.contains("firefox") && !shown.contains("Install now"),
            "{shown}"
        );
        assert!(footer(&shown).contains("esc close filter"), "{shown}");
        press(&mut app, &[Key::Enter]);
        assert_eq!(screen(&mut app, &mut term), shown, "enter starts nothing");
        press(&mut app, &[Key::Char('z')]);
        assert!(screen(&mut app, &mut term).contains("No matches"));
        press(&mut app, &[Key::Esc, Key::Char('f')]);
        for c in "tchx84".chars() {
            press(&mut app, &[Key::Char(c)]);
        }
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Flatseal"), "the id finds the name: {shown}");
        press(&mut app, &[Key::Esc]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("firefox") && shown.contains("Install now"),
            "{shown}"
        );
        press(&mut app, &[Key::Down, Key::Char('f'), Key::Esc]);
        let shown = screen(&mut app, &mut term);
        let chosen = shown
            .lines()
            .find(|l| l.contains("Install now"))
            .unwrap_or("");
        assert!(
            chosen.contains(app.ctx.style.glyphs.cursor),
            "the cursor stays: {shown}"
        );
    }

    /// Flatpak by the name it shows, the branch beside it where two share that name, the id where
    /// two share both. Packages keep their names.
    #[test]
    fn a_flatpak_shows_its_name_and_only_where_that_repeats_more() {
        let pending = crate::data::updates::parse(
            "repo\tmesa\t1-1\t2-1\n\
             flatpak\tcom.github.tchx84.Flatseal\t2.4.0\t2.4.1\tFlatseal\tstable\n\
             flatpak\torg.freedesktop.Platform.GL.default//25.08\ta\tb\tMesa\t25.08\n\
             flatpak\torg.freedesktop.Platform.GL.default//26.08\tc\td\tMesa\t26.08\n\
             flatpak\torg.freedesktop.Platform.GL32.default\te\tf\tMesa\t26.08\n\
             flatpak\torg.x.Old\tg\th\n",
        );
        assert_eq!(
            labels(&pending.updates),
            [
                "mesa",
                "Flatseal",
                "Mesa 25.08",
                "org.freedesktop.Platform.GL.default//26.08",
                "org.freedesktop.Platform.GL32.default",
                "org.x.Old",
            ]
        );
    }

    #[test]
    fn foreign_packages_without_source_are_named() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        open(&mut app, Box::new(UpgradePage::new()));
        assert!(!screen(&mut app, &mut term).contains("without source"));
        app.ctx.shared.dropped = Loaded::Ready(vec!["gone".into(), "old".into()]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("2 packages without source: gone, old."),
            "{shown}"
        );
    }

    /// The old version gives way first, at its front, and padding gives way without a mark.
    #[test]
    fn forty_columns_keep_every_new_version_whole() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, _) = app();
        example(&mut app);
        open(&mut app, Box::new(UpgradePage::new()));
        let mut term = Terminal::new(TestBackend::new(40, 30)).unwrap();
        let shown = screen(&mut app, &mut term);
        for new in [
            "7.2.9.zen1-1",
            "1:26.2.4-1",
            "144.0.1-1",
            "258.2-1",
            "2.4.1",
        ] {
            assert!(shown.contains(&format!("→  {new}")), "{new}:\n{shown}");
        }
        let paru = shown.lines().find(|l| l.contains("paru")).unwrap();
        assert!(paru.ends_with("2.1.0-1  →  2.1.1-1"), "{shown}");
    }
}
