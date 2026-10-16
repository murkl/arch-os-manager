//! The stack of pages, the shared values, and the keys no page takes.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use ratatui::Frame;

use super::target::Target;
use super::{Ctx, Flow, Fresh, Loaded, Msg, Nav, Page, PageId, Shared, Style, draw, pages};
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
        tx: Sender<Msg>,
        wake: Arc<dyn Fn() + Send + Sync>,
        console: bool,
        start: Option<Target>,
    ) -> App {
        let (settings, settings_error) = Settings::load();
        if let Some(e) = &settings_error {
            crate::warn!("settings.yaml not used: {e}");
        }
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
            state: State::load(),
            style: Style::new(&themes[0].colors, console),
            console,
            themes,
            theme_errors,
            shared: Shared::default(),
            generation: 0,
            authorized: false,
            reload: false,
            pending: Default::default(),
            tx,
            wake,
            page: 0,
        };
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
        app.push(Box::new(pages::splash::SplashPage::new()));
        app.reload();
        app
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
            Msg::Data(id, data) => self.deliver(id, |page, ctx| page.data(data, ctx)),
            Msg::Run(id, event) => self.deliver(id, |page, ctx| page.run(event, ctx)),
            Msg::Fresh(fresh) => self.fresh(fresh),
        }
        if self.ctx.reload && self.ctx.pending.get() == 0 {
            self.ctx.reload = false;
            self.reload();
        }
    }

    /// Hands a result to the page that asked; its next step counts only while it is on top.
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
                if self.pages.len() > 1 {
                    self.pages.pop();
                }
                self.push(page);
            }
            Flow::Pop => {
                if self.pages.len() > 1 {
                    self.pages.pop();
                    if let Some((id, page)) = self.pages.last_mut() {
                        self.ctx.page = *id;
                        page.resumed(&mut self.ctx);
                    }
                }
            }
            Flow::Quit => self.quit = true,
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let path: Vec<String> = self.pages.iter().skip(1).map(|(_, p)| p.title()).collect();
        let (id, page) = self.pages.last_mut().expect("the start page stays");
        self.ctx.page = *id;
        if page.bare() {
            frame.buffer_mut().set_style(area, self.ctx.style.base());
            page.draw(area, frame.buffer_mut(), &mut self.ctx);
            return;
        }
        let content = draw::frame(area, frame.buffer_mut(), &self.ctx, &path);
        page.draw(content, frame.buffer_mut(), &mut self.ctx);
        let hints = page.hints(&self.ctx);
        draw::footer(area, frame.buffer_mut(), &self.ctx, &hints);
    }

    /// Reads every shared value anew; what is shown stays until the new value arrives.
    fn reload(&mut self) {
        self.ctx.generation += 1;
        let ctx = &self.ctx;
        ctx.fresh(|| Fresh::Features(Features::load()));
        ctx.fresh(|| {
            Fresh::System(query(op::SYSTEM_STATE, &[]).map(|o| data::system::System::parse(&o)))
        });
        ctx.fresh(|| Fresh::News(load_news()));
        ctx.fresh(|| {
            Fresh::Installed(
                query(op::PACKAGES_INSTALLED, &[]).map(|o| data::packages::parse_installed(&o)),
            )
        });
        ctx.fresh(|| Fresh::Services(load_services()));
        ctx.fresh(|| Fresh::Journal(query(op::LOG_ERRORS, &[]).map(|o| data::journal::parse(&o))));
        ctx.fresh(|| {
            Fresh::Mirrors(query(op::MIRRORS_LIST, &[]).map(|o| data::mirrors::Mirrors::parse(&o)))
        });
    }

    /// The rest waits for the optional packages: which are there decides what runs.
    fn after_features(&mut self, features: &Features) {
        script::set_aur(features.aur_helper(self.ctx.settings.aur_helper.as_deref()));
        let ctx = &mut self.ctx;
        let missing =
            |op: crate::script::Op| op.meta().needs.iter().find(|p| !features.ok(p)).cloned();
        macro_rules! gated {
            ($field:ident, $script:expr, $variant:ident, $load:expr) => {
                match missing($script) {
                    Some(package) => ctx.shared.$field = Loaded::Missing(package),
                    None => ctx.fresh(move || Fresh::$variant($load)),
                }
            };
        }
        gated!(
            updates,
            op::UPDATE_LIST,
            Updates,
            query(op::UPDATE_LIST, &[]).map(|o| data::updates::parse(&o))
        );
        gated!(
            pacnew,
            op::PACNEW_LIST,
            Pacnew,
            query(op::PACNEW_LIST, &[]).map(|o| o.lines().map(str::to_string).collect())
        );
        gated!(
            cleanup,
            op::CLEANUP_PLAN,
            Cleanup,
            query(op::CLEANUP_PLAN, &[]).map(|o| data::cleanup::Plan::parse(&o))
        );
        gated!(firmware, op::FIRMWARE_LIST, Firmware, load_firmware());
    }

    fn fresh(&mut self, fresh: Fresh) {
        self.ctx
            .pending
            .set(self.ctx.pending.get().saturating_sub(1));
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
                }
            }
            Fresh::System(r) => s.system = Loaded::from(r),
            Fresh::Updates(r) => s.updates = Loaded::from(r),
            Fresh::News(r) => {
                s.news = Loaded::from(r);
                if let Some((items, _)) = s.news.ready()
                    && self.ctx.state.news_read.is_none()
                {
                    // First start: only the newest counts as unread, not the whole feed.
                    self.ctx.state.news_read =
                        Some(items.iter().skip(1).map(|i| i.link.clone()).collect());
                    self.ctx.state.save();
                }
            }
            Fresh::Installed(r) => s.installed = Loaded::from(r),
            Fresh::Services(r) => s.services = Loaded::from(r),
            Fresh::Journal(r) => s.journal = Loaded::from(r),
            Fresh::Pacnew(r) => s.pacnew = Loaded::from(r),
            Fresh::Cleanup(r) => s.cleanup = Loaded::from(r),
            Fresh::Mirrors(r) => s.mirrors = Loaded::from(r),
            Fresh::Firmware(r) => s.firmware = Loaded::from(r),
        }
    }
}

