//! The homelab: each server with what its Cup says, a server's stacks, its export and its secrets.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::ask::{BrowsePage, SelectPage, TextPage};
use super::run::{Job, start};
use crate::data::homelab::{self, Homelab, Server};
use crate::i18n::{t, tf, tn};
use crate::script::op;
use crate::ui::detail::{Block, Detail, loaded, show};
use crate::ui::draw::{put, wrap};
use crate::ui::icon;
use crate::ui::list::{Lead, List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Page, Tone};

/// What the start page says of the homelab: updates waiting, else whether every Cup answered.
pub fn summary(lab: &Homelab) -> (String, Tone) {
    if lab.servers.is_empty() {
        return match lab.problem {
            Some(_) => (t("not usable").into(), Tone::Error),
            None => (t("not set up").into(), Tone::Muted),
        };
    }
    let now = jiff::Timestamp::now();
    let waiting: Vec<Option<usize>> = lab
        .servers
        .iter()
        .map(|s| homelab::waiting(s, now))
        .collect();
    match waiting.iter().flatten().sum::<usize>() {
        0 if waiting.contains(&None) => (t("not reached").into(), Tone::Muted),
        0 => (t("up to date").into(), Tone::Ok),
        n => (tn("{n} update", "{n} updates", n, &[]), Tone::Accent),
    }
}

/// The question that makes a homelab, and the run that switches it on once its files are there.
pub fn setup() -> Box<dyn Page> {
    Box::new(TextPage::new(
        t("Homelab"),
        t("Make a homelab? The name of its server in the network. Empty, the homelab stays off."),
        "nas.fritz.box",
        String::new(),
        |server, ctx| {
            if server.is_empty() {
                return Flow::Pop;
            }
            let text = tf(
                "Make the homelab with {server}?\nWrites ~/.dotfiles/homelab/homelab.yaml and the stack cup. Rolled out on the server with docker compose up -d in its folder, cup tells the manager which images have updates.",
                &[("server", &server)],
            );
            let job = Job::new(op::DOTFILES_HOMELAB_INIT, vec![server])
                .confirm(text)
                .named(t("Homelab"))
                .then(|ctx| {
                    ctx.settings.homelab = Some(true);
                    ctx.save_settings();
                });
            Flow::Push(start(job, ctx))
        },
    ))
}

/// The server a page shows, as last read.
fn find<'a>(ctx: &'a Ctx, name: &str) -> Option<&'a Server> {
    ctx.shared
        .homelab
        .ready()
        .and_then(|lab| lab.servers.iter().find(|s| s.name == name))
}

pub struct HomelabPage {
    list: List<String>,
}

impl HomelabPage {
    pub fn new() -> HomelabPage {
        HomelabPage {
            list: List::filterable(),
        }
    }
}

