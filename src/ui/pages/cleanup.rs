//! Clean-up: what goes, how much space it frees, what stays for a recovery.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::cleanup::Plan;
use crate::data::format_size;
use crate::i18n::{t, tn};
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail, done, loaded};
use crate::ui::draw::width;
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

/// The plan as a sheet: each part with its count and size, the sum under the sizes, and below
/// it what else is good to know.
fn sheet(plan: &Plan, ctx: &Ctx) -> Vec<Block> {
    let bytes = |v: &[(String, u64)]| v.iter().map(|(_, b)| b).sum();
    let mut parts = Vec::new();
    for (glyph, label, list) in [
        (icon::PACKAGE, t("Unneeded packages"), &plan.orphans),
        (icon::CACHE, t("Package cache"), &plan.cache),
        (icon::ARCH, t("Old AUR builds"), &plan.aur),
    ] {
        if !list.is_empty() {
            let size = (format_size(bytes(list)), Tone::Normal);
            parts.push((glyph, label, list.len().to_string(), size));
        }
    }
    if plan.flatpak {
        let size = (t("unknown").to_string(), Tone::Muted);
        parts.push((icon::APP, t("Flatpak runtimes"), String::new(), size));
    }
    let sum = format_size(plan.bytes());
    // Every size as wide as the widest, so the counts before them stand in one column too.
    let column = parts
        .iter()
        .map(|(.., (size, _))| width(size))
        .chain([width(&sum)])
        .max()
        .unwrap_or(0);
    let pad = |s: &str| format!("{}{s}", " ".repeat(column - width(s)));
    let mut body: Vec<Block> = parts
        .into_iter()
        .map(|(glyph, label, count, (size, tone))| {
            Block::Row(
                Lead::Icon(glyph, Tone::Muted),
                label.into(),
                vec![(count, Tone::Muted), (format!("   {}", pad(&size)), tone)],
            )
        })
        .collect();
    body.push(Block::Row(
        Lead::None,
        String::new(),
        vec![(ctx.style.glyphs.rule.repeat(column), Tone::Muted)],
    ));
    body.push(Block::Row(
        Lead::None,
        t("Frees").into(),
        vec![(pad(&sum), Tone::Accent)],
    ));
    let mut notes = Vec::new();
    if !plan.kept.is_empty() {
        notes.push(Block::Note(
            Lead::Icon(icon::RECOVERY, Tone::Muted),
            tn(
                "{n} kernel package stays: the snapshots need it for a recovery.",
                "{n} kernel packages stay: the snapshots need them for a recovery.",
                plan.kept.len(),
                &[],
            ),
            Tone::Muted,
        ));
    }
    if !plan.orphans.is_empty() {
        let names: Vec<&str> = plan.orphans.iter().map(|(n, _)| n.as_str()).collect();
        notes.push(Block::Note(
            Lead::Icon(icon::PACKAGE, Tone::Muted),
            names.join(", "),
            Tone::Muted,
        ));
    }
    if plan.flatpak {
        notes.push(Block::Note(
            Lead::Icon(icon::APP, Tone::Muted),
            t("Flatpak looks for unused runtimes while cleaning up.").into(),
            Tone::Muted,
        ));
    }
    if !notes.is_empty() {
        body.push(Block::Blank);
        body.extend(notes);
    }
    body
}

impl Page for CleanupPage {
    fn title(&self) -> String {
        t("Clean up").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(plan) = loaded(&ctx.shared.cleanup, area, buf, ctx).cloned() else {
            return;
        };
        let mut acts = Vec::new();
        let body = if plan.is_empty() && !plan.flatpak {
            vec![done(t("Nothing to clean up."))]
        } else {
            acts.push(Act::new(t("Clean up now"), |ctx: &mut Ctx| {
                let text = t("Clean up now?\nWhat is listed is removed for good. Three versions of every installed package stay in the cache.").to_string();
                Flow::Push(start(Job::new(op::CLEANUP_RUN, Vec::new()).confirm(text), ctx))
            })
            .icon(icon::CLEANUP));
            sheet(&plan, ctx)
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_under_the_sizes_and_tells_the_rest_below() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (app, _) = crate::ui::app::tests::app();
        let plan = Plan::parse(
            "orphan\texample-lib\t12.00 MiB\ncache\t/c/a.pkg.tar.zst\t901775360\nkeep\tlinux-zen-7.2.8.zen1-1\nflatpak\n",
        );
        let body = sheet(&plan, &app.ctx);
        let frees = body
            .iter()
            .position(|b| matches!(b, Block::Row(_, label, _) if label == t("Frees")))
            .unwrap();
        let parts: Vec<&Vec<(String, Tone)>> = body[..frees]
            .iter()
            .filter_map(|b| match b {
                Block::Row(Lead::Icon(..), _, detail) => Some(detail),
                _ => None,
            })
            .collect();
        assert_eq!(parts.len(), 3);
        let sizes: Vec<usize> = parts.iter().map(|d| width(&d[1].0)).collect();
        assert!(
            sizes.iter().all(|w| *w == sizes[0]),
            "the counts end in one column: {parts:?}"
        );
        assert!(body[..frees].iter().all(|b| !matches!(b, Block::Note(..))));
        assert!(body[frees..].iter().any(
            |b| matches!(b, Block::Note(_, text, _) if text.contains("kernel package stays"))
        ));
    }
}
