//! The stack of pages, the shared values, and the keys no page takes.

use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use ratatui::Frame;

use super::target::Target;
use super::{
    Ctx, Does, Flow, Fresh, Key, Loaded, Msg, Nav, Page, PageId, Shared, Style, draw, pages,
};
use crate::data::{self, features::Features};
use crate::i18n::{self, Lang};
use crate::script::{self, Failure, op, query};
use crate::settings::Settings;
use crate::state::State;

pub struct App {
    pages: Vec<(PageId, Box<dyn Page>)>,
    next: PageId,
    pub ctx: Ctx,
    quit: bool,
}

impl App {
    pub fn new(
        tx: SyncSender<Msg>,
        wake: Arc<dyn Fn() + Send + Sync>,
        console: bool,
        start: Option<Target>,
    ) -> App {
        let (settings, settings_error) = Settings::load();
        if let Some(e) = &settings_error {
            crate::warn!("settings.yaml not used: {e}");
        }
        script::set_terminal(settings.terminal());
        i18n::set(
            settings
                .language
                .as_deref()
                .and_then(Lang::from_code)
                .unwrap_or_else(Lang::detect),
        );
        let (themes, theme_errors) = crate::theme::all();
        for e in &theme_errors {
            crate::warn!("theme skipped: {e}");
        }
        let mut ctx = Ctx {
            settings,
            settings_error,
            settings_unsaved: None,
            state: State::load(),
            style: Style::new(&themes[0].colors, console),
            console,
            themes,
            theme_errors,
            shared: Shared::default(),
            usage: None,
            generation: 0,
            authorized: false,
            reload: false,
            loads: Vec::new(),
            load: 0,
            cursor: Default::default(),
            tx: tx.clone(),
            wake: wake.clone(),
            page: 0,
            action: None,
            grace: super::GRACE,
            shown: (0, std::time::Instant::now()),
        };
        sample(tx, wake);
        ctx.style = Style::new(&ctx.theme().colors, console);
        let mut app = App {
            pages: Vec::new(),
            next: 0,
            ctx,
            quit: false,
        };
        app.push(Box::new(pages::home::HomePage::new()));
        if let Some(target) = start {
            for page in target.open(&mut app.ctx) {
                app.push(page);
            }
        }
        let splash = pages::splash::SplashPage::new(&app.ctx);
        app.push(Box::new(splash));
        app
    }

    /// Reads everything, once the splash is on the screen: begun before, the processes it
    /// starts hold the first frame back.
    pub fn start(&mut self) {
        self.reload();
        let (tx, wake) = (self.ctx.tx.clone(), self.ctx.wake.clone());
        std::thread::spawn(move || {
            if let Some(legacy) = crate::migrate::read() {
                let _ = tx.send(Msg::Legacy(legacy));
                wake();
            }
        });
    }

    fn push(&mut self, page: Box<dyn Page>) {
        self.next += 1;
        self.pages.push((self.next, page));
    }

    pub fn done(&self) -> bool {
        self.quit
    }

    /// When the driver wakes for a tick: on the next beat of what moves, the header's spinner
    /// included, else on the next idle one. On a grid, so events in between do not delay it.
    pub fn wake(&self, now: std::time::Instant) -> std::time::Instant {
        let page = self.pages.last().map(|(_, p)| p);
        let pace = match page {
            Some(p) if p.animating(&self.ctx) => p.pace(),
            _ if self.ctx.busy() => super::BEAT,
            _ => super::IDLE,
        };
        super::next_beat(now, pace)
    }

    pub fn handle(&mut self, msg: Msg) {
        let top = self.pages.last().map(|(id, _)| *id).unwrap_or(0);
        let typing = self.pages.last().is_some_and(|(_, p)| p.typing());
        let msg = match msg {
            Msg::Held(key) if key.repeats(typing) => Msg::Key(key),
            Msg::Held(_) => return,
            msg => msg,
        };
        match msg {
            Msg::Key(key) => {
                self.ctx.page = top;
                let (_, page) = self.pages.last_mut().expect("the start page stays");
                let typing = page.typing();
                let flow = match page.key(key, &mut self.ctx) {
                    Flow::Ignored => match key.nav(typing) {
                        Some(Nav::Back) => Flow::Pop,
                        Some(Nav::Quit) => Flow::Quit,
                        _ => Flow::Stay,
                    },
                    flow => flow,
                };
                self.apply(flow);
            }
            Msg::Tick => {
                self.ctx.page = top;
                if let Some((_, page)) = self.pages.last_mut() {
                    let flow = page.tick(&mut self.ctx);
                    self.apply(flow);
                }
            }
            Msg::Resize | Msg::Held(_) => {}
            Msg::Done(id) => self.deliver(id, |page, ctx| page.done(ctx)),
            Msg::Run(id, event) => self.deliver(id, |page, ctx| page.run(event, ctx)),
            Msg::Fresh(fresh) => self.fresh(fresh),
            Msg::Loaded(id) => self.ctx.loads.retain(|l| l.id != id),
            Msg::Usage(usage) => self.ctx.usage = Some(usage),
            Msg::Legacy(legacy) => self.legacy(legacy),
            // Elsewhere its letters would be commands, a q among them.
            Msg::Paste(text) if typing => {
                for c in text.chars().filter(|c| !c.is_control()) {
                    self.handle(Msg::Key(Key::Char(c)));
                }
            }
            Msg::Paste(_) => {}
        }
        if self.ctx.reload && self.ctx.loads.is_empty() {
            self.ctx.reload = false;
            self.reload();
        }
    }

    /// Hands a result to the page that asked. Its next step counts only while it is on top.
    fn deliver(&mut self, id: PageId, f: impl FnOnce(&mut Box<dyn Page>, &mut Ctx) -> Flow) {
        let top = self.pages.last().map(|(i, _)| *i);
        let Some((_, page)) = self.pages.iter_mut().find(|(i, _)| *i == id) else {
            return;
        };
        self.ctx.page = id;
        let flow = f(page, &mut self.ctx);
        if top == Some(id) {
            self.apply(flow);
        }
    }

