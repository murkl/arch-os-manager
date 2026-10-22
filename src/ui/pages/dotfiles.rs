//! The dotfiles: this machine set up from its profile and backed up into it, the groups of the
//! profile, and what of the home folder they keep.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::ask::{BrowsePage, SelectPage, home};
use super::run::{Job, start};
use crate::data::ago;
use crate::data::dotfiles::{Dotfiles, Kept, Ran, Repo, State, parse_kept};
use crate::i18n::{t, tf, tn};
use crate::script::{Failure, op, query};
use crate::ui::detail::loaded;
use crate::ui::draw::{put, wrap};
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Page, Tone};

/// What a setup would apply first, as a row says it.
pub fn due(s: &State) -> Option<(String, Tone)> {
    let changed = |ran: &Ran| *ran == Ran::Changed;
    if s.behind > 0 {
        return Some((
            tn("{n} new commit", "{n} new commits", s.behind, &[]),
            Tone::Accent,
        ));
    }
    if s.new {
        return Some((t("new profile").into(), Tone::Accent));
    }
    if s.init.as_ref().is_some_and(|(ran, _)| changed(ran)) {
        return Some((t("init.sh changed").into(), Tone::Accent));
    }
    if s.links > 0 {
        return Some((
            tn("{n} link missing", "{n} links missing", s.links, &[]),
            Tone::Warn,
        ));
    }
    if s.due_secrets > 0 {
        return Some((
            tn("{n} new secret", "{n} new secrets", s.due_secrets, &[]),
            Tone::Accent,
        ));
    }
    if s.groups.iter().any(|(_, ran)| changed(ran)) {
        return Some((t("group changed").into(), Tone::Accent));
    }
    s.due_gnome
        .then(|| (t("GNOME changed").into(), Tone::Accent))
}

/// What the start page says of the dotfiles: what waits, else whether they are backed up.
pub fn summary(d: &Dotfiles, ctx: &Ctx) -> (String, Tone) {
    let s = &d.state;
    match s.repo {
        Repo::Missing => (t("empty").into(), Tone::Muted),
        Repo::Invalid => (t("not usable").into(), Tone::Error),
        Repo::Ready => due(s)
            .or_else(|| s.unsaved().then(|| (t("not backed up").into(), Tone::Warn)))
            // Up to date only where the repository said so.
            .unwrap_or_else(|| match ctx.shared.unreached {
                Some(_) => (t("offline").into(), Tone::Muted),
                None => (t("up to date").into(), Tone::Ok),
            }),
    }
}

/// A path of the home folder as people write it, from `~`.
fn tilde(path: &str) -> String {
    let home = home();
    match std::path::Path::new(path).strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_string(),
    }
}

