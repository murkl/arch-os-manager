//! Settings: what is written to settings.yaml, and what follows from it on the system.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::features::FeaturesPage;
use super::log::HiddenPage;
use crate::i18n::{self, Lang, t, tf};
use crate::script::{self, Failure, op};
use crate::settings::AUR_HELPERS;
use crate::ui::detail::{Block, Detail};
use crate::ui::draw::put;
use crate::ui::list::{List, Picked, Row};
use crate::ui::target::{SETTINGS, Target};
use crate::ui::{Ctx, Flow, Key, Page, Style, Tone};

pub struct SettingsPage {
    list: List,
    focus: Option<Target>,
    /// The switch being put in place; its row waits and no switch takes a press.
    applying: Option<Target>,
    /// Why the last switch could not be put in place.
    failed: Option<String>,
}

impl SettingsPage {
    pub fn new(focus: Option<Target>) -> SettingsPage {
        SettingsPage {
            list: List::default(),
            focus,
            applying: None,
            failed: None,
        }
    }

    fn value(&self, target: Target, ctx: &Ctx) -> (String, Tone, bool) {
        let on_off = |on: bool| {
            if self.applying == Some(target) {
                (ctx.style.spinner().to_string(), Tone::Accent)
            } else if on {
                (t("on").to_string(), Tone::Ok)
            } else {
                (t("off").to_string(), Tone::Muted)
            }
        };
        match target {
            Target::Language => (i18n::current().name().into(), Tone::Muted, false),
            Target::Theme => (ctx.theme().name.clone(), Tone::Muted, false),
            Target::AurHelper => {
                let helper = script::aur().unwrap_or_else(|| t("none").into());
                let text = if ctx.settings.aur_helper.is_none() {
                    tf("{helper}, automatic", &[("helper", &helper)])
                } else {
                    helper
                };
                (text, Tone::Muted, false)
            }
            Target::Notifications | Target::Desktop
                if let Some(why) = ctx.features().and_then(|f| f.lacks("desktop")) =>
            {
                (t(why).to_string(), Tone::Muted, true)
            }
            Target::Notifications => {
                let missing = ctx.features().is_some() && !ctx.available(op::DESKTOP_NOTIFY);
                if missing {
                    (
                        tf("needs {package}", &[("package", &"libnotify")]),
                        Tone::Muted,
                        true,
                    )
                } else {
                    let (text, tone) = on_off(ctx.settings.notify.unwrap_or(true));
                    (text, tone, false)
                }
            }
            Target::Desktop => {
                let (text, tone) = on_off(ctx.settings.app_menu.unwrap_or(true));
                (text, tone, false)
            }
            Target::Hidden => (
                ctx.settings.hidden_log.len().to_string(),
                Tone::Muted,
                false,
            ),
            Target::Features => match ctx.features() {
                Some(f) => {
                    let ok = f.list.iter().filter(|(p, _)| f.ok(p)).count();
                    (format!("{ok}/{}", f.list.len()), Tone::Muted, false)
                }
                None => ("…".into(), Tone::Muted, false),
            },
            _ => (String::new(), Tone::Muted, false),
        }
    }

    /// Flips a switch in settings.yaml and puts the entry and the timer in place to match.
    fn toggle(&mut self, target: Target, ctx: &mut Ctx) {
        if self.applying.is_some() {
            return;
        }
        let setting = if target == Target::Desktop {
            &mut ctx.settings.app_menu
        } else {
            &mut ctx.settings.notify
        };
        *setting = crate::settings::switch(!setting.unwrap_or(true));
        ctx.save_settings();
        let Some(features) = ctx.features().cloned() else {
            return;
        };
        let settings = ctx.settings.clone();
        self.applying = Some(target);
        self.failed = None;
        ctx.spawn(move || crate::integration::apply(&settings, &features));
    }
}