    fn apply(&mut self, flow: Flow) {
        match flow {
            Flow::Stay | Flow::Ignored => {}
            Flow::Push(page) => self.push(page),
            Flow::PushAll(pages) => pages.into_iter().for_each(|p| self.push(p)),
            Flow::Replace(page) => {
                self.pages.truncate(self.pages.len() - self.closes());
                self.push(page);
            }
            Flow::Pop => {
                self.pages.truncate(self.pages.len() - self.closes());
                if let Some((id, _)) = self.pages.last() {
                    self.ctx.page = *id;
                }
            }
            Flow::Quit => self.quit = true,
        }
    }

    /// The pages back closes: the top one, and the one beneath where its action is done. The
    /// start page stays.
    fn closes(&self) -> usize {
        let below = self.pages.last().is_some_and(|(_, p)| p.closes_below());
        (1 + usize::from(below)).min(self.pages.len().saturating_sub(1))
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let path: Vec<String> = self.pages.iter().skip(1).map(|(_, p)| p.title()).collect();
        let over_home = self.pages.len() > 1 && self.pages.len() - self.closes() == 1;
        let (id, page) = self.pages.last_mut().expect("the start page stays");
        self.ctx.page = *id;
        if self.ctx.shown.0 != *id {
            self.ctx.shown = (*id, std::time::Instant::now());
        }
        self.ctx.cursor.set(None);
        if page.bare() {
            frame.buffer_mut().set_style(area, self.ctx.style.base());
            page.draw(area, frame.buffer_mut(), &mut self.ctx);
        } else {
            let content = draw::frame(area, frame.buffer_mut(), &self.ctx, &path);
            page.draw(content, frame.buffer_mut(), &mut self.ctx);
            let mut hints = page.hints(&self.ctx);
            // Back to the start page leads to the menu, and says so.
            if over_home {
                for (_, does) in &mut hints {
                    if *does == Does::Back {
                        *does = Does::Menu;
                    }
                }
            }
            draw::footer(area, frame.buffer_mut(), &self.ctx, &hints);
        }
        // The terminal's own cursor where a text box was drawn, hidden everywhere else.
        if let Some(at) = self.ctx.cursor.get() {
            frame.set_cursor_position(at);
        }
    }

    /// Takes the old manager over: its settings where none are chosen, written before anything
    /// of it goes, and read anew where they decide what runs.
    fn legacy(&mut self, legacy: crate::migrate::Legacy) {
        if self.ctx.settings_error.is_some() {
            crate::warn!("migrate: settings.yaml is not used");
            return;
        }
        if !legacy.take_over(&mut self.ctx.settings) {
            std::thread::spawn(move || crate::migrate::clean(&legacy));
            return;
        }
        let settings = self.ctx.settings.clone();
        self.ctx.store(move || {
            let saved = settings.save().map_err(|e| e.to_string());
            match &saved {
                Ok(()) => drop(std::thread::spawn(move || crate::migrate::clean(&legacy))),
                Err(e) => crate::warn!("migrate: settings.yaml not written: {e}"),
            }
            Fresh::Saved(saved)
        });
        self.ctx.reload();
    }

    /// Reads every shared value anew. What is shown stays until the new value arrives.
    fn reload(&mut self) {
        self.ctx.generation += 1;
        let ctx = &mut self.ctx;
        ctx.fresh(op::FEATURES_PROBE, || Fresh::Features(Features::load()));
        ctx.fresh(op::SYSTEM_STATE, || {
            Fresh::System(query(op::SYSTEM_STATE, &[]).map(|o| data::system::System::parse(&o)))
        });
        ctx.remote(op::NEWS_FETCH, || Fresh::News(load_news()));
        ctx.fresh(op::PACKAGES_INSTALLED, || {
            Fresh::Installed(
                query(op::PACKAGES_INSTALLED, &[]).map(|o| data::packages::parse_installed(&o)),
            )
        });
        ctx.fresh(op::SERVICES_LIST, || Fresh::Services(load_services()));
        ctx.fresh(op::LOG_ERRORS, || {
            Fresh::Journal(query(op::LOG_ERRORS, &[]).map(|o| data::journal::parse(&o)))
        });
        ctx.fresh(op::MIRRORS_LIST, || {
            Fresh::Mirrors(query(op::MIRRORS_LIST, &[]).map(|o| data::mirrors::Mirrors::parse(&o)))
        });
    }

    /// The rest waits for the optional packages: which are there decides what runs.
    fn after_features(&mut self, features: &Features) {
        script::set_aur(features.aur_helper(self.ctx.settings.aur_helper.as_deref()));
        script::set_aur_review(self.ctx.settings.aur_review.unwrap_or(true));
        script::set_flatpak(features.flatpak(self.ctx.settings.flatpak.unwrap_or(true)));
        let ctx = &mut self.ctx;
        let missing = |op| script::missing(op, features).map(str::to_string);
        macro_rules! gated {
            ($field:ident, $script:expr, $queue:ident, $variant:ident, $load:expr) => {
                match missing($script) {
                    Some(package) => ctx.shared.$field = Loaded::Missing(package),
                    None => ctx.$queue($script, move || Fresh::$variant($load)),
                }
            };
        }
        gated!(
            updates,
            op::UPGRADE_LIST,
            remote,
            Updates,
            query(op::UPGRADE_LIST, &[]).map(|o| data::updates::parse(&o))
        );
        gated!(
            pacnew,
            op::PACNEW_LIST,
            fresh,
            Pacnew,
            query(op::PACNEW_LIST, &[]).map(|o| o.lines().map(str::to_string).collect())
        );
        gated!(
            cleanup,
            op::CLEANUP_PLAN,
            fresh,
            Cleanup,
            query(op::CLEANUP_PLAN, &[]).map(|o| data::cleanup::Plan::parse(&o))
        );
        gated!(
            firmware,
            op::FIRMWARE_LIST,
            fresh,
            Firmware,
            load_firmware()
        );
    }

