//! The homelab as Arcane serves it: each server with its stacks, a stack's images, and what the
//! manager does to a stack: sync it, update it, deploy it anew.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::ask::TextPage;
use super::run::{Job, start};
use crate::data::homelab::{self, Homelab, Server, Stack};
use crate::i18n::{t, tf, tn};
use crate::script::op;
use crate::ui::detail::{Block, loaded, show};
use crate::ui::draw::{put, wrap};
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Page, Tone};

/// What the start page says of the homelab: updates waiting, else whether Arcane reached every
/// server.
pub fn summary(lab: &Homelab) -> (String, Tone) {
    if lab.servers.is_empty() {
        return (t("no server").into(), Tone::Muted);
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

/// The address of Arcane, then the run that makes the key and switches the homelab on.
pub fn setup() -> Box<dyn Page> {
    Box::new(TextPage::new(
        t("Homelab"),
        t("The address of Arcane, https only. Empty, the homelab stays off."),
        "https://arcane.fritz.box",
        String::new(),
        |url, ctx| {
            let url = url.trim_end_matches('/').to_string();
            if url.is_empty() {
                return Flow::Pop;
            }
            let job = Job::new(op::HOMELAB_LOGIN, vec![url.clone()])
                .named(t("Homelab"))
                .then(move |ctx| {
                    ctx.settings.homelab = Some(url.clone());
                    ctx.save_settings();
                });
            Flow::Push(start(job, ctx))
        },
    ))
}

/// The run that takes the key out of the keyring and switches the homelab off.
pub fn disconnect(url: &str, ctx: &mut Ctx) -> Box<dyn Page> {
    let job = Job::new(op::HOMELAB_LOGOUT, vec![url.to_string()])
        .confirm(t("Disconnect the homelab?\nThe key leaves the keyring. Arcane keeps it until it is deleted there.").into())
        .named(t("Homelab"))
        .then(|ctx| {
            ctx.settings.homelab = None;
            ctx.save_settings();
        });
    start(job, ctx)
}

/// The server a page shows, as last read.
fn find<'a>(ctx: &'a Ctx, name: &str) -> Option<&'a Server> {
    ctx.shared
        .homelab
        .ready()
        .and_then(|lab| lab.servers.iter().find(|s| s.name == name))
}

