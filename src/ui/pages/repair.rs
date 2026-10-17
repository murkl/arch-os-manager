//! Repair: for an update that fails on a stale lock or an outdated keyring, and for files a
//! package lost.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::i18n::t;
use crate::script::op;
use crate::ui::detail::{Act, Block, Detail};
use crate::ui::icon;
use crate::ui::list::Lead;
use crate::ui::{Ctx, Flow, Key, Page, Tone};

pub struct RepairPage {
    detail: Detail,
}

impl RepairPage {
    pub fn new() -> RepairPage {
        let mut detail = Detail::default();
        // Each case under the icon of the action that answers it.
        let case = |glyph, when: &str, what: &str| {
            [
                Block::Row(
                    Lead::Icon(glyph, Tone::Normal),
                    vec![(when.into(), Tone::Normal)],
                    Vec::new(),
                ),
                Block::Note(Lead::None, what.into(), Tone::Muted),
            ]
        };
        let mut body = case(
            icon::REPAIR,
            t("When an update keeps failing"),
            t("On a database lock no package manager holds any more, or on signatures that cannot be verified. The stale lock is removed, the keyring is created anew from the packaged keys, and the system is updated."),
        )
        .to_vec();
        body.push(Block::Blank);
        body.extend(case(
            icon::CHECK,
            t("When files went missing or changed"),
            t("After a crash or a full disk. Every file of every package is compared with what the package brought, and a package with a difference is installed again. A configuration changed by hand stays."),
        ));
        detail.set(
            body,
            vec![
                Act::new(t("Renew the keyring"), |ctx: &mut Ctx| {
                    Flow::Push(start(Job::new(op::REPAIR_RUN, Vec::new()), ctx))
                })
                .icon(icon::REPAIR),
                Act::new(t("Check the files"), |ctx: &mut Ctx| {
                    Flow::Push(start(Job::new(op::REPAIR_FILES, Vec::new()), ctx))
                })
                .icon(icon::CHECK),
            ],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_named_after_the_action_chosen() {
        use crate::ui::app::tests::{app, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = app();
        open(&mut app, Box::new(RepairPage::new()));
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Repair › Check the files"),
            "the header names the action, not the step: {shown}"
        );
    }
}
