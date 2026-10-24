//! Packages: one search over what is installed, the repositories and the AUR.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::run::{Job, start};
use crate::data::packages::{Found, escape_regex, field, parse_info, parse_search};
use crate::data::vercmp::vercmp;
use crate::data::versions::{Version, parse as parse_versions};
use crate::data::{aur, date};
use crate::i18n::{t, tf};
use crate::script::{self, Failure, op, query};
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::fetch::{Fetch, Task};
use crate::ui::icon;
use crate::ui::input::Input;
use crate::ui::list::{Lead, List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Loaded, Nav, Page, Tone};

/// Typing waits this long before it searches, so a word is searched once.
const PAUSE: Duration = Duration::from_millis(300);
const LIMIT: usize = 200;

struct Results {
    query: String,
    repo: Result<Vec<Found>, Failure>,
    aur: Option<Result<Vec<aur::Package>, Failure>>,
}

/// A row of the search: a package installed, found in a repository, or found in the AUR.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Pick {
    Local(String),
    Repo(String, String),
    Aur(String),
}

pub struct PackagesPage {
    input: Input,
    list: List<Pick>,
    /// The search under way, and what the last one found.
    search: Option<Task<Results>>,
    results: Option<Results>,
    due: Option<Instant>,
}

impl PackagesPage {
    pub fn new() -> PackagesPage {
        let mut list = List::default();
        list.markable = true;
        PackagesPage {
            input: Input::new(t("Search packages")),
            list,
            search: None,
            results: None,
            due: None,
        }
    }

    fn rows(&self, ctx: &Ctx) -> Vec<Row<Pick>> {
        let Some(installed) = ctx.shared.installed.ready() else {
            return Vec::new();
        };
        let q = self.input.text.trim().to_lowercase();
        let dropped = ctx.shared.dropped.ready();
        let local = |p: &crate::data::packages::Installed| {
            let (detail, tone) = if dropped.is_some_and(|d| d.contains(&p.name)) {
                (format!("{}  {}", p.version, t("no source")), Tone::Warn)
            } else if p.foreign {
                (format!("{}  AUR", p.version), Tone::Muted)
            } else {
                (p.version.clone(), Tone::Muted)
            };
            let glyph = if p.foreign { icon::ARCH } else { icon::PACKAGE };
            Row::new(Pick::Local(p.name.clone()), p.name.clone())
                .icon(glyph, Tone::Ok)
                .detail(detail, tone)
        };
        if q.is_empty() {
            let mut rows = vec![Row::heading(t("Installed by choice"))];
            rows.extend(installed.iter().filter(|p| p.explicit).map(local));
            return rows;
        }
        let mut rows = vec![Row::heading(t("Installed"))];
        rows.extend(
            installed
                .iter()
                .filter(|p| p.name.to_lowercase().contains(&q))
                .take(LIMIT)
                .map(local),
        );
        let Some(r) = self
            .results
            .as_ref()
            .filter(|r| r.query == self.input.text.trim())
        else {
            rows.push(Row::heading(t("Searching")));
            return rows;
        };
        let is_installed = |n: &str| installed.iter().any(|p| p.name == n);
        rows.push(Row::heading(t("Repositories")));
        match &r.repo {
            Ok(found) => rows.extend(
                found
                    .iter()
                    .filter(|f| !is_installed(&f.name))
                    .take(LIMIT)
                    .map(|f| {
                        Row::new(Pick::Repo(f.repo.clone(), f.name.clone()), f.name.clone())
                            .icon(icon::PACKAGE, Tone::Muted)
                            .detail(format!("{}  {}", f.version, f.repo), Tone::Muted)
                    }),
            ),
            Err(f) => rows.push(Row::heading(f.headline())),
        }
        if let Some(aur) = &r.aur {
            rows.push(Row::heading("AUR"));
            match aur {
                Ok(found) => {
                    let mut found: Vec<&aur::Package> =
                        found.iter().filter(|p| !is_installed(&p.name)).collect();
                    found.sort_by(|a, b| b.popularity.total_cmp(&a.popularity));
                    rows.extend(found.into_iter().take(LIMIT).map(|p| {
                        Row::new(Pick::Aur(p.name.clone()), p.name.clone())
                            .icon(icon::ARCH, Tone::Muted)
                            .detail(p.version.clone(), Tone::Muted)
                    }));
                }
                Err(f) => rows.push(Row::heading(f.headline())),
            }
        }
        rows
    }

