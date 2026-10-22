//! The interface: pages on a stack, drawn into a cell grid that a terminal or a window shows.

mod app;
mod detail;
mod draw;
mod fetch;
mod gauge;
mod highlight;
pub(crate) mod icon;
mod input;
mod list;
mod pages;
mod style;
mod target;

pub use app::App;
#[cfg(test)]
pub(crate) use app::tests as app_tests;
pub use fetch::settle;
pub use style::Style;
pub use target::Target;

use std::sync::mpsc::SyncSender;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::data::cleanup::Plan;
use crate::data::dotfiles::Dotfiles;
use crate::data::features::Features;
use crate::data::firmware::Firmware;
use crate::data::homelab::Homelab;
use crate::data::journal::Entry;
use crate::data::mirrors::Mirrors;
use crate::data::news::Item;
use crate::data::packages::Installed;
use crate::data::services::Service;
use crate::data::system::System;
use crate::data::updates::Pending;
use crate::data::usage::Usage;
use crate::script::{self, Failure, Op};
use crate::settings::Settings;
use crate::state::State;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    /// A notch of the mouse wheel: a list moves as with the arrows, facts above actions scroll.
    WheelUp,
    WheelDown,
}

/// What a key means outside a text box. In front of one, only the keys that cannot be typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Open,
    Back,
    Quit,
    Filter,
}

impl Key {
    pub fn nav(self, typing: bool) -> Option<Nav> {
        let nav = match self {
            Key::Up | Key::WheelUp => Nav::Up,
            Key::Down | Key::WheelDown => Nav::Down,
            Key::PageUp => Nav::PageUp,
            Key::PageDown => Nav::PageDown,
            Key::Enter => Nav::Open,
            Key::Esc => Nav::Back,
            Key::Ctrl('c') => Nav::Quit,
            _ if typing => return None,
            Key::Home | Key::Char('g') => Nav::Top,
            Key::End | Key::Char('G') => Nav::Bottom,
            Key::Char('k') => Nav::Up,
            Key::Char('j') => Nav::Down,
            Key::Char('l') | Key::Right => Nav::Open,
            Key::Char('h') | Key::Left | Key::Backspace => Nav::Back,
            Key::Char('q') => Nav::Quit,
            Key::Char('f') | Key::Char('/') => Nav::Filter,
            _ => return None,
        };
        Some(nav)
    }

    /// Whether holding the key repeats it: moving and typing do, everything else happens once
    /// per press, so a held enter answers no question that follows.
    pub fn repeats(self, typing: bool) -> bool {
        matches!(
            self.nav(false),
            Some(Nav::Up | Nav::Down | Nav::PageUp | Nav::PageDown)
        ) || typing && matches!(self, Key::Char(_) | Key::Backspace | Key::Delete)
    }

    /// The bytes a terminal sends for the key, for the details view of a run.
    pub fn bytes(self) -> Vec<u8> {
        match self {
            Key::Char(c) => c.to_string().into_bytes(),
            Key::Ctrl(c) => vec![(c.to_ascii_lowercase() as u8) & 0x1f],
            Key::Enter => b"\r".to_vec(),
            Key::Esc => b"\x1b".to_vec(),
            Key::Backspace => b"\x7f".to_vec(),
            Key::Delete => b"\x1b[3~".to_vec(),
            Key::Tab => b"\t".to_vec(),
            Key::BackTab => b"\x1b[Z".to_vec(),
            // The wheel scrolls. Typed, it would answer a question with an arrow.
            Key::WheelUp | Key::WheelDown => Vec::new(),
            Key::Up => b"\x1b[A".to_vec(),
            Key::Down => b"\x1b[B".to_vec(),
            Key::Right => b"\x1b[C".to_vec(),
            Key::Left => b"\x1b[D".to_vec(),
            Key::PageUp => b"\x1b[5~".to_vec(),
            Key::PageDown => b"\x1b[6~".to_vec(),
            Key::Home => b"\x1b[H".to_vec(),
            Key::End => b"\x1b[F".to_vec(),
        }
    }
}

/// The step of what moves: a running bar moves a cell, the spinner turns every third. The
/// drivers wake on the beat rather than a while after the last event, so every frame stands
/// exactly as long as the next.
pub const BEAT: Duration = Duration::from_millis(40);
/// A frame of what moves smoothly, and the shortest time between two draws.
pub const FRAME: Duration = Duration::from_millis(16);
/// The beat of a page where nothing moves.
const IDLE: Duration = Duration::from_millis(500);

