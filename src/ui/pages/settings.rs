//! Settings: what is written to settings.yaml, and what follows from it on the system.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::ask::TextPage;
use super::features::FeaturesPage;
use super::log::HiddenPage;
use super::run::{Job, start};
use crate::data::dotfiles::Repo;
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
            Target::UseDotfiles => {
                let (text, tone) = on_off(ctx.settings.dotfiles());
                (text.into(), tone, false)
            }
            Target::Repository => match &ctx.shared.dotfiles {
                Loaded::Ready(d) => match (d.state.repo, &d.state.remote) {
                    (Repo::Invalid, _) => (t("not usable").into(), Tone::Error, false),
                    (Repo::Ready, Some(url)) => (short(url), Tone::Muted, false),
                    _ => (t("this machine only").into(), Tone::Muted, false),
                },
                Loaded::Loading => ctx.loading(),
                Loaded::Missing(p) => (tf("needs {package}", &[("package", p)]), Tone::Muted, true),
                Loaded::Failed(_) => (t("unknown").into(), Tone::Muted, false),
            },
            // Whether there is a homelab to switch on comes with the dotfiles.
            Target::UseHomelab => match (ctx.settings.homelab(), &ctx.shared.dotfiles) {
                (false, Loaded::Loading) => ctx.loading(),
                (on, _) => {
                    let (text, tone) = on_off(on);
                    (text.into(), tone, false)
                }
            },
            Target::Profile => {
                match (&ctx.settings.dotfiles_profile, ctx.shared.dotfiles.ready()) {
                    (Some(chosen), _) => (chosen.clone(), Tone::Muted, false),
                    (None, Some(d)) => (
                        tf("{profile}, automatic", &[("profile", &d.state.profile)]),
                        Tone::Muted,
                        false,
                    ),
                    (None, None) => ctx.loading(),
                }
            }
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

/// A repository's address as it reads best: host and path, without scheme, user and `.git`.
fn short(url: &str) -> String {
    let rest = match url.split_once("://") {
        Some((_, rest)) => rest.to_string(),
        // scp-like, as `git@github.com:tux/dotfiles.git`.
        None => url.replacen(':', "/", 1),
    };
    // A user stands before the host, which ends at the first slash.
    let host = rest.split('/').next().unwrap_or_default();
    let rest = match host.split_once('@') {
        Some((user, _)) => &rest[user.len() + 1..],
        None => rest.as_str(),
    };
    rest.trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string()
}