impl Page for HomelabPage {
    fn title(&self) -> String {
        t("Homelab").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(lab) = loaded(&ctx.shared.homelab, area, buf, ctx) else {
            return;
        };
        if lab.servers.is_empty() {
            let text = lab.problem.clone().unwrap_or_else(|| {
                t("No homelab yet. Switching it on under Settings makes one.").into()
            });
            show(vec![Block::Text(text, Tone::Muted)], area, buf, ctx);
            return;
        }
        let now = jiff::Timestamp::now();
        let rows = lab
            .servers
            .iter()
            .map(|s| {
                let (text, tone) = homelab::summary(s, now, ctx.style.glyphs.separator);
                Row::new(s.name.clone(), s.name.clone())
                    .icon(icon::HOMELAB, Tone::Normal)
                    .detail(text, tone)
            })
            .collect();
        let problem = lab.problem.clone();
        self.list.set(rows);
        let below = u16::from(problem.is_some()) * 2;
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
        if let Some(problem) = problem {
            put(
                buf,
                area.x + 2,
                area.bottom().saturating_sub(1),
                &problem,
                area.width.saturating_sub(2),
                ctx.style.tone(Tone::Warn),
            );
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, _: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(name) => Flow::Push(Box::new(ServerPage::new(name))),
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        ctx.busy()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Stack(String),
    Export,
    AddSecret,
    RemoveSecret,
}

pub struct ServerPage {
    name: String,
    list: List<Item>,
}

impl ServerPage {
    pub fn new(name: String) -> ServerPage {
        ServerPage {
            name,
            list: List::default(),
        }
    }
}

/// What a server's Cup tells beside its stacks: when it checked, or how to have it answer.
fn note(server: &Server, now: jiff::Timestamp) -> (String, Tone) {
    match &server.report {
        Ok(report) => (homelab::checked(report, now), Tone::Muted),
        Err(why) => (
            tf(
                "Cup did not answer: {why}. It answers once the stack cup runs on the server: docker compose up -d in homelab/{server}/cup.",
                &[("why", why), ("server", &server.name)],
            ),
            Tone::Warn,
        ),
    }
}

impl Page for ServerPage {
    fn title(&self) -> String {
        self.name.clone()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(server) = find(ctx, &self.name) else {
            show(
                vec![Block::Text(
                    t("Not in homelab.yaml any more.").into(),
                    Tone::Muted,
                )],
                area,
                buf,
                ctx,
            );
            return;
        };
        let now = jiff::Timestamp::now();
        let report = server.report.as_ref().ok();
        let mut rows: Vec<Row<Item>> = server
            .stacks
            .iter()
            .map(|stack| {
                let (text, tone) = homelab::status(stack, report, now, ctx.style.glyphs.separator);
                Row::new(Item::Stack(stack.name.clone()), stack.name.clone())
                    .icon(icon::STACK, Tone::Normal)
                    .detail(text, tone)
            })
            .collect();
        let secrets = server.secrets.len();
        rows.extend([
            Row::heading(""),
            Row::new(Item::Export, t("Export")).icon(icon::EXPORT, Tone::Normal),
            Row::new(Item::AddSecret, t("Add a secret")).icon(icon::SECRET_ADD, Tone::Normal),
            Row::new(Item::RemoveSecret, t("Remove a secret"))
                .icon(icon::SECRET_REMOVE, Tone::Normal)
                .detail(
                    match secrets {
                        0 => String::new(),
                        n => n.to_string(),
                    },
                    Tone::Muted,
                )
                .dim(secrets == 0),
        ]);
        let (text, tone) = note(server, now);
        let lines = wrap(&text, area.width.saturating_sub(2) as usize);
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
        for (i, line) in lines.iter().enumerate() {
            let y = top + i as u16;
            if y < area.bottom() {
                put(
                    buf,
                    area.x + 2,
                    y,
                    line,
                    area.width.saturating_sub(2),
                    ctx.style.tone(tone),
                );
            }
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
        let Some(server) = find(ctx, &self.name).cloned() else {
            return Flow::Stay;
        };
        let name = server.name.clone();
        match item {
            Item::Stack(stack) => Flow::Push(Box::new(StackPage::new(name, stack))),
            Item::Export => {
                let job = Job::new(op::DOTFILES_HOMELAB_EXPORT, vec![name.clone()]).confirm(tf(
                    "Export {server}?\nPacks its folder with every secret decrypted as {server}.tar into the downloads folder, readable by you only.",
                    &[("server", &name)],
                ));
                Flow::Push(start(job, ctx))
            }
            Item::AddSecret => Flow::Push(Box::new(BrowsePage::files(
                t("Add a secret"),
                move |file, _| {
                    let server = name.clone();
                    let source = file.display().to_string();
                    let shown = file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    Flow::Push(Box::new(TextPage::new(
                        t("Add a secret"),
                        t("Its path in the folder of the server."),
                        "traefik/.env",
                        shown,
                        move |path, ctx| {
                            if path.is_empty() {
                                return Flow::Pop;
                            }
                            let job = Job::new(
                                op::DOTFILES_HOMELAB_SECRET_ADD,
                                vec![server.clone(), source.clone(), path.clone()],
                            )
                            .confirm(tf(
                                "Add {path} as a secret of {server}?\nIt is encrypted into homelab/{server}/{path}.age, a secret there before is replaced. The first secret of the homelab asks for a new passphrase.",
                                &[("path", &path), ("server", &server)],
                            ));
                            Flow::Push(start(job, ctx))
                        },
                    )))
                },
            ))),
            Item::RemoveSecret if server.secrets.is_empty() => Flow::Stay,
            Item::RemoveSecret => {
                let rows: Vec<Row<String>> = server
                    .secrets
                    .iter()
                    .map(|s| Row::new(s.clone(), s.clone()).icon(icon::FILE, Tone::Normal))
                    .collect();
                Flow::Push(Box::new(SelectPage::reading(
                    t("Remove a secret"),
                    t("The server holds no secret."),
                    move || Ok(rows),
                    move |path, ctx| {
                        let job = Job::new(
                            op::DOTFILES_HOMELAB_SECRET_REMOVE,
                            vec![name.clone(), path.clone()],
                        )
                        .confirm(tf(
                            "Take the secret {path} out of {server}?\nWhere it was rolled out, it stays.",
                            &[("path", &path), ("server", &name)],
                        ));
                        Flow::Push(start(job, ctx))
                    },
                )))
            }
        }
    }
}

/// The images of a stack, each with what its server's Cup says of it.
pub struct StackPage {
    server: String,
    stack: String,
    detail: Detail,
}

impl StackPage {
    fn new(server: String, stack: String) -> StackPage {
        StackPage {
            server,
            stack,
            detail: Detail::default(),
        }
    }
}

impl Page for StackPage {
    fn title(&self) -> String {
        self.stack.clone()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(server) = find(ctx, &self.server) else {
            return;
        };
        let Some(stack) = server.stacks.iter().find(|s| s.name == self.stack) else {
            return;
        };
        let body = homelab::images(stack, server.report.as_ref().ok(), jiff::Timestamp::now())
            .into_iter()
            .map(|(image, state, tone)| {
                Block::Row(Lead::None, vec![(image, Tone::Normal)], vec![(state, tone)])
            })
            .collect();
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
    use crate::data::homelab::example;
    use crate::ui::Loaded;
    use crate::ui::app::tests::{app, open, press, screen};

    #[test]
    fn the_start_page_counts_updates_and_says_up_to_date_only_where_every_cup_answered() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let mut lab = example();
        assert_eq!(summary(&lab), ("1 update".into(), Tone::Accent));
        lab.servers[0].report = Err("timeout".into());
        assert_eq!(summary(&lab).0, "not reached");
        assert_eq!(summary(&Homelab::default()).0, "not set up");
    }

    #[test]
    fn a_server_shows_its_stacks_and_how_cup_answers() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        app.ctx.shared.homelab = Loaded::Ready(example());
        open(&mut app, Box::new(HomelabPage::new()));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown
                .lines()
                .any(|l| l.contains("nas.fritz.box") && l.contains("3/3")),
            "{shown}"
        );
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        let row = |name: &str| {
            shown
                .lines()
                .find(|l| l.contains(name))
                .unwrap_or_default()
                .to_string()
        };
        assert!(row("jellyfin").ends_with("1/1 · update"), "{shown}");
        assert!(row("cup").ends_with("2/2"), "{shown}");
        assert!(shown.contains("Cup checked"), "{shown}");
        press(&mut app, &[Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("jellyfin/jellyfin:10") && shown.contains("update"),
            "{shown}"
        );
        press(&mut app, &[Key::Esc, Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Export nas.fritz.box?"), "{shown}");
    }

    #[test]
    fn a_cup_that_does_not_answer_says_how_it_will() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut lab = example();
        lab.servers[0].report = Err("The server could not be reached".into());
        app.ctx.shared.homelab = Loaded::Ready(lab);
        open(&mut app, Box::new(ServerPage::new("nas.fritz.box".into())));
        let shown = screen(&mut app, &mut term);
        let said = shown.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            said.contains("docker compose up -d in homelab/nas.fritz.box/cup"),
            "{shown}"
        );
        assert!(
            shown
                .lines()
                .any(|l| l.contains("jellyfin") && l.ends_with("unknown")),
            "{shown}"
        );
    }
}
