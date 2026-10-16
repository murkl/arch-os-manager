//! Repair: for an update that fails on a stale lock or an outdated keyring.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::i18n::t;
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail};
use crate::ui::icon;
use crate::ui::{Ctx, Flow, Key, Page, Tone};

pub struct RepairPage {
    detail: Detail,
}

impl RepairPage {
    pub fn new() -> RepairPage {
        let mut detail = Detail::default();
        detail.set(
            vec![
                Block::Text(t("For an update that keeps failing: on a database lock no package manager holds any more, or on signatures that cannot be verified.").into(), Tone::Normal),
                Block::Blank,
                Block::Text(t("A stale lock is removed, the keyring is created anew from the packaged keys, and the system is updated.").into(), Tone::Muted),
            ],
            vec![Act::new(t("Repair"), |ctx: &mut Ctx| Flow::Push(start(Job::new(op::REPAIR_RUN, Vec::new()), ctx))).icon(icon::REPAIR)],
        );
        RepairPage { detail }
    }
}

impl Page for RepairPage {
    fn title(&self) -> String {
        t("Repair").into()
    }
    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.detail.draw(area, buf, ctx);
    }
    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        self.detail.hints()
    }
    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }
}
