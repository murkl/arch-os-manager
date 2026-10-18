//! Services of the system and of the user: state, log, and the usual switches.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::journal::{self, Entry};
use crate::data::services::Service;
use crate::i18n::t;
use crate::script::{op, query};
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::fetch::Fetch;
use crate::ui::icon;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

fn id(s: &Service) -> String {
    format!("{}:{}", if s.user { "user" } else { "system" }, s.unit)
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
            list: List::default(),
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
        let mut rows = Vec::new();
        for (user, heading) in [(false, t("System")), (true, t("User"))] {
            rows.push(Row::heading(heading));
            rows.extend(all.iter().filter(|s| s.user == user).map(|s| {
                let (text, tone) = state(s);
                Row::new(id(s), s.unit.trim_end_matches(".service"))
                    .detail(text, tone)
                    .dot(tone)
            }));
        }
        self.list.set(rows);
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
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
                "enabled" | "enabled-runtime" => t("starts by itself"),
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
        let mut verb = |label: &str, verb: &'static str, glyph: &'static str| {
            let unit = unit.clone();
            acts.push(
                Act::new(label, move |ctx: &mut Ctx| {
                    Flow::Push(start(
                        Job::new(script, vec![verb.into(), unit.clone()]),
                        ctx,
                    ))
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
            "enabled" => verb(t("Do not start by itself"), "disable", icon::DISABLE),
            "disabled" => verb(t("Start by itself"), "enable", icon::ENABLE),
            _ => {}
        }
        self.detail.set(body, acts);
        // The facts stay while the log scrolls.
        self.detail.pin_head();
    }
}

impl Page for ServicePage {
    fn title(&self) -> String {
        self.service.unit.trim_end_matches(".service").to_string()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.log.ensure(ctx);
        self.build(ctx);
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
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
