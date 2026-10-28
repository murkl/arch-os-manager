//! The dotfiles as chezmoi keeps them: this machine set up from them and backed up into them, and
//! what of the home folder they keep.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::ask::{BrowsePage, SelectPage, home};
use super::run::{Job, start};
use crate::data::dotfiles::{Chezmoi, Dotfiles, Kept, Repo, State, parse_kept};
use crate::i18n::{t, tf, tn};
use crate::script::{Failure, op, query};
use crate::ui::detail::loaded;
use crate::ui::draw::{put, wrap};
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Page, Tone};

/// What a setup would apply first, as a row says it.
pub fn due(s: &State) -> Option<(String, Tone)> {
    if s.behind > 0 {
        return Some((
            tn("{n} new commit", "{n} new commits", s.behind, &[]),
            Tone::Accent,
        ));
    }
    match s.chezmoi {
        Chezmoi::Unconfigured => return Some((t("not set up").into(), Tone::Accent)),
        Chezmoi::Failed => return Some((t("not readable").into(), Tone::Error)),
        Chezmoi::Ready => {}
    }
    (s.due > 0).then(|| {
        (
            tn("{n} change to apply", "{n} changes to apply", s.due, &[]),
            Tone::Accent,
        )
    })
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Setup,
    Backup,
    Files,
}

pub struct DotfilesPage {
    list: List<Item>,
}

impl DotfilesPage {
    pub fn new() -> DotfilesPage {
        DotfilesPage {
            list: List::default(),
        }
    }

    fn rows(d: &Dotfiles) -> Vec<Row<Item>> {
        let s = &d.state;
        let ready = s.repo == Repo::Ready;
        let (setup, setup_tone) = due(s).unwrap_or((String::new(), Tone::Muted));
        let (backup, backup_tone) = if !ready {
            (String::new(), Tone::Muted)
        } else if s.changed > 0 || s.changes > 0 {
            (t("not backed up").into(), Tone::Warn)
        } else if s.ahead > 0 {
            (t("not uploaded").into(), Tone::Warn)
        } else {
            (t("backed up").into(), Tone::Ok)
        };
        vec![
            Row::new(Item::Setup, t("Set up"))
                .icon(icon::SETUP, Tone::Normal)
                .detail(setup, setup_tone)
                .dim(!ready),
            Row::new(Item::Backup, t("Back up"))
                .icon(icon::BACKUP, Tone::Normal)
                .detail(backup, backup_tone),
            Row::heading(""),
            Row::new(Item::Files, t("Files"))
                .icon(icon::FOLDER, Tone::Normal)
                .detail((s.files + s.secrets).to_string(), Tone::Muted),
        ]
    }

    /// The sentences under the list: what keeps the dotfiles from working.
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
        if s.chezmoi == Chezmoi::Failed {
            notes.push((
                t("A template of the dotfiles does not render. chezmoi's words are in the log.")
                    .into(),
                Tone::Error,
            ));
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
        notes
    }
}

/// The question before a setup: exactly what happens.
fn setup_job() -> Job {
    Job::new(op::DOTFILES_SETUP, Vec::new())
        .confirm(t("Set up this machine from the dotfiles?\nFetches the latest state and applies it with chezmoi. It asks in the terminal for passphrases and values of its configuration, and before it replaces a file changed here or found here before.").into())
        .named(t("Set up"))
}

fn backup_job(s: &State) -> Job {
    let text = match &s.remote {
        Some(remote) => tf(
            "Back up this machine into the dotfiles?\nTakes in what changed here, commits every change and uploads it to {remote}.",
            &[("remote", remote)],
        ),
        None => t("Back up this machine into the dotfiles?\nTakes in what changed here and commits every change. Without a repository they stay on this machine.").into(),
    };
    Job::new(op::DOTFILES_BACKUP, Vec::new())
        .confirm(text)
        .named(t("Back up"))
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
        vec![("↑↓", Does::Choose), ("⏎", Does::Open), ("esc", Does::Back)]
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
            Item::Setup => Flow::Push(start(setup_job(), ctx)),
            Item::Backup => Flow::Push(start(backup_job(s), ctx)),
            Item::Files => Flow::Push(Box::new(FilesPage::new())),
        }
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
}

