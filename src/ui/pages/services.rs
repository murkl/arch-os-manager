//! Services of the system and of the user: state, log, and the usual switches.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::journal::{self, Entry};
use crate::data::services::Service;
use crate::i18n::{t, tf};
use crate::script::{op, query};
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::fetch::Fetch;
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Page, Tone};

fn id(s: &Service) -> String {
    format!("{}:{}", if s.user { "user" } else { "system" }, s.unit)
}

fn label(s: &Service) -> &str {
    s.unit.trim_end_matches(".service")
}

/// The question before a change that costs the graphical session, then what it costs.
fn warning(verb: &str, unit: &str) -> Option<String> {
    let unit: [(&str, &dyn std::fmt::Display); 1] = [("unit", &unit)];
    let (question, effect) = match verb {
        "restart" => (
            tf("Restart {unit}?", &unit),
            t("Ends the session. Unsaved work is lost."),
        ),
        "stop" => (
            tf("Stop {unit}?", &unit),
            t("Ends the session. Unsaved work is lost."),
        ),
        "disable" => (
            tf("Disable {unit}?", &unit),
            t("The next boot has no graphical session."),
        ),
        _ => return None,
    };
    Some(format!("{question}\n{effect}"))
}

fn state(s: &Service) -> (String, Tone) {
    match s.active.as_str() {
        "failed" => (t("failed").into(), Tone::Error),
        "active" => (t("running").into(), Tone::Ok),
        "activating" | "reloading" => (t("starting").into(), Tone::Accent),
        "" => (t("not loaded").into(), Tone::Muted),
        _ => (t("stopped").into(), Tone::Muted),
    }
}

pub struct ServicesPage {
    list: List<String>,
}

impl ServicesPage {
    pub fn new() -> ServicesPage {
        ServicesPage {
            list: List::filterable(),
        }
    }
}

impl Page for ServicesPage {
    fn title(&self) -> String {
        t("Services").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some(all) = loaded(&ctx.shared.services, area, buf, ctx) else {
            return;
        };
        let scope = |user| if user { t("User") } else { t("System") };
        let mut rows = Vec::new();
        // What failed comes first, of both scopes, and the cursor starts there. Its heading
        // says the state, so the name keeps the room and the note says what it is.
        let failed: Vec<&Service> = all.iter().filter(|s| s.failed()).collect();
        if !failed.is_empty() {
            rows.push(Row::heading(t("Failed")));
            rows.extend(failed.into_iter().map(|s| {
                let note = match s.description.as_str() {
                    "" => scope(s.user).to_string(),
                    about => format!("{} {} {about}", scope(s.user), ctx.style.glyphs.separator),
                };
                Row::new(id(s), label(s)).dot(Tone::Error).note(note)
            }));
        }
        for user in [false, true] {
            rows.push(Row::heading(scope(user)));
            rows.extend(
                all.iter()
                    .filter(|s| s.user == user && !s.failed())
                    .map(|s| {
                        let (text, tone) = state(s);
                        Row::new(id(s), label(s)).detail(text, tone).dot(tone)
                    }),
            );
        }
        self.list.set(rows);
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(id) => {
                let Some(service) = ctx
                    .shared
                    .services
                    .ready()
                    .and_then(|all| all.iter().find(|s| self::id(s) == id))
                    .cloned()
                else {
                    return Flow::Stay;
                };
                Flow::Push(Box::new(ServicePage::new(service)))
            }
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.services, Loaded::Loading)
    }
}

pub struct ServicePage {
    service: Service,
    log: Fetch<Vec<Entry>>,
    detail: Detail,
}

impl ServicePage {
    fn new(service: Service) -> ServicePage {
        let (scope, unit) = (
            if service.user { "user" } else { "system" },
            service.unit.clone(),
        );
        let log = Fetch::new(move || {
            query(op::SERVICES_LOG, &[scope, &unit]).map(|o| journal::parse(&o))
        });
        ServicePage {
            service,
            log,
            detail: Detail::default(),
        }
    }

