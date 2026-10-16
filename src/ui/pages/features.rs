//! The optional packages: which are there, what each adds, and installing or removing one.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::features::{self, EXTENSION, Status};
use crate::i18n::{t, tf};
use crate::script::op;
use crate::settings::AUR_HELPERS;
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

fn status(s: &Status) -> (String, Tone) {
    match s {
        Status::Ok(version) if version.is_empty() => (t("installed").into(), Tone::Ok),
        Status::Ok(version) => (version.clone(), Tone::Ok),
        Status::Missing => (t("not installed").into(), Tone::Muted),
        Status::Broken => (t("not working").into(), Tone::Error),
    }
}

pub struct FeaturesPage {
    list: List,
    focus: Option<String>,
}

impl FeaturesPage {
    pub fn new(focus: Option<String>) -> FeaturesPage {
        FeaturesPage {
            list: List::default(),
            focus,
        }
    }
}

impl Page for FeaturesPage {
    fn title(&self) -> String {
        t("Optional packages").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(features) = loaded(&ctx.shared.features, area, buf, ctx) else {
            return;
        };
        let rows = features
            .list
            .iter()
            .map(|(package, s)| {
                let (text, tone) = status(s);
                let kind = features::package(package).map(|(kind, _)| format!("{kind}/"));
                let row =
                    Row::new(package.clone(), package.clone()).prefix(kind.unwrap_or_default());
                match features.unmet(package).filter(|_| *s == Status::Missing) {
                    Some(why) => row.detail(t(why), Tone::Muted).dot(Tone::Muted).dim(true),
                    None => row.detail(text, tone).dot(tone),
                }
            })
            .collect();
        self.list.set(rows);
        if let Some(p) = self.focus.take() {
            self.list.focus(&p);
        }
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, _: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(package) => Flow::Push(Box::new(FeaturePage {
                package,
                detail: Detail::default(),
            })),
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.features, Loaded::Loading)
    }
}

struct FeaturePage {
    package: String,
    detail: Detail,
}

impl Page for FeaturePage {
    fn title(&self) -> String {
        self.package.clone()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(features) = ctx.features() else {
            return;
        };
        let state = features
            .list
            .iter()
            .find(|(p, _)| *p == self.package)
            .map(|(_, s)| s.clone())
            .unwrap_or(Status::Missing);
        let (text, tone) = status(&state);
        let known = features::package(&self.package);
        let adds = known
            .map(|(_, p)| t(&p.adds).to_string())
            .unwrap_or_default();
        // An extension is no package: the desktop script sets it up for this user.
        let extension = known.is_some_and(|(kind, _)| kind == EXTENSION);
        let (install, remove, args) = if extension {
            (
                op::DESKTOP_EXTENSION_INSTALL,
                op::DESKTOP_EXTENSION_REMOVE,
                Vec::new(),
            )
        } else {
            (
                op::FEATURES_INSTALL,
                op::FEATURES_REMOVE,
                vec![self.package.clone()],
            )
        };
        let mut body = vec![
            Block::Text(adds, Tone::Normal),
            Block::Blank,
            Block::Field(t("State").into(), text, tone),
        ];
        let package = self.package.clone();
        let install_args = args.clone();
        let unmet = features.unmet(&self.package);
        let acts = if let (Status::Missing, Some(why)) = (&state, unmet) {
            body.push(Block::Blank);
            body.push(Block::Text(
                tf("Not offered here: it {why}.", &[("why", &t(why))]),
                Tone::Muted,
            ));
            Vec::new()
        } else if state == Status::Missing {
            vec![
                Act::new(t("Install"), move |ctx: &mut Ctx| {
                    Flow::Push(start(Job::new(install, install_args.clone()), ctx))
                })
                .icon(icon::PACKAGE),
            ]
        } else {
            vec![
                Act::new(t("Remove"), move |ctx: &mut Ctx| {
                    let text = tf(
                        "Remove {name}?\nWhat it adds to the manager is gone with it.",
                        &[("name", &package)],
                    );
                    Flow::Push(start(Job::new(remove, args.clone()).confirm(text), ctx))
                })
                .icon(icon::CLEANUP),
            ]
        };
        if state == Status::Missing && AUR_HELPERS.contains(&self.package.as_str()) {
            body.push(Block::Blank);
            body.push(Block::Text(
                t("Built from source from the AUR; git and base-devel come along.").into(),
                Tone::Muted,
            ));
        }
        if state == Status::Missing && extension && unmet.is_none() {
            body.push(Block::Blank);
            body.push(Block::Text(
                t("Downloaded from extensions.gnome.org; GNOME loads it at the next login.").into(),
                Tone::Muted,
            ));
        }
        if state == Status::Missing && self.package == "reflector" {
            body.push(Block::Blank);
            body.push(Block::Text(
                t("Set up to rank by speed within the country of the time zone, weekly.").into(),
                Tone::Muted,
            ));
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::features::Features;
    use crate::ui::app::tests::{app, screen};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn names_each_package_after_its_kind_and_cuts_the_kind_first() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::De);
        let (mut app, _) = app();
        app.ctx.shared.features = Loaded::Ready(Features::parse(
            "paru\tok\t2.1.0-1\narch-update\tmissing\n@gnome\tok\n",
        ));
        let mut term = Terminal::new(TestBackend::new(40, 16)).unwrap();
        crate::ui::app::tests::open(&mut app, Box::new(FeaturesPage::new(None)));
        let shown = screen(&mut app, &mut term);
        crate::i18n::set(crate::i18n::Lang::En);
        assert!(shown.contains("aur-helper/paru"), "{shown}");
        assert!(shown.contains("…on/arch-update"), "{shown}");
    }
}