/// The feed, or the copy from the last time it was reached.
fn load_news() -> Result<(Vec<data::news::Item>, bool), Failure> {
    let cache = crate::paths::news_cache();
    match query(op::NEWS_FETCH, &[]) {
        Ok(xml) => {
            let items = data::news::parse(&xml).map_err(|e| Failure::new(op::NEWS_FETCH, e))?;
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
    let user = user
        .join()
        .unwrap_or_else(|_| Err(Failure::new(op::SERVICES_LIST, "panicked")))?;
    all.extend(data::services::parse(&user, true).map_err(|e| Failure::new(op::SERVICES_LIST, e))?);
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
        let (tx, _rx) = std::sync::mpsc::channel();
        let dir = std::env::temp_dir().join(format!("sm-test-{}", std::process::id()));
        // SAFETY: tests that read the environment read only SM_HOME, set to the same value by all.
        unsafe { std::env::set_var("SM_HOME", &dir) };
        let (themes, _) = crate::theme::all();
        let ctx = Ctx {
            settings: Settings::default(),
            settings_error: None,
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
            generation: 0,
            authorized: false,
            reload: false,
            pending: Default::default(),
            tx,
            wake: Arc::new(|| {}),
            page: 0,
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

    /// Example data in every shared value, as the screenshots show it.
    fn example(app: &mut App) {
        use crate::data::{
            cleanup::Plan, features::Features, journal::Entry, mirrors::Mirrors, news::Item,
        };
        use crate::data::{packages::Installed, services::Service, system::System, updates};
        let s = &mut app.ctx.shared;
        s.features = Loaded::Ready(Features::parse(
            "paru\tok\t2.1.0-1\nyay\tmissing\nfwupd\tok\t2.0.16-1\narch-update\tmissing\nmeld\tmissing\nlibnotify\tok\t0.8.8-1\nflatpak\tok\t1:1.18.4-1\npacman-contrib\tok\t1.13.1-1\nreflector\tok\t2023-2\n@desktop\tok\n@gnome\tok\n",
        ));
        crate::script::set_aur(Some("paru"));
        s.system = Loaded::Ready(System {
            hostname: "zeus".into(),
            kernel: "7.2.8-zen1-1-zen".into(),
            pacman: "7.1.0.r9.g54d9411-1".into(),
            desktop: Some("GNOME 51.1".into()),
            session: Some("wayland".into()),
            ..System::default()
        });
        s.updates = Loaded::Ready(updates::parse(
            "repo\tlinux-zen\t7.2.8.zen1-1\t7.2.9.zen1-1\nrepo\tmesa\t1:26.2.3-1\t1:26.2.4-1\nrepo\tfirefox\t144.0-1\t144.0.1-1\nrepo\tsystemd\t258.1-1\t258.2-1\naur\tparu\t2.1.0-1\t2.1.1-1\nflatpak\tcom.github.tchx84.Flatseal\t\t2.4.1\n",
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
            sub: String::new(),
            enabled: "enabled".into(),
        };
        s.services = Loaded::Ready(vec![
            service("example.service", "failed"),
            service("sshd.service", "active"),
        ]);
        s.journal = Loaded::Ready(
            (0..3)
                .map(|i| Entry {
                    time: i,
                    ident: "kernel".into(),
                    message: format!("example {i}"),
                })
                .collect(),
        );
        s.pacnew = Loaded::Ready(vec!["/etc/pacman.conf.pacnew".into()]);
        s.cleanup = Loaded::Ready(Plan::parse(
            "orphan\texample-lib\t12.00 MiB\ncache\t/var/cache/pacman/pkg/a.pkg.tar.zst\t901775360\nkeep\tlinux-zen-7.2.8.zen1-1\nflatpak\n",
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
    /// twice the size it is shown at. `docs/screenshots.sh`
    #[test]
    #[ignore]
    fn screenshots() {
        use crate::script::{Run, op};
        let (mut app, _) = app();
        example(&mut app);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/screenshots");
        std::fs::create_dir_all(&dir).unwrap();
        let shoot = |app: &mut App, name: &str| {
            crate::window::tests::png(app, 2160, 1280, 2.0, &dir.join(format!("{name}.png")))
        };
        shoot(&mut app, "home");
        press(&mut app, &[Key::Enter]);
        shoot(&mut app, "update");
        let job = || pages::run::Job::new(op::UPDATE_RUN, Vec::new());
        let output = ":: Starting full system upgrade...\r\n\
            :: Retrieving packages...\r\n\
            :: Processing package changes...\r\n\
            (3/5) upgrading mesa                       [##########] 100%\r\n";
        app.pages.truncate(1);
        app.push(Box::new(pages::run::RunPage::running(
            job(),
            Run::fed(op::UPDATE_RUN, "repo", output),
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
        let (tx, rx) = std::sync::mpsc::channel();
        unsafe { std::env::set_var("SM_HOME", std::env::temp_dir().join("sm-preview")) };
        let mut app = App::new(tx, Arc::new(|| {}), false, None);
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
        assert!(!shown.contains("Update now"), "{shown}");
        press(&mut app, &[Key::Enter]);
        assert_eq!(app.pages.len(), 2);
        app.ctx.shared.updates = ready;
        assert!(screen(&mut app, &mut term).contains("Update now"));
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
            let job = pages::run::Job::new(op::UPDATE_RUN, Vec::new());
            vec![Box::new(pages::run::RunPage::finished(job, outcome))]
        };
        let locked = "error: failed to init transaction (unable to lock database)";
        let mut all: Vec<Vec<Box<dyn Page>>> = crate::ui::target::ALL
            .iter()
            .map(|target| target.open(&mut app.ctx))
            .collect();
        all.push(vec![Box::new(pages::splash::SplashPage::after(2500))]);
        all.push(run(Ok(())));
        all.push(run(Err(Failure::classify(
            op::UPDATE_RUN,
            Some(1),
            None,
            vec![locked.into()],
        ))));
        all
    }

    /// A terminal can be made as small as one cell; no page may panic in it.
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
    fn the_splash_draws_at_every_moment() {
        let (mut app, mut term) = app();
        for ms in 0..pages::splash::END {
            app.pages.truncate(1);
            app.push(Box::new(pages::splash::SplashPage::after(ms)));
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
    fn ctrl_c_quits_from_anywhere_and_esc_on_the_start_page_does_not() {
        let (mut app, _) = app();
        press(&mut app, &[Key::Esc, Key::Backspace, Key::Char('h')]);
        assert!(!app.done());
        press(&mut app, &[Key::Char('f'), Key::Ctrl('c')]);
        assert!(app.done());
    }
}