/// The start every animation counts from.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Beats of `pace` passed by `now`.
pub fn beats(now: Instant, pace: Duration) -> u64 {
    (now.saturating_duration_since(epoch()).as_nanos() / pace.as_nanos()) as u64
}

/// The first beat of `pace` after `now`, or the start where `now` lies before it.
pub fn next_beat(now: Instant, pace: Duration) -> Instant {
    let start = epoch();
    if now < start {
        return start;
    }
    start + Duration::from_nanos((beats(now, pace) + 1) * pace.as_nanos() as u64)
}

pub type PageId = u64;

/// Messages waiting for the interface at most. A run that prints faster than the interface takes
/// it waits at its terminal, rather than filling memory: some 8 MiB of output at most.
pub const QUEUE: usize = 1024;

pub enum Msg {
    Key(Key),
    /// A key held down, sent again by the keyboard's repeat.
    Held(Key),
    Tick,
    Resize,
    /// A job a page started is done: its result waits in the page's own channel.
    Done(PageId),
    Run(PageId, script::Event),
    Fresh(Fresh),
    /// A reading job sent its value: its load ends.
    Loaded(u64),
    Usage(Usage),
    /// What the old manager left, read behind the splash.
    Legacy(crate::migrate::Legacy),
    /// Text pasted into the terminal as one, typed where a text box has the keys.
    Paste(String),
}

/// A shared value read anew.
pub enum Fresh {
    Features(Result<Features, Failure>),
    System(Result<System, Failure>),
    Updates(Result<Pending, Failure>),
    News(Result<(Vec<Item>, bool), Failure>),
    Installed(Result<Vec<Installed>, Failure>),
    Services(Result<Vec<Service>, Failure>),
    Journal(Result<Vec<Entry>, Failure>),
    Pacnew(Result<Vec<String>, Failure>),
    Cleanup(Result<Plan, Failure>),
    Mirrors(Result<Mirrors, Failure>),
    Firmware(Result<Vec<Firmware>, Failure>),
    /// Boxed, as they are many times the size of the other values.
    Dotfiles(Result<Box<Dotfiles>, Failure>),
    /// The dotfiles read after fetching their repository, or why it was not reached.
    DotfilesFetched(Result<Box<Dotfiles>, Failure>),
    Homelab(Result<Box<Homelab>, Failure>),
    /// settings.yaml written, or why not.
    Saved(Result<(), String>),
}

pub enum Flow {
    Stay,
    /// The key meant nothing here. The app applies back, quit and abort.
    Ignored,
    Push(Box<dyn Page>),
    /// Several at once, as a search result that lives below the settings page.
    PushAll(Vec<Box<dyn Page>>),
    Replace(Box<dyn Page>),
    Pop,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Muted,
    Accent,
    Ok,
    Warn,
    Error,
}

/// What a key does, as the footer names it. Every page takes its words from here, so one deed
/// reads the same everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Does {
    Choose,
    Scroll,
    Open,
    /// Opens the marked rows, this many.
    OpenMarked(usize),
    Run,
    Answer,
    Use,
    Hide,
    Show,
    Mark,
    Read,
    Unread,
    Details,
    Overview,
    Search,
    Filter,
    CloseSearch,
    CloseFilter,
    ClearSearch,
    Reload,
    /// One folder up, in a browser of folders.
    Parent,
    /// Closes the page. The app names it `Menu` where that leads to the start page.
    Back,
    Menu,
    Stop,
    Quit,
}

impl Does {
    pub fn label(self) -> String {
        use crate::i18n::{t, tf};
        match self {
            Does::OpenMarked(n) => return tf("open {n}", &[("n", &n)]),
            Does::Choose => t("choose"),
            Does::Scroll => t("scroll"),
            Does::Open => t("open"),
            Does::Run => t("run"),
            Does::Answer => t("answer"),
            Does::Use => t("use"),
            Does::Hide => t("hide"),
            Does::Show => t("show"),
            Does::Mark => t("mark"),
            Does::Read => t("read"),
            Does::Unread => t("unread"),
            Does::Details => t("details"),
            Does::Overview => t("overview"),
            Does::Search => t("search"),
            Does::Filter => t("filter"),
            Does::CloseSearch => t("close search"),
            Does::CloseFilter => t("close filter"),
            Does::ClearSearch => t("clear search"),
            Does::Reload => t("reload"),
            Does::Parent => t("parent folder"),
            Does::Back => t("back"),
            Does::Menu => t("main menu"),
            Does::Stop => t("stop"),
            Does::Quit => t("quit"),
        }
        .to_string()
    }
}