    fn fresh(&mut self, fresh: Fresh) {
        let s = &mut self.ctx.shared;
        match fresh {
            Fresh::Features(r) => {
                s.features = Loaded::from(r);
                if let Some(features) = s.features.ready().cloned() {
                    // The entry and the timer follow the settings, this program and libnotify,
                    // which an install or a removal may just have changed.
                    let settings = self.ctx.settings.clone();
                    let probed = features.clone();
                    std::thread::spawn(move || {
                        if let Err(f) = crate::integration::apply(&settings, &probed) {
                            crate::warn!("menu entry and notification: {}", f.detail);
                        }
                    });
                    self.after_features(&features);
                } else if let Loaded::Failed(f) = &s.features {
                    // What waits for the probe would wait for ever.
                    s.updates.give_up(f);
                    s.pacnew.give_up(f);
                    s.cleanup.give_up(f);
                    s.firmware.give_up(f);
                }
            }
            Fresh::System(r) => s.system = Loaded::from(r),
            Fresh::Updates(r) => {
                s.updates = Loaded::from(r);
                // Installed, here or elsewhere: a notification still about them goes.
                if s.updates
                    .ready()
                    .is_some_and(|p| p.updates.is_empty() && p.failed.is_empty())
                {
                    super::fetch::serial(crate::integration::withdraw);
                }
            }
            Fresh::News(r) => {
                s.news = Loaded::from(r);
                if let Some((items, _)) = s.news.ready()
                    && self.ctx.state.news_read.is_none()
                {
                    // First start: only the newest counts as unread, not the whole feed.
                    let older: Vec<String> = items.iter().skip(1).map(|i| i.link.clone()).collect();
                    self.ctx.state.news_read = Some(older.clone());
                    super::fetch::serial(move || {
                        State::update(|state| {
                            state.news_read.get_or_insert(older);
                        });
                    });
                }
            }
            Fresh::Installed(r) => {
                s.installed = Loaded::from(r);
                if let Some(installed) = s.installed.ready() {
                    let foreign: Vec<String> = installed
                        .iter()
                        .filter(|p| p.foreign)
                        .map(|p| p.name.clone())
                        .collect();
                    if foreign.is_empty() {
                        s.dropped = Loaded::Ready(Vec::new());
                    } else {
                        self.ctx.remote(op::PACKAGES_AUR_INFO, move || {
                            Fresh::Dropped(load_dropped(&foreign))
                        });
                    }
                }
            }
            Fresh::Dropped(r) => s.dropped = Loaded::from(r),
            Fresh::Services(r) => s.services = Loaded::from(r),
            Fresh::Journal(r) => s.journal = Loaded::from(r),
            Fresh::Pacnew(r) => s.pacnew = Loaded::from(r),
            Fresh::Cleanup(r) => s.cleanup = Loaded::from(r),
            Fresh::Mirrors(r) => s.mirrors = Loaded::from(r),
            Fresh::Firmware(r) => s.firmware = Loaded::from(r),
            Fresh::Saved(Ok(())) => {
                self.ctx.settings_error = None;
                self.ctx.settings_unsaved = None;
            }
            Fresh::Saved(Err(e)) => {
                crate::error!("settings not saved: {e}");
                self.ctx.settings_unsaved = Some(e);
            }
        }
    }
}

/// How often the usage is read. The processors' share is the one over this time.
const USAGE: std::time::Duration = std::time::Duration::from_secs(2);

/// Reads the usage off the drawing thread, until the app is gone.
fn sample(tx: SyncSender<Msg>, wake: Arc<dyn Fn() + Send + Sync>) {
    use data::usage::{self, Ticks, Usage};
    let ticks = || {
        std::fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|s| Ticks::parse(&s))
    };
    std::thread::spawn(move || {
        let mut before = ticks();
        loop {
            std::thread::sleep(USAGE);
            let now = ticks();
            let cpu = before.zip(now).and_then(|(b, n)| n.since(b));
            before = now.or(before);
            let memory = std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|s| usage::memory(&s));
            let disk = rustix::fs::statvfs("/")
                .ok()
                .and_then(|v| usage::disk(v.f_blocks, v.f_bfree, v.f_bavail));
            if tx.send(Msg::Usage(Usage { cpu, memory, disk })).is_err() {
                return;
            }
            wake();
        }
    });
}

/// Of the foreign packages, those the AUR does not know.
fn load_dropped(foreign: &[String]) -> Result<Vec<String>, Failure> {
    let args: Vec<&str> = foreign.iter().map(String::as_str).collect();
    let out = query(op::PACKAGES_AUR_INFO, &args)?;
    let found = data::aur::parse(&out).map_err(|e| Failure::new(op::PACKAGES_AUR_INFO, e))?;
    Ok(data::aur::unknown(foreign, &found))
}

/// The feed, or the copy from the last time it was reached.
fn load_news() -> Result<(Vec<data::news::Item>, bool), Failure> {
    let cache = crate::paths::news_cache();
    // An answer that is no feed, as a captive portal gives, counts as none.
    let fetched = query(op::NEWS_FETCH, &[]).and_then(|xml| {
        let items = data::news::parse(&xml).map_err(|e| Failure::new(op::NEWS_FETCH, e))?;
        Ok((xml, items))
    });
    match fetched {
        Ok((xml, items)) => {
            if let Err(e) = crate::paths::write_atomic(&cache, xml.as_bytes()) {
                crate::warn!("news cache not written: {e}");
            }
            Ok((items, false))
        }
        Err(failure) => match std::fs::read_to_string(&cache) {
            Ok(xml) => Ok((
                data::news::parse(&xml).map_err(|e| Failure::new(op::NEWS_FETCH, e))?,
                true,
            )),
            Err(_) => Err(failure),
        },
    }
}

