//! Settings: what is written to settings.yaml, and what follows from it on the system.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::features::FeaturesPage;
use super::log::HiddenPage;
use crate::i18n::{self, Lang, t, tf};
use crate::script::{self, Failure, op};
use crate::settings::{AUR_HELPERS, NOTIFY};
use crate::ui::detail::{Block, Detail};
use crate::ui::draw::put;
use crate::ui::fetch::Task;
use crate::ui::list::{List, Picked, Row};
use crate::ui::target::{SETTINGS, Target};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Style, Tone};

pub struct SettingsPage {
    list: List<Target>,
    focus: Option<Target>,
    /// The app menu entry is being put in place. Its row waits and takes no press.
    applying: Option<Task<Result<crate::integration::Applied, Failure>>>,
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
        match target {
            Target::Language => (i18n::current().name().into(), Tone::Muted, false),
            Target::Theme => (ctx.theme().name.clone(), Tone::Muted, false),
            Target::Usage => {
                let (text, tone) = on_off(ctx.settings.usage.unwrap_or(true));
                (text.into(), tone, false)
            }
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
                    let every = ctx.settings.notify();
                    let tone = if every == "off" {
                        Tone::Muted
                    } else {
                        Tone::Ok
                    };
                    (label(every).into(), tone, false)
                }
            }
            Target::Desktop if self.applying.is_some() => ctx.loading(),
            Target::Desktop => {
                let (text, tone) = on_off(ctx.settings.app_menu.unwrap_or(true));
                (text.into(), tone, false)
            }
            Target::Hidden => (
                ctx.settings.hidden_log.len().to_string(),
                Tone::Muted,
                false,
            ),
            Target::Features => match &ctx.shared.features {
                Loaded::Ready(f) => {
                    let ok = f.list.iter().filter(|(p, _)| f.ok(p)).count();
                    (format!("{ok}/{}", f.list.len()), Tone::Muted, false)
                }
                Loaded::Loading => ctx.loading(),
                _ => (t("unknown").into(), Tone::Muted, false),
            },
            _ => (String::new(), Tone::Muted, false),
        }
    }

    /// Flips the app menu entry in settings.yaml and puts it in place to match.
    fn toggle(&mut self, ctx: &mut Ctx) {
        if self.applying.is_some() {
            return;
        }
        let on = ctx.settings.app_menu.unwrap_or(true);
        ctx.settings.app_menu = crate::settings::switch(!on);
        ctx.save_settings();
        let Some(features) = ctx.features().cloned() else {
            return;
        };
        let settings = ctx.settings.clone();
        self.failed = None;
        self.applying = Some(Task::start(ctx, move || {
            crate::integration::apply(&settings, &features)
        }));
    }
}

fn on_off(on: bool) -> (&'static str, Tone) {
    match on {
        true => (t("on"), Tone::Ok),
        false => (t("off"), Tone::Muted),
    }
}