/// What of the home folder the dotfiles keep: files, and secrets chezmoi encrypts.
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
        let dir = tilde(&s.dir.display().to_string());
        match act {
            Act::AddFile => Flow::Push(Box::new(BrowsePage::new(
                t("Add a file"),
                move |path, ctx| {
                    let job = Job::new(op::DOTFILES_ADD, vec![path.display().to_string()]).confirm(
                        tf(
                            "Add {path} to the dotfiles?\nchezmoi copies it into {dir}. The file stays where it is.",
                            &[("path", &tilde(&path.display().to_string())), ("dir", &dir)],
                        ),
                    );
                    Flow::Push(start(job, ctx))
                },
            ))),
            Act::RemoveFile if s.files == 0 => Flow::Stay,
            Act::RemoveFile => Flow::Push(Box::new(SelectPage::reading(
                t("Remove a file"),
                t("The dotfiles keep no file."),
                || kept(false),
                |path, ctx| {
                    let job = Job::new(op::DOTFILES_REMOVE, vec![path.clone()]).confirm(tf(
                        "Take {path} out of the dotfiles?\nThe file stays where it is, and the dotfiles no longer keep it.",
                        &[("path", &tilde(&path))],
                    ));
                    Flow::Push(start(job, ctx))
                },
            ))),
            Act::AddSecret => Flow::Push(Box::new(BrowsePage::new(
                t("Add a secret"),
                move |path, ctx| {
                    let job = Job::new(
                        op::DOTFILES_SECRET_ADD,
                        vec![path.display().to_string()],
                    )
                    .confirm(tf(
                        "Add {path} as a secret?\nchezmoi encrypts it into the dotfiles as its configuration says. The file stays where it is.",
                        &[("path", &tilde(&path.display().to_string()))],
                    ));
                    Flow::Push(start(job, ctx))
                },
            ))),
            Act::RemoveSecret if s.secrets == 0 => Flow::Stay,
            Act::RemoveSecret => Flow::Push(Box::new(SelectPage::reading(
                t("Remove a secret"),
                t("The dotfiles keep no secret."),
                || kept(true),
                |path, ctx| {
                    let job = Job::new(op::DOTFILES_SECRET_REMOVE, vec![path.clone()]).confirm(tf(
                        "Take the secret {path} out of the dotfiles?\nIts file stays where it is.",
                        &[("path", &tilde(&path))],
                    ));
                    Flow::Push(start(job, ctx))
                },
            ))),
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
    fn a_setup_names_what_waits_first() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let mut s = example().state;
        assert_eq!(due(&s), None);
        s.due = 2;
        assert_eq!(due(&s).unwrap().0, "2 changes to apply");
        s.chezmoi = Chezmoi::Unconfigured;
        assert_eq!(due(&s).unwrap().0, "not set up");
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
    fn the_page_asks_before_a_setup_on_no() {
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
        assert!(row("Back up").ends_with("not backed up"), "{shown}");
        assert!(row("Files").ends_with("34"), "{shown}");
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Set up this machine from the dotfiles?") && shown.contains("chezmoi"),
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
        assert!(shown.contains("Secret entfernen"), "{shown}");
        crate::i18n::set(crate::i18n::Lang::En);
    }

    #[test]
    fn the_files_page_offers_only_what_there_is() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut d = example();
        d.state.secrets = 0;
        app.ctx.shared.dotfiles = Loaded::Ready(d);
        open(&mut app, Box::new(FilesPage::new()));
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Add a secret"), "{shown}");
        press(&mut app, &[Key::Down, Key::Down, Key::Down, Key::Enter]);
        assert!(
            screen(&mut app, &mut term).contains("Remove a secret"),
            "nothing to remove, nothing opens"
        );
    }
}
