//! Clean-up: what can go, part by part, each marked as settings.yaml keeps it, the action with
//! what the marked parts free, and what the row under the cursor removes.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::cleanup::{Engine, Part, Plan};
use crate::data::format_size;
use crate::i18n::{t, tf, tn};
use crate::script::op;
use crate::ui::detail::loaded;
use crate::ui::draw::{put, wrap};
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Part(Part),
    Remove,
}

pub struct CleanupPage {
    list: List<Item>,
    /// The cursor stands on the row likely meant, once the plan is there.
    placed: bool,
}

impl CleanupPage {
    pub fn new() -> CleanupPage {
        let mut list = List::default();
        list.markable = true;
        CleanupPage {
            list,
            placed: false,
        }
    }
}

/// The parts this system has: Flatpak where the probe found it, an engine where the plan did.
fn parts(plan: &Plan, ctx: &Ctx) -> Vec<Part> {
    Part::ALL
        .into_iter()
        .filter(|p| match p {
            Part::Flatpak => ctx.features().is_some_and(|f| f.ok("flatpak")),
            Part::Docker | Part::Podman => plan.engine(*p).is_some(),
            _ => true,
        })
        .collect()
}

/// The parts a run removes: marked, ready, and with something to remove, as far as known.
fn removes(plan: &Plan, ctx: &Ctx) -> Vec<Part> {
    parts(plan, ctx)
        .into_iter()
        .filter(|p| ctx.settings.cleans(*p) && plan.ready(*p))
        .filter(|p| plan.bytes(*p).is_none_or(|b| b > 0))
        .collect()
}

fn label(part: Part) -> &'static str {
    match part {
        Part::Orphans => t("Unneeded packages"),
        Part::Cache => t("Package cache"),
        Part::Aur => t("Old AUR builds"),
        Part::Flatpak => t("Flatpak runtimes"),
        Part::Docker => t("Docker images"),
        Part::Podman => t("Podman images"),
    }
}

/// What a part frees, a quiet `?` where only the removal tells, as the usage does for a value it
/// cannot read, or why it cannot be cleaned up now.
fn size(plan: &Plan, part: Part) -> String {
    match plan.engine(part) {
        Some(Engine::Stopped) => t("not running").into(),
        Some(Engine::Denied) => t("no access").into(),
        _ => plan.bytes(part).map_or_else(|| "?".into(), format_size),
    }
}

