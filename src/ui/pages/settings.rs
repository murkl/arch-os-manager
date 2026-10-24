//! Settings: what is written to settings.yaml, and what follows from it on the system.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::features::FeaturesPage;
use super::log::HiddenPage;
use crate::i18n::{self, Lang, t, tf};
use crate::script::{self, Failure, op};
use crate::settings::{AUR_HELPERS, NOTIFY, OUTPUT};
use crate::ui::detail::{Block, Detail};
use crate::ui::draw::{cut, put};
use crate::ui::fetch::Task;
use crate::ui::list::{List, Picked, Row};
use crate::ui::target::{SETTINGS, Target};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Nav, Page, Style, Tone};

/// What the system has to offer for a setting to apply: the entry opens the window, which
/// speaks Wayland only.
fn fact(target: Target) -> &'static str {
    match target {
        Target::Desktop => "wayland",
        _ => "desktop",
    }
}

pub struct SettingsPage {
    list: List<Target>,
    focus: Option<Target>,
    /// The app menu entry is being put in place. Its row waits and takes no press.
    applying: Option<(Instant, Task<Result<crate::integration::Applied, Failure>>)>,
    /// Why the last switch could not be put in place.
    failed: Option<String>,
}

impl SettingsPage {
    pub fn new(focus: Option<Target>) -> SettingsPage {
        SettingsPage {
            list: List::filterable(),
            focus,
            applying: None,
            failed: None,
        }
    }

