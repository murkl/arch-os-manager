//! What an update would bring, the news to read first, and the update itself.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::news::{NewsPage, unread};
use super::run::{Job, start};
use crate::data::updates::Source;
use crate::i18n::{t, tf, tn};
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail};
use crate::ui::draw::waiting;
use crate::ui::icon;
use crate::ui::list::{Lead, changes};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

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
                tn(
                    "An unread news item. Read it first: it may announce a step an update needs by hand.",
                    "{n} unread news. Read them first: they may announce steps an update needs by hand.",
                    news,
                    &[],
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
                tf("The list of updates needs {package}.", &[("package", p)]),
                Tone::Muted,
            )),
            Loaded::Failed(f) => {
                body.push(Block::Text(f.headline(), Tone::Warn));
                body.push(Block::Text(f.explanation(), Tone::Muted));
            }
            Loaded::Ready(list) if list.is_empty() => {
                known_current = true;
                body.push(Block::Row(
                    Lead::Icon(icon::DONE, Tone::Ok),
                    t("The system is up to date.").to_string(),
                    String::new(),
                    Tone::Muted,
                ));
            }
            Loaded::Ready(list) => {
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
                    body.push(Block::Heading(format!("{heading} ({})", part.len())));
                    for (u, change) in part {
                        body.push(Block::Change(
                            Lead::Icon(glyph, Tone::Muted),
                            u.name.clone(),
                            change.clone(),
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
                .icon(icon::NEWS),
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
                .unless(checking),
            );
        } else {
            acts.push(
                Act::new(t("Update now"), |ctx: &mut Ctx| {
                    Flow::Push(start(Job::new(op::UPDATE_RUN, Vec::new()), ctx))
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
            waiting(buf, area, t("Looking for updates"), None, None, ctx);
            return;
        }
        self.build(ctx);
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, String)> {
        if self.animating(ctx) {
            return vec![("esc", t("back").into())];
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
