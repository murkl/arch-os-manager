//! Clean-up: what goes, how much space it frees, what stays for a recovery.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::format_size;
use crate::i18n::{t, tn};
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::icon;
use crate::ui::list::Lead;
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

pub struct CleanupPage {
    detail: Detail,
}

impl CleanupPage {
    pub fn new() -> CleanupPage {
        CleanupPage {
            detail: Detail::default(),
        }
    }
}

impl Page for CleanupPage {
    fn title(&self) -> String {
        t("Clean up").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(plan) = loaded(&ctx.shared.cleanup, area, buf, ctx).cloned() else {
            return;
        };
        let mut body = Vec::new();
        let sum = |v: &[(String, u64)]| format_size(v.iter().map(|(_, b)| b).sum());
        if !plan.orphans.is_empty() {
            body.push(Block::Row(
                Lead::Icon(icon::PACKAGE, Tone::Muted),
                t("Unneeded packages").into(),
                format!("{}  ({})", plan.orphans.len(), sum(&plan.orphans)),
                Tone::Normal,
            ));
            let names: Vec<&str> = plan.orphans.iter().map(|(n, _)| n.as_str()).collect();
            body.push(Block::Text(names.join(", "), Tone::Muted));
            body.push(Block::Blank);
        }
        if !plan.cache.is_empty() {
            body.push(Block::Row(
                Lead::Icon(icon::CLEANUP, Tone::Muted),
                t("Package cache").into(),
                tn(
                    "{n} file ({size})",
                    "{n} files ({size})",
                    plan.cache.len(),
                    &[("size", &sum(&plan.cache))],
                ),
                Tone::Normal,
            ));
        }
        if !plan.kept.is_empty() {
            body.push(Block::Text(
                tn(
                    "{n} kernel package stays: the snapshots need it for a recovery.",
                    "{n} kernel packages stay: the snapshots need them for a recovery.",
                    plan.kept.len(),
                    &[],
                ),
                Tone::Muted,
            ));
        }
        if !plan.aur.is_empty() {
            body.push(Block::Row(
                Lead::Icon(icon::ARCH, Tone::Muted),
                t("Old AUR builds").into(),
                tn(
                    "{n} folder ({size})",
                    "{n} folders ({size})",
                    plan.aur.len(),
                    &[("size", &sum(&plan.aur))],
                ),
                Tone::Normal,
            ));
        }
        if plan.flatpak {
            body.push(Block::Row(
                Lead::Icon(icon::APP, Tone::Muted),
                "Flatpak".into(),
                t("unused runtimes, found while cleaning up").into(),
                Tone::Normal,
            ));
        }
        let mut acts = Vec::new();
        if plan.is_empty() && !plan.flatpak {
            body.push(Block::Row(
                Lead::Icon(icon::DONE, Tone::Ok),
                t("Nothing to clean up.").into(),
                String::new(),
                Tone::Muted,
            ));
        } else {
            body.push(Block::Blank);
            body.push(Block::Row(
                Lead::None,
                t("Frees").into(),
                format_size(plan.bytes()),
                Tone::Accent,
            ));
            acts.push(Act::new(t("Clean up now"), |ctx: &mut Ctx| {
                let text = t("Clean up now?\nWhat is listed is removed for good. Three versions of every installed package stay in the cache.").to_string();
                Flow::Push(start(Job::new(op::CLEANUP_RUN, Vec::new()).confirm(text), ctx))
            })
            .icon(icon::CLEANUP));
        }
        self.detail.set(body, acts);
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.cleanup, Loaded::Loading)
    }
}
