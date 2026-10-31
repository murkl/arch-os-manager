//! Configuration updates: the .pacnew and .pacsave files pacman left beside a changed file.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::i18n::{t, tf};
use crate::script::{op, query};
use crate::ui::detail::{Act, Block, Detail, done, failure, loaded, recheck, show};
use crate::ui::fetch::Fetch;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

pub struct PacnewPage {
    list: List<String>,
    /// What stands instead of the list while no file is left.
    none: Detail,
}

impl PacnewPage {
    pub fn new() -> PacnewPage {
        PacnewPage {
            list: List::filterable(),
            none: Detail::default(),
        }
    }
}

impl Page for PacnewPage {
    fn title(&self) -> String {
        t("Config updates").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(files) = loaded(&ctx.shared.pacnew, area, buf, ctx) else {
            return;
        };
        if files.is_empty() {
            self.none.set(
                vec![done(t("Every configuration is up to date."))],
                vec![recheck(ctx)],
            );
            self.none.draw(area, buf, ctx);
            return;
        }
        self.list.set(
            files
                .iter()
                .map(|f| Row::new(f.clone(), f.clone()).icon(crate::ui::icon::PACNEW, Tone::Muted))
                .collect(),
        );
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, Does)> {
        if ctx.checking() {
            return vec![("esc", Does::Back)];
        }
        if self.empty(ctx) {
            return self.none.hints();
        }
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if self.animating(ctx) {
            return Flow::Ignored;
        }
        if self.empty(ctx) {
            return self.none.key(key, ctx);
        }
        match self.list.key(key) {
            Picked::Open(file) => Flow::Push(Box::new(FilePage::new(file))),
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.pacnew, Loaded::Loading) || ctx.checking()
    }
}

impl PacnewPage {
    /// Whether the files are read and none is left.
    fn empty(&self, ctx: &Ctx) -> bool {
        ctx.shared.pacnew.ready().is_some_and(Vec::is_empty)
    }
}

/// What pacnew.sh merges only and never replaces. The reason stands there.
const MERGE_ONLY: [&str; 5] = [
    "/etc/passwd",
    "/etc/group",
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/sudoers",
];

struct FilePage {
    file: String,
    diff: Fetch<String>,
    detail: Detail,
}

impl FilePage {
    fn new(file: String) -> FilePage {
        let f = file.clone();
        FilePage {
            file,
            diff: Fetch::new(move || query(op::PACNEW_DIFF, &[&f])),
            detail: Detail::default(),
        }
    }
}

impl Page for FilePage {
    fn title(&self) -> String {
        self.file
            .rsplit('/')
            .next()
            .unwrap_or(&self.file)
            .to_string()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.diff.ensure(ctx);
        // The file was dealt with: nothing is left to show.
        if ctx
            .shared
            .pacnew
            .ready()
            .is_some_and(|files| !files.contains(&self.file))
        {
            show(vec![done(t("Done"))], area, buf, ctx);
            return;
        }
        let mut body = vec![Block::Text(self.file.clone(), Tone::Muted), Block::Blank];
        match &self.diff.value {
            Loaded::Ready(diff) => body.push(Block::Code(
                diff.lines()
                    .skip(2)
                    .map(|l| {
                        let tone = match l.chars().next() {
                            Some('+') => Tone::Ok,
                            Some('-') => Tone::Error,
                            Some('@') => Tone::Accent,
                            _ => Tone::Muted,
                        };
                        (l.to_string(), tone)
                    })
                    .collect(),
            )),
            Loaded::Failed(f) => body.extend(failure(f).into_iter().take(2)),
            _ => body.push(Block::Text(t("Loading").into(), Tone::Muted)),
        }
        let (target, kind) = self
            .file
            .rsplit_once(".pac")
            .unwrap_or((self.file.as_str(), "new"));
        let target = target.to_string();
        // A .pacsave or .pacorig is the configuration as it was, which pacman saved.
        let saved = kind != "new";
        let apply = |verb: crate::script::Op, label: &str, question: String, file: String| {
            let glyph = if verb == op::PACNEW_TAKE {
                crate::ui::icon::TAKE
            } else {
                crate::ui::icon::KEEP
            };
            Act::new(label, move |ctx: &mut Ctx| {
                Flow::Push(start(
                    Job::new(verb, vec![file.clone()]).confirm(question.clone()),
                    ctx,
                ))
            })
            .icon(glyph)
        };
        let (take, taken, kept) = match saved {
            true => (
                t("Restore the saved version"),
                tf(
                    "Restore the saved version?\nReplaces {target}.",
                    &[("target", &target)],
                ),
                t("Keep the current version?\nDeletes the saved version."),
            ),
            false => (
                t("Use the new version"),
                tf(
                    "Use the new version?\nReplaces {target}.",
                    &[("target", &target)],
                ),
                t("Keep the current version?\nDeletes the new version."),
            ),
        };
        let mut acts = vec![
            apply(op::PACNEW_TAKE, take, taken, self.file.clone()).unless(
                MERGE_ONLY
                    .contains(&target.as_str())
                    .then(|| t("merge required").to_string()),
            ),
            apply(
                op::PACNEW_KEEP,
                t("Keep the current version"),
                kept.to_string(),
                self.file.clone(),
            ),
        ];
        let file = self.file.clone();
        let meld = (!ctx.available(op::PACNEW_MERGE)).then(|| t("needs meld").to_string());
        acts.push(
            Act::new(t("Merge in Meld"), move |ctx: &mut Ctx| {
                Flow::Push(start(Job::new(op::PACNEW_MERGE, vec![file.clone()]), ctx))
            })
            .icon(crate::ui::icon::PACNEW)
            .unless(meld),
        );
        self.detail.set(body, acts);
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        self.diff.receive();
        Flow::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::tests::{app, open, press, screen};

    /// What pacnew.sh never replaces is offered for merging only, with the reason.
    #[test]
    fn an_account_file_is_only_merged() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let page = FilePage {
            file: "/etc/shadow.pacnew".into(),
            diff: Fetch::new(|| Ok(String::new())),
            detail: Detail::default(),
        };
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("merge required"), "{shown}");
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(!shown.contains("Replaces /etc/shadow."), "{shown}");
    }

    /// The files are the ones pacnew.sh refuses to replace, both ways.
    #[test]
    fn merges_only_what_the_script_refuses_to_replace() {
        let script = include_str!("../../../scripts/pacnew/pacnew.sh");
        let line = script
            .lines()
            .find(|l| l.contains("/etc/passwd |"))
            .unwrap();
        let mut listed: Vec<&str> = line
            .split(')')
            .next()
            .unwrap()
            .split('|')
            .map(str::trim)
            .collect();
        let mut here = MERGE_ONLY.to_vec();
        listed.sort_unstable();
        here.sort_unstable();
        assert_eq!(listed, here);
    }

    /// A .pacsave is the configuration as it was: the actions say so, and keeping the current
    /// version names what goes.
    #[test]
    fn a_saved_version_is_named_as_such() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let page = FilePage {
            file: "/etc/foo.conf.pacsave".into(),
            diff: Fetch::new(|| Ok(String::new())),
            detail: Detail::default(),
        };
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Restore the saved version"), "{shown}");
        assert!(!shown.contains("new version"), "{shown}");
        press(&mut app, &[Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Deletes the saved version."), "{shown}");
    }
}