fn load_services() -> Result<Vec<data::services::Service>, Failure> {
    let user = std::thread::spawn(|| query(op::SERVICES_LIST, &["user"]));
    let system = query(op::SERVICES_LIST, &["system"])?;
    let mut all =
        data::services::parse(&system, false).map_err(|e| Failure::new(op::SERVICES_LIST, e))?;
    // Without a service manager of the user's own, as over ssh, the system's still count.
    match user.join() {
        Ok(Ok(user)) => all.extend(
            data::services::parse(&user, true).map_err(|e| Failure::new(op::SERVICES_LIST, e))?,
        ),
        Ok(Err(f)) => crate::warn!("services of the user: {}", f.detail),
        Err(_) => crate::warn!("services of the user: panicked"),
    }
    Ok(all)
}

fn load_firmware() -> Result<Vec<data::firmware::Firmware>, Failure> {
    let out = query(op::FIRMWARE_LIST, &[])?;
    data::firmware::parse(&out).map_err(|e| Failure::new(op::FIRMWARE_LIST, e))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::ui::Key;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// An app without background loads, for pressing keys and reading the screen.
    pub fn app() -> (App, Terminal<TestBackend>) {
        let (tx, _rx) = std::sync::mpsc::sync_channel(crate::ui::QUEUE);
        let dir = std::env::temp_dir().join(format!("sm-test-{}", std::process::id()));
        // SAFETY: tests that read the environment read only SM_HOME, set to the same value by all.
        unsafe { std::env::set_var("SM_HOME", &dir) };
        let (themes, _) = crate::theme::all();
        let ctx = Ctx {
            settings: Settings::default(),
            settings_error: None,
            settings_unsaved: None,
            state: State::default(),
            style: Style::new(
                &themes
                    .iter()
                    .find(|t| t.id == crate::theme::DEFAULT)
                    .unwrap()
                    .colors,
                false,
            ),
            console: false,
            themes,
            theme_errors: Vec::new(),
            shared: Shared::default(),
            usage: None,
            generation: 0,
            authorized: false,
            reload: false,
            loads: Vec::new(),
            load: 0,
            cursor: Default::default(),
            tx,
            wake: Arc::new(|| {}),
            page: 0,
            action: None,
            grace: std::time::Duration::ZERO,
            shown: (0, std::time::Instant::now()),
        };
        let mut app = App {
            pages: Vec::new(),
            next: 0,
            ctx,
            quit: false,
        };
        app.push(Box::new(pages::home::HomePage::new()));
        (app, Terminal::new(TestBackend::new(80, 30)).unwrap())
    }

    pub fn screen(app: &mut App, term: &mut Terminal<TestBackend>) -> String {
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn open(app: &mut App, page: Box<dyn Page>) {
        app.push(page);
    }

    pub fn press(app: &mut App, keys: &[Key]) {
        for k in keys {
            app.handle(Msg::Key(*k));
        }
    }

    #[test]
    fn typing_puts_the_terminals_cursor_after_the_text() {
        let (mut app, mut term) = app();
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Char('f')]);
        for c in "Pak".chars() {
            press(&mut app, &[Key::Char(c)]);
        }
        let shown = screen(&mut app, &mut term);
        let at = term.get_cursor_position().unwrap();
        let line = shown.lines().nth(at.y as usize).unwrap();
        assert_eq!(
            line.find("Pak").map(|i| line[..i].chars().count() + 3),
            Some(at.x as usize),
            "the cursor stands after what was typed: {line}"
        );
    }

    /// Example data in every shared value, as the screenshots show it.
    pub fn example(app: &mut App) {
        use crate::data::{
            cleanup::Plan, features::Features, journal::Entry, mirrors::Mirrors, news::Item,
        };
        use crate::data::{packages::Installed, services::Service, system::System, updates};
        let s = &mut app.ctx.shared;
        s.features = Loaded::Ready(Features::parse(
            "paru\tok\t2.1.0-1\nyay\tmissing\nfwupd\tok\t2.0.16-1\ngnome-arch-update\tmissing\nmeld\tmissing\nlibnotify\tok\t0.8.8-1\nflatpak\tok\t1:1.18.4-1\nreflector\tok\t2023-2\n@desktop\tok\n@wayland\tok\n@gnome\tok\n",
        ));
        crate::script::set_aur(Some("paru"));
        s.system = Loaded::Ready(System {
            hostname: "tux-notebook".into(),
            kernel: "7.2.8-zen1-1-zen".into(),
            pacman: "7.1.0.r9.g54d9411-1".into(),
            desktop: Some("GNOME 51.1".into()),
            session: Some("wayland".into()),
            ..System::default()
        });
        s.updates = Loaded::Ready(updates::parse(
            "repo\tlinux-zen\t7.2.8.zen1-1\t7.2.9.zen1-1\nrepo\tmesa\t1:26.2.3-1\t1:26.2.4-1\nrepo\tfirefox\t144.0-1\t144.0.1-1\nrepo\tsystemd\t258.1-1\t258.2-1\naur\tparu\t2.1.0-1\t2.1.1-1\nflatpak\tcom.github.tchx84.Flatseal\t2.4.0\t2.4.1\n",
        ));
        let item = |title: &str, date: &str, link: &str| Item {
            title: title.into(),
            link: link.into(),
            date: date.into(),
            html: "<p>Example text.</p>".into(),
        };
        s.news = Loaded::Ready((
            vec![
                item(
                    "Example: a manual step before the next update",
                    "2026-10-12",
                    "a",
                ),
                item("Example: an older announcement", "2026-09-30", "b"),
            ],
            false,
        ));
        app.ctx.state.news_read = Some(vec!["b".into()]);
        app.ctx.usage = Some(data::usage::Usage {
            cpu: Some(0.18),
            memory: Some(0.4),
            disk: Some(0.82),
        });
        let installed = |n: &str| Installed {
            name: n.into(),
            version: "1.0-1".into(),
            explicit: true,
            foreign: false,
        };
        s.installed = Loaded::Ready((0..1284).map(|i| installed(&format!("p{i}"))).collect());
        let service = |unit: &str, active: &str| Service {
            user: false,
            unit: unit.into(),
            description: String::new(),
            active: active.into(),
            enabled: "enabled".into(),
            session: false,
        };
        s.services = Loaded::Ready(vec![
            service("example.service", "failed"),
            service("sshd.service", "active"),
        ]);
        s.journal = Loaded::Ready(
            (0..3)
                .map(|i| Entry::new(i, "kernel", &format!("example {i}")))
                .collect(),
        );
        s.pacnew = Loaded::Ready(vec!["/etc/pacman.conf.pacnew".into()]);
        s.cleanup = Loaded::Ready(Plan::parse(
            "orphan\texample-lib\t12.00 MiB\ncache\t/var/cache/pacman/pkg/a.pkg.tar.zst\t901775360\nkeep\tlinux-zen-7.2.8.zen1-1\n",
        ));
        s.mirrors = Loaded::Ready(Mirrors {
            changed: Some(jiff::Timestamp::now() - jiff::SignedDuration::from_hours(50)),
            timer: true,
            servers: vec!["mirror.example.org".into()],
        });
        s.firmware = Loaded::Ready(Vec::new());
    }

    #[test]
    fn every_page_fits_forty_columns_in_german() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::De);
        let (mut app, _) = app();
        example(&mut app);
        let mut term = Terminal::new(TestBackend::new(40, 16)).unwrap();
        for target in crate::ui::target::ALL {
            app.pages.truncate(1);
            for page in target.open(&mut app.ctx) {
                app.push(page);
            }
            let shown = screen(&mut app, &mut term);
            assert!(
                shown
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .contains("System Manager")
                    || shown.contains('…'),
                "{target:?}:\n{shown}"
            );
            assert!(shown.lines().count() <= 16, "{target:?}");
        }
        crate::i18n::set(crate::i18n::Lang::En);
    }

    /// Not a check: the screenshots in docs/, from example data through the window's painter,
    /// twice the size it is shown at. `make screenshots`
    #[test]
    #[ignore]
    fn screenshots() {
        use crate::script::{Run, op};
        let (mut app, _) = app();
        example(&mut app);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/screenshots");
        std::fs::create_dir_all(&dir).unwrap();
        let shoot = |app: &mut App, name: &str| {
            crate::window::tests::png(app, 2160, 1440, 2.0, &dir.join(format!("{name}.png")))
        };
        shoot(&mut app, "home");
        press(&mut app, &[Key::Enter]);
        shoot(&mut app, "update");
        let job =
            || pages::run::Job::new(op::UPGRADE_RUN, Vec::new()).named(i18n::t("Install now"));
        let output = ":: Starting full system upgrade...\r\n\
            :: Retrieving packages...\r\n\
            :: Processing package changes...\r\n\
            (3/5) upgrading mesa                       [##########] 100%\r\n";
        app.pages.truncate(1);
        app.push(Box::new(pages::run::RunPage::running(
            job(),
            Run::fed(op::UPGRADE_RUN, "repo", output),
        )));
        shoot(&mut app, "run");
        app.pages.truncate(1);
        app.push(Box::new(pages::run::RunPage::finished(job(), Ok(()))));
        shoot(&mut app, "done");
    }

    /// Not a check: renders pages with the real scripts, for a look at the layout.
    /// `cargo test preview -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn preview() {
        let (tx, rx) = std::sync::mpsc::sync_channel(crate::ui::QUEUE);
        unsafe { std::env::set_var("SM_HOME", std::env::temp_dir().join("sm-preview")) };
        let mut app = App::new(tx, Arc::new(|| {}), false, None);
        app.start();
        press(&mut app, &[Key::Esc]);
        let mut term = Terminal::new(TestBackend::new(96, 28)).unwrap();
        let pump = |app: &mut App, secs: u64| {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
            while let Ok(msg) =
                rx.recv_timeout(until.saturating_duration_since(std::time::Instant::now()))
            {
                app.handle(msg);
            }
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/preview");
        std::fs::create_dir_all(&dir).unwrap();
        pump(&mut app, 25);
        println!("{}\n", screen(&mut app, &mut term));
        crate::window::tests::png(&mut app, 1080, 720, 1.0, &dir.join("0.png"));
        let steps = [
            vec![Key::Enter],
            vec![Key::Esc, Key::Down, Key::Down, Key::Down, Key::Enter],
            vec![Key::Esc, Key::Char('f'), Key::Char('t'), Key::Char('h')],
        ];
        for (i, keys) in steps.into_iter().enumerate() {
            press(&mut app, &keys);
            pump(&mut app, 4);
            println!("{}\n", screen(&mut app, &mut term));
            crate::window::tests::png(
                &mut app,
                1080,
                720,
                1.0,
                &dir.join(format!("{}.png", i + 1)),
            );
        }
    }

    #[test]
    fn every_page_keeps_to_its_column_and_names_whole_keys_at_any_size() {
        use crate::ui::target::{ALL, Target};
        let _lang = i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let many: String = (0..300)
            .map(|i| format!("repo\tpackage-{i}\t1.0.{i}-1\t1.0.{}-1\n", i + 1))
            .collect();
        // One level deeper only from lists: the pages under them read, they change nothing.
        let lists = [
            Target::News,
            Target::Packages,
            Target::Services,
            Target::Log,
            Target::Pacnew,
            Target::Settings,
        ];
        for &target in ALL {
            for deeper in [false, true] {
                if deeper && !lists.contains(&target) {
                    continue;
                }
                for (w, h) in [(40, 12), (47, 25), (80, 24), (120, 40)] {
                    let (mut app, _) = app();
                    example(&mut app);
                    app.ctx.shared.updates = Loaded::Ready(data::updates::parse(&many));
                    for page in target.open(&mut app.ctx) {
                        app.push(page);
                    }
                    let mut term_at = Terminal::new(TestBackend::new(w, h)).unwrap();
                    if deeper {
                        screen(&mut app, &mut term_at);
                        press(&mut app, &[Key::Enter]);
                    }
                    let shown = screen(&mut app, &mut term_at);
                    let at = format!("{target:?} deeper {deeper} at {w}x{h}\n{shown}");
                    let (x, width, ..) = draw::geometry(ratatui::layout::Rect::new(0, 0, w, h))
                        .expect("room for a page");
                    let buf = term_at.backend().buffer();
                    for y in 0..h {
                        for col in [x - 1, x + width] {
                            assert_eq!(buf[(col, y)].symbol(), " ", "column {col}, row {y}: {at}");
                        }
                    }
                    let footer = shown.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
                    let footer = footer.trim_end();
                    assert!(
                        !footer.ends_with('…') || footer.ends_with(" · …"),
                        "a key cut: {at}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_update_page_waits_in_the_middle_until_the_list_is_there() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        let ready = std::mem::replace(&mut app.ctx.shared.updates, Loaded::Loading);
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        let line = shown
            .lines()
            .find(|l| l.contains("Looking for updates"))
            .expect(&shown);
        let left = line.len() - line.trim_start().len();
        assert!(left.abs_diff(80 - line.len()) <= 1, "{shown}");
        assert!(!shown.contains("Install now"), "{shown}");
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.pages.len(), 2);
        app.ctx.shared.updates = ready;
        assert!(screen(&mut app, &mut term).contains("Install now"));
        press(&mut app, &[Key::Esc]);
        assert_eq!(app.pages.len(), 1);
    }

    #[test]
    fn the_new_version_stands_out_from_the_old() {
        let (mut app, mut term) = app();
        example(&mut app);
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter]);
        screen(&mut app, &mut term);
        let buf = term.backend().buffer();
        let colour = |text: &str| {
            let area = buf.area;
            (0..area.height)
                .flat_map(|y| (0..area.width).map(move |x| (x, y)))
                .find(|&(x, y)| {
                    text.chars().enumerate().all(|(i, c)| {
                        x + (i as u16) < area.width
                            && buf[(x + i as u16, y)].symbol() == c.to_string()
                    })
                })
                .map(|p| buf[p].fg)
                .expect(text)
        };
        assert_eq!(colour("7.2.8.zen1-1"), app.ctx.style.muted);
        assert_eq!(colour("7.2.9.zen1-1"), app.ctx.style.accent);
    }

    /// Every page, each as the stack its target opens.
    fn every_page(app: &mut App) -> Vec<Vec<Box<dyn Page>>> {
        use crate::script::{Failure, op};
        let run = |outcome| -> Vec<Box<dyn Page>> {
            let job = pages::run::Job::new(op::UPGRADE_RUN, Vec::new());
            vec![Box::new(pages::run::RunPage::finished(job, outcome))]
        };
        let locked = "error: failed to init transaction (unable to lock database)";
        let mut all: Vec<Vec<Box<dyn Page>>> = crate::ui::target::ALL
            .iter()
            .map(|target| target.open(&mut app.ctx))
            .collect();
        all.push(vec![Box::new(pages::splash::SplashPage::after(
            pages::splash::END + 1000,
            &app.ctx,
        ))]);
        all.push(run(Ok(())));
        all.push(run(Err(Failure::classify(
            op::UPGRADE_RUN,
            Some(1),
            None,
            vec![locked.into()],
        ))));
        all
    }

    /// The console's font has letters, but none of the glyphs. Every page draws it those only.
    #[test]
    fn every_page_is_ascii_on_the_console() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = app();
        example(&mut app);
        let colors = app.ctx.theme().colors;
        app.ctx.style = Style::new(&colors, true);
        for stack in every_page(&mut app) {
            app.pages.truncate(1);
            for page in stack {
                app.push(page);
            }
            let shown = screen(&mut app, &mut term);
            let foreign: String = shown
                .chars()
                .filter(|c| !c.is_ascii() && !c.is_alphabetic())
                .collect();
            assert!(foreign.is_empty(), "{foreign:?} in\n{shown}");
        }
    }

    /// A terminal can be made as small as one cell. No page may panic in it.
    #[test]
    fn every_page_draws_at_any_size() {
        let (mut app, _) = app();
        example(&mut app);
        for (w, h) in [(1, 1), (2, 2), (8, 3), (20, 5), (39, 8), (120, 2), (3, 60)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            for stack in every_page(&mut app) {
                app.pages.truncate(1);
                for page in stack {
                    app.push(page);
                }
                screen(&mut app, &mut term);
            }
        }
    }

    #[test]
    fn the_header_spinner_turns_on_the_beat_while_something_loads() {
        let (mut app, _) = app();
        example(&mut app);
        let now = std::time::Instant::now();
        let idle = app.wake(now) - super::super::epoch();
        assert_eq!(idle.as_nanos() % super::super::IDLE.as_nanos(), 0);
        app.ctx.reload = true;
        assert!(app.wake(now) - now <= super::super::BEAT);
    }

    #[test]
    fn a_load_ends_after_its_value_and_one_from_outside_holds_no_splash() {
        let (mut app, _) = app();
        let (tx, rx) = std::sync::mpsc::sync_channel(crate::ui::QUEUE);
        app.ctx.tx = tx;
        app.ctx.remote(op::NEWS_FETCH, || Fresh::Saved(Ok(())));
        assert!(app.ctx.busy() && !app.ctx.settling());
        let id = app.ctx.loads[0].id;
        let limit = std::time::Duration::from_secs(5);
        assert!(matches!(rx.recv_timeout(limit), Ok(Msg::Fresh(_))));
        assert!(matches!(rx.recv_timeout(limit), Ok(Msg::Loaded(i)) if i == id));
        app.handle(Msg::Fresh(Fresh::Saved(Ok(()))));
        assert!(app.ctx.busy(), "a value ends no load");
        app.handle(Msg::Loaded(id));
        assert!(!app.ctx.busy());
        app.ctx.fresh(op::SYSTEM_STATE, || Fresh::Saved(Ok(())));
        assert!(app.ctx.settling());
    }

    #[test]
    fn the_splash_draws_at_every_moment() {
        let (mut app, mut term) = app();
        for ms in 0..pages::splash::END + 1000 {
            app.pages.truncate(1);
            let splash = pages::splash::SplashPage::after(ms, &app.ctx);
            app.push(Box::new(splash));
            screen(&mut app, &mut term);
        }
    }

    #[test]
    fn a_held_key_moves_and_types_but_does_nothing_else_again() {
        let (mut app, mut term) = app();
        example(&mut app);
        app.handle(Msg::Held(Key::Char('r')));
        assert!(
            !app.ctx.busy() && app.pages.len() == 1,
            "r held is no press"
        );
        let before = screen(&mut app, &mut term);
        app.handle(Msg::Held(Key::Down));
        assert_ne!(screen(&mut app, &mut term), before);
        press(&mut app, &[Key::Char('f')]);
        app.handle(Msg::Held(Key::Char('x')));
        app.handle(Msg::Held(Key::Char('x')));
        assert!(screen(&mut app, &mut term).contains("xx"));
        assert!(!Key::Enter.repeats(true) && !Key::Esc.repeats(true) && !Key::Tab.repeats(true));
        assert!(Key::Backspace.repeats(true) && Key::Down.repeats(true));
        assert!(Key::Left.repeats(true) && Key::CtrlBackspace.repeats(true));
        assert!(!Key::Left.repeats(false), "back happens once");
        assert!(Key::WheelUp.bytes().is_empty(), "the wheel types nothing");
        assert_eq!(Key::CtrlBackspace.bytes(), b"\x08", "as terminals send it");
        assert_eq!(Key::CtrlLeft.bytes(), b"\x1b[1;5D");
    }

    #[test]
    fn q_quits_from_a_list_but_is_typed_into_a_filter() {
        let (mut app, mut term) = app();
        press(&mut app, &[Key::Char('f'), Key::Char('q')]);
        assert!(!app.done());
        assert!(screen(&mut app, &mut term).contains("q"));
        press(&mut app, &[Key::Esc, Key::Char('q')]);
        assert!(app.done());
    }

    #[test]
    fn esc_names_the_main_menu_right_above_it_and_back_deeper() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        for page in Target::Settings.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("esc main menu"), "{shown}");
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("esc back") && !shown.contains("main menu"),
            "{shown}"
        );
        press(&mut app, &[Key::Esc]);
        assert!(screen(&mut app, &mut term).contains("esc main menu"));
        press(&mut app, &[Key::Esc]);
        assert_eq!(app.pages.len(), 1);
    }

    #[test]
    fn back_after_a_success_leads_past_the_page_the_action_was_chosen_on() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let run = |outcome| {
            let job = pages::run::Job::new(op::REPAIR_RUN, Vec::new());
            Box::new(pages::run::RunPage::finished(job, outcome))
        };
        open(&mut app, Box::new(pages::repair::RepairPage::new()));
        open(&mut app, run(Err(Failure::new(op::REPAIR_RUN, "failed"))));
        assert!(screen(&mut app, &mut term).contains("esc back"));
        press(&mut app, &[Key::Esc]);
        assert_eq!(app.pages.len(), 2, "a failure leaves it for another try");
        open(&mut app, run(Ok(())));
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("esc main menu"), "{shown}");
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.pages.len(), 1, "Back, the only action offered");
        open(&mut app, Box::new(pages::services::ServicesPage::new()));
        open(&mut app, Box::new(pages::repair::RepairPage::new()));
        open(&mut app, run(Ok(())));
        assert!(screen(&mut app, &mut term).contains("esc back"));
        press(&mut app, &[Key::Esc]);
        assert_eq!(app.pages.len(), 2);
    }

    #[test]
    fn a_step_offered_after_a_success_takes_the_place_of_the_page_the_action_was_chosen_on() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        open(&mut app, Box::new(pages::upgrade::UpgradePage::new()));
        let job = pages::run::Job::new(op::UPGRADE_RUN, Vec::new());
        open(
            &mut app,
            Box::new(pages::run::RunPage::finished(job, Ok(()))),
        );
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Config updates"), "{shown}");
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.pages.len(), 2);
        press(&mut app, &[Key::Esc]);
        assert_eq!(app.pages.len(), 1);
    }

    #[test]
    fn esc_in_a_search_names_what_it_closes_or_clears() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        press(&mut app, &[Key::Char('f')]);
        assert!(screen(&mut app, &mut term).contains("esc close search"));
        press(&mut app, &[Key::Esc]);
        for page in Target::Packages.open(&mut app.ctx) {
            open(&mut app, page);
        }
        press(&mut app, &[Key::Char('x')]);
        assert!(screen(&mut app, &mut term).contains("esc clear search"));
        press(&mut app, &[Key::Esc]);
        assert!(screen(&mut app, &mut term).contains("esc main menu"));
        assert_eq!(app.pages.len(), 2);
    }

    #[test]
    fn a_source_not_asked_is_named_and_up_to_date_is_not_claimed() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        example(&mut app);
        app.ctx.shared.updates = Loaded::Ready(data::updates::parse("failed\taur\n"));
        let home = screen(&mut app, &mut term);
        let row = home
            .lines()
            .find(|l| l.contains("Update"))
            .unwrap_or_default();
        assert!(row.ends_with("unknown"), "{home}");
        for page in Target::Upgrade.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("AUR unreachable"), "{shown}");
        assert!(
            shown.contains("Install now") && !shown.contains("up to date"),
            "{shown}"
        );
    }

    #[test]
    fn a_failed_probe_leaves_nothing_waiting_for_it() {
        let (mut app, mut term) = app();
        let failure = crate::script::Failure::new(op::FEATURES_PROBE, "no answer within 120 s");
        app.handle(Msg::Fresh(Fresh::Features(Err(failure))));
        for page in Target::Upgrade.open(&mut app.ctx) {
            open(&mut app, page);
        }
        assert!(matches!(app.ctx.shared.firmware, Loaded::Failed(_)));
        let shown = screen(&mut app, &mut term);
        assert!(!shown.contains("Looking for updates"), "{shown}");
        assert!(!app.pages.last().unwrap().1.animating(&app.ctx));
    }

    #[test]
    fn a_message_with_an_emoji_sequence_draws_at_any_width() {
        let (mut app, _) = app();
        example(&mut app);
        app.ctx.shared.journal = Loaded::Ready(vec![data::journal::Entry::new(
            0,
            "kernel",
            "⚠️ Warnung: ⚠️ Lüfter ⚠️ dreht nicht ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️ ⚠️",
        )]);
        for page in Target::Log.open(&mut app.ctx) {
            open(&mut app, page);
        }
        for w in [40, 47, 80] {
            let mut term = Terminal::new(TestBackend::new(w, 12)).unwrap();
            assert!(screen(&mut app, &mut term).contains("Warnung"));
        }
    }

    #[test]
    fn f_on_an_article_hides_nothing_and_q_still_quits() {
        let (mut app, mut term) = app();
        example(&mut app);
        for page in Target::News.open(&mut app.ctx) {
            open(&mut app, page);
        }
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Enter]);
        let article = screen(&mut app, &mut term);
        assert_eq!(app.pages.len(), 3, "{article}");
        press(&mut app, &[Key::Char('f')]);
        assert_eq!(screen(&mut app, &mut term), article);
        press(&mut app, &[Key::Char('q')]);
        assert!(app.done());
    }

    #[test]
    fn pasted_text_is_typed_into_a_box_and_commands_nothing_elsewhere() {
        let (mut app, mut term) = app();
        app.handle(Msg::Paste("quit\n".into()));
        assert!(!app.done() && !app.pages.last().unwrap().1.typing());
        press(&mut app, &[Key::Char('f')]);
        app.handle(Msg::Paste("upd\nate".into()));
        assert!(screen(&mut app, &mut term).contains("update"));
    }

    /// What ends sooner than the grace never flashes: the header and a page that waits show it
    /// only once it lasts.
    #[test]
    fn a_wait_shows_only_once_it_lasts() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        app.ctx.grace = std::time::Duration::from_millis(300);
        let long_ago = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let spins = |shown: &str| {
            let header = shown.lines().nth(1).unwrap_or_default().to_string();
            crate::ui::Style::new(&crate::theme::all().0[0].colors, false)
                .glyphs
                .spinner
                .iter()
                .any(|f| header.contains(f))
        };
        app.ctx.save_settings();
        assert!(!spins(&screen(&mut app, &mut term)), "a write just begun");
        app.ctx.loads[0].started = long_ago;
        assert!(spins(&screen(&mut app, &mut term)), "a write that lasts");
        app.ctx.loads.clear();
        open(
            &mut app,
            Box::new(crate::ui::pages::upgrade::UpgradePage::new()),
        );
        assert!(!screen(&mut app, &mut term).contains("Looking for updates"));
        app.ctx.shown.1 = long_ago;
        assert!(screen(&mut app, &mut term).contains("Looking for updates"));
    }

    /// In a text box backspace only deletes: it neither closes the box nor leaves the page.
    #[test]
    fn backspace_in_a_text_box_never_leaves_it() {
        use crate::script::{Run, op};
        use crate::ui::pages::packages::PackagesPage;
        use crate::ui::pages::run::{Job, RunPage};
        use crate::ui::pages::settings::SettingsPage;
        let erase = [Key::Backspace; 3];
        let (mut home, _) = app();
        press(&mut home, &[Key::Char('f'), Key::Char('x')]);
        press(&mut home, &erase);
        assert!(
            home.pages.last().unwrap().1.typing(),
            "the search on the start page"
        );
        let run = Run::fed(op::UPGRADE_RUN, "aur", "==> ");
        let pages: [(Box<dyn Page>, &[Key]); 3] = [
            (
                Box::new(SettingsPage::new(None)),
                &[Key::Char('f'), Key::Char('x')],
            ),
            (Box::new(PackagesPage::new()), &[Key::Char('x')]),
            (
                Box::new(RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run)),
                &[Key::Char('x')],
            ),
        ];
        for (page, typed) in pages {
            let (mut app, _) = app();
            open(&mut app, page);
            press(&mut app, typed);
            press(&mut app, &erase);
            let top = &app.pages.last().unwrap().1;
            assert!(app.pages.len() == 2 && top.typing(), "{}", top.title());
        }
    }

    #[test]
    fn ctrl_c_quits_from_anywhere_and_esc_on_the_start_page_does_not() {
        let (mut app, _) = app();
        press(&mut app, &[Key::Esc, Key::Backspace, Key::Char('h')]);
        assert!(!app.done());
        press(&mut app, &[Key::Char('f'), Key::Ctrl('c')]);
        assert!(app.done());
    }
}