/// The address of the repository, and the run that clones it or binds the dotfiles to it.
pub fn repository(ctx: &mut Ctx) -> Box<dyn Page> {
    let state = ctx.shared.dotfiles.ready().map(|d| d.state.clone());
    let current = state
        .as_ref()
        .and_then(|s| s.remote.clone())
        .unwrap_or_default();
    Box::new(TextPage::new(
        t("Repository"),
        t(
            "The address to clone from and to upload to, as GitHub or Gitea show it for cloning. Empty, the dotfiles stay on this machine.",
        ),
        "https://github.com/tux/dotfiles.git",
        current.clone(),
        move |url, ctx| {
            let cloning = state.as_ref().is_none_or(|s| s.repo == Repo::Missing);
            if url == current || (cloning && url.is_empty()) {
                return Flow::Pop;
            }
            let profile = state
                .as_ref()
                .map(|s| s.profile.clone())
                .unwrap_or_default();
            let text = if cloning {
                tf(
                    "Clone {url} into ~/.dotfiles?\nSet up applies the profile {profile} to this machine afterwards.",
                    &[("url", &url), ("profile", &profile)],
                )
            } else if url.is_empty() {
                t("Disconnect the repository?\nThe dotfiles stay on this machine. The repository keeps what it holds.").into()
            } else {
                tf(
                    "Connect ~/.dotfiles to {url}?\nBack up uploads to it from then on.",
                    &[("url", &url)],
                )
            };
            let job = Job::new(op::DOTFILES_CONNECT, vec![url])
                .confirm(text)
                .named(t("Repository"));
            Flow::Push(start(job, ctx))
        },
    ))
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
        // Flatpak's row only once Flatpak is known to be installed, the repository and the profile
        // only with the dotfiles on.
        let shown = |t: &Target| match t {
            Target::Flatpak => ctx.features().is_some_and(|f| f.ok("flatpak")),
            Target::Repository | Target::Profile | Target::UseHomelab => ctx.settings.dotfiles(),
            _ => true,
        };
        let headings = [t("Appearance"), t("System"), t("Dotfiles"), ""];
        let mut rows = Vec::new();
        for (heading, group) in headings.into_iter().zip(SETTINGS) {
            rows.push(Row::heading(heading));
            for &target in group.iter().filter(|t| shown(t)) {
                let (detail, tone, dim) = self.value(target, ctx);
                let row = Row::new(target, target.label())
                    .icon(target.icon(), Tone::Normal)
                    .detail(detail, tone)
                    .dim(dim);
                rows.push(match target {
                    Target::Repository => row.front(),
                    _ => row,
                });
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
            // Off is the default, so only on is stored. The dotfiles are read as it switches on.
            Target::UseDotfiles => {
                let on = !ctx.settings.dotfiles();
                ctx.settings.dotfiles = on.then_some(true);
                ctx.save_settings();
                ctx.reload();
                Flow::Stay
            }
            Target::Repository => match Target::Repository.missing(ctx) {
                Some(package) => Flow::Push(Box::new(FeaturesPage::new(Some(package.into())))),
                None => Flow::Push(repository(ctx)),
            },
            Target::Profile => Flow::Push(choice(Target::Profile, ctx)),
            // On only with its files: where there are none yet, the question that makes them.
            Target::UseHomelab => {
                let there = ctx.shared.dotfiles.ready().is_some_and(|d| d.state.homelab);
                if ctx.settings.homelab() {
                    ctx.settings.homelab = None;
                } else if matches!(ctx.shared.dotfiles, Loaded::Loading) {
                    return Flow::Stay;
                } else if there {
                    ctx.settings.homelab = Some(true);
                    ctx.reload();
                } else {
                    return Flow::Push(super::homelab::setup());
                }
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
        self.applying.is_some()
            || matches!(ctx.shared.features, Loaded::Loading)
            || ctx.settings.dotfiles() && matches!(ctx.shared.dotfiles, Loaded::Loading)
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
        // Automatic is the id no folder can have: empty.
        Target::Profile => {
            let d = ctx.shared.dotfiles.ready();
            let host = d.map(|d| d.state.host.clone()).unwrap_or_default();
            let mut rows = vec![Row::new(String::new(), t("Automatic")).detail(host, Tone::Muted)];
            rows.extend(
                d.map(|d| d.profiles.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| Row::new(p.clone(), p)),
            );
            (
                rows,
                ctx.settings.dotfiles_profile.clone().unwrap_or_default(),
            )
        }
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
            Target::Profile => {
                ctx.settings.dotfiles_profile = (!id.is_empty()).then(|| id.to_string());
                script::set_profile(ctx.settings.dotfiles_profile.as_deref());
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

    /// The dotfiles are off by default: their rows appear with the switch, and so does the row
    /// on the start page. A profile chosen goes to the scripts at once.
    #[test]
    fn the_dotfiles_switch_brings_their_rows_and_a_profile_is_chosen_from_the_repository() {
        use crate::ui::app::tests::{app, example, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        let shown = screen(&mut app, &mut term);
        assert!(!shown.contains("Dotfiles"), "off by default: {shown}");
        for page in Target::UseDotfiles.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        assert!(
            !shown.contains("Repository") && !shown.contains("Profile"),
            "{shown}"
        );
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.ctx.settings.dotfiles, Some(true));
        assert!(app.ctx.busy(), "the dotfiles are read as they switch on");
        let shown = screen(&mut app, &mut term);
        let row = |name: &str| {
            shown
                .lines()
                .find(|l| l.contains(name) && l.trim() != name)
                .unwrap_or_default()
                .to_string()
        };
        assert!(row("Dotfiles").ends_with("on"), "{shown}");
        assert!(
            row("Repository").ends_with("github.com/tux/dotfiles"),
            "{shown}"
        );
        assert!(
            row("Profile").ends_with("tux-notebook, automatic"),
            "{shown}"
        );
        // The profile, chosen among those of the repository, then automatic again.
        press(&mut app, &[Key::Down, Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Automatic") && shown.contains("tux-desktop"),
            "{shown}"
        );
        press(&mut app, &[Key::Down, Key::Enter]);
        assert_eq!(
            app.ctx.settings.dotfiles_profile.as_deref(),
            Some("tux-desktop")
        );
        let shown = screen(&mut app, &mut term);
        assert!(
            shown
                .lines()
                .any(|l| l.contains("Profile") && l.ends_with("tux-desktop")),
            "{shown}"
        );
        press(&mut app, &[Key::Enter, Key::Up, Key::Enter]);
        assert_eq!(
            app.ctx.settings.dotfiles_profile, None,
            "automatic is the default"
        );
        // The start page shows them now, and the switch takes them off again.
        press(&mut app, &[Key::Esc]);
        assert!(screen(&mut app, &mut term).contains("Dotfiles"));
        for page in Target::UseDotfiles.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter, Key::Esc]);
        assert_eq!(app.ctx.settings.dotfiles, None);
        assert!(!screen(&mut app, &mut term).contains("Dotfiles"));
    }

    /// Whether the homelab is there comes from the dotfiles: until they are read, its row spins and
    /// Enter asks nothing.
    #[test]
    fn the_homelab_switch_waits_until_the_dotfiles_are_read() {
        use crate::ui::app::tests::{app, example, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        app.ctx.settings.dotfiles = Some(true);
        app.ctx.shared.dotfiles = Loaded::Loading;
        for page in Target::UseHomelab.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let row = |shown: &str| {
            let line = shown.lines().find(|l| l.contains("Homelab"));
            line.unwrap_or_default().trim_end().to_string()
        };
        let shown = screen(&mut app, &mut term);
        let spinner = app.ctx.style.glyphs.spinner;
        assert!(spinner.iter().any(|f| row(&shown).ends_with(f)), "{shown}");
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(!shown.contains("Make a homelab"), "{shown}");
        assert_eq!(app.ctx.settings.homelab, None);
        let mut dotfiles = crate::data::dotfiles::example();
        dotfiles.state.homelab = true;
        app.ctx.shared.dotfiles = Loaded::Ready(dotfiles);
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.ctx.settings.homelab, Some(true));
        assert!(row(&screen(&mut app, &mut term)).ends_with("on"));
    }

    /// The address is asked with what is there, and the question before the run says what happens.
    #[test]
    fn the_repository_is_asked_as_an_address_and_a_change_asks_before_it_runs() {
        use crate::ui::app::tests::{app, example, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        app.ctx.settings.dotfiles = Some(true);
        for page in Target::Repository.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("git@github.com:tux/dotfiles.git"), "{shown}");
        press(&mut app, &[Key::Enter]);
        assert!(
            screen(&mut app, &mut term).contains("Repository"),
            "unchanged, nothing runs"
        );
        for page in Target::Repository.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Ctrl('u')]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("https://github.com/tux/dotfiles.git"),
            "emptied, the box shows an example: {shown}"
        );
        for c in "https://git.example.org/tux/dotfiles.git".chars() {
            press(&mut app, &[Key::Char(c)]);
        }
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Connect ~/.dotfiles to https://git.example.org/tux/dotfiles.git?"),
            "{shown}"
        );
        let no = shown
            .lines()
            .find(|l| l.trim_end().ends_with("No"))
            .unwrap_or_default();
        assert!(
            no.contains(app.ctx.style.glyphs.cursor),
            "opens on no: {shown}"
        );
    }

    #[test]
    fn an_address_reads_as_host_and_path() {
        for (url, read) in [
            ("git@github.com:tux/dotfiles.git", "github.com/tux/dotfiles"),
            (
                "https://github.com/tux/dotfiles.git",
                "github.com/tux/dotfiles",
            ),
            (
                "ssh://git@git.example.org:2222/tux/dotfiles.git",
                "git.example.org:2222/tux/dotfiles",
            ),
            (
                "https://git.example.org/tux/a@b/",
                "git.example.org/tux/a@b",
            ),
            ("file:///srv/dotfiles.git", "/srv/dotfiles"),
        ] {
            assert_eq!(short(url), read, "{url}");
        }
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
        let dotfiles = heading("Dotfiles");
        assert!(
            at("Optional packages") < dotfiles && dotfiles < at("About"),
            "{shown}"
        );
        assert_eq!(lines[dotfiles - 1], "", "{shown}");
        let about = at("About");
        assert!(
            lines[about - 1].is_empty() && !lines[about - 2].is_empty(),
            "{shown}"
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
