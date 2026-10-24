//! The mirror list: how old, which servers, and ranking it anew with reflector.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::{ago, date};
use crate::i18n::t;
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::icon;
use crate::ui::list::Lead;
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

pub struct MirrorsPage {
    detail: Detail,
}

impl MirrorsPage {
    pub fn new() -> MirrorsPage {
        MirrorsPage {
            detail: Detail::default(),
        }
    }
}

impl Page for MirrorsPage {
    fn title(&self) -> String {
        t("Mirrors").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(m) = loaded(&ctx.shared.mirrors, area, buf, ctx).cloned() else {
            return;
        };
        let mut body = Vec::new();
        if let Some(changed) = m.changed {
            body.push(Block::Field(
                t("Ranked").into(),
                format!("{}  ({})", date(changed), ago(changed)),
                Tone::Normal,
            ));
        }
        let weekly = if m.timer {
            (t("every week by itself"), Tone::Ok)
        } else {
            (t("manual only"), Tone::Muted)
        };
        body.push(Block::Field(t("Ranking").into(), weekly.0.into(), weekly.1));
        body.push(Block::Blank);
        for server in &m.servers {
            body.push(Block::Row(
                Lead::Icon(icon::MIRRORS, Tone::Muted),
                vec![(server.clone(), Tone::Normal)],
                Vec::new(),
            ));
        }
        let missing = (!ctx.available(op::MIRRORS_RANK)).then(|| t("needs reflector").to_string());
        let acts = vec![
            Act::new(t("Refresh now"), |ctx: &mut Ctx| {
                Flow::Push(start(Job::new(op::MIRRORS_RANK, Vec::new()), ctx))
            })
            .icon(icon::MIRRORS)
            .unless(missing),
        ];
        self.detail.set(body, acts);
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.mirrors, Loaded::Loading)
    }
}