    fn search(&mut self, ctx: &Ctx) {
        let q = self.input.text.trim().to_string();
        if q.is_empty() {
            return;
        }
        let with_aur = script::aur().is_some() && q.chars().count() >= 2;
        self.search = Some(Task::start(ctx, move || Results {
            repo: query(op::PACKAGES_SEARCH, &[&escape_regex(&q)]).map(|o| parse_search(&o)),
            aur: with_aur.then(|| {
                query(op::PACKAGES_AUR_SEARCH, &[&q]).and_then(|o| {
                    aur::parse(&o).map_err(|e| Failure::new(op::PACKAGES_AUR_SEARCH, e))
                })
            }),
            query: q,
        }));
    }

    fn open(&self, pick: Pick) -> Flow {
        if !self.list.marked.is_empty() {
            return Flow::Push(Box::new(MarkedPage::new(self.list.marked.clone())));
        }
        let page = match pick {
            Pick::Repo(repo, name) => PackagePage::new(&name, Some(format!("{repo}/{name}"))),
            Pick::Local(name) | Pick::Aur(name) => PackagePage::new(&name, None),
        };
        Flow::Push(Box::new(page))
    }
}

impl Page for PackagesPage {
    fn title(&self) -> String {
        t("Packages").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.input.draw(buf, area.x, area.y, area.width, ctx);
        let rest = Rect::new(
            area.x,
            area.y + 2,
            area.width,
            area.height.saturating_sub(2),
        );
        if loaded(&ctx.shared.installed, rest, buf, ctx).is_none() {
            return;
        }
        let rows = self.rows(ctx);
        self.list.set(rows);
        self.list.draw(rest, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        let open = match self.list.marked.len() {
            0 => Does::Open,
            n => Does::OpenMarked(n),
        };
        let esc = match self.input.text.is_empty() {
            true => Does::Back,
            false => Does::ClearSearch,
        };
        vec![
            ("↑↓", Does::Choose),
            ("⏎", open),
            ("tab", Does::Mark),
            ("esc", esc),
        ]
    }

    fn key(&mut self, key: Key, _: &mut Ctx) -> Flow {
        match key {
            Key::Tab => self.list.toggle_mark(),
            Key::Esc if !self.input.text.is_empty() => {
                self.input.text.clear();
                self.list.marked.clear();
            }
            Key::Esc | Key::Ctrl('c') => return Flow::Ignored,
            Key::Up
            | Key::Down
            | Key::WheelUp
            | Key::WheelDown
            | Key::PageUp
            | Key::PageDown
            | Key::Enter => {
                if let Picked::Open(pick) = self.list.key(key) {
                    return self.open(pick);
                }
            }
            k => {
                if self.input.key(k) {
                    self.due = Some(Instant::now() + PAUSE);
                }
            }
        }
        Flow::Stay
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        if let Some(r) = self.search.as_mut().and_then(Task::take) {
            self.results = Some(r);
        }
        Flow::Stay
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Flow {
        if self.due.is_some_and(|d| Instant::now() >= d) {
            self.due = None;
            self.search(ctx);
        }
        Flow::Stay
    }

    fn typing(&self) -> bool {
        true
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        self.due.is_some() || matches!(ctx.shared.installed, Loaded::Loading)
    }
}

/// What is known about one package: installed, in a repository, in the AUR.
#[derive(Clone, Default)]
struct Info {
    local: Option<Vec<(String, String)>>,
    sync: Option<Vec<(String, String)>>,
    aur: Option<aur::Package>,
}

pub struct PackagePage {
    name: String,
    fetch: Fetch<Info>,
    detail: Detail,
}

impl PackagePage {
    /// `repo` is `repo/name` where the search found it in a repository.
    pub fn new(name: &str, repo: Option<String>) -> PackagePage {
        let n = name.to_string();
        let fetch = Fetch::new(move || {
            let local = query(op::PACKAGES_INFO, &["local", &n])
                .ok()
                .map(|o| parse_info(&o));
            let sync = query(op::PACKAGES_INFO, &["sync", repo.as_deref().unwrap_or(&n)])
                .ok()
                .map(|o| parse_info(&o));
            let aur = if sync.is_none() {
                query(op::PACKAGES_AUR_INFO, &[&n])
                    .ok()
                    .and_then(|o| aur::parse(&o).ok())
                    .and_then(|mut v| v.pop())
            } else {
                None
            };
            if local.is_none() && sync.is_none() && aur.is_none() {
                return Err(Failure::classify(
                    op::PACKAGES_INFO,
                    Some(11),
                    None,
                    Vec::new(),
                ));
            }
            Ok(Info { local, sync, aur })
        });
        PackagePage {
            name: name.to_string(),
            fetch,
            detail: Detail::default(),
        }
    }

    fn build(&mut self, info: &Info) {
        let mut body = Vec::new();
        let main = info.sync.as_ref().or(info.local.as_ref());
        let get = |key: &str| main.and_then(|f| field(f, key)).map(str::to_string);
        let repo = get("Repository").unwrap_or_else(|| {
            if info.aur.is_some() {
                "aur".into()
            } else {
                String::new()
            }
        });
        let aur = info.aur.as_ref();
        fact(
            &mut body,
            t("Description"),
            get("Description").or_else(|| aur.and_then(|a| a.description.clone())),
            Tone::Normal,
        );
        fact(&mut body, t("Source"), Some(repo.clone()), Tone::Normal);
        fact(
            &mut body,
            t("Version"),
            get("Version").or_else(|| aur.map(|a| a.version.clone())),
            Tone::Normal,
        );
        fact(&mut body, t("License"), get("Licenses"), Tone::Normal);
        // pacman's own figure, in the units of the rest of the interface.
        let size =
            get("Installed Size").map(|s| crate::data::format_size(crate::data::parse_size(&s)));
        fact(&mut body, t("Size"), size, Tone::Normal);
        fact(
            &mut body,
            t("Website"),
            get("URL").or_else(|| aur.and_then(|a| a.url.clone())),
            Tone::Normal,
        );
        let depends = get("Depends On")
            .map(|d| d.split_whitespace().count())
            .or_else(|| aur.map(|a| a.depends.len()));
        fact(
            &mut body,
            t("Dependencies"),
            depends.map(|n| n.to_string()),
            Tone::Normal,
        );
        if let Some(a) = aur {
            fact(
                &mut body,
                t("Maintainer"),
                Some(
                    a.maintainer
                        .clone()
                        .unwrap_or_else(|| t("none, orphaned").into()),
                ),
                if a.maintainer.is_none() {
                    Tone::Warn
                } else {
                    Tone::Normal
                },
            );
            fact(
                &mut body,
                t("Votes"),
                Some(format!("{}  ({:.2})", a.num_votes, a.popularity)),
                Tone::Normal,
            );
            fact(
                &mut body,
                t("Changed"),
                jiff::Timestamp::from_second(a.last_modified).ok().map(date),
                Tone::Normal,
            );
            if let Some(flagged) = a
                .out_of_date
                .and_then(|s| jiff::Timestamp::from_second(s).ok())
            {
                fact(
                    &mut body,
                    t("Out of date since"),
                    Some(date(flagged)),
                    Tone::Error,
                );
            }
        } else {
            fact(&mut body, t("Built"), get("Build Date"), Tone::Normal);
        }
        body.push(Block::Blank);
        match &info.local {
            Some(local) => {
                let installed = |k: &str| field(local, k).map(str::to_string);
                fact(&mut body, t("Installed"), installed("Version"), Tone::Ok);
                fact(
                    &mut body,
                    t("Since"),
                    installed("Install Date"),
                    Tone::Normal,
                );
                let reason = installed("Install Reason").map(|r| {
                    if r.starts_with("Explicitly") {
                        t("by choice").to_string()
                    } else {
                        t("as a dependency").to_string()
                    }
                });
                fact(&mut body, t("Reason"), reason, Tone::Normal);
                let required =
                    installed("Required By").map(|r| r.split_whitespace().count().to_string());
                fact(&mut body, t("Needed by"), required, Tone::Normal);
            }
            None => fact(&mut body, t("Installed"), Some(t("no").into()), Tone::Muted),
        }

        let n = self.name.clone();
        let mut acts = Vec::new();
        if info.local.is_some() {
            let name = n.clone();
            acts.push(
                Act::new(t("Remove"), move |ctx: &mut Ctx| {
                    let text = tf(
                        "Remove {name}?\nRemoves with pacman -Rns.",
                        &[("name", &name)],
                    );
                    Flow::Push(start(
                        Job::new(op::PACKAGES_REMOVE, vec![name.clone()]).confirm(text),
                        ctx,
                    ))
                })
                .icon(icon::REMOVE),
            );
            if info.sync.is_some() {
                let name = n.clone();
                acts.push(
                    Act::new(t("Install an older version"), move |_| {
                        Flow::Push(Box::new(VersionsPage::new(&name)))
                    })
                    .icon(icon::OLDER),
                );
            }
        } else if info.sync.is_some() {
            let id = format!("{repo}/{n}");
            acts.push(
                Act::new(t("Install"), move |ctx: &mut Ctx| {
                    Flow::Push(start(
                        Job::new(op::PACKAGES_INSTALL, vec!["repo".into(), id.clone()]),
                        ctx,
                    ))
                })
                .icon(icon::INSTALL),
            );
        } else if info.aur.is_some() {
            let name = n.clone();
            let helper = script::aur()
                .is_none()
                .then(|| t("needs paru or yay").to_string());
            acts.push(
                Act::new(t("Install from the AUR"), move |ctx: &mut Ctx| {
                    Flow::Push(start(
                        Job::new(op::PACKAGES_INSTALL, vec!["aur".into(), name.clone()]),
                        ctx,
                    ))
                })
                .icon(icon::INSTALL)
                .unless(helper),
            );
        }
        if let Some(a) = aur {
            let base = a.package_base.clone();
            acts.push(
                Act::new(t("Show the PKGBUILD"), move |_| {
                    Flow::Push(Box::new(PkgbuildPage::new(&base)))
                })
                .icon(icon::CODE),
            );
        }
        self.detail.set(body, acts);
        // What the repository says stays while the rest scrolls.
        self.detail.pin_head();
    }
}

impl Page for PackagePage {
    fn title(&self) -> String {
        self.name.clone()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.fetch.ensure(ctx);
        let Some(info) = loaded(&self.fetch.value, area, buf, ctx).cloned() else {
            return;
        };
        self.build(&info);
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        self.fetch.receive();
        Flow::Stay
    }

    fn animating(&self, _: &Ctx) -> bool {
        matches!(self.fetch.value, Loaded::Loading)
    }
}

fn fact(body: &mut Vec<Block>, label: &str, value: Option<String>, tone: Tone) {
    if let Some(v) = value.filter(|v| !v.is_empty()) {
        body.push(Block::Field(label.to_string(), v, tone));
    }
}

/// The marked search results: install what is not there, remove what is.
struct MarkedPage {
    detail: Detail,
}

impl MarkedPage {
    fn new(marked: BTreeSet<Pick>) -> MarkedPage {
        let mut repo = Vec::new();
        let mut from_aur = Vec::new();
        let mut local = Vec::new();
        for pick in marked {
            match pick {
                Pick::Repo(r, name) => repo.push(format!("{r}/{name}")),
                Pick::Aur(name) => from_aur.push(name),
                Pick::Local(name) => local.push(name),
            }
        }
        let mut body = Vec::new();
        let mut acts = Vec::new();
        for (heading, list) in [
            (t("Install"), [repo.clone(), from_aur.clone()].concat()),
            (t("Remove"), local.clone()),
        ] {
            if !list.is_empty() {
                body.push(Block::Heading(heading.to_string()));
                for p in list {
                    let name = p.rsplit('/').next().unwrap_or(&p).to_string();
                    body.push(Block::Row(
                        Lead::Icon(icon::PACKAGE, Tone::Muted),
                        vec![(name, Tone::Normal)],
                        Vec::new(),
                    ));
                }
                body.push(Block::Blank);
            }
        }
        if !local.is_empty() && (!repo.is_empty() || !from_aur.is_empty()) {
            let installs: Vec<&str> = repo
                .iter()
                .map(|p| p.rsplit('/').next().unwrap_or(p))
                .chain(from_aur.iter().map(String::as_str))
                .collect();
            let label = tf(
                "Install {n} and remove {m}",
                &[("n", &installs.len()), ("m", &local.len())],
            );
            let text = tf(
                "Install {install} and remove {remove}?\nRemoves with pacman -Rns.",
                &[
                    ("install", &installs.join(", ")),
                    ("remove", &local.join(", ")),
                ],
            );
            let args: Vec<String> = repo
                .iter()
                .map(|p| format!("repo:{p}"))
                .chain(from_aur.iter().map(|p| format!("aur:{p}")))
                .chain(local.iter().map(|p| format!("remove:{p}")))
                .collect();
            let helper = (!from_aur.is_empty() && script::aur().is_none())
                .then(|| t("needs paru or yay").to_string());
            acts.push(
                Act::new(label, move |ctx: &mut Ctx| {
                    Flow::Push(start(
                        Job::new(op::PACKAGES_CHANGE, args.clone()).confirm(text.clone()),
                        ctx,
                    ))
                })
                .icon(icon::INSTALL)
                .unless(helper),
            );
        }
        if !repo.is_empty() {
            acts.push(
                Act::new(
                    tf("Install {n} from the repositories", &[("n", &repo.len())]),
                    move |ctx: &mut Ctx| {
                        Flow::Push(start(
                            Job::new(
                                op::PACKAGES_INSTALL,
                                [vec!["repo".to_string()], repo.clone()].concat(),
                            ),
                            ctx,
                        ))
                    },
                )
                .icon(icon::INSTALL),
            );
        }
        if !from_aur.is_empty() {
            let helper = script::aur()
                .is_none()
                .then(|| t("needs paru or yay").to_string());
            acts.push(
                Act::new(
                    tf("Install {n} from the AUR", &[("n", &from_aur.len())]),
                    move |ctx: &mut Ctx| {
                        Flow::Push(start(
                            Job::new(
                                op::PACKAGES_INSTALL,
                                [vec!["aur".to_string()], from_aur.clone()].concat(),
                            ),
                            ctx,
                        ))
                    },
                )
                .icon(icon::INSTALL)
                .unless(helper),
            );
        }
        if !local.is_empty() {
            acts.push(
                Act::new(
                    tf("Remove {n}", &[("n", &local.len())]),
                    move |ctx: &mut Ctx| {
                        let text = tf(
                            "Remove {names}?\nRemoves with pacman -Rns.",
                            &[("names", &local.join(", "))],
                        );
                        Flow::Push(start(
                            Job::new(op::PACKAGES_REMOVE, local.clone()).confirm(text),
                            ctx,
                        ))
                    },
                )
                .icon(icon::REMOVE),
            );
        }
        let mut detail = Detail::default();
        detail.set(body, acts);
        MarkedPage { detail }
    }
}

impl Page for MarkedPage {
    fn title(&self) -> String {
        t("Marked").into()
    }
    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.detail.draw(area, buf, ctx);
    }
    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        self.detail.hints()
    }
    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }
}