/// What the row under the cursor removes.
fn describe(plan: &Plan, item: Item, nothing: bool) -> String {
    let part = match item {
        Item::Remove if nothing => return t("Nothing to clean up.").into(),
        Item::Remove => return t("Removes what is marked, for good.").into(),
        Item::Part(part) => part,
    };
    match (part, plan.engine(part)) {
        (Part::Orphans, _) if plan.orphans.is_empty() => t("Every package is still needed.").into(),
        (Part::Orphans, _) => {
            let names: Vec<&str> = plan.orphans.iter().map(|(n, _)| n.as_str()).collect();
            tn(
                "{n} package nothing needs any more: {names}.",
                "{n} packages nothing needs any more: {names}.",
                names.len(),
                &[("names", &names.join(", "))],
            )
        }
        (Part::Cache, _) => tn(
            "{n} file from the package cache. Three versions of every installed package stay.",
            "{n} files from the package cache. Three versions of every installed package stay.",
            plan.cache.len(),
            &[],
        ),
        (Part::Aur, _) => tn(
            "The build folder of {n} package no longer installed.",
            "The build folders of {n} packages no longer installed.",
            plan.aur.len(),
            &[],
        ),
        (Part::Flatpak, _) => {
            t("Runtimes no app needs any more. Flatpak finds them only while it cleans up.").into()
        }
        (_, Some(Engine::Ready { unused, .. })) => {
            let mut text = match unused.len() {
                0 => t("Every image is in use.").to_string(),
                n => tn(
                    "{n} image no container uses: {names}.",
                    "{n} images no container uses: {names}.",
                    n,
                    &[("names", &unused.join(", "))],
                ),
            };
            if part == Part::Docker {
                text += " ";
                text += t("The build cache goes too.");
            }
            text
        }
        (_, Some(Engine::Stopped)) => {
            t("Docker is not running, and asking it would start it.").into()
        }
        (_, Some(Engine::Denied)) => {
            t("Only members of the group docker may clean up its images.").into()
        }
        (_, _) => {
            let engine = if part == Part::Docker {
                "Docker"
            } else {
                "Podman"
            };
            tf("{engine} did not answer.", &[("engine", &engine)])
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
        let parts = parts(&plan, ctx);
        let removes = removes(&plan, ctx);
        let nothing = removes.is_empty();
        let mut rows: Vec<Row<Item>> = parts
            .iter()
            .map(|&p| {
                let marked = ctx.settings.cleans(p);
                let counts = marked && plan.ready(p) && plan.bytes(p).is_some_and(|b| b > 0);
                let tone = if counts { Tone::Normal } else { Tone::Muted };
                Row::new(Item::Part(p), label(p))
                    .detail(size(&plan, p), tone)
                    .dim(!marked)
            })
            .collect();
        let frees = plan.frees(|p| removes.contains(&p));
        let tone = if frees > 0 { Tone::Accent } else { Tone::Muted };
        rows.push(Row::heading(""));
        rows.push(
            Row::new(Item::Remove, t("Remove now"))
                .icon(icon::CLEANUP, Tone::Normal)
                .detail(format_size(frees), tone)
                .dim(nothing),
        );
        let height = rows.len() as u16;
        self.list.set(rows);
        self.list.marked = parts
            .iter()
            .filter(|p| ctx.settings.cleans(**p))
            .map(|p| Item::Part(*p))
            .collect();
        if !self.placed {
            self.placed = true;
            if !nothing {
                self.list.focus(&Item::Remove);
            }
        }
        self.list.draw(
            Rect {
                height: height.min(area.height),
                ..area
            },
            buf,
            ctx,
        );
        // Under the list, where the labels start, as a note under a row.
        let Some(item) = self.list.selected().and_then(|r| r.id) else {
            return;
        };
        let x = area.x + 3;
        let room = area.width.saturating_sub(3);
        let below = (area.y + height + 1)..area.bottom();
        for (y, line) in below.zip(wrap(&describe(&plan, item, nothing), room as usize)) {
            put(buf, x, y, &line, room, ctx.style.tone(Tone::Muted));
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        let enter = match self.list.selected().and_then(|r| r.id) {
            Some(Item::Remove) => Does::Run,
            _ => Does::Mark,
        };
        vec![("↑↓", Does::Choose), ("⏎", enter), ("esc", Does::Back)]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(Item::Part(part)) => {
                let marked = ctx.settings.cleans(part);
                ctx.settings.set_cleans(part, !marked);
                ctx.save_settings();
                Flow::Stay
            }
            Picked::Open(Item::Remove) => {
                let Some(plan) = ctx.shared.cleanup.ready() else {
                    return Flow::Stay;
                };
                let removes = removes(plan, ctx);
                if removes.is_empty() {
                    return Flow::Stay;
                }
                let names: Vec<&str> = removes.iter().map(|p| label(*p)).collect();
                let text = tf(
                    "Clean up now?\nThese go for good: {parts}.",
                    &[("parts", &names.join(", "))],
                );
                let args = removes.iter().map(|p| p.id().to_string()).collect();
                Flow::Push(start(Job::new(op::CLEANUP_RUN, args).confirm(text), ctx))
            }
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.cleanup, Loaded::Loading)
    }
}

/// What the marked parts free, for the start page and the run's next steps.
pub fn frees(plan: &Plan, ctx: &Ctx) -> u64 {
    let removes = removes(plan, ctx);
    plan.frees(|p| removes.contains(&p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::tests::{app, example, open, press, screen};

    fn engines() -> Plan {
        Plan::parse(concat!(
            "orphan\texample-lib\t12.00 MiB\n",
            "cache\t/c/a.pkg.tar.zst\t901775360\n",
            "docker\tready\n",
            r#"docker-df	{"Type":"Images","Reclaimable":"2.1GB (60%)"}"#,
            "\n",
            r#"docker-du	{"Images":[{"Containers":"0","ID":"sha256:b0f6405e5582","Repository":"example/dev","Tag":"latest"}]}"#,
            "\n",
            "podman\tfailed\n",
        ))
    }

    #[test]
    fn every_part_there_is_listed_marked_by_default_with_the_action_and_its_sum() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        app.ctx.shared.cleanup = Loaded::Ready(engines());
        open(&mut app, Box::new(CleanupPage::new()));
        let shown = screen(&mut app, &mut term);
        let line = |label: &str| {
            shown
                .lines()
                .find(|l| l.contains(label))
                .unwrap_or_else(|| panic!("no {label}: {shown}"))
                .to_string()
        };
        let mark = app.ctx.style.glyphs.mark;
        assert!(line("Unneeded packages").contains(mark), "{shown}");
        assert!(line("Package cache").contains(mark), "{shown}");
        assert!(
            !line("Docker images").contains(mark),
            "off by default: {shown}"
        );
        assert!(line("Docker images").ends_with("2.0 GiB"), "{shown}");
        assert!(line("Podman images").ends_with(" ?"), "{shown}");
        assert!(line("Flatpak runtimes").contains(mark), "{shown}");
        assert!(line("Flatpak runtimes").ends_with(" ?"), "{shown}");
        assert!(!shown.contains("unknown"), "{shown}");
        // 12 MiB of packages and 860 MiB of cache, the cursor on the action.
        assert!(line("Remove now").ends_with("872.0 MiB"), "{shown}");
        assert!(
            line("Remove now").contains(app.ctx.style.glyphs.cursor),
            "{shown}"
        );
        assert!(shown.contains("Removes what is marked"), "{shown}");
        press(&mut app, &[Key::Up, Key::Up]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("1 image no container uses: example/dev:latest."),
            "{shown}"
        );
        assert!(shown.contains("⏎ mark"), "{shown}");
    }

    #[test]
    fn enter_marks_a_part_in_the_settings_and_the_action_takes_the_marked() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        app.ctx.shared.cleanup = Loaded::Ready(engines());
        open(&mut app, Box::new(CleanupPage::new()));
        screen(&mut app, &mut term);
        // Up from the action: Podman, then Docker.
        press(&mut app, &[Key::Up, Key::Up, Key::Enter]);
        assert!(app.ctx.settings.cleans(Part::Docker));
        assert_eq!(app.ctx.settings.cleanup.get("docker"), Some(&true));
        let shown = screen(&mut app, &mut term);
        let remove = shown.lines().find(|l| l.contains("Remove now")).unwrap();
        assert!(remove.ends_with("2.8 GiB"), "{shown}");
        press(&mut app, &[Key::Up, Key::Enter]);
        assert!(!app.ctx.settings.cleans(Part::Flatpak));
        press(&mut app, &[Key::End, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("These go for good: Unneeded packages, Package cache, Docker images."),
            "the podman that did not answer stays out: {shown}"
        );
    }

    #[test]
    fn with_nothing_marked_the_action_waits() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        for part in Part::ALL {
            app.ctx.settings.set_cleans(part, false);
        }
        app.ctx.shared.cleanup = Loaded::Ready(engines());
        open(&mut app, Box::new(CleanupPage::new()));
        let shown = screen(&mut app, &mut term);
        assert!(!shown.contains(app.ctx.style.glyphs.mark), "{shown}");
        let first = shown
            .lines()
            .find(|l| l.contains("Unneeded packages"))
            .unwrap();
        assert!(
            first.contains(app.ctx.style.glyphs.cursor),
            "on the first part: {shown}"
        );
        press(&mut app, &[Key::End]);
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Nothing to clean up."), "{shown}");
        press(&mut app, &[Key::Enter]);
        assert!(!screen(&mut app, &mut term).contains("Clean up now?"));
    }
}
