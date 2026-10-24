//! Updates the firmware of the device through fwupd.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::i18n::t;
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail, done, loaded};
use crate::ui::icon;
use crate::ui::list::{Lead, changes};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

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
            body.push(done(t("The firmware is up to date.")));
        } else {
            let pairs: Vec<_> = list.iter().map(|f| (&*f.current, &*f.new)).collect();
            let change = changes(&pairs, ctx.style.glyphs.arrow);
            for (f, (was, becomes)) in list.iter().zip(change) {
                body.push(Block::Row(
                    Lead::Icon(icon::FIRMWARE, Tone::Muted),
                    vec![(f.device.clone(), Tone::Normal)],
                    vec![(was, Tone::Muted), (becomes, Tone::Accent)],
                ));
                if !f.summary.is_empty() {
                    body.push(Block::Text(f.summary.clone(), Tone::Muted));
                }
            }
            body.push(Block::Blank);
            body.push(Block::Text(
                t("Some firmware is installed on the next reboot.").into(),
                Tone::Muted,
            ));
            acts.push(
                Act::new(t("Update now"), |ctx: &mut Ctx| {
                    Flow::Push(start(Job::new(op::FIRMWARE_UPDATE, Vec::new()), ctx))
                })
                .icon(icon::FIRMWARE),
            );
        }
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
        matches!(ctx.shared.firmware, Loaded::Loading)
    }
}