/// The versions a package can go back to, newest first.
pub struct VersionsPage {
    name: String,
    fetch: Fetch<Vec<Version>>,
    list: List<String>,
    focused: bool,
}

impl VersionsPage {
    pub fn new(name: &str) -> VersionsPage {
        let n = name.to_string();
        let fetch =
            Fetch::new(move || query(op::PACKAGES_VERSIONS, &[&n]).map(|o| parse_versions(&o, &n)));
        VersionsPage {
            name: name.to_string(),
            fetch,
            list: List::filterable(),
            focused: false,
        }
    }
}

impl Page for VersionsPage {
    fn title(&self) -> String {
        t("Older versions").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.fetch.ensure(ctx);
        let Some(versions) = loaded(&self.fetch.value, area, buf, ctx) else {
            return;
        };
        let current = ctx
            .shared
            .installed
            .ready()
            .and_then(|all| all.iter().find(|p| p.name == self.name))
            .map(|p| p.version.clone());
        if versions.is_empty() {
            crate::ui::detail::show(
                vec![Block::Text(
                    t("No other version available.").into(),
                    Tone::Muted,
                )],
                area,
                buf,
                ctx,
            );
            return;
        }
        let rows = versions
            .iter()
            .map(|v| {
                let place = if v.cached {
                    t("in the cache")
                } else {
                    t("in the archive")
                };
                let mut row =
                    Row::new(v.location.clone(), v.version.clone()).detail(place, Tone::Muted);
                if current.as_deref() == Some(v.version.as_str()) {
                    row = row.dot(Tone::Ok).detail(t("installed"), Tone::Ok);
                } else {
                    row = row.dot(Tone::Muted);
                }
                row
            })
            .collect();
        self.list.set(rows);
        // The one meant is the newest older than the installed one.
        if !self.focused {
            self.focused = true;
            if let Some(older) = versions.iter().find(|v| {
                current
                    .as_deref()
                    .is_some_and(|c| vercmp(&v.version, c).is_lt())
            }) {
                self.list.focus(&older.location);
            }
        }
        self.list.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(location) => {
                let Some(version) = self.list.selected().map(|r| r.label.clone()) else {
                    return Flow::Stay;
                };
                let text = tf(
                    "Install {name} {version}?",
                    &[("name", &self.name), ("version", &version)],
                );
                Flow::Push(start(
                    Job::new(op::PACKAGES_DOWNGRADE, vec![location]).confirm(text),
                    ctx,
                ))
            }
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        self.fetch.receive();
        Flow::Stay
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, _: &Ctx) -> bool {
        matches!(self.fetch.value, Loaded::Loading)
    }
}

