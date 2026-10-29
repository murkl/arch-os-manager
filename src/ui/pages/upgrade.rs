//! What an update would bring, the news to read first, and the update itself.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::news::{NewsPage, unread};
use super::run::{Job, start};
use crate::data::updates::Source;
use crate::i18n::{t, tf, tn};
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail, done};
use crate::ui::draw::waiting;
use crate::ui::icon;
use crate::ui::list::{Lead, changes};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

pub struct UpgradePage {
    detail: Detail,
    focused: bool,
}

impl UpgradePage {
    pub fn new() -> UpgradePage {
        UpgradePage {
            detail: Detail::default(),
            focused: false,
        }
    }

    fn build(&mut self, ctx: &Ctx) {
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
                let list = &pending.updates;
                if list.is_empty() && pending.failed.is_empty() {
                    known_current = true;
                    body.push(done(t("The system is up to date.")));
                }
                let pairs: Vec<_> = list.iter().map(|u| (&*u.old, &*u.new)).collect();
                let change = changes(&pairs, ctx.style.glyphs.arrow);
                for (source, heading, glyph) in [
                    (Source::Repo, t("Official packages"), icon::PACKAGE),
                    (Source::Aur, "AUR", icon::ARCH),
                    (Source::Flatpak, "Flatpak", icon::APP),
                ] {
                    let part: Vec<_> = list
                        .iter()
                        .zip(&change)
                        .filter(|(u, _)| u.source == source)
                        .collect();
                    if part.is_empty() {
                        continue;
                    }
                    body.push(Block::Styled(vec![vec![
                        (heading.to_string(), Tone::Normal, true),
                        (format!(" ({})", part.len()), Tone::Muted, false),
                    ]]));
                    for (u, change) in part {
                        let (was, becomes) = change.clone();
                        body.push(Block::Row(
                            Lead::Icon(glyph, Tone::Muted),
                            vec![(u.name.clone(), Tone::Normal)],
                            vec![(was, Tone::Muted), (becomes, Tone::Accent)],
                        ));
                    }
                    body.push(Block::Blank);
                }
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
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, Does)> {
        if self.animating(ctx) {
            return vec![("esc", Does::Back)];
        }
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if self.animating(ctx) {
            return Flow::Ignored;
        }
        self.build(ctx);
        self.detail.key(key, ctx)
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.updates, Loaded::Loading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::tests::{app, example, open, screen};
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
