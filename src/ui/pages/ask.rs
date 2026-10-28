//! What a page asks before it acts: a text, a path of the home folder, or one of several read
//! in the background.

use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::i18n::t;
use crate::script::Failure;
use crate::ui::detail::{Block, failure, show};
use crate::ui::draw::{cut_front, put, waiting, wrap};
use crate::ui::fetch::Task;
use crate::ui::icon;
use crate::ui::input::Input;
use crate::ui::list::{List, Picked, Row};
use crate::ui::{Ctx, Does, Flow, Key, Nav, Page, Tone};

type Then<T> = Box<dyn FnMut(T, &mut Ctx) -> Flow>;
/// Answers as rows whose id is the answer.
type Answers = Result<Vec<Row<String>>, Failure>;
type Read = Box<dyn FnOnce() -> Answers + Send>;

/// A text, with a sentence above it on what it is for and an example in the empty box. Whoever
/// takes it checks it.
pub struct TextPage {
    title: String,
    question: String,
    input: Input,
    then: Then<String>,
}

impl TextPage {
    pub fn new(
        title: &str,
        question: &str,
        example: &str,
        prefill: String,
        then: impl FnMut(String, &mut Ctx) -> Flow + 'static,
    ) -> TextPage {
        let mut input = Input::new(example);
        input.text = prefill;
        TextPage {
            title: title.to_string(),
            question: question.to_string(),
            input,
            then: Box::new(then),
        }
    }
}

impl Page for TextPage {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let s = ctx.style;
        let mut y = area.y;
        for line in wrap(&self.question, area.width as usize) {
            put(buf, area.x, y, &line, area.width, s.tone(Tone::Normal));
            y += 1;
        }
        self.input.draw(buf, area.x, y + 1, area.width, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        vec![("⏎", Does::Use), ("esc", Does::Back)]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match key {
            Key::Enter => (self.then)(self.input.text.trim().to_string(), ctx),
            Key::Esc => Flow::Pop,
            k => {
                self.input.key(k);
                Flow::Stay
            }
        }
    }

    fn typing(&self) -> bool {
        true
    }
}

/// One of several, read in the background once the page is on top, so the answer finds it.
pub struct SelectPage {
    title: String,
    list: List<String>,
    read: Option<Read>,
    task: Option<Task<Answers>>,
    failed: Option<Failure>,
    /// What the page says where there is nothing to choose.
    empty: &'static str,
    then: Then<String>,
}

impl SelectPage {
    pub fn reading(
        title: &str,
        empty: &'static str,
        read: impl FnOnce() -> Answers + Send + 'static,
        then: impl FnMut(String, &mut Ctx) -> Flow + 'static,
    ) -> SelectPage {
        SelectPage {
            title: title.to_string(),
            list: List::filterable(),
            read: Some(Box::new(read)),
            task: None,
            failed: None,
            empty,
            then: Box::new(then),
        }
    }
}