    fn value(&self, target: Target, ctx: &Ctx) -> (String, Tone, bool) {
        if matches!(target, Target::Notifications | Target::Desktop)
            && let Some(why) = ctx.features().and_then(|f| f.lacks(fact(target)))
        {
            return (t(why).to_string(), Tone::Muted, true);
        }
        match target {
            Target::Language => (i18n::current().name().into(), Tone::Muted, false),
            Target::Theme => (ctx.theme().name.clone(), Tone::Muted, false),
            Target::Usage => {
                let (text, tone) = on_off(ctx.settings.usage.unwrap_or(true));
                (text.into(), tone, false)
            }
            Target::Status => {
                let (text, tone) = on_off(ctx.settings.status.unwrap_or(true));
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
            Target::Output => (
                output_label(ctx.settings.output()).0.into(),
                Tone::Muted,
                false,
            ),
            Target::Flatpak => {
                let (text, tone) = on_off(ctx.settings.flatpak.unwrap_or(true));
                (text.into(), tone, false)
            }
            Target::AurReview => {
                let (text, tone) = on_off(ctx.settings.aur_review.unwrap_or(true));
                (text.into(), tone, false)
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
            Target::Desktop
                if self
                    .applying
                    .as_ref()
                    .is_some_and(|(since, _)| ctx.waited(*since)) =>
            {
                ctx.loading()
            }
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
        let task = Task::start(ctx, move || crate::integration::apply(&settings, &features));
        self.applying = Some((Instant::now(), task));
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

/// What a way of `OUTPUT` is called, and what it does.
fn output_label(way: &str) -> (&'static str, &'static str) {
    match way {
        "verbose" => (t("Verbose"), t("terminal, every command and prompt")),
        _ => (t("Compact"), t("progress, prompts as dialogs")),
    }
}

impl Page for SettingsPage {
    fn title(&self) -> String {
        t("Settings").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        // Flatpak's row only once Flatpak is known to be installed, the review's only with an AUR
        // helper.
        let flatpak = ctx.features().is_some_and(|f| f.ok("flatpak"));
        let aur = script::aur().is_some();
        let shown = |t: Target| match t {
            Target::Flatpak => flatpak,
            Target::AurReview => aur,
            _ => true,
        };
        let mut rows = Vec::new();
        for (heading, group) in [t("Appearance"), t("System"), ""].into_iter().zip(SETTINGS) {
            rows.push(Row::heading(heading));
            for &target in group.iter().filter(|t| shown(**t)) {
                let (detail, tone, dim) = self.value(target, ctx);
                rows.push(
                    Row::new(target, target.label())
                        .icon(target.icon(), Tone::Normal)
                        .detail(detail, tone)
                        .dim(dim),
                );
            }
        }
        // A row still to come keeps the focus waiting.
        let focus = self.focus.filter(|t| rows.iter().any(|r| r.id == Some(*t)));
        self.list.set(rows);
        if let Some(target) = focus {
            self.focus = None;
            self.list.focus(&target);
        }
        let unsaved = ctx
            .settings_unsaved
            .as_ref()
            .map(|e| tf("settings.yaml not saved: {error}", &[("error", e)]));
        let error = self.failed.clone().or(unsaved);
        let room = Rect {
            height: area
                .height
                .saturating_sub(if error.is_some() { 2 } else { 0 }),
            ..area
        };
        let drawn = self.list.draw(room, buf, ctx);
        if let Some(e) = error {
            let width = area.width.saturating_sub(2);
            let e = cut(&e, width.into(), ctx.style.glyphs.ellipsis);
            put(
                buf,
                area.x + 2,
                area.y + drawn + 1,
                &e,
                width,
                ctx.style.tone(Tone::Error),
            );
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        let target = match self.list.key(key) {
            Picked::Open(target) => target,
            Picked::Used => return Flow::Stay,
            Picked::Ignored => return Flow::Ignored,
        };
        match target {
            t @ (Target::Language | Target::Theme | Target::AurHelper | Target::Output) => {
                Flow::Push(choice(t, ctx))
            }
            Target::Notifications | Target::Desktop
                if ctx
                    .features()
                    .is_some_and(|f| f.lacks(fact(target)).is_some()) =>
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
            Target::Status => {
                let on = ctx.settings.status.unwrap_or(true);
                ctx.settings.status = crate::settings::switch(!on);
                ctx.save_settings();
                Flow::Stay
            }
            // The check, the update and the notification follow at once.
            Target::Flatpak => {
                let on = !ctx.settings.flatpak.unwrap_or(true);
                ctx.settings.flatpak = crate::settings::switch(on);
                if let Some(features) = ctx.features() {
                    script::set_flatpak(features.flatpak(on));
                }
                ctx.reload();
                ctx.save_settings();
                Flow::Stay
            }
            Target::AurReview => {
                let on = !ctx.settings.aur_review.unwrap_or(true);
                ctx.settings.aur_review = crate::settings::switch(on);
                script::set_aur_review(on);
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
        if let Some(result) = self.applying.as_mut().and_then(|(_, task)| task.take()) {
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
        Target::Output => (
            OUTPUT
                .iter()
                .map(|way| {
                    let (name, does) = output_label(way);
                    Row::new(way.to_string(), name).detail(does, Tone::Muted)
                })
                .collect(),
            ctx.settings.output().into(),
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
            Target::Output => {
                ctx.settings.output = (id != OUTPUT[0]).then(|| id.to_string());
                script::set_terminal(ctx.settings.terminal());
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

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        vec![("↑↓", Does::Choose), ("⏎", Does::Use), ("esc", Does::Back)]
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
            // Leaving without a choice takes the preview back. Another key changes nothing.
            Picked::Ignored => {
                if matches!(key.nav(false), Some(Nav::Back | Nav::Quit))
                    && let Some(style) = self.before
                {
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
        page.applying = Some((Instant::now(), Task::pending()));
        let spinning = |target| {
            let (text, ..) = page.value(target, &app.ctx);
            app.ctx.style.glyphs.spinner.contains(&text.as_str())
        };
        assert!(spinning(Target::Desktop));
        assert!(!spinning(Target::Notifications));
    }

    /// With an AUR helper the review's row shows, on by default, and its switch is stored.
    #[test]
    fn the_aur_review_is_on_until_switched_off() {
        use crate::ui::app::tests::{app, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(Lang::En);
        let (mut app, mut term) = app();
        script::set_aur(Some("paru"));
        for page in Target::AurReview.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        let row = shown.lines().find(|l| l.contains("AUR review"));
        assert!(row.is_some_and(|r| r.ends_with("on")), "{shown}");
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.ctx.settings.aur_review, Some(false));
        let shown = screen(&mut app, &mut term);
        let row = shown.lines().find(|l| l.contains("AUR review"));
        assert!(row.is_some_and(|r| r.ends_with("off")), "{shown}");
        script::set_aur_review(true);
    }

    /// Flatpak's row shows only where Flatpak is there. Its switch is stored and the updates read
    /// anew.
    #[test]
    fn flatpak_updates_switch_only_where_it_is_there() {
        use crate::ui::app::tests::{app, example, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(Lang::En);
        let (mut app, mut term) = app();
        for page in Target::Settings.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        assert!(!shown.contains("Flatpak"), "not before the probe: {shown}");
        example(&mut app);
        let shown = screen(&mut app, &mut term);
        let row = shown.lines().find(|l| l.contains("Flatpak"));
        assert!(row.is_some_and(|r| r.ends_with("on")), "{shown}");
        for page in Target::Flatpak.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.ctx.settings.flatpak, Some(false));
        assert!(app.ctx.busy(), "the updates are read anew");
        let shown = screen(&mut app, &mut term);
        let row = shown.lines().find(|l| l.contains("Flatpak"));
        assert!(row.is_some_and(|r| r.ends_with("off")), "{shown}");
        app.ctx.shared.features =
            Loaded::Ready(crate::data::features::Features::parse("flatpak\tmissing\n"));
        assert!(!screen(&mut app, &mut term).contains("Flatpak"));
        let pages = Target::Flatpak.open(&mut app.ctx);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].title(), "Optional packages");
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

    /// The scripts follow the choice at once.
    #[test]
    fn the_output_is_chosen_from_a_list_and_compact_is_the_default() {
        use crate::ui::app::tests::{app, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(Lang::En);
        let (mut app, mut term) = app();
        for page in Target::Output.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Compact") && shown.contains("every command and prompt"),
            "{shown}"
        );
        press(&mut app, &[Key::Down, Key::Enter]);
        assert_eq!(app.ctx.settings.output.as_deref(), Some("verbose"));
        assert!(script::terminal());
        let shown = screen(&mut app, &mut term);
        let row = shown.lines().find(|l| l.contains("Output"));
        assert!(row.is_some_and(|r| r.ends_with("Verbose")), "{shown}");
        for page in Target::Output.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Up, Key::Enter]);
        assert_eq!(app.ctx.settings.output, None);
        assert!(!script::terminal());
    }

    #[test]
    fn a_theme_stays_in_preview_until_the_page_is_left() {
        use crate::ui::app::tests::{app, open, press, screen};
        let (mut app, mut term) = app();
        let before = app.ctx.style;
        for page in Target::Theme.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Down]);
        let preview = app.ctx.style;
        assert_ne!(preview.background, before.background);
        press(&mut app, &[Key::Char('x')]);
        assert_eq!(
            app.ctx.style.background, preview.background,
            "x is no way out"
        );
        press(&mut app, &[Key::Esc]);
        assert_eq!(app.ctx.style.background, before.background);
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

    #[test]
    fn appearance_and_system_stand_apart_and_about_last_on_its_own() {
        use crate::ui::app::tests::{app, open, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(Lang::En);
        let (mut app, _) = app();
        open(&mut app, Box::new(SettingsPage::new(None)));
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 30)).unwrap();
        let shown = screen(&mut app, &mut term);
        let lines: Vec<&str> = shown.lines().map(str::trim).collect();
        let at = |text: &str| {
            lines
                .iter()
                .position(|l| l.contains(text))
                .unwrap_or_else(|| panic!("{text}: {shown}"))
        };
        let heading = |text: &str| lines.iter().position(|l| *l == text).unwrap_or(0);
        let (appearance, system) = (heading("Appearance"), heading("System"));
        assert!(appearance > 0 && appearance < at("Language"), "{shown}");
        assert!(
            at("Hidden messages") < system && system < at("AUR helper"),
            "{shown}"
        );
        assert_eq!(lines[system - 1], "", "{shown}");
        assert!(at("Optional packages") < at("About"), "{shown}");
        assert_eq!(lines[at("Optional packages") + 1], "", "{shown}");
        assert_eq!(
            at("Optional packages") + 2,
            at("About"),
            "one blank line: {shown}"
        );
    }

    #[test]
    fn the_status_switch_clears_the_right_of_the_start_page_and_back() {
        use crate::ui::app::tests::{app, example, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        let update = |shown: &str| {
            let row = shown
                .lines()
                .find(|l| l.contains("Update"))
                .unwrap_or_default();
            row.trim_end().to_string()
        };
        assert!(update(&screen(&mut app, &mut term)).ends_with("updates"));
        for page in Target::Status.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.ctx.settings.status, Some(false));
        let shown = screen(&mut app, &mut term);
        let row = shown.lines().find(|l| l.contains("Main menu status"));
        assert!(row.is_some_and(|r| r.ends_with("off")), "{shown}");
        press(&mut app, &[Key::Esc]);
        let shown = screen(&mut app, &mut term);
        assert!(update(&shown).ends_with("Update"), "{shown}");
        for page in Target::Status.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter, Key::Esc]);
        assert_eq!(app.ctx.settings.status, None, "on is the default");
        assert!(update(&screen(&mut app, &mut term)).ends_with("updates"));
    }

    #[test]
    fn a_setting_not_written_says_so_until_a_write_succeeds() {
        use crate::ui::app::tests::{app, open, screen};
        use crate::ui::{Fresh, Msg};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let (tx, rx) = std::sync::mpsc::sync_channel(crate::ui::QUEUE);
        app.ctx.tx = tx;
        open(&mut app, Box::new(SettingsPage::new(None)));
        app.handle(Msg::Fresh(Fresh::Saved(
            Err("Read-only file system".into()),
        )));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("settings.yaml not saved: Read-only file system"),
            "{shown}"
        );
        app.ctx.save_settings();
        let written = rx.recv_timeout(std::time::Duration::from_secs(10));
        app.handle(written.expect("the write reports back"));
        let shown = screen(&mut app, &mut term);
        assert!(!shown.contains("not saved"), "{shown}");
    }
}