pub trait Page {
    /// The page's name in the header path.
    fn title(&self) -> String;
    fn draw(
        &mut self,
        area: ratatui::layout::Rect,
        buf: &mut ratatui::buffer::Buffer,
        ctx: &mut Ctx,
    );
    /// The keys the footer names, as key and what it does.
    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, Does)>;
    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow;
    /// A job of this page is done: its result is ready to take.
    fn done(&mut self, _ctx: &mut Ctx) -> Flow {
        Flow::Stay
    }
    fn run(&mut self, _event: script::Event, _ctx: &mut Ctx) -> Flow {
        Flow::Stay
    }
    fn tick(&mut self, _ctx: &mut Ctx) -> Flow {
        Flow::Stay
    }
    /// A text box has the keys, so letters are typed rather than read as commands.
    fn typing(&self) -> bool {
        false
    }
    /// Moves by itself, so the driver wakes on every beat.
    fn animating(&self, _ctx: &Ctx) -> bool {
        false
    }
    /// The beat it moves on while animating, the spinner's unless it moves smoothly.
    fn pace(&self) -> Duration {
        BEAT
    }
    /// Drawn on the whole screen, without the header and the keys.
    fn bare(&self) -> bool {
        false
    }
    /// Back closes the page beneath as well, whose action is done.
    fn closes_below(&self) -> bool {
        false
    }
}

/// A value read in the background: nothing yet, the value, or why not. A reload keeps the old value.
#[derive(Clone, Debug, Default)]
pub enum Loaded<T> {
    #[default]
    Loading,
    Ready(T),
    Failed(Failure),
    /// The script needs this optional package, which is not installed.
    Missing(String),
}

impl<T> Loaded<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Loaded::Ready(v) => Some(v),
            _ => None,
        }
    }

    /// Gives up a value still to come, for a reason that keeps it from ever coming. A value
    /// already there stays.
    pub fn give_up(&mut self, why: &Failure) {
        if matches!(self, Loaded::Loading) {
            *self = Loaded::Failed(why.clone());
        }
    }

    pub fn from(result: Result<T, Failure>) -> Loaded<T> {
        match result {
            Ok(v) => Loaded::Ready(v),
            Err(f) => Loaded::Failed(f),
        }
    }
}

/// What several pages show, read once at the start and after every run.
#[derive(Default)]
pub struct Shared {
    pub features: Loaded<Features>,
    pub system: Loaded<System>,
    pub updates: Loaded<Pending>,
    /// The news, and whether they came from the cache because the feed was out of reach.
    pub news: Loaded<(Vec<Item>, bool)>,
    pub installed: Loaded<Vec<Installed>>,
    pub services: Loaded<Vec<Service>>,
    pub journal: Loaded<Vec<Entry>>,
    pub pacnew: Loaded<Vec<String>>,
    pub cleanup: Loaded<Plan>,
    pub mirrors: Loaded<Mirrors>,
    pub firmware: Loaded<Vec<Firmware>>,
    pub dotfiles: Loaded<Dotfiles>,
    /// Why the repository of the dotfiles was not reached at the last fetch.
    pub unreached: Option<Failure>,
    pub homelab: Loaded<Homelab>,
}

/// A job reading a shared value or writing settings, until its value is in.
struct Load {
    id: u64,
    /// What it runs, none for a write.
    op: Option<Op>,
    /// From outside the machine: the splash does not wait for it.
    remote: bool,
}

pub struct Ctx {
    pub settings: Settings,
    /// Why settings.yaml was not used, until it is written again.
    pub settings_error: Option<String>,
    /// Why the settings chosen last could not be written, until a write succeeds.
    pub settings_unsaved: Option<String>,
    pub state: State,
    pub themes: Vec<Theme>,
    /// User theme files that were skipped, and why.
    pub theme_errors: Vec<String>,
    pub style: Style,
    /// The Linux console: sixteen colours and ASCII glyphs.
    pub console: bool,
    pub shared: Shared,
    /// What the machine uses, read anew every few seconds, none before the first reading.
    pub usage: Option<Usage>,
    /// Counts the runs, so a page can tell its data may be stale.
    pub generation: u64,
    /// The password was proven, or is not needed, this session.
    pub authorized: bool,
    reload: bool,
    /// Jobs reading shared values or writing settings, oldest first.
    loads: Vec<Load>,
    /// The id of the last load.
    load: u64,
    tx: SyncSender<Msg>,
    wake: Arc<dyn Fn() + Send + Sync>,
    page: PageId,
    /// The action chosen, while it runs: the name of the run it starts.
    pub action: Option<String>,
    /// Where a text box is typed into this frame, for the terminal's own cursor.
    pub cursor: std::cell::Cell<Option<(u16, u16)>>,
}