impl Page for SettingsPage {
    fn title(&self) -> String {
        t("Settings").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let rows = SETTINGS
            .iter()
            .map(|&target| {
                let (detail, tone, dim) = self.value(target, ctx);
                Row::new(target.id(), target.label())
                    .icon(target.icon(), Tone::Normal)
                    .detail(detail, tone)
                    .dim(dim)
            })
            .collect();
        self.list.set(rows);
        if let Some(target) = self.focus.take() {
            self.list.focus(target.id());
        }
        let height = SETTINGS.len() as u16;
        self.list.draw(
            Rect {
                height: area.height.min(height),
                ..area
            },
            buf,
            ctx,
        );
        if let Some(e) = &self.failed {
            put(
                buf,
                area.x + 2,
                area.y + height + 1,
                e,
                area.width.saturating_sub(2),
                ctx.style.tone(Tone::Error),
            );
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        let id = match self.list.key(key) {
            Picked::Open(id) => id,
            Picked::Used => return Flow::Stay,
            Picked::Ignored => return Flow::Ignored,
        };
        match Target::from_id(&id) {
            Some(t @ (Target::Language | Target::Theme | Target::AurHelper)) => {
                Flow::Push(choice(t, ctx))
            }
            Some(Target::Notifications | Target::Desktop)
                if ctx.features().is_some_and(|f| f.lacks("desktop").is_some()) =>
            {
                Flow::Stay
            }
            Some(Target::Notifications)
                if ctx.features().is_some() && !ctx.available(op::DESKTOP_NOTIFY) =>
            {
                Flow::Push(Box::new(FeaturesPage::new(Some("libnotify".into()))))
            }
            Some(t @ (Target::Notifications | Target::Desktop)) => {
                self.toggle(t, ctx);
                Flow::Stay
            }
            Some(Target::Hidden) => Flow::Push(Box::new(HiddenPage::new())),
            Some(Target::Features) => Flow::Push(Box::new(FeaturesPage::new(None))),
            Some(Target::About) => Flow::Push(Box::new(AboutPage::new())),
            _ => Flow::Stay,
        }
    }

    fn data(&mut self, data: Box<dyn std::any::Any + Send>, _: &mut Ctx) -> Flow {
        if let Ok(result) = data.downcast::<Result<crate::integration::Applied, Failure>>() {
            self.applying = None;
            self.failed = result
                .err()
                .map(|f| format!("{}: {}", f.headline(), f.detail));
        }
        Flow::Stay
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, _: &Ctx) -> bool {
        self.applying.is_some()
    }
}

/// A choice among a few values, opening on the current one.
pub struct ChoicePage {
    target: Target,
    list: List,
    /// The theme as it was, for leaving a preview without choosing.
    before: Option<Style>,
}

pub fn choice(target: Target, ctx: &mut Ctx) -> Box<dyn Page> {
    let (rows, current): (Vec<Row>, String) = match target {
        Target::Language => (
            Lang::ALL
                .iter()
                .map(|l| Row::new(l.code(), l.name()))
                .collect(),
            i18n::current().code().into(),
        ),
        Target::Theme => (
            ctx.themes
                .iter()
                .map(|th| Row::new(th.id.clone(), th.name.clone()))
                .collect(),
            ctx.theme().id.clone(),
        ),
        _ => {
            let installed = |h: &str| ctx.features().is_some_and(|f| f.ok(h));
            let mut rows =
                vec![Row::new("auto", t("Automatic")).detail(t("paru, else yay"), Tone::Muted)];
            rows.extend(AUR_HELPERS.iter().map(|h| {
                let (text, tone) = if installed(h) {
                    (t("installed"), Tone::Ok)
                } else {
                    (t("not installed"), Tone::Muted)
                };
                Row::new(*h, *h).detail(text, tone).dim(!installed(h))
            }));
            rows.push(Row::new("none", t("None")).detail(t("no AUR"), Tone::Muted));
            (
                rows,
                ctx.settings
                    .aur_helper
                    .clone()
                    .unwrap_or_else(|| "auto".into()),
            )
        }
    };
    let rows = rows
        .into_iter()
        .map(|r| {
            let lead = if r.id == current {
                crate::ui::icon::CHECK
            } else {
                ""
            };
            r.icon(lead, Tone::Accent)
        })
        .collect();
    let mut list = List::new(rows);
    list.focus(&current);
    let before = (target == Target::Theme).then_some(ctx.style);
    Box::new(ChoicePage {
        target,
        list,
        before,
    })
}

impl ChoicePage {
    fn apply(&self, id: &str, ctx: &mut Ctx) {
        match self.target {
            Target::Language => {
                ctx.settings.language = Some(id.to_string());
                i18n::set(Lang::from_code(id).unwrap_or(Lang::En));
            }
            Target::Theme => ctx.settings.theme = Some(id.to_string()),
            _ => {
                ctx.settings.aur_helper = (id != "auto").then(|| id.to_string());
                let helper = ctx
                    .features()
                    .and_then(|f| f.aur_helper(ctx.settings.aur_helper.as_deref()));
                script::set_aur(helper);
                ctx.reload();
            }
        }
        ctx.save_settings();
    }

