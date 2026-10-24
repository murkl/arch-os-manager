//! The optional packages: which are there, what each adds, and installing or removing one.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::features::{self, Status};
use crate::i18n::{t, tf};
use crate::script::op;
use crate::settings::AUR_HELPERS;
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

fn status(s: &Status) -> (String, Tone) {
    match s {
        Status::Ok(version) if version.is_empty() => (t("installed").into(), Tone::Ok),
        Status::Ok(version) => (version.clone(), Tone::Ok),
        Status::Missing => (t("not installed").into(), Tone::Muted),
        Status::Broken => (t("not working").into(), Tone::Error),
    }
}

/// A group's name and icon.
fn group(id: &str) -> (&'static str, &'static str) {
    match id {
        "desktop" => (t("Desktop"), icon::APP),
        "dotfiles" => (t("Dotfiles"), icon::DOTFILES),
        _ => (t("Packages"), icon::PACKAGE),
    }
}

pub struct FeaturesPage {
    list: List<String>,
    focus: Option<String>,
}

impl FeaturesPage {
    pub fn new(focus: Option<String>) -> FeaturesPage {
        FeaturesPage {
            list: List::filterable(),
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
        // Under its group, each with the package icon, the group with one of its own.
        let mut rows = Vec::new();
        let mut last = None;
        for (package, s) in &features.list {
            let id = features::package(package).map_or("", |(group, _)| group);
            if last != Some(id) {
                let (name, glyph) = group(id);
                let heading = match ctx.style.glyphs.icons {
                    true => format!("{glyph}  {name}"),
                    false => name.to_string(),
                };
                rows.push(Row::heading(heading));
                last = Some(id);
            }
            let (text, tone) = status(s);
            let row = Row::new(package.clone(), package.clone()).icon(icon::PACKAGE, Tone::Normal);
            rows.push(
                match features.unmet(package).filter(|_| *s == Status::Missing) {
                    Some(why) => row.detail(t(why), Tone::Muted).dim(true),
                    None => row.detail(text, tone),
                },
            );
        }
        self.list.set(rows);
        if let Some(p) = self.focus.take() {
            self.list.focus(&p);
        }
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
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
        let extension = known.is_some_and(|(_, p)| p.extension);
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
                tf("Unavailable: {why}.", &[("why", &t(why))]),
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
                        "Remove {name}?\nRemoves with pacman -Rns.",
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
                t("Built with makepkg, installs git and base-devel.").into(),
                Tone::Muted,
            ));
        }
        if state == Status::Missing && extension && unmet.is_none() {
            body.push(Block::Blank);
            body.push(Block::Text(
                t("From extensions.gnome.org, active after the next login.").into(),
                Tone::Muted,
            ));
        }
        if state == Status::Missing && self.package == "reflector" {
            body.push(Block::Blank);
            body.push(Block::Text(
                t("Weekly by speed, country from the time zone.").into(),
                Tone::Muted,
            ));
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::features::Features;
    use crate::ui::app::tests::{app, screen};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn lists_each_package_under_its_group() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, _) = app();
        app.ctx.shared.features = Loaded::Ready(Features::parse(
            "paru\tok\t2.1.0-1\ngnome-arch-update\tmissing\n@gnome\tok\n",
        ));
        let mut term = Terminal::new(TestBackend::new(40, 16)).unwrap();
        crate::ui::app::tests::open(&mut app, Box::new(FeaturesPage::new(None)));
        let shown = screen(&mut app, &mut term);
        let lines: Vec<&str> = shown.lines().collect();
        let heading = lines
            .iter()
            .position(|l| l.ends_with("Packages"))
            .expect(&shown);
        assert!(lines[heading + 1].contains("paru") && lines[heading + 1].ends_with("2.1.0-1"));
        let desktop = lines.iter().position(|l| l.ends_with("Desktop")).unwrap();
        assert!(lines[desktop + 1].contains("gnome-arch-update"), "{shown}");
    }
}
