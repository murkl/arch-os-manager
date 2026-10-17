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

/// The plan as a sheet: every part with its count and size, what is good to know about it
/// right under it, and the sum under the sizes.
fn sheet(plan: &Plan, ctx: &Ctx) -> Vec<Block> {
    let bytes = |v: &[(String, u64)]| v.iter().map(|(_, b)| b).sum::<u64>();
    let size = |b: u64| {
        let tone = if b == 0 { Tone::Muted } else { Tone::Normal };
        (format_size(b), tone)
    };
    let names: Vec<&str> = plan.orphans.iter().map(|(n, _)| n.as_str()).collect();
    let kept = (!plan.kept.is_empty()).then(|| {
        tn(
            "{n} kernel package stays: the snapshots need it for a recovery.",
            "{n} kernel packages stay: the snapshots need them for a recovery.",
            plan.kept.len(),
            &[],
        )
    });
    // The label, then its count in parentheses, muted.
    let counted = |label: &str, count: String| {
        vec![
            (label.to_string(), Tone::Normal),
            (format!(" ({count})"), Tone::Muted),
        ]
    };
    let packages = |n| tn("{n} package", "{n} packages", n, &[]);
    let mut parts = vec![
        (
            icon::PACKAGE,
            counted(t("Unneeded packages"), packages(plan.orphans.len())),
            size(bytes(&plan.orphans)),
            (!names.is_empty()).then(|| names.join(", ")),
        ),
        (
            icon::CACHE,
            counted(t("Package cache"), packages(plan.cache.len())),
            size(bytes(&plan.cache)),
            kept,
        ),
        (
            icon::ARCH,
            counted(
                t("Old AUR builds"),
                tn("{n} build", "{n} builds", plan.aur.len(), &[]),
            ),
            size(bytes(&plan.aur)),
            None,
        ),
    ];
    if plan.flatpak {
        parts.push((
            icon::APP,
            vec![(t("Flatpak runtimes").to_string(), Tone::Normal)],
            (t("unknown").to_string(), Tone::Muted),
            Some(t("Flatpak looks for unused runtimes while cleaning up.").into()),
        ));
    }
    let sum = format_size(plan.bytes());
    // Every size as wide as the widest, so they stand in one column.
    let column = parts
        .iter()
        .map(|(_, _, (size, _), _)| width(size))
        .chain([width(&sum)])
        .max()
        .unwrap_or(0);
    let pad = |s: &str| format!("{}{s}", " ".repeat(column - width(s)));
    let mut body = Vec::new();
    for (glyph, label, (size, tone), note) in parts {
        body.push(Block::Row(
            Lead::Icon(glyph, Tone::Muted),
            label,
            vec![(pad(&size), tone)],
        ));
        if let Some(note) = note {
            body.push(Block::Note(Lead::None, note, Tone::Muted));
        }
    }
    body.push(Block::Row(
        Lead::None,
        Vec::new(),
        vec![(ctx.style.glyphs.rule.repeat(column), Tone::Muted)],
    ));
    body.push(Block::Row(
        Lead::None,
        vec![(t("Frees").into(), Tone::Normal)],
        vec![(pad(&sum), Tone::Accent)],
    ));
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
        let mut body = sheet(&plan, ctx);
        if plan.is_empty() && !plan.flatpak {
            body.push(Block::Blank);
            body.push(done(t("Nothing to clean up.")));
        } else {
            acts.push(Act::new(t("Remove now"), |ctx: &mut Ctx| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_is_listed_with_its_note_under_it_and_the_sum_under_the_sizes() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (app, _) = crate::ui::app::tests::app();
        let plan = Plan::parse(
            "orphan\texample-lib\t12.00 MiB\ncache\t/c/a.pkg.tar.zst\t901775360\nkeep\tlinux-zen-7.2.8.zen1-1\nflatpak\n",
        );
        let body = sheet(&plan, &app.ctx);
        let rows: Vec<(String, &Vec<(String, Tone)>)> = body
            .iter()
            .filter_map(|b| match b {
                Block::Row(Lead::Icon(..), label, detail) => {
                    Some((label.iter().map(|(t, _)| t.as_str()).collect(), detail))
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 4, "{body:?}");
        assert_eq!(rows[0].0, "Unneeded packages (1 package)");
        assert_eq!(rows[2].0, "Old AUR builds (0 builds)");
        assert_eq!(rows[2].1[0].1, Tone::Muted, "nothing of it is muted");
        assert!(rows[2].1[0].0.ends_with("0 bytes"));
        let sizes: Vec<usize> = rows.iter().map(|(_, d)| width(&d[0].0)).collect();
        assert!(
            sizes.iter().all(|w| *w == sizes[0]),
            "the sizes stand in one column: {rows:?}"
        );
        let cache = body
            .iter()
            .position(
                |b| matches!(b, Block::Row(_, label, _) if label.first().is_some_and(|(l, _)| l == "Package cache")),
            )
            .unwrap();
        assert!(
            matches!(&body[cache + 1], Block::Note(Lead::None, text, _) if text.contains("kernel package stays")),
            "{body:?}"
        );
        assert!(
            matches!(&body[cache - 1], Block::Note(Lead::None, text, _) if text == "example-lib")
        );
        let empty = sheet(&Plan::default(), &app.ctx);
        assert!(
            empty
                .iter()
                .filter(|b| matches!(b, Block::Row(Lead::Icon(..), ..)))
                .count()
                == 3
        );
    }
}