/// What an interval of `NOTIFY` says in a sentence.
fn label(every: &str) -> &'static str {
    match every {
        "hourly" => t("every hour"),
        "daily" => t("every day"),
        "weekly" => t("every week"),
        _ => t("off"),
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
                Row::new(target, target.label())
                    .icon(target.icon(), Tone::Normal)
                    .detail(detail, tone)
                    .dim(dim)
            })
            .collect();
        self.list.set(rows);
        if let Some(target) = self.focus.take() {
            self.list.focus(&target);
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
        let target = match self.list.key(key) {
            Picked::Open(target) => target,
            Picked::Used => return Flow::Stay,
            Picked::Ignored => return Flow::Ignored,
        };
        match target {
            t @ (Target::Language | Target::Theme | Target::AurHelper) => {
                Flow::Push(choice(t, ctx))
            }
            Target::Notifications | Target::Desktop
                if ctx.features().is_some_and(|f| f.lacks("desktop").is_some()) =>
            {
                Flow::Stay
            }
            Target::Notifications
                if ctx.features().is_some() && !ctx.available(op::DESKTOP_NOTIFY) =>
            {
                Flow::Push(Box::new(FeaturesPage::new(Some("libnotify".into()))))
            }
            Target::Notifications => Flow::Push(choice(Target::Notifications, ctx)),
            Target::Desktop => {
                self.toggle(ctx);
                Flow::Stay
            }
            Target::Usage => {
                let on = ctx.settings.usage.unwrap_or(true);
                ctx.settings.usage = crate::settings::switch(!on);
                ctx.save_settings();
                Flow::Stay
            }
            Target::Hidden => Flow::Push(Box::new(HiddenPage::new())),
            Target::Features => Flow::Push(Box::new(FeaturesPage::new(None))),
            Target::About => Flow::Push(Box::new(AboutPage::new())),
            _ => Flow::Stay,
        }
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        if let Some(result) = self.applying.as_mut().and_then(Task::take) {
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

    fn animating(&self, ctx: &Ctx) -> bool {
        self.applying.is_some() || matches!(ctx.shared.features, Loaded::Loading)
    }
}

/// A choice among a few values, opening on the current one.
pub struct ChoicePage {
    target: Target,
    list: List<String>,
    /// The theme as it was, for leaving a preview without choosing.
    before: Option<Style>,
}

pub fn choice(target: Target, ctx: &mut Ctx) -> Box<dyn Page> {
    let (rows, current): (Vec<Row<String>>, String) = match target {
        Target::Language => (
            Lang::ALL
                .iter()
                .map(|l| Row::new(l.code().to_string(), l.name()))
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
        Target::Notifications => (
            NOTIFY
                .iter()
                .map(|every| Row::new(every.to_string(), label(every)))
                .collect(),
            ctx.settings.notify().into(),
        ),
        _ => {
            let installed = |h: &str| ctx.features().is_some_and(|f| f.ok(h));
            let mut rows = vec![
                Row::new("auto".to_string(), t("Automatic"))
                    .detail(t("paru, else yay"), Tone::Muted),
            ];
            rows.extend(AUR_HELPERS.iter().map(|h| {
                let (text, tone) = if installed(h) {
                    (t("installed"), Tone::Ok)
                } else {
                    (t("not installed"), Tone::Muted)
                };
                Row::new(h.to_string(), *h)
                    .detail(text, tone)
                    .dim(!installed(h))
            }));
            rows.push(Row::new("none".to_string(), t("None")).detail(t("no AUR"), Tone::Muted));
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
            let lead = if r.id.as_deref() == Some(current.as_str()) {
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
            // The timer follows at the reading, which puts it in place after the packages.
            Target::Notifications => {
                ctx.settings.notify = (id != NOTIFY[0]).then(|| id.to_string());
                ctx.reload();
            }
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
            && let Some(id) = self.list.selected().and_then(|r| r.id.as_deref())
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
            body.push(field("Pacman", sys.pacman.clone()));
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
        ]);
        for (title, rows) in crate::cli::commands() {
            body.push(Block::Blank);
            body.push(Block::Heading(title.into()));
            body.extend(
                rows.into_iter()
                    .map(|(arg, what)| Block::Field(arg.into(), what, Tone::Normal)),
            );
        }
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

    /// The optional packages show the spinner until they are probed, and the page draws meanwhile.
    #[test]
    fn the_optional_packages_show_the_spinner_until_they_are_probed() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, _) = crate::ui::app::tests::app();
        let page = SettingsPage::new(None);
        let frames = app.ctx.style.glyphs.spinner;
        app.ctx.shared.features = Loaded::Loading;
        let (text, tone, _) = page.value(Target::Features, &app.ctx);
        assert!(frames.contains(&text.as_str()) && tone == Tone::Accent);
        assert!(page.animating(&app.ctx));
        app.ctx.shared.features = Loaded::Failed(Failure::new(op::FEATURES_PROBE, "x"));
        let (text, ..) = page.value(Target::Features, &app.ctx);
        assert!(
            !frames.contains(&text.as_str()) && !page.animating(&app.ctx),
            "{text}"
        );
        crate::ui::app::tests::example(&mut app);
        let (text, ..) = page.value(Target::Features, &app.ctx);
        assert!(!frames.contains(&text.as_str()));
        assert!(!page.animating(&app.ctx));
    }

    #[test]
    fn only_the_app_menu_entry_waits_while_it_is_put_in_place() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (app, _) = crate::ui::app::tests::app();
        let mut page = SettingsPage::new(None);
        page.applying = Some(Task::pending());
        let spinning = |target| {
            let (text, ..) = page.value(target, &app.ctx);
            app.ctx.style.glyphs.spinner.contains(&text.as_str())
        };
        assert!(spinning(Target::Desktop));
        assert!(!spinning(Target::Notifications));
    }

    #[test]
    fn the_notification_is_chosen_from_a_list_and_hourly_is_the_default() {
        use crate::ui::app::tests::{app, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = app();
        for page in Target::Notifications.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("every hour") && shown.contains("every week") && shown.contains("off"),
            "{shown}"
        );
        press(&mut app, &[Key::Down, Key::Enter]);
        assert_eq!(app.ctx.settings.notify.as_deref(), Some("daily"));
        assert!(app.ctx.busy(), "the timer follows at the reading");
        let shown = screen(&mut app, &mut term);
        let row = shown.lines().find(|l| l.contains("Update notification"));
        assert!(row.is_some_and(|r| r.ends_with("every day")), "{shown}");
        for page in Target::Notifications.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Up, Key::Enter]);
        assert_eq!(app.ctx.settings.notify, None);
    }

    #[test]
    fn the_usage_switch_takes_the_row_off_the_start_page_and_back() {
        use crate::ui::app::tests::{app, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = app();
        app.ctx.usage = Some(crate::data::usage::Usage {
            cpu: Some(0.2),
            memory: Some(0.3),
            disk: Some(0.4),
        });
        assert!(screen(&mut app, &mut term).contains("CPU"));
        for page in Target::Usage.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.ctx.settings.usage, Some(false));
        let shown = screen(&mut app, &mut term);
        let row = shown.lines().find(|l| l.contains("System usage"));
        assert!(row.is_some_and(|r| r.ends_with("off")), "{shown}");
        press(&mut app, &[Key::Esc]);
        let shown = screen(&mut app, &mut term);
        assert!(
            !shown.contains("CPU") && shown.contains("Settings"),
            "{shown}"
        );
        for page in Target::Usage.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter, Key::Esc]);
        assert_eq!(app.ctx.settings.usage, None, "on is the default");
        assert!(screen(&mut app, &mut term).contains("CPU"));
    }
}