/// The address of Arcane and the server's id for a run on it.
fn target(ctx: &Ctx, server: &Server) -> Option<(String, String)> {
    let url = ctx.settings.homelab()?.to_string();
    Some((url, server.id.clone()))
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
            show(
                vec![Block::Text(
                    t("Arcane names no server.").into(),
                    Tone::Muted,
                )],
                area,
                buf,
                ctx,
            );
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
        self.list.set(rows);
        self.list.draw(area, buf, ctx);
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

pub struct ServerPage {
    name: String,
    list: List<String>,
}

impl ServerPage {
    pub fn new(name: String) -> ServerPage {
        ServerPage {
            name,
            list: List::default(),
        }
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
                    t("Arcane names this server no more.").into(),
                    Tone::Muted,
                )],
                area,
                buf,
                ctx,
            );
            return;
        };
        if !server.online {
            show(
                vec![Block::Text(
                    t("Arcane does not reach this server.").into(),
                    Tone::Warn,
                )],
                area,
                buf,
                ctx,
            );
            return;
        }
        let now = jiff::Timestamp::now();
        let rows: Vec<Row<String>> = server
            .stacks
            .iter()
            .map(|stack| {
                let (text, tone) = homelab::status(stack, now, ctx.style.glyphs.separator);
                Row::new(stack.name.clone(), stack.name.clone())
                    .icon(icon::STACK, Tone::Normal)
                    .detail(text, tone)
            })
            .collect();
        self.list.set(rows);
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        vec![("↑↓", Does::Choose), ("⏎", Does::Open), ("esc", Does::Back)]
    }

    fn key(&mut self, key: Key, _: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(stack) => Flow::Push(Box::new(StackPage::new(self.name.clone(), stack))),
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Act {
    Image(String),
    Sync,
    Update,
    Redeploy,
}

/// The images of a stack, each with what Arcane says of it, and what the manager does to it.
pub struct StackPage {
    server: String,
    stack: String,
    list: List<Act>,
}

impl StackPage {
    fn new(server: String, stack: String) -> StackPage {
        StackPage {
            server,
            stack,
            list: List::default(),
        }
    }
}

/// What a stack tells beside its images: when Arcane checked them, and why its sync failed.
fn note(stack: &Stack, now: jiff::Timestamp) -> (String, Tone) {
    match stack.sync.as_ref().and_then(|s| s.failed.as_ref()) {
        Some(why) => (tf("Sync failed: {why}", &[("why", why)]), Tone::Warn),
        None => (homelab::checked(stack, now), Tone::Muted),
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
            show(
                vec![Block::Text(
                    t("Arcane names this stack no more.").into(),
                    Tone::Muted,
                )],
                area,
                buf,
                ctx,
            );
            return;
        };
        let now = jiff::Timestamp::now();
        let mut rows: Vec<Row<Act>> = homelab::images(stack, now)
            .into_iter()
            .map(|(image, state, tone)| {
                Row::new(Act::Image(image.clone()), image)
                    .icon(icon::PACKAGE, Tone::Normal)
                    .detail(state, tone)
            })
            .collect();
        rows.push(Row::heading(""));
        if stack.sync.is_some() {
            rows.push(Row::new(Act::Sync, t("Sync")).icon(icon::SYNC, Tone::Normal));
        }
        rows.extend([
            Row::new(Act::Update, t("Update")).icon(icon::UPDATE, Tone::Normal),
            Row::new(Act::Redeploy, t("Redeploy")).icon(icon::RESTART, Tone::Normal),
        ]);
        let (text, tone) = note(stack, now);
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
        let act = match self.list.key(key) {
            Picked::Open(act) => act,
            Picked::Used => return Flow::Stay,
            Picked::Ignored => return Flow::Ignored,
        };
        let Some(server) = find(ctx, &self.server).cloned() else {
            return Flow::Stay;
        };
        let Some(stack) = server.stacks.iter().find(|s| s.name == self.stack).cloned() else {
            return Flow::Stay;
        };
        let Some((url, id)) = target(ctx, &server) else {
            return Flow::Stay;
        };
        let name = &stack.name;
        let job = match act {
            Act::Image(_) => return Flow::Stay,
            Act::Sync => {
                let Some(sync) = &stack.sync else {
                    return Flow::Stay;
                };
                Job::new(op::HOMELAB_SYNC, vec![url, id, sync.id.clone()])
            }
            Act::Update => Job::new(op::HOMELAB_UPDATE, vec![url, id, stack.id.clone()]).confirm(
                tf(
                    "Update {stack}?\nPulls the newest images of its tags and starts anew each service whose image changed.",
                    &[("stack", name)],
                ),
            ),
            Act::Redeploy => Job::new(op::HOMELAB_REDEPLOY, vec![url, id, stack.id.clone()])
                .confirm(tf(
                    "Redeploy {stack}?\nEvery service of it starts anew.",
                    &[("stack", name)],
                )),
        };
        Flow::Push(start(job, ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::homelab::example;
    use crate::ui::Loaded;
    use crate::ui::app::tests::{app, open, press, screen};

    #[test]
    fn the_start_page_counts_updates_and_says_up_to_date_only_where_arcane_reached_every_server() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let mut lab = example();
        assert_eq!(summary(&lab), ("1 update".into(), Tone::Accent));
        lab.servers[0].online = false;
        assert_eq!(summary(&lab).0, "not reached");
        assert_eq!(summary(&Homelab::default()).0, "no server");
    }

    #[test]
    fn a_server_shows_its_stacks_and_a_stack_what_the_manager_does_to_it() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        app.ctx.settings.homelab = Some("https://arcane.fritz.box".into());
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
        press(&mut app, &[Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("jellyfin/jellyfin:10") && shown.contains("update"),
            "{shown}"
        );
        assert!(shown.contains("Arcane checked"), "{shown}");
        press(&mut app, &[Key::Down, Key::Down, Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Update jellyfin?"), "{shown}");
    }

    #[test]
    fn a_server_arcane_does_not_reach_says_so() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut lab = example();
        lab.servers[0].online = false;
        app.ctx.shared.homelab = Loaded::Ready(lab);
        open(&mut app, Box::new(ServerPage::new("nas.fritz.box".into())));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Arcane does not reach this server."),
            "{shown}"
        );
    }
}