    fn preview(&self, ctx: &mut Ctx) {
        if self.target == Target::Theme
            && let Some(id) = self.list.selected().map(|r| r.id.clone())
            && let Some(theme) = ctx.themes.iter().find(|th| th.id == id)
        {
            ctx.style = Style::new(&theme.colors, ctx.console);
        }
    }
}

impl Page for ChoicePage {
    fn title(&self) -> String {
        self.target.label().into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        vec![
            ("↑↓", t("choose").into()),
            ("⏎", t("use").into()),
            ("esc", t("back").into()),
        ]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(id) => {
                self.apply(&id, ctx);
                Flow::Pop
            }
            Picked::Used => {
                self.preview(ctx);
                Flow::Stay
            }
            Picked::Ignored => {
                if let Some(style) = self.before {
                    ctx.style = style;
                }
                Flow::Ignored
            }
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }
}

pub struct AboutPage {
    detail: Detail,
}

impl AboutPage {
    pub fn new() -> AboutPage {
        AboutPage {
            detail: Detail::default(),
        }
    }
}

impl Page for AboutPage {
    fn title(&self) -> String {
        t("About").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let path = |p: std::path::PathBuf| p.display().to_string();
        let field = |label: &str, value: String| Block::Field(label.into(), value, Tone::Normal);
        let mut body = Vec::new();
        if let Some(sys) = ctx.shared.system.ready() {
            body.push(field(t("Kernel"), sys.kernel.clone()));
            if let Some(desktop) = &sys.desktop {
                let session = sys.session.as_deref().map(|kind| {
                    let mut kind = kind.to_string();
                    kind[..1].make_ascii_uppercase();
                    format!(" {} {kind}", ctx.style.glyphs.separator)
                });
                body.push(field(
                    t("Desktop"),
                    format!("{desktop}{}", session.unwrap_or_default()),
                ));
            }
            body.push(field("pacman", sys.pacman.clone()));
            if let Some(booted) = sys.booted {
                let local = booted.to_zoned(jiff::tz::TimeZone::system());
                body.push(field(
                    t("Booted"),
                    local.strftime("%Y-%m-%d %H:%M").to_string(),
                ));
            }
            body.push(Block::Blank);
        }
        body.extend([
            Block::Field(
                t("Version").into(),
                crate::version::version().into(),
                Tone::Normal,
            ),
            field(t("Project"), env!("CARGO_PKG_REPOSITORY").into()),
            Block::Field(
                t("Settings").into(),
                path(crate::paths::settings()),
                Tone::Normal,
            ),
            Block::Field(
                t("Own themes").into(),
                path(crate::paths::themes()),
                Tone::Normal,
            ),
            Block::Field(t("Log").into(), path(crate::paths::log()), Tone::Normal),
            Block::Blank,
            Block::Heading(t("Command line").into()),
        ]);
        body.push(Block::Code(
            crate::cli::USAGE
                .iter()
                .chain(&["system-manager --help"])
                .map(|l| (l.to_string(), Tone::Muted))
                .collect(),
        ));
        if let Some(e) = &ctx.settings_error {
            body.push(Block::Blank);
            body.push(Block::Text(
                tf("settings.yaml is not used: {error}", &[("error", e)]),
                Tone::Warn,
            ));
        }
        for e in &ctx.theme_errors {
            body.push(Block::Text(
                tf("Theme skipped: {error}", &[("error", e)]),
                Tone::Warn,
            ));
        }
        self.detail.set(body, Vec::new());
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
    fn only_the_switch_being_put_in_place_waits() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (app, _) = crate::ui::app::tests::app();
        let mut page = SettingsPage::new(None);
        page.applying = Some(Target::Notifications);
        let spinning = |target| {
            let (text, ..) = page.value(target, &app.ctx);
            app.ctx.style.glyphs.spinner.contains(&text.as_str())
        };
        assert!(spinning(Target::Notifications));
        assert!(!spinning(Target::Desktop));
    }
}