pub struct PkgbuildPage {
    fetch: Fetch<String>,
    detail: Detail,
    shown: bool,
}

impl PkgbuildPage {
    pub fn new(base: &str) -> PkgbuildPage {
        let b = base.to_string();
        PkgbuildPage {
            fetch: Fetch::new(move || query(op::PACKAGES_PKGBUILD, &[&b])),
            detail: Detail::default(),
            shown: false,
        }
    }
}

impl Page for PkgbuildPage {
    fn title(&self) -> String {
        "PKGBUILD".into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.fetch.ensure(ctx);
        let Some(text) = loaded(&self.fetch.value, area, buf, ctx) else {
            return;
        };
        if !self.shown {
            self.shown = true;
            self.detail.set(
                vec![Block::Styled(crate::ui::highlight::bash(text))],
                Vec::new(),
            );
        }
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if key.nav(false) == Some(Nav::Open) {
            return Flow::Stay;
        }
        self.detail.key(key, ctx)
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        self.fetch.receive();
        Flow::Stay
    }

    fn animating(&self, _: &Ctx) -> bool {
        matches!(self.fetch.value, Loaded::Loading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::packages::parse_installed;

    #[test]
    fn a_foreign_package_without_source_says_so() {
        use crate::ui::app::tests::{app, open, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        app.ctx.shared.installed = Loaded::Ready(parse_installed(
            "pkg	paru	2-1
pkg	gone	1-1
explicit	paru
explicit	gone
foreign	paru
foreign	gone
",
        ));
        app.ctx.shared.dropped = Loaded::Ready(vec!["gone".into()]);
        open(&mut app, Box::new(PackagesPage::new()));
        let shown = screen(&mut app, &mut term);
        let row = |name: &str| shown.lines().find(|l| l.contains(name)).unwrap_or_default();
        assert!(row("gone").ends_with("1-1  no source"), "{shown}");
        assert!(row("paru").ends_with("2-1  AUR"), "{shown}");
    }

    #[test]
    fn the_search_takes_letters_where_the_cursor_stands() {
        use crate::ui::app::tests::{app, open, press, screen};
        let (mut app, mut term) = app();
        app.ctx.shared.installed = Loaded::Ready(Vec::new());
        open(&mut app, Box::new(PackagesPage::new()));
        let keys: Vec<Key> = "fire".chars().map(Key::Char).collect();
        press(&mut app, &keys);
        press(
            &mut app,
            &[Key::Left, Key::Left, Key::Char('x'), Key::CtrlBackspace],
        );
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("› re"), "{shown}");
    }

    #[test]
    fn a_search_lists_the_installed_then_the_repositories_then_the_aur() {
        use crate::ui::app::tests::{app, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        app.ctx.shared.installed = Loaded::Ready(parse_installed(
            "pkg\tfirefox\t1-1\npkg\tbash\t5-1\nexplicit\tfirefox\n",
        ));
        let mut page = PackagesPage::new();
        for c in "fire".chars() {
            page.key(Key::Char(c), &mut app.ctx);
        }
        let found = |name: &str, installed| Found {
            repo: "extra".into(),
            name: name.into(),
            version: "1-1".into(),
            description: String::new(),
            installed,
        };
        page.results = Some(Results {
            query: "fire".into(),
            repo: Ok(vec![found("firefox", true), found("firejail", false)]),
            aur: Some(
                aur::parse(r#"{"resultcount":1,"results":[{"Name":"fire-bin","PackageBase":"fire-bin","Version":"3-1","Popularity":0.5}],"type":"search","version":5}"#)
                    .map_err(|e| Failure::new(op::PACKAGES_AUR_SEARCH, e)),
            ),
        });
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        let at = |text: &str| {
            shown
                .find(text)
                .unwrap_or_else(|| panic!("{text}: {shown}"))
        };
        assert!(at("Installed") < at("firefox") && at("firefox") < at("Repositories"));
        assert!(at("Repositories") < at("firejail") && at("firejail") < at("AUR"));
        assert!(at("AUR") < at("fire-bin"));
        assert_eq!(
            shown.matches("firefox").count(),
            1,
            "installed is listed once"
        );
        assert!(!shown.contains("bash"));
        press(&mut app, &[Key::Tab, Key::Down, Key::Tab]);
        assert!(screen(&mut app, &mut term).contains("⏎ open 2"));
        press(&mut app, &[Key::Enter]);
        let marked = screen(&mut app, &mut term);
        assert!(
            marked.contains("firejail") && marked.contains("firefox"),
            "{marked}"
        );
    }

    #[test]
    fn what_is_marked_to_install_and_to_remove_goes_in_one_run_after_one_question() {
        use crate::ui::app::tests::{app, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let marked = BTreeSet::from([
            Pick::Local("pulseaudio".into()),
            Pick::Repo("extra".into(), "pipewire-pulse".into()),
        ]);
        open(&mut app, Box::new(MarkedPage::new(marked)));
        let shown = screen(&mut app, &mut term);
        for act in [
            "Install 1 and remove 1",
            "Install 1 from the repositories",
            "Remove 1",
        ] {
            assert!(shown.contains(act), "{act}: {shown}");
        }
        press(&mut app, &[Key::Enter]);
        let asked = screen(&mut app, &mut term);
        assert!(
            asked.contains("Install pipewire-pulse and remove pulseaudio?"),
            "{asked}"
        );
    }

    #[test]
    fn older_versions_open_on_the_newest_before_the_installed() {
        let (mut app, _) = crate::ui::app::tests::app();
        app.ctx.shared.installed = Loaded::Ready(parse_installed("pkg\tfoo\t2.0-1\n"));
        let mut page = VersionsPage::new("foo");
        let version = |v: &str, cached| Version {
            version: v.into(),
            location: format!("/cache/foo-{v}"),
            cached,
        };
        page.fetch.value = Loaded::Ready(vec![
            version("3.0-1", false),
            version("2.0-1", true),
            version("1.5-1", true),
            version("1.0-1", false),
        ]);
        let area = Rect::new(0, 0, 60, 10);
        page.draw(area, &mut Buffer::empty(area), &mut app.ctx);
        assert_eq!(
            page.list.selected().map(|r| r.label.as_str()),
            Some("1.5-1")
        );
    }
}
