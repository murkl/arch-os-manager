//! Firmware updates through fwupd.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::i18n::t;
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::icon;
use crate::ui::list::{Lead, changes};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

pub struct FirmwarePage {
    detail: Detail,
}

impl FirmwarePage {
    pub fn new() -> FirmwarePage {
        FirmwarePage {
            detail: Detail::default(),
        }
    }
}

impl Page for FirmwarePage {
    fn title(&self) -> String {
        t("Firmware").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(list) = loaded(&ctx.shared.firmware, area, buf, ctx).cloned() else {
            return;
        };
        let mut body = Vec::new();
        let mut acts = Vec::new();
        if list.is_empty() {
            body.push(Block::Row(
                Lead::Icon(icon::DONE, Tone::Ok),
                t("The firmware is up to date.").into(),
                String::new(),
                Tone::Muted,
            ));
        } else {
            let pairs: Vec<_> = list.iter().map(|f| (&*f.current, &*f.new)).collect();
            let change = changes(&pairs, ctx.style.glyphs.arrow);
            for (f, change) in list.iter().zip(change) {
                body.push(Block::Change(
                    Lead::Icon(icon::FIRMWARE, Tone::Muted),
                    f.device.clone(),
                    change,
                ));
                if !f.summary.is_empty() {
                    body.push(Block::Text(f.summary.clone(), Tone::Muted));
                }
            }
            body.push(Block::Blank);
            body.push(Block::Text(
                t("Some firmware is written during the next restart.").into(),
                Tone::Muted,
            ));
            acts.push(
                Act::new(t("Update the firmware"), |ctx: &mut Ctx| {
                    Flow::Push(start(Job::new(op::FIRMWARE_UPDATE, Vec::new()), ctx))
                })
                .icon(icon::FIRMWARE),
            );
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
        matches!(ctx.shared.firmware, Loaded::Loading)
    }
}