impl Page for SelectPage {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        if let Some(read) = self.read.take() {
            self.task = Some(Task::start(ctx, read));
        }
        if let Some(f) = &self.failed {
            show(failure(f), area, buf, ctx);
        } else if self.task.is_some() {
            waiting(buf, area, t("Loading"), None, None, ctx);
        } else if self.list.rows().is_empty() {
            show(
                vec![Block::Text(self.empty.into(), Tone::Muted)],
                area,
                buf,
                ctx,
            );
        } else {
            self.list.draw(area, buf, ctx);
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        if self.task.is_some() || self.read.is_some() || self.list.rows().is_empty() {
            return vec![("esc", Does::Back)];
        }
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(answer) => (self.then)(answer, ctx),
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        if let Some(result) = self.task.as_mut().and_then(Task::take) {
            self.task = None;
            match result {
                Ok(rows) => self.list.set(rows),
                Err(f) => self.failed = Some(f),
            }
        }
        Flow::Stay
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, _: &Ctx) -> bool {
        self.task.is_some() || self.read.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Up,
    Here,
    Dir(String),
    File(String),
}

/// The folders and the files of a folder, each sorted by name.
type Listing = (Vec<String>, Vec<String>);

/// The home folder, as the session sets it.
pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// A file or folder of the home folder, folder by folder. Links are left out: one is either kept
/// by the dotfiles already or points elsewhere.
pub struct BrowsePage {
    title: String,
    /// The folder shown, or being read.
    dir: PathBuf,
    list: List<Item>,
    /// The row for the cursor once the folder is read.
    focus: Option<Item>,
    /// The folder still to read, started once the page is on top, so its answer finds the page.
    unread: bool,
    task: Option<Task<Listing>>,
    then: Then<PathBuf>,
}

impl BrowsePage {
    pub fn new(title: &str, then: impl FnMut(PathBuf, &mut Ctx) -> Flow + 'static) -> BrowsePage {
        BrowsePage {
            title: title.to_string(),
            dir: home(),
            list: List::filterable(),
            focus: None,
            unread: true,
            task: None,
            then: Box::new(then),
        }
    }

    /// The home folder is as far up as it goes, and is no file of itself.
    fn below_home(&self) -> bool {
        self.dir != home() && self.dir.starts_with(home())
    }

    fn build(&mut self, (dirs, files): Listing) {
        let mut rows = Vec::new();
        if self.below_home() {
            rows.push(Row::new(Item::Up, t("Parent folder")).icon(icon::BACK, Tone::Normal));
            rows.push(
                Row::new(Item::Here, t("Choose this folder")).icon(icon::CHECK, Tone::Normal),
            );
        }
        rows.extend(dirs.into_iter().map(|d| {
            Row::new(Item::Dir(d.clone()), format!("{d}/")).icon(icon::FOLDER, Tone::Normal)
        }));
        rows.extend(
            files
                .into_iter()
                .map(|f| Row::new(Item::File(f.clone()), f).icon(icon::FILE, Tone::Normal)),
        );
        self.list.set(rows);
        if let Some(item) = self.focus.take() {
            self.list.focus(&item);
        }
    }

    /// Shows another folder once it is read. A reading still under way for the folder before is
    /// dropped with its channel, so it cannot show up here.
    fn enter(&mut self, dir: PathBuf, focus: Option<Item>) {
        self.dir = dir;
        self.focus = focus;
        self.list = List::filterable();
        self.unread = true;
        self.task = None;
    }

    /// One folder up, the cursor on the folder it came from.
    fn ascend(&mut self) {
        let Some(parent) = self.dir.parent().map(Path::to_path_buf) else {
            return;
        };
        let from = self
            .dir
            .file_name()
            .map(|n| Item::Dir(n.to_string_lossy().into_owned()));
        self.enter(parent, from);
    }

    fn reading(&self) -> bool {
        self.unread || self.task.is_some()
    }
}

/// What a folder holds, read off the drawing thread: a folder on a stalled network mount must not
/// hold the interface.
fn listing(dir: &Path) -> Listing {
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(kind) = e.file_type() else { continue };
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        if kind.is_dir() {
            dirs.push(name);
        } else if kind.is_file() {
            files.push(name);
        }
    }
    dirs.sort_by_key(|n| n.to_lowercase());
    files.sort_by_key(|n| n.to_lowercase());
    (dirs, files)
}