/// Where the profile lies, for the questions before a run.
fn place(s: &State) -> String {
    tilde(&s.dir.join(&s.profile).display().to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Setup,
    Backup,
    /// A group of dotfiles.yaml, by its script.
    Group(String),
    Files,
}

pub struct DotfilesPage {
    list: List<Item>,
}

impl DotfilesPage {
    pub fn new() -> DotfilesPage {
        DotfilesPage {
            list: List::filterable(),
        }
    }

    fn rows(d: &Dotfiles) -> Vec<Row<Item>> {
        let s = &d.state;
        let ready = s.repo == Repo::Ready;
        let (setup, setup_tone) = match due(s) {
            Some(due) => due,
            None => match s.setup {
                Some(at) => (ago(at), Tone::Muted),
                None => (String::new(), Tone::Muted),
            },
        };
        let (backup, backup_tone) = if !ready {
            (String::new(), Tone::Muted)
        } else if s.new || s.changes > 0 || !s.unsaved.is_empty() {
            (t("not backed up").into(), Tone::Warn)
        } else if s.ahead > 0 {
            (t("not uploaded").into(), Tone::Warn)
        } else {
            (t("backed up").into(), Tone::Ok)
        };
        let mut rows = vec![
            Row::new(Item::Setup, t("Set up"))
                .icon(icon::SETUP, Tone::Normal)
                .detail(setup, setup_tone)
                .dim(!ready),
            Row::new(Item::Backup, t("Back up"))
                .icon(icon::BACKUP, Tone::Normal)
                .detail(backup, backup_tone),
        ];
        if let Ok(groups) = &d.groups
            && !groups.is_empty()
        {
            rows.push(Row::heading(""));
            for g in groups {
                let (on, tone) = match s.on(&g.script) {
                    true => (t("on"), Tone::Ok),
                    false => (t("off"), Tone::Muted),
                };
                rows.push(
                    Row::new(Item::Group(g.script.clone()), g.name.clone())
                        .icon(icon::PACKAGE, Tone::Normal)
                        .detail(on, tone)
                        .note(g.description.clone().unwrap_or_default()),
                );
            }
        }
        rows.push(Row::heading(""));
        rows.push(
            Row::new(Item::Files, t("Files"))
                .icon(icon::FOLDER, Tone::Normal)
                .detail((s.files + s.secrets).to_string(), Tone::Muted),
        );
        rows
    }

    /// The sentences under the list: what keeps the dotfiles from working, and what stood in the
    /// way of a link.
    fn notes(d: &Dotfiles, ctx: &Ctx) -> Vec<(String, Tone)> {
        let s = &d.state;
        let mut notes = Vec::new();
        match s.repo {
            Repo::Missing => notes.push((
                t("No dotfiles yet. Back up makes them, Settings › Repository fetches them from a repository.").into(),
                Tone::Muted,
            )),
            // In the words of the script's own error for it.
            Repo::Invalid => {
                let f = Failure::classify(op::DOTFILES_STATE, Some(10), None, Vec::new());
                notes.push((format!("{}. {}", f.headline(), f.explanation()), Tone::Error));
            }
            Repo::Ready => {}
        }
        if let Err(e) = &d.groups {
            notes.push((e.clone(), Tone::Warn));
        }
        if let Some(f) = &ctx.shared.unreached {
            notes.push((
                tf(
                    "The repository was not reached: {why}",
                    &[("why", &f.headline())],
                ),
                Tone::Muted,
            ));
        }
        if !s.backups.is_empty() {
            notes.push((
                t("What stood in the way of a link lies in ~/.dotfiles-backup.").into(),
                Tone::Muted,
            ));
        }
        notes
    }
}

/// The question before a setup: exactly what happens.
fn setup_job(s: &State) -> Job {
    let text = tf(
        "Set up this machine from {profile}?\nFetches the latest state, links the files, writes the secrets, runs init.sh and the scripts of the groups where they changed, and installs what the package lists name. What stands in the way of a link goes to ~/.dotfiles-backup.",
        &[("profile", &place(s))],
    );
    Job::new(op::DOTFILES_SETUP, Vec::new())
        .confirm(text)
        .named(t("Set up"))
}

fn backup_job(s: &State) -> Job {
    let text = match &s.remote {
        Some(remote) => tf(
            "Back up this machine into {profile}?\nWrites the package lists, the GNOME extensions and the changed secrets into the profile, commits every change of the dotfiles and uploads it to {remote}.",
            &[("profile", &place(s)), ("remote", remote)],
        ),
        None => tf(
            "Back up this machine into {profile}?\nWrites the package lists, the GNOME extensions and the changed secrets into the profile and commits every change of the dotfiles. Without a repository they stay on this machine.",
            &[("profile", &place(s))],
        ),
    };
    Job::new(op::DOTFILES_BACKUP, Vec::new())
        .confirm(text)
        .named(t("Back up"))
}

fn group_job(s: &State, name: &str, script: &str) -> Job {
    let args = vec![script.to_string()];
    let what: &[(&str, &dyn std::fmt::Display)] = &[
        ("name", &name),
        ("script", &script),
        ("profile", &s.profile),
    ];
    if s.on(script) {
        Job::new(op::DOTFILES_GROUP_OFF, args)
            .confirm(tf(
                "Switch {name} off?\nSet up no longer runs {script}. What it installed stays.",
                what,
            ))
            .named(name)
    } else {
        Job::new(op::DOTFILES_GROUP_ON, args)
            .confirm(tf(
                "Set up {name}?\nRuns {script} and switches the group on in the profile {profile}, so set up runs it on its other machines too.",
                what,
            ))
            .named(name)
    }
}

impl Page for DotfilesPage {
    fn title(&self) -> String {
        t("Dotfiles").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(d) = loaded(&ctx.shared.dotfiles, area, buf, ctx) else {
            return;
        };
        let rows = DotfilesPage::rows(d);
        let notes = DotfilesPage::notes(d, ctx);
        let lines: Vec<(String, Tone)> = notes
            .iter()
            .flat_map(|(text, tone)| {
                wrap(text, area.width.saturating_sub(2) as usize)
                    .into_iter()
                    .map(|l| (l, *tone))
                    .collect::<Vec<_>>()
            })
            .collect();
        let below = (lines.len() as u16 + 1).min(area.height / 2);
        self.list.set(rows);
        self.list.draw(
            Rect::new(
                area.x,
                area.y,
                area.width,
                area.height.saturating_sub(below),
            ),
            buf,
            ctx,
        );
        let top = area.bottom().saturating_sub(below) + 1;
        for (i, (line, tone)) in lines.iter().enumerate() {
            let y = top + i as u16;
            if y >= area.bottom() {
                break;
            }
            put(
                buf,
                area.x + 2,
                y,
                line,
                area.width.saturating_sub(2),
                ctx.style.tone(*tone),
            );
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        let item = match self.list.key(key) {
            Picked::Open(item) => item,
            Picked::Used => return Flow::Stay,
            Picked::Ignored => return Flow::Ignored,
        };
        let Some(d) = ctx.shared.dotfiles.ready().cloned() else {
            return Flow::Stay;
        };
        let s = &d.state;
        match item {
            Item::Setup if s.repo != Repo::Ready => Flow::Stay,
            Item::Setup => Flow::Push(start(setup_job(s), ctx)),
            Item::Backup => Flow::Push(start(backup_job(s), ctx)),
            Item::Group(script) => {
                let name = d
                    .groups
                    .iter()
                    .flatten()
                    .find(|g| g.script == script)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| script.clone());
                Flow::Push(start(group_job(s, &name, &script), ctx))
            }
            Item::Files => Flow::Push(Box::new(FilesPage::new())),
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        ctx.busy()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Act {
    AddFile,
    RemoveFile,
    AddSecret,
    RemoveSecret,
    Passphrase,
}

/// What of the home folder the dotfiles keep: files linked from the profile, secrets encrypted in it.
pub struct FilesPage {
    list: List<Act>,
}

impl FilesPage {
    pub fn new() -> FilesPage {
        FilesPage {
            list: List::default(),
        }
    }
}

/// What the dotfiles keep, as rows to choose one from: files and folders, or secrets.
fn kept(secrets: bool) -> Result<Vec<Row<String>>, Failure> {
    let out = query(op::DOTFILES_MANAGED, &[])?;
    Ok(parse_kept(&out)
        .into_iter()
        .filter(|k| matches!(k, Kept::Secret(_)) == secrets)
        .map(|k| {
            let glyph = match k {
                Kept::Folder(_) => icon::FOLDER,
                _ => icon::FILE,
            };
            Row::new(k.path().to_string(), tilde(k.path())).icon(glyph, Tone::Normal)
        })
        .collect())
}

impl Page for FilesPage {
    fn title(&self) -> String {
        t("Files").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(d) = loaded(&ctx.shared.dotfiles, area, buf, ctx) else {
            return;
        };
        let s = &d.state;
        let count = |n: usize| match n {
            0 => String::new(),
            n => n.to_string(),
        };
        let rows = vec![
            Row::new(Act::AddFile, t("Add a file")).icon(icon::FILE_ADD, Tone::Normal),
            Row::new(Act::RemoveFile, t("Remove a file"))
                .icon(icon::FILE_REMOVE, Tone::Normal)
                .detail(count(s.files), Tone::Muted)
                .dim(s.files == 0),
            Row::heading(""),
            Row::new(Act::AddSecret, t("Add a secret")).icon(icon::SECRET_ADD, Tone::Normal),
            Row::new(Act::RemoveSecret, t("Remove a secret"))
                .icon(icon::SECRET_REMOVE, Tone::Normal)
                .detail(count(s.secrets), Tone::Muted)
                .dim(s.secrets == 0),
            Row::new(Act::Passphrase, t("Change the passphrase"))
                .icon(icon::PASSPHRASE, Tone::Normal)
                .dim(s.unlocked.is_none()),
        ];
        self.list.set(rows);
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        vec![("↑↓", Does::Choose), ("⏎", Does::Open), ("esc", Does::Back)]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        let act = match self.list.key(key) {
            Picked::Open(act) => act,
            Picked::Used => return Flow::Stay,
            Picked::Ignored => return Flow::Ignored,
        };
        let Some(s) = ctx.shared.dotfiles.ready().map(|d| d.state.clone()) else {
            return Flow::Stay;
        };
        let profile = place(&s);
        match act {
            Act::AddFile => Flow::Push(Box::new(BrowsePage::new(
                t("Add a file"),
                move |path, ctx| {
                    let shown = tilde(&path.display().to_string());
                    let job = Job::new(op::DOTFILES_ADD, vec![path.display().to_string()]).confirm(tf(
                    "Add {path} to the dotfiles?\nIt moves into {profile} and a link takes its place.",
                    &[("path", &shown), ("profile", &profile)],
                ));
                    Flow::Push(start(job, ctx))
                },
            ))),
            Act::RemoveFile if s.files == 0 => Flow::Stay,
            Act::RemoveFile => Flow::Push(Box::new(SelectPage::reading(
                t("Remove a file"),
                t("The dotfiles keep no file of this machine."),
                || kept(false),
                |path, ctx| {
                    let job = Job::new(op::DOTFILES_REMOVE, vec![path.clone()]).confirm(tf(
                        "Take {path} out of the dotfiles?\nThe file takes the place of its link again, and the profile no longer keeps it.",
                        &[("path", &tilde(&path))],
                    ));
                    Flow::Push(start(job, ctx))
                },
            ))),
            Act::AddSecret => Flow::Push(Box::new(BrowsePage::new(
                t("Add a secret"),
                move |path, ctx| {
                    let shown = tilde(&path.display().to_string());
                    let job = Job::new(op::DOTFILES_SECRET_ADD, vec![path.display().to_string()]).confirm(tf(
                    "Add {path} as a secret?\nIt is encrypted into {profile} and stays where it is. The first secret of a profile asks for a new passphrase.",
                    &[("path", &shown), ("profile", &profile)],
                ));
                    Flow::Push(start(job, ctx))
                },
            ))),
            Act::RemoveSecret if s.secrets == 0 => Flow::Stay,
            Act::RemoveSecret => Flow::Push(Box::new(SelectPage::reading(
                t("Remove a secret"),
                t("The profile holds no secret."),
                || kept(true),
                |path, ctx| {
                    let job = Job::new(op::DOTFILES_SECRET_REMOVE, vec![path.clone()]).confirm(tf(
                        "Take the secret {path} out of the dotfiles?\nIts file stays where it is.",
                        &[("path", &tilde(&path))],
                    ));
                    Flow::Push(start(job, ctx))
                },
            ))),
            Act::Passphrase if s.unlocked.is_none() => Flow::Stay,
            Act::Passphrase => {
                let job = Job::new(op::DOTFILES_PASSPHRASE, Vec::new()).confirm(
                    t("Change the passphrase of the secrets?\nAsks for the new one twice. After the next back up, machines that have not unlocked yet take the new one.").into(),
                );
                Flow::Push(start(job, ctx))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::dotfiles::example;
    use crate::ui::Loaded;
    use crate::ui::app::tests::{app, open, press, screen};

    #[test]
    fn a_setup_names_what_waits_first_and_else_when_it_ran() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let mut s = example().state;
        assert_eq!(due(&s), None);
        s.links = 2;
        assert_eq!(due(&s).unwrap().0, "2 links missing");
        s.init = Some((Ran::Changed, None));
        assert_eq!(due(&s).unwrap().0, "init.sh changed");
        s.behind = 1;
        assert_eq!(due(&s).unwrap().0, "1 new commit");
    }

    #[test]
    fn the_start_page_says_up_to_date_only_where_the_repository_said_so() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, _) = app();
        let mut d = example();
        assert_eq!(summary(&d, &app.ctx).0, "not backed up");
        d.state.changes = 0;
        assert_eq!(summary(&d, &app.ctx), ("up to date".into(), Tone::Ok));
        app.ctx.shared.unreached = Some(Failure::new(op::DOTFILES_FETCH, "x"));
        assert_eq!(summary(&d, &app.ctx).0, "offline");
        d.state.repo = Repo::Missing;
        assert_eq!(summary(&d, &app.ctx).0, "empty");
    }

    #[test]
    fn the_page_shows_the_groups_switched_on_and_asks_before_switching() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        app.ctx.shared.dotfiles = Loaded::Ready(example());
        open(&mut app, Box::new(DotfilesPage::new()));
        let shown = screen(&mut app, &mut term);
        let row = |name: &str| {
            shown
                .lines()
                .find(|l| l.contains(name))
                .unwrap_or_default()
                .to_string()
        };
        assert!(
            row("Development").ends_with("on") && row("Gaming").ends_with("off"),
            "{shown}"
        );
        assert!(shown.contains("Neovim, Rust and the linters"), "{shown}");
        assert!(row("Back up").ends_with("not backed up"), "{shown}");
        assert!(row("Files").ends_with("34"), "{shown}");
        press(&mut app, &[Key::Down, Key::Down, Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Set up Gaming?") && shown.contains("groups/gaming.sh"),
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
    fn a_broken_dotfiles_yaml_is_named_under_the_page() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut d = example();
        d.groups = Err("dotfiles.yaml: groups/x.sh does not exist".into());
        app.ctx.shared.dotfiles = Loaded::Ready(d);
        open(&mut app, Box::new(DotfilesPage::new()));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("groups/x.sh does not exist") && shown.contains("Files"),
            "{shown}"
        );
        assert!(!shown.contains("Gaming"), "{shown}");
    }

    #[test]
    fn both_pages_fit_forty_columns_in_german() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::De);
        let (mut app, _) = app();
        app.ctx.shared.dotfiles = Loaded::Ready(example());
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 16)).unwrap();
        open(&mut app, Box::new(DotfilesPage::new()));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Einrichten") && shown.contains("nicht gesichert"),
            "{shown}"
        );
        open(&mut app, Box::new(FilesPage::new()));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Passphrase ändern") && shown.contains("Secret entfernen"),
            "{shown}"
        );
        crate::i18n::set(crate::i18n::Lang::En);
    }

    #[test]
    fn the_files_page_offers_only_what_there_is() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut d = example();
        d.state.secrets = 0;
        d.state.unlocked = None;
        app.ctx.shared.dotfiles = Loaded::Ready(d);
        open(&mut app, Box::new(FilesPage::new()));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Add a secret") && shown.contains("Change the passphrase"),
            "{shown}"
        );
        press(
            &mut app,
            &[Key::Down, Key::Down, Key::Down, Key::Down, Key::Enter],
        );
        assert!(
            screen(&mut app, &mut term).contains("Change the passphrase"),
            "nothing to change, nothing opens"
        );
        assert!(!screen(&mut app, &mut term).contains("Asks for the new one twice"));
    }
}