    fn build(&mut self, ctx: &Ctx) {
        // The state as the last reload saw it, which follows every switch.
        let s = ctx
            .shared
            .services
            .ready()
            .and_then(|all| all.iter().find(|x| id(x) == id(&self.service)))
            .cloned()
            .unwrap_or(self.service.clone());
        let (text, tone) = state(&s);
        let mut body = Vec::new();
        if !s.description.is_empty() {
            body.push(Block::Field(
                t("Description").into(),
                s.description.clone(),
                Tone::Normal,
            ));
        }
        body.push(Block::Field(t("State").into(), text, tone));
        if !s.enabled.is_empty() {
            let enabled = match s.enabled.as_str() {
                "enabled" | "enabled-runtime" => t("enabled"),
                "disabled" => t("starts when asked"),
                "static" | "indirect" => t("started by others"),
                "masked" => t("masked"),
                other => other,
            };
            body.push(Block::Field(
                t("Start").into(),
                enabled.to_string(),
                Tone::Normal,
            ));
        }
        body.push(Block::Blank);
        match &self.log.value {
            Loaded::Ready(entries) if !entries.is_empty() => {
                body.push(Block::Heading(t("Log").into()));
                body.push(Block::Code(
                    entries
                        .iter()
                        .map(|e| (format!("{}  {}", e.clock(), e.message), Tone::Muted))
                        .collect(),
                ));
            }
            Loaded::Ready(_) => body.push(Block::Text(t("No log entries.").into(), Tone::Muted)),
            _ => {}
        }
        let (script, unit) = (
            if s.user {
                op::SERVICES_USER
            } else {
                op::SERVICES_SYSTEM
            },
            s.unit.clone(),
        );
        let mut acts = Vec::new();
        let mut verb = |name: &str, verb: &'static str, glyph: &'static str| {
            let unit = unit.clone();
            let mut job = Job::new(script, vec![verb.into(), unit]);
            if s.session
                && let Some(text) = warning(verb, label(&s))
            {
                job = job.confirm(text);
            }
            acts.push(
                Act::new(name, move |ctx: &mut Ctx| {
                    Flow::Push(start(job.clone(), ctx))
                })
                .icon(glyph),
            );
        };
        if s.running() {
            verb(t("Restart"), "restart", icon::RESTART);
            verb(t("Stop"), "stop", icon::STOP);
        } else {
            verb(t("Start"), "start", icon::START);
        }
        match s.enabled.as_str() {
            "enabled" => verb(t("Disable"), "disable", icon::DISABLE),
            "disabled" => verb(t("Enable"), "enable", icon::ENABLE),
            _ => {}
        }
        self.detail.set(body, acts);
        // The facts stay while the log scrolls.
        self.detail.pin_head();
    }
}

impl Page for ServicePage {
    fn title(&self) -> String {
        label(&self.service).to_string()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.log.ensure(ctx);
        self.build(ctx);
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        self.log.receive();
        Flow::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::tests::{app, open, press, screen};

    #[test]
    fn what_failed_stands_on_top_with_what_it_is_and_the_cursor_on_it() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let service = |user, unit: &str, active: &str, description: &str| Service {
            user,
            unit: unit.into(),
            description: description.into(),
            active: active.into(),
            enabled: "enabled".into(),
            session: false,
        };
        app.ctx.shared.services = Loaded::Ready(vec![
            service(false, "sshd.service", "active", "OpenSSH Daemon"),
            service(true, "nas-sync.service", "failed", "Copies the home folder"),
        ]);
        open(&mut app, Box::new(ServicesPage::new()));
        let shown = screen(&mut app, &mut term);
        let lines: Vec<&str> = shown.lines().collect();
        let at = |text: &str| {
            lines
                .iter()
                .position(|l| l.contains(text))
                .unwrap_or_else(|| panic!("{text}: {shown}"))
        };
        let heading = |text: &str| {
            lines
                .iter()
                .position(|l| l.trim() == text)
                .unwrap_or_else(|| panic!("{text}: {shown}"))
        };
        assert!(heading("Failed") < at("nas-sync") && at("nas-sync") < heading("System"));
        assert!(heading("System") < at("sshd") && at("sshd") < heading("User"));
        assert_eq!(at("User · Copies the home folder"), at("nas-sync") + 1);
        assert_eq!(shown.matches("nas-sync").count(), 1, "listed once");
        assert!(
            !lines[at("nas-sync")].contains("failed"),
            "the heading says it"
        );
        assert!(lines[at("nas-sync")].contains(app.ctx.style.glyphs.cursor));
        press(&mut app, &[Key::Enter]);
        assert!(screen(&mut app, &mut term).contains("Services › nas-sync"));
    }

    /// What the graphical session cannot do without asks first, opening on no.
    #[test]
    fn a_unit_the_session_needs_asks_before_it_stops() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let gdm = Service {
            user: false,
            unit: "gdm.service".into(),
            description: "GNOME Display Manager".into(),
            active: "active".into(),
            enabled: "enabled".into(),
            session: true,
        };
        open(&mut app, Box::new(ServicePage::new(gdm)));
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter]);
        let asked = screen(&mut app, &mut term);
        assert!(asked.contains("Restart gdm?"), "{asked}");
        assert!(asked.contains("Ends the session."), "{asked}");
        let no = asked.lines().find(|l| l.contains("No")).unwrap_or_default();
        assert!(no.contains(app.ctx.style.glyphs.cursor), "{asked}");
        assert_eq!(
            warning("disable", "gdm").as_deref(),
            Some("Disable gdm?\nThe next boot has no graphical session.")
        );
        assert_eq!(warning("start", "gdm"), None);
    }
}