impl Ctx {
    /// Runs a job off the drawing thread and tells the page that started it once it is done.
    /// The result goes through the page's own channel: a `Fetch` or a `Task`.
    pub fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        let (tx, wake, page) = (self.tx.clone(), self.wake.clone(), self.page);
        std::thread::spawn(move || {
            job();
            let _ = tx.send(Msg::Done(page));
            wake();
        });
    }

    /// Where the events of a run go: to the page that started it.
    pub fn run_sink(&self) -> impl Fn(script::Event) + Send + Sync + 'static {
        let (tx, wake, page) = (self.tx.clone(), self.wake.clone(), self.page);
        move |e| {
            let _ = tx.send(Msg::Run(page, e));
            wake();
        }
    }

    /// Reads a shared value off the drawing thread.
    fn fresh(&mut self, op: Op, job: impl FnOnce() -> Fresh + Send + 'static) {
        let send = self.begin(Some(op), false);
        std::thread::spawn(move || send(job()));
    }

    /// As `fresh`, for a value from outside the machine, which may take long.
    fn remote(&mut self, op: Op, job: impl FnOnce() -> Fresh + Send + 'static) {
        let send = self.begin(Some(op), true);
        std::thread::spawn(move || send(job()));
    }

    /// Notes a load, and returns what sends its value and then its end.
    fn begin(&mut self, op: Option<Op>, remote: bool) -> impl FnOnce(Fresh) + Send + 'static {
        self.load += 1;
        let id = self.load;
        self.loads.push(Load { id, op, remote });
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        move |fresh| {
            let _ = tx.send(Msg::Fresh(fresh));
            let _ = tx.send(Msg::Loaded(id));
            wake();
        }
    }

    /// Reads every shared value anew once nothing is loading any more. Asked again meanwhile,
    /// it still happens once.
    pub fn reload(&mut self) {
        self.reload = true;
    }

    /// Something loads in the background, which the header shows.
    pub fn busy(&self) -> bool {
        !self.loads.is_empty() || self.reload
    }

    /// Something of this machine still loads, which the splash waits for.
    pub fn settling(&self) -> bool {
        self.loads.iter().any(|l| !l.remote) || self.reload
    }

    /// What the oldest job the splash waits for does.
    pub fn doing(&self) -> Option<&'static str> {
        let mut near = self.loads.iter().filter(|l| !l.remote);
        near.find_map(|l| l.op).map(Op::title)
    }

    /// What a row says while its value is still being read: the spinner, in the accent colour,
    /// and not dimmed.
    pub fn loading(&self) -> (String, Tone, bool) {
        (self.style.spinner().into(), Tone::Accent, false)
    }

    /// Writes on the thread of `fetch::serial`, and sends what came of it as a shared value.
    fn store(&mut self, job: impl FnOnce() -> Fresh + Send + 'static) {
        let send = self.begin(None, false);
        fetch::serial(move || send(job()));
    }

    /// Writes the settings as they are now, behind the interface. A failure shows in Settings.
    pub fn save_settings(&mut self) {
        let settings = self.settings.clone();
        self.store(move || Fresh::Saved(settings.save().map_err(|e| e.to_string())));
    }

    pub fn theme(&self) -> &Theme {
        let id = self
            .settings
            .theme
            .as_deref()
            .unwrap_or(crate::theme::DEFAULT);
        self.themes
            .iter()
            .find(|t| t.id == id)
            .or_else(|| self.themes.iter().find(|t| t.id == crate::theme::DEFAULT))
            .unwrap_or(&self.themes[0])
    }

    pub fn features(&self) -> Option<&Features> {
        self.shared.features.ready()
    }

    /// Whether a script can run here, unknown while the packages are still being checked.
    pub fn available(&self, op: script::Op) -> bool {
        self.features().is_some_and(|f| script::available(op, f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beat_falls_on_the_grid_whenever_it_is_asked_for() {
        let start = epoch();
        for offset in [0, 1, 119, 120, 121, 500, 12_345] {
            let now = start + Duration::from_millis(offset);
            let next = next_beat(now, BEAT);
            assert!(next > now);
            assert!(next - now <= BEAT);
            assert_eq!((next - start).as_nanos() % BEAT.as_nanos(), 0, "{offset}");
        }
    }
}