impl Page for BrowsePage {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        if self.unread {
            self.unread = false;
            let dir = self.dir.clone();
            self.task = Some(Task::start(ctx, move || listing(&dir)));
        }
        let s = ctx.style;
        let path = self.dir.display().to_string();
        let path = cut_front(&path, area.width as usize, s.glyphs.ellipsis);
        put(buf, area.x, area.y, &path, area.width, s.tone(Tone::Muted));
        let below = Rect::new(
            area.x,
            area.y + 2,
            area.width,
            area.height.saturating_sub(2),
        );
        if self.task.is_some() {
            waiting(buf, below, t("Loading"), None, None, ctx);
        } else {
            self.list.draw(below, buf, ctx);
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        if self.reading() {
            return vec![("←", Does::Parent), ("esc", Does::Back)];
        }
        if self.list.typing() {
            return super::list_hints(true);
        }
        vec![
            ("↑↓", Does::Choose),
            ("⏎", Does::Open),
            ("←", Does::Parent),
            ("f", Does::Filter),
            ("esc", Does::Back),
        ]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        let ascends = matches!(key, Key::Left | Key::Backspace | Key::Char('h'));
        if !self.list.typing() && ascends {
            if self.below_home() {
                self.ascend();
            }
            return Flow::Stay;
        }
        // A folder still being read has no rows to take yet. Back and quit go to the app.
        if self.reading() {
            return Flow::Ignored;
        }
        match self.list.key(key) {
            Picked::Open(Item::Up) => {
                self.ascend();
                Flow::Stay
            }
            Picked::Open(Item::Here) => (self.then)(self.dir.clone(), ctx),
            Picked::Open(Item::Dir(name)) => {
                self.enter(self.dir.join(name), None);
                Flow::Stay
            }
            Picked::Open(Item::File(name)) => (self.then)(self.dir.join(name), ctx),
            Picked::Used => Flow::Stay,
            Picked::Ignored if key.nav(false) == Some(Nav::Back) => Flow::Pop,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn done(&mut self, _: &mut Ctx) -> Flow {
        if let Some(listing) = self.task.as_mut().and_then(Task::take) {
            self.task = None;
            self.build(listing);
        }
        Flow::Stay
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, _: &Ctx) -> bool {
        self.reading()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Msg;
    use crate::ui::app::tests::{app, open, press, screen};
    use std::time::{Duration, Instant};

    /// Handles what arrives until the screen shows `text`, for at most a few seconds.
    fn until(
        app: &mut crate::ui::App,
        term: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
        rx: &std::sync::mpsc::Receiver<Msg>,
        text: &str,
    ) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let shown = screen(app, term);
            if shown.contains(text) || Instant::now() > deadline {
                return shown;
            }
            if let Ok(msg) = rx.recv_timeout(Duration::from_millis(20)) {
                app.handle(msg);
            }
        }
    }

    #[test]
    fn a_folder_is_read_behind_the_page_and_back_goes_up_to_the_home_folder() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let (tx, rx) = std::sync::mpsc::sync_channel(crate::ui::QUEUE);
        app.ctx.tx = tx;
        let chosen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let took = chosen.clone();
        open(
            &mut app,
            Box::new(BrowsePage::new("Add", move |path, _| {
                *took.borrow_mut() = Some(path);
                Flow::Pop
            })),
        );
        // Into the first folder, up again onto it, and in once more to choose it.
        let Some(first) = listing(&home()).0.first().cloned() else {
            return;
        };
        let row = format!("{first}/");
        let shown = until(&mut app, &mut term, &rx, &row);
        assert!(
            !shown.contains("Parent folder"),
            "the home folder is the top: {shown}"
        );
        press(&mut app, &[Key::Enter]);
        let shown = until(&mut app, &mut term, &rx, "Choose this folder");
        assert!(shown.contains("Parent folder"), "{shown}");
        press(&mut app, &[Key::Left]);
        let shown = until(&mut app, &mut term, &rx, &row);
        let cursor = app.ctx.style.glyphs.cursor;
        assert!(
            shown
                .lines()
                .any(|l| l.contains(&row) && l.contains(cursor)),
            "back on the folder it came from: {shown}"
        );
        press(&mut app, &[Key::Enter]);
        until(&mut app, &mut term, &rx, "Choose this folder");
        press(&mut app, &[Key::Down, Key::Enter]);
        assert_eq!(
            chosen.borrow().as_deref(),
            Some(home().join(&first).as_path())
        );
    }
}
