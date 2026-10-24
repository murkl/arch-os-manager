//! Repair: for an update that fails on a stale lock or an outdated keyring, for a keyring beyond
//! that, and for files a package lost.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::i18n::t;
use crate::script::{Op, op};
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Page, Tone};

/// A repair: its name, its icon, and when it helps and what it does.
struct Repair {
    op: Op,
    name: &'static str,
    icon: &'static str,
    note: &'static str,
}

pub struct RepairPage {
    repairs: [Repair; 3],
    list: List<usize>,
}

impl RepairPage {
    pub fn new() -> RepairPage {
        let repairs = [
            Repair {
                op: op::REPAIR_RUN,
                name: t("Repair the package management"),
                icon: icon::REPAIR,
                note: t("Removes a stale lock, updates the keyring and the system."),
            },
            Repair {
                op: op::REPAIR_KEYRING,
                name: t("Recreate the keyring"),
                icon: icon::KEYRING,
                note: t("Creates the keyring anew from the keyring packages."),
            },
            Repair {
                op: op::REPAIR_FILES,
                name: t("Check the package files"),
                icon: icon::VERIFY,
                note: t("Finds packages with missing or altered files."),
            },
        ];
        let mut rows = Vec::new();
        for (i, r) in repairs.iter().enumerate() {
            // An empty heading is a blank line, between one repair and the next.
            if i > 0 {
                rows.push(Row::heading(""));
            }
            rows.push(Row::new(i, r.name).icon(r.icon, Tone::Normal).note(r.note));
        }
        RepairPage {
            repairs,
            list: List::new(rows),
        }
    }
}

impl Page for RepairPage {
    fn title(&self) -> String {
        t("Repair").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        vec![("↑↓", Does::Choose), ("⏎", Does::Run), ("esc", Does::Back)]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(i) => {
                let r = &self.repairs[i];
                Flow::Push(start(Job::new(r.op, Vec::new()).named(r.name), ctx))
            }
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
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
        press(&mut app, &[Key::Down, Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Repair › Check the package files"),
            "the header names the action, not the step: {shown}"
        );
    }

    #[test]
    fn each_repair_says_under_it_when_it_helps_and_the_cursor_steps_to_the_next() {
        use crate::ui::app::tests::{app, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        open(&mut app, Box::new(RepairPage::new()));
        let shown = screen(&mut app, &mut term);
        let lines: Vec<&str> = shown.lines().collect();
        let at = |text: &str| {
            lines
                .iter()
                .position(|l| l.contains(text))
                .unwrap_or_else(|| panic!("{text}: {shown}"))
        };
        assert_eq!(
            at("Removes a stale lock"),
            at("Repair the package management") + 1
        );
        assert_eq!(
            at("Creates the keyring anew"),
            at("Recreate the keyring") + 1
        );
        assert_eq!(at("Finds packages"), at("Check the package files") + 1);
        press(&mut app, &[Key::Down]);
        let shown = screen(&mut app, &mut term);
        let row = shown
            .lines()
            .find(|l| l.contains("Recreate the keyring"))
            .unwrap_or_default();
        assert!(row.contains(app.ctx.style.glyphs.cursor), "{shown}");
    }
}
