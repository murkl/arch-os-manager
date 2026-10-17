//! Configuration updates: the .pacnew and .pacsave files pacman left beside a changed file.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::i18n::{t, tf};
use crate::script::{op, query};
use crate::ui::detail::{Act, Block, Detail, done, failure, loaded, show};
use crate::ui::fetch::Fetch;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

pub struct PacnewPage {
    list: List,
}

impl PacnewPage {
    pub fn new() -> PacnewPage {
        PacnewPage {
            list: List::default(),
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
            show(
                vec![done(t("Every configuration is up to date."))],
                area,
                buf,
                ctx,
            );
        }
        self.list.set(
            files
                .iter()
                .map(|f| Row::new(f.clone(), f.clone()).icon(crate::ui::icon::PACNEW, Tone::Muted))
                .collect(),
        );
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, _: &mut Ctx) -> Flow {
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
        matches!(ctx.shared.pacnew, Loaded::Loading)
    }
}

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
            diff: Fetch::new("diff", move || query(op::PACNEW_DIFF, &[&f])),
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
        let target = self
            .file
            .rsplit_once(".pac")
            .map_or(self.file.as_str(), |(t, _)| t)
            .to_string();
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
        let mut acts = vec![
            apply(
                op::PACNEW_TAKE,
                t("Use the new version"),
                tf(
                    "Use the new version?\n{target} is replaced. Changes made to it by hand stay only in a snapshot, where snapper keeps one.",
                    &[("target", &target)],
                ),
                self.file.clone(),
            ),
            apply(
                op::PACNEW_KEEP,
                t("Keep the current version"),
                tf(
                    "Keep the current version?\nThe new version is deleted. {target} stays as it is.",
                    &[("target", &target)],
                ),
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

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }

    fn data(&mut self, data: Box<dyn std::any::Any + Send>, _: &mut Ctx) -> Flow {
        self.diff.receive(&mut Some(data));
        Flow::Stay
    }
}
