//! A script run: asked first where it says so, the password once, then shown as it works.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style as Cell};

use crate::data::cleanup::Part;
use crate::i18n::{t, tf, tn};
use crate::script::{self, Failure, Op, Phase, Prompt, Run, op};
use crate::ui::detail::{Act, Block, Detail, done, failure, show};
use crate::ui::draw::{BAR, card, center, cut, cut_front, put, scrollbar, waiting, width, wrap};
use crate::ui::icon;
use crate::ui::input::Input;
use crate::ui::list::Lead;
use crate::ui::list::{List, Picked, Row};
use crate::ui::target::Target;
use crate::ui::{Ctx, Does, Flow, Key, Nav, Page, Tone};

/// The pause before a result takes keys: what was typed while waiting must not dismiss it.
const SETTLE: Duration = Duration::from_millis(618);
/// Lines a notch of the wheel moves the terminal's history, as terminals scroll.
const NOTCH: isize = 3;

#[derive(Clone)]
pub struct Job {
    pub op: Op,
    pub args: Vec<String>,
    /// Asked before the run, replacing the yaml's confirm: question, then the irreversible effect.
    pub confirm: Option<String>,
    /// The action as the person chose it, which the header names.
    name: Option<String>,
}

impl Job {
    pub fn new(op: Op, args: Vec<String>) -> Job {
        Job {
            op,
            args,
            confirm: None,
            name: None,
        }
    }

    pub fn confirm(mut self, text: String) -> Job {
        self.confirm = Some(text);
        self
    }

    pub fn named(mut self, name: &str) -> Job {
        self.name = Some(name.to_string());
        self
    }

    fn title(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.op.title().to_string())
    }
}

/// The first page of a job: the question, the password, or the run itself.
pub fn start(mut job: Job, ctx: &mut Ctx) -> Box<dyn Page> {
    if job.name.is_none() {
        job.name = ctx.action.clone();
    }
    let text = job
        .confirm
        .clone()
        .or_else(|| job.op.meta().confirm.as_ref().map(|c| t(c).to_string()));
    match text {
        Some(text) => Box::new(ConfirmPage::new(job, text)),
        None => authorize(job, ctx),
    }
}

fn authorize(job: Job, ctx: &Ctx) -> Box<dyn Page> {
    if job.op.meta().root && !ctx.authorized {
        Box::new(PasswordPage::new(job))
    } else {
        Box::new(RunPage::new(job))
    }
}

/// Yes or no before something that cannot be undone. The cursor starts on no.
pub struct ConfirmPage {
    job: Job,
    detail: Detail,
    list: List<bool>,
}

impl ConfirmPage {
    fn new(job: Job, text: String) -> ConfirmPage {
        let mut lines = text.lines();
        let question = lines.next().unwrap_or_default().to_string();
        let mut detail = Detail::default();
        detail.set(
            vec![
                Block::Heading(question),
                Block::Text(lines.collect::<Vec<_>>().join(" "), Tone::Normal),
            ],
            Vec::new(),
        );
        let list = List::new(vec![Row::new(false, t("No")), Row::new(true, t("Yes"))]);
        ConfirmPage { job, detail, list }
    }
}

impl Page for ConfirmPage {
    fn title(&self) -> String {
        self.job.title()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        // The answers right under the question, not at the foot of the page.
        let text = self
            .detail
            .height(area.width, ctx)
            .min(area.height.saturating_sub(3));
        self.detail
            .draw(Rect::new(area.x, area.y, area.width, text), buf, ctx);
        self.list.draw(
            Rect::new(area.x, area.y + text + 1, area.width, 2),
            buf,
            ctx,
        );
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        vec![
            ("↑↓", Does::Choose),
            ("⏎", Does::Answer),
            ("esc", Does::Back),
        ]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(true) => Flow::Replace(authorize(self.job.clone(), ctx)),
            Picked::Open(false) => Flow::Pop,
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }
}

/// Why the typed password was not taken.
fn note(empty: bool, rejected: bool) -> Option<&'static str> {
    if empty {
        Some(t("Password required."))
    } else if rejected {
        Some(t("Wrong password."))
    } else {
        None
    }
}

/// The password in a card, the same before a run and within one. Without a box it is checked.
fn password_card(
    area: Rect,
    buf: &mut Buffer,
    input: Option<&Input>,
    note: Option<&str>,
    ctx: &Ctx,
) {
    let s = ctx.style;
    let (heading, text) = (Prompt::Password.friendly(), t("Enter the sudo password."));
    let indent = if s.glyphs.icons { 3 } else { 0 };
    let widest = width(&heading).max(width(text)) as u16;
    // Within a run the waiting bar lies behind the card, which covers it.
    let w = (widest + indent + 6).max(BAR).min(area.width);
    let room = w.saturating_sub(6 + indent) as usize;
    let lines: Vec<(String, Cell)> = wrap(&heading, room)
        .into_iter()
        .map(|l| (l, s.bold(Tone::Normal)))
        .chain(
            wrap(text, room)
                .into_iter()
                .map(|l| (l, s.tone(Tone::Muted))),
        )
        .collect();
    let inner = card(area, buf, w, lines.len() as u16 + 6, ctx);
    let (x, end) = (inner.x + indent, inner.bottom());
    let room = inner.width.saturating_sub(indent);
    if indent > 0 {
        put(
            buf,
            inner.x,
            inner.y + 1,
            icon::LOCK,
            1,
            s.tone(Tone::Accent),
        );
    }
    let mut y = inner.y + 1;
    for (line, style) in &lines {
        if y < end {
            put(buf, x, y, line, room, *style);
        }
        y += 1;
    }
    y += 1;
    if y >= end {
        return;
    }
    match input {
        Some(input) => input.draw(buf, x, y, room, ctx),
        None => {
            let after = put(buf, x, y, s.spinner(), 2, s.tone(Tone::Accent));
            put(
                buf,
                after + 1,
                y,
                t("Checking"),
                room.saturating_sub(3),
                s.tone(Tone::Muted),
            );
        }
    }
    if let Some(note) = note.filter(|_| y + 1 < end) {
        let note = cut(note, room as usize, s.glyphs.ellipsis);
        put(buf, x, y + 1, &note, room, s.tone(Tone::Error));
    }
}

/// The password, proven by sudo before the run that needs it, once per session. sudo starts only
/// with one to give: a prompt left unanswered counts towards the lock of pam_faillock.
pub struct PasswordPage {
    job: Job,
    input: Input,
    /// Asks sudo without a prompt whether it goes without the password.
    probe: Option<Run>,
    /// sudo needs the password, so it is asked.
    needed: bool,
    auth: Option<Run>,
    failed: Option<Failure>,
    /// ⏎ came with nothing typed.
    empty: bool,
    /// Since when sudo checks the password, and how many letters it had.
    checking: Option<(Instant, usize)>,
}

impl PasswordPage {
    fn new(job: Job) -> PasswordPage {
        PasswordPage {
            job,
            input: Input::masked(t("Password")),
            probe: None,
            needed: false,
            auth: None,
            failed: None,
            empty: false,
            checking: None,
        }
    }

    /// Whether the input is shown: before sudo runs, or while it asks again after a wrong one.
    fn asking(&self) -> bool {
        self.needed
            && self
                .auth
                .as_ref()
                .is_none_or(|r| r.prompt == Some(Prompt::Password))
    }

    fn authorized(&self, ctx: &mut Ctx) -> Flow {
        ctx.authorized = true;
        Flow::Replace(Box::new(RunPage::new(self.job.clone())))
    }
}

impl Page for PasswordPage {
    fn title(&self) -> String {
        self.job.title()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        if !self.needed && self.probe.is_none() && self.failed.is_none() {
            match Run::start(op::SYSTEM_NOPASSWD, &[], (24, 80), ctx.run_sink()) {
                Ok(run) => self.probe = Some(run),
                Err(f) => self.failed = Some(f),
            }
        }
        if let Some(f) = &self.failed {
            show(failure(f), area, buf, ctx);
            return;
        }
        if !self.needed {
            if ctx.page_waited() {
                password_card(area, buf, None, None, ctx);
            }
            return;
        }
        let rejected = self.auth.as_ref().is_some_and(|r| r.rejected);
        let mut typed = Input::masked(t("Password"));
        let input = match self.checking {
            _ if self.asking() => Some(&self.input),
            Some((since, letters)) if !ctx.waited(since) => {
                typed.text = "x".repeat(letters);
                Some(&typed)
            }
            _ => None,
        };
        password_card(area, buf, input, note(self.empty, rejected), ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        vec![("⏎", Does::Answer), ("esc", Does::Back)]
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if self.failed.is_some() {
            return match key.nav(false) {
                Some(Nav::Open) => {
                    self.failed = None;
                    Flow::Stay
                }
                _ => Flow::Ignored,
            };
        }
        match key {
            Key::Enter if !self.asking() => Flow::Stay,
            Key::Enter if self.input.text.is_empty() => {
                self.empty = true;
                Flow::Stay
            }
            Key::Enter => {
                let password = self.input.take();
                self.checking = Some((Instant::now(), password.chars().count()));
                match self.auth.as_mut() {
                    Some(run) => run.answer(&password),
                    None => {
                        // Held for the prompt, which the run answers as it appears.
                        script::sudo::set(password);
                        match Run::start(op::SYSTEM_AUTH, &[], (24, 80), ctx.run_sink()) {
                            Ok(run) => self.auth = Some(run),
                            Err(f) => {
                                script::sudo::forget();
                                self.failed = Some(f);
                            }
                        }
                    }
                }
                Flow::Stay
            }
            Key::Esc | Key::Ctrl('c') => {
                for run in self.probe.iter().chain(&self.auth) {
                    run.interrupt();
                }
                if key == Key::Esc {
                    Flow::Pop
                } else {
                    Flow::Quit
                }
            }
            k => {
                if self.input.key(k) {
                    self.empty = false;
                }
                Flow::Stay
            }
        }
    }

    fn run(&mut self, event: script::Event, _ctx: &mut Ctx) -> Flow {
        if let Some(run) = self.auth.as_mut().or(self.probe.as_mut()) {
            run.on_event(event);
        }
        Flow::Stay
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Flow {
        if let Some(probe) = &self.probe {
            match probe.result() {
                Some(Ok(())) => return self.authorized(ctx),
                Some(Err(f)) => {
                    self.probe = None;
                    if f.code == Some(10) {
                        self.needed = true;
                    } else {
                        self.failed = Some(f);
                    }
                }
                None => {}
            }
            return Flow::Stay;
        }
        let Some(run) = self.auth.as_mut() else {
            return Flow::Stay;
        };
        run.check_prompt();
        match run.result() {
            Some(Ok(())) => self.authorized(ctx),
            Some(Err(f)) => {
                self.auth = None;
                self.failed = Some(f);
                Flow::Stay
            }
            None => Flow::Stay,
        }
    }

    fn typing(&self) -> bool {
        true
    }

    fn animating(&self, _: &Ctx) -> bool {
        true
    }
}

/// The outcome in a word. The header already names the job, only a recognised cause adds to it.
fn headline(outcome: &Result<(), Failure>) -> String {
    match outcome {
        Ok(()) => t("Done").into(),
        Err(f) if f.known.is_some() => f.headline(),
        Err(_) => t("Failed").into(),
    }
}

pub struct RunPage {
    job: Job,
    run: Option<Run>,
    outcome: Option<(Result<(), Failure>, Instant)>,
    /// The terminal view: chosen with tab, or open while the run needs the person there.
    details: bool,
    /// The run needs the person on the terminal: it handed over, or a question in unknown words
    /// waits.
    needed: bool,
    /// The view chosen before that need, taken back after it.
    before: bool,
    choices: List<String>,
    input: Input,
    /// ⏎ came at the password prompt with nothing typed.
    empty: bool,
    /// The question whether to stop the run, while it is asked.
    cancel: Option<List<bool>>,
    size: (u16, u16),
    /// The outcome and what to do next.
    result: Detail,
}

impl RunPage {
    pub fn new(job: Job) -> RunPage {
        RunPage {
            job,
            run: None,
            outcome: None,
            details: false,
            needed: false,
            before: false,
            choices: List::default(),
            input: Input::new(""),
            empty: false,
            cancel: None,
            size: (0, 0),
            result: Detail::default(),
        }
    }

    /// Under way, for the screenshots.
    #[cfg(test)]
    pub fn running(job: Job, run: Run) -> RunPage {
        let mut page = RunPage::new(job);
        page.run = Some(run);
        page
    }

    /// Already ended, for the screenshots.
    #[cfg(test)]
    pub fn finished(job: Job, outcome: Result<(), Failure>) -> RunPage {
        let mut page = RunPage::new(job);
        page.outcome = Some((outcome, Instant::now() - SETTLE));
        page
    }

    fn settled(&self) -> bool {
        self.outcome
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= SETTLE)
    }

    fn prompt(&self) -> Option<&Prompt> {
        self.run
            .as_ref()
            .filter(|_| self.outcome.is_none())
            .and_then(|r| r.prompt.as_ref())
    }

    /// Opens the terminal while the run needs the person there, and returns to the view before.
    fn follow(&mut self) {
        let needed = self.run.as_ref().is_some_and(|r| r.handed || r.waiting());
        if needed == self.needed {
            return;
        }
        self.needed = needed;
        if needed {
            self.before = self.details;
            self.details = true;
        } else {
            self.details = self.before;
        }
    }

    /// Fills the answer list when a new question appears.
    fn prepare(&mut self) {
        let rows = match self.prompt().cloned() {
            Some(Prompt::YesNo { yes, .. }) => {
                let rows = vec![
                    Row::new("y".to_string(), t("Yes")),
                    Row::new("n".to_string(), t("No")),
                ];
                let default = String::from(if yes { "y" } else { "n" });
                if self.choices.rows() != rows.as_slice() {
                    self.choices = List::new(rows);
                    self.choices.focus(&default);
                }
                return;
            }
            Some(Prompt::Choice {
                options, default, ..
            }) => {
                let rows: Vec<Row<String>> = options
                    .iter()
                    .map(|(n, label)| Row::new(n.clone(), label.clone()))
                    .collect();
                if self.choices.rows() != rows.as_slice() {
                    self.choices = List::new(rows);
                    self.choices.focus(&default);
                }
                return;
            }
            _ => Vec::new(),
        };
        self.choices = List::new(rows);
    }

    fn draw_progress(&self, area: Rect, buf: &mut Buffer, ctx: &Ctx) {
        let s = &ctx.style;
        let run = self.run.as_ref();
        let progress = run.map(|r| r.progress.clone()).unwrap_or_default();
        let below = waiting(
            buf,
            area,
            &self.job.title(),
            run.and_then(|r| r.step.as_deref()),
            progress.fraction(),
            ctx,
        );
        let phase = progress.phase.map(|p| match p {
            Phase::Sync => t("Syncing the package databases"),
            Phase::Download => t("Downloading"),
            Phase::Verify => t("Checking the packages"),
            Phase::Install => t("Installing"),
            Phase::Remove => t("Removing"),
            Phase::Hooks => t("Finishing"),
            Phase::Build => t("Building"),
        });
        if let Some(phase) = phase {
            let text = if progress.total > 0 {
                let through = tf(
                    "{done} of {total} done",
                    &[("done", &progress.done), ("total", &progress.total)],
                );
                // Short of room the count stays, as a progress bar keeps its end.
                let text = format!("{phase} {} {through}", s.glyphs.separator);
                cut_front(&text, area.width as usize, s.glyphs.ellipsis)
            } else {
                phase.to_string()
            };
            center(
                buf,
                area,
                below,
                &text,
                s.tone(Tone::Muted),
                s.glyphs.ellipsis,
            );
            center(
                buf,
                area,
                below + 1,
                &progress.item,
                s.tone(Tone::Muted),
                s.glyphs.ellipsis,
            );
        }
    }

    fn draw_prompt(&mut self, area: Rect, buf: &mut Buffer, ctx: &Ctx) {
        let Some(prompt) = self.prompt().cloned() else {
            return;
        };
        self.input.masked = prompt == Prompt::Password;
        self.input.placeholder = prompt.placeholder().to_string();
        if prompt == Prompt::Password {
            let rejected = self.run.as_ref().is_some_and(|r| r.rejected);
            password_card(
                area,
                buf,
                Some(&self.input),
                note(self.empty, rejected),
                ctx,
            );
            return;
        }
        let s = ctx.style;
        let w = area.width.min(68);
        let question = wrap(&prompt.friendly(), w.saturating_sub(6) as usize);
        let answers = match &prompt {
            Prompt::YesNo { .. } | Prompt::Choice { .. } => self.choices.rows().len() as u16,
            _ => 1,
        };
        let inner = card(area, buf, w, question.len() as u16 + answers + 5, ctx);
        // Short of room the question keeps a line for the answers, which scroll.
        let room = inner.height.saturating_sub(3);
        let asked = (question.len() as u16).min(room.saturating_sub(answers.min(room / 2).max(1)));
        let mut y = inner.y + 1;
        for line in question.iter().take(asked as usize) {
            put(buf, inner.x, y, line, inner.width, s.bold(Tone::Normal));
            y += 1;
        }
        y += 1;
        match &prompt {
            Prompt::YesNo { .. } | Prompt::Choice { .. } => {
                self.choices.draw(
                    Rect::new(
                        inner.x + 2,
                        y,
                        inner.width.saturating_sub(2),
                        answers.min(inner.bottom().saturating_sub(y)),
                    ),
                    buf,
                    ctx,
                );
            }
            Prompt::Password | Prompt::Text { .. } => {
                self.input.draw(buf, inner.x, y, inner.width, ctx);
            }
        }
    }

    fn draw_details(&mut self, area: Rect, buf: &mut Buffer, ctx: &Ctx) {
        let Some(run) = self.run.as_mut() else { return };
        if self.size != (area.height, area.width) {
            self.size = (area.height, area.width);
            run.resize(area.height, area.width);
        }
        let (back, reach) = run.history();
        let s = &ctx.style;
        let screen = run.screen();
        let (rows, cols) = screen.size();
        for r in 0..rows.min(area.height) {
            for c in 0..cols.min(area.width) {
                let Some(cell) = screen.cell(r, c) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let fg = match cell.fgcolor() {
                    vt100::Color::Idx(1 | 9) => s.error,
                    vt100::Color::Idx(2 | 10) => s.success,
                    vt100::Color::Idx(3 | 11) => s.warning,
                    vt100::Color::Idx(4 | 5 | 6 | 12 | 13 | 14) => s.accent,
                    vt100::Color::Idx(0 | 8) => s.muted,
                    _ => s.text,
                };
                let mut style = s.base().fg(fg);
                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.inverse() {
                    style = style.fg(s.background).bg(fg);
                }
                let text = cell.contents();
                buf[(area.x + c, area.y + r)]
                    .set_symbol(if text.is_empty() { " " } else { text })
                    .set_style(style);
            }
        }
        let rows = usize::from(rows);
        scrollbar(buf, area, rows + reach, reach - back, ctx);
        if run.running() && !screen.hide_cursor() && back == 0 {
            let (r, c) = screen.cursor_position();
            if r < area.height && c < area.width {
                buf[(area.x + c, area.y + r)].set_bg(if s.accent == Color::Reset {
                    Color::Gray
                } else {
                    s.accent
                });
            }
        }
    }

    /// The outcome, then what may come next: each a choice, none a must.
    fn draw_outcome(&mut self, area: Rect, buf: &mut Buffer, ctx: &Ctx) {
        let s = &ctx.style;
        let Some((outcome, _)) = &self.outcome else {
            return;
        };
        let (body, acts) = match outcome {
            // The machine goes down: nothing comes next here.
            Ok(()) if self.job.op == op::SYSTEM_REBOOT => {
                let line = format!("{}  {}", s.spinner(), t("Restarting"));
                (
                    vec![Block::Row(
                        Lead::Icon(icon::RESTART, Tone::Accent),
                        vec![(line, Tone::Normal)],
                        Vec::new(),
                    )],
                    Vec::new(),
                )
            }
            Ok(()) => {
                let mut body = vec![done(headline(outcome))];
                // The check found what the person may reinstall.
                let altered = self
                    .run
                    .as_ref()
                    .filter(|_| self.job.op == op::REPAIR_FILES)
                    .map(|r| r.found.clone())
                    .unwrap_or_default();
                if !altered.is_empty() {
                    body.push(Block::Blank);
                    body.push(Block::Text(
                        tn(
                            "{n} package with altered files: {names}.",
                            "{n} packages with altered files: {names}.",
                            altered.len(),
                            &[("names", &altered.join(", "))],
                        ),
                        Tone::Normal,
                    ));
                }
                if ctx.busy() {
                    if ctx.busy_shown() {
                        body.push(Block::Blank);
                        body.push(Block::Text(
                            format!("{}  {}", s.spinner(), t("Reading the system")),
                            Tone::Muted,
                        ));
                    }
                    (body, Vec::new())
                } else {
                    let mut acts = Vec::new();
                    if !altered.is_empty() {
                        acts.push(reinstall(altered));
                    }
                    acts.extend(next_steps(self.job.op, ctx));
                    acts.push(
                        Act::new(t("Back"), |_| Flow::Pop)
                            .icon(icon::BACK)
                            .does(Does::Back),
                    );
                    (body, acts)
                }
            }
            Err(f) => {
                let mut body = vec![Block::Row(
                    Lead::Icon(icon::FAILED, Tone::Error),
                    vec![(headline(outcome), Tone::Normal)],
                    Vec::new(),
                )];
                body.extend(failure(f).into_iter().skip(1));
                let mut acts = Vec::new();
                if let Some(target) = f.offer.and_then(Target::from_id) {
                    acts.push(
                        Act::new(target.label(), move |ctx: &mut Ctx| {
                            replace_with(target, ctx)
                        })
                        .icon(target.icon())
                        .does(Does::Open),
                    );
                }
                let job = self.job.clone();
                acts.push(
                    Act::new(t("Try again"), move |ctx: &mut Ctx| {
                        Flow::Replace(start(job.clone(), ctx))
                    })
                    .icon(icon::UPDATE),
                );
                acts.push(
                    Act::new(t("Back"), |_| Flow::Pop)
                        .icon(icon::BACK)
                        .does(Does::Back),
                );
                (body, acts)
            }
        };
        self.result.set(body, acts);
        // Over the terminal it lies on a card, the last output readable around it.
        let area = if self.details || ctx.settings.terminal() {
            let w = area.width.min(68);
            let h = self.result.height(w.saturating_sub(6), ctx) + 2;
            card(area, buf, w, h, ctx)
        } else {
            area
        };
        self.result.draw(area, buf, ctx);
    }

    fn answer(&mut self, text: &str) {
        if let Some(run) = self.run.as_mut() {
            run.answer(text);
        }
        self.input.text.clear();
    }
}

impl Page for RunPage {
    fn title(&self) -> String {
        self.job.title()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        if self.run.is_none() && self.outcome.is_none() {
            self.size = (area.height, area.width);
            match Run::start(self.job.op, &self.job.args, self.size, ctx.run_sink()) {
                Ok(run) => self.run = Some(run),
                Err(f) => self.outcome = Some((Err(f), Instant::now())),
            }
        }
        let terminal = ctx.settings.terminal();
        if (self.details || terminal) && self.run.is_some() {
            self.draw_details(area, buf, ctx);
            if self.outcome.is_some() {
                self.draw_outcome(area, buf, ctx);
            }
        } else if self.outcome.is_some() {
            self.draw_outcome(area, buf, ctx);
        } else if self.prompt().is_some() {
            self.draw_progress(area, buf, ctx);
            self.draw_prompt(area, buf, ctx);
        } else {
            self.draw_progress(area, buf, ctx);
        }
        if let Some(list) = self.cancel.as_mut() {
            let s = ctx.style;
            let inner = card(area, buf, area.width.min(60), 8, ctx);
            let line = |text| cut(text, inner.width as usize, s.glyphs.ellipsis);
            put(
                buf,
                inner.x,
                inner.y + 1,
                &line(t("Stop the operation?")),
                inner.width,
                s.bold(Tone::Normal),
            );
            put(
                buf,
                inner.x,
                inner.y + 2,
                &line(t("Sends Ctrl+C.")),
                inner.width,
                s.tone(Tone::Muted),
            );
            list.draw(
                Rect::new(inner.x + 2, inner.y + 4, inner.width.saturating_sub(2), 2),
                buf,
                ctx,
            );
        }
    }

    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, Does)> {
        let details = if self.details {
            Does::Compact
        } else {
            Does::Details
        };
        // Verbose output has no compact view to switch to.
        let terminal = ctx.settings.terminal();
        if self.cancel.is_some() {
            return vec![("↑↓", Does::Choose), ("⏎", Does::Answer)];
        }
        if self.outcome.is_some() {
            // The keys of what is offered, none while the system is read again.
            return if self.settled() {
                let mut h = self.result.hints();
                if !terminal {
                    h.insert(h.len() - 1, ("tab", details));
                }
                h
            } else {
                Vec::new()
            };
        }
        if terminal {
            return vec![("ctrl+c", Does::Stop)];
        }
        if self.details {
            return vec![("tab", details), ("ctrl+c", Does::Stop)];
        }
        match self.prompt() {
            Some(Prompt::Password | Prompt::Text { .. }) => {
                vec![("⏎", Does::Answer), ("tab", details), ("esc", Does::Stop)]
            }
            Some(_) => vec![("↑↓", Does::Choose), ("⏎", Does::Answer), ("tab", details)],
            None => vec![("tab", details), ("esc", Does::Stop)],
        }
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if let Some(list) = self.cancel.as_mut() {
            match list.key(key) {
                Picked::Open(yes) => {
                    if yes && let Some(run) = &self.run {
                        run.interrupt();
                    }
                    self.cancel = None;
                }
                Picked::Ignored if key.nav(false) == Some(Nav::Back) => self.cancel = None,
                _ => {}
            }
            return Flow::Stay;
        }
        let terminal = ctx.settings.terminal();
        if key == Key::Tab && !terminal {
            self.details = !self.details;
            return Flow::Stay;
        }
        if self.outcome.is_some() {
            if !self.settled() {
                return Flow::Stay;
            }
            return match self.result.key(key, ctx) {
                Flow::Ignored if key.nav(false) == Some(Nav::Back) => Flow::Pop,
                Flow::Ignored if key.nav(false) == Some(Nav::Quit) => Flow::Quit,
                flow => flow,
            };
        }
        let details = self.details || terminal;
        let typing = matches!(self.prompt(), Some(Prompt::Password | Prompt::Text { .. }));
        let asks_to_stop = key == Key::Ctrl('c')
            || (!details && (key == Key::Esc || (key == Key::Char('q') && !typing)));
        if asks_to_stop {
            self.cancel = Some(List::new(vec![
                Row::new(false, t("No")),
                Row::new(true, t("Yes")),
            ]));
            return Flow::Stay;
        }
        if details {
            if let Some(run) = self.run.as_mut() {
                let mode = run.screen().application_cursor();
                match key {
                    // A program on the whole screen keeps no history: the wheel moves in it, as
                    // terminals do.
                    Key::WheelUp | Key::WheelDown if run.screen().alternate_screen() => {
                        let arrow = if key == Key::WheelUp {
                            Key::Up
                        } else {
                            Key::Down
                        };
                        run.type_bytes(&arrow.bytes(mode));
                    }
                    Key::WheelUp => run.scroll(NOTCH),
                    Key::WheelDown => run.scroll(-NOTCH),
                    _ => run.type_bytes(&key.bytes(mode)),
                }
            }
            return Flow::Stay;
        }
        match self.prompt().cloned() {
            Some(prompt @ (Prompt::Password | Prompt::Text { .. })) => {
                if key != Key::Enter {
                    if self.input.key(key) {
                        self.empty = false;
                    }
                } else if prompt == Prompt::Password && self.input.text.is_empty() {
                    self.empty = true;
                } else {
                    let text = self.input.text.clone();
                    self.answer(&text);
                }
            }
            Some(_) => {
                if let Picked::Open(id) = self.choices.key(key) {
                    self.answer(&id);
                }
            }
            None => {}
        }
        Flow::Stay
    }

    fn run(&mut self, event: script::Event, _ctx: &mut Ctx) -> Flow {
        if let Some(run) = self.run.as_mut() {
            run.on_event(event);
        }
        self.follow();
        self.prepare();
        Flow::Stay
    }

    /// In the terminal pasted text goes to the program as a whole. Under the question to stop it
    /// goes nowhere, as it answers neither.
    fn paste(&mut self, text: &str, ctx: &mut Ctx) -> bool {
        if self.cancel.is_some() {
            return true;
        }
        let terminal = self.details || ctx.settings.terminal();
        match self.run.as_mut() {
            Some(run) if terminal && self.outcome.is_none() => {
                run.paste(text);
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Flow {
        if self.outcome.is_some() {
            return Flow::Stay;
        }
        let Some(run) = self.run.as_mut() else {
            return Flow::Stay;
        };
        run.check_prompt();
        if let Some(result) = run.result() {
            match &result {
                Ok(()) => crate::info!("{}: done", self.job.op),
                Err(f) => crate::error!("{}: {}\n{}", self.job.op, f.headline(), f.detail),
            }
            // Under tab the terminal tells as well that the run ended.
            let lead = match (ctx.style.glyphs.icons, &result) {
                (false, _) => String::new(),
                (true, Ok(())) => format!("{}  ", icon::DONE),
                (true, Err(_)) => format!("{}  ", icon::FAILED),
            };
            run.conclude(&format!("{lead}{}", headline(&result)), result.is_ok());
            self.outcome = Some((result, Instant::now()));
            self.cancel = None;
            ctx.reload();
        }
        self.follow();
        self.prepare();
        Flow::Stay
    }

    fn typing(&self) -> bool {
        self.outcome.is_none()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        self.outcome.is_none() || !self.settled() || ctx.busy() || self.job.op == op::SYSTEM_REBOOT
    }

    /// Done, the page the action was chosen on has nothing left to show. After a failure it
    /// stays for another try.
    fn closes_below(&self) -> bool {
        matches!(self.outcome, Some((Ok(()), _)))
    }
}

/// Leaves the finished run for a page as back leaves it, or opens the pages of a setting above it.
fn replace_with(target: Target, ctx: &mut Ctx) -> Flow {
    let mut pages = target.open(ctx);
    match pages.len() {
        0 => Flow::Pop,
        1 => Flow::Replace(pages.remove(0)),
        _ => Flow::PushAll(pages),
    }
}

/// The packages a check found altered, back as installed, once the person agrees.
fn reinstall(packages: Vec<String>) -> Act {
    let text = format!(
        "{}\n{}",
        tn(
            "Reinstall {n} package?",
            "Reinstall {n} packages?",
            packages.len(),
            &[]
        ),
        t("Changes to their files are lost.")
    );
    Act::new(t("Reinstall"), move |ctx: &mut Ctx| {
        Flow::Replace(start(
            Job::new(op::REPAIR_REINSTALL, packages.clone()).confirm(text.clone()),
            ctx,
        ))
    })
    .icon(icon::REPAIR)
}

/// What the system asks for after a run, read from it anew: a restart, configuration updates,
/// failed services, a clean-up worth doing, further updates. What the run just did is left out.
fn next_steps(done: Op, ctx: &Ctx) -> Vec<Act> {
    let s = &ctx.shared;
    let mut acts = Vec::new();
    let mut step = |target: Target, note: String, tone: Tone| {
        acts.push(
            Act::new(target.label(), move |ctx: &mut Ctx| {
                replace_with(target, ctx)
            })
            .icon(target.icon())
            .note(note, tone)
            .does(Does::Open),
        );
    };
    if s.system.ready().is_some_and(|x| x.reboot) {
        step(Target::Reboot, t("new kernel").into(), Tone::Warn);
    }
    if let Some(files) = s.pacnew.ready().filter(|f| !f.is_empty()) {
        step(
            Target::Pacnew,
            tf("{n} open", &[("n", &files.len())]),
            Tone::Warn,
        );
    }
    if let Some(n) = s
        .services
        .ready()
        .map(|all| all.iter().filter(|x| x.failed()).count())
        .filter(|n| *n > 0)
    {
        step(
            Target::Services,
            tf("{n} failed", &[("n", &n)]),
            Tone::Error,
        );
    }
    if done != op::CLEANUP_RUN
        && let Some(plan) = s.cleanup.ready()
        && let frees = super::cleanup::frees(plan, ctx)
        && (frees >= CLEANUP_WORTH
            || ctx.settings.cleans(Part::Orphans) && !plan.orphans.is_empty())
    {
        step(
            Target::Cleanup,
            crate::data::format_size(frees),
            Tone::Muted,
        );
    }
    if done != op::UPGRADE_RUN
        && let Some(list) = s
            .updates
            .ready()
            .map(|p| &p.updates)
            .filter(|u| !u.is_empty())
    {
        step(
            Target::Upgrade,
            tn("{n} update", "{n} updates", list.len(), &[]),
            Tone::Accent,
        );
    }
    acts
}

/// Below this the cache is not worth a step of its own.
const CLEANUP_WORTH: u64 = 1024 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::sudo;
    use crate::ui::Msg;
    use crate::ui::app::tests::{app, open, press, screen, tell};

    #[test]
    fn an_empty_password_starts_no_sudo_and_says_why() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = app();
        let mut page = PasswordPage::new(Job::new(op::SYSTEM_REBOOT, Vec::new()));
        page.needed = true;
        page.key(Key::Enter, &mut app.ctx);
        assert!(page.auth.is_none() && page.failed.is_none() && page.empty);
        page.key(Key::Char('x'), &mut app.ctx);
        assert!(!page.empty, "typing clears the note");
        page.key(Key::Backspace, &mut app.ctx);
        open(&mut app, Box::new(page));
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Password required."), "{shown}");
    }

    #[test]
    fn the_password_reads_the_same_before_a_run_and_within_it() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let card = |page: Box<dyn Page>| {
            let (mut app, mut term) = app();
            open(&mut app, page);
            let shown = screen(&mut app, &mut term);
            let lines: Vec<String> = shown
                .lines()
                .skip_while(|l| !l.contains('╭'))
                .take_while(|l| !l.contains('╰'))
                .map(String::from)
                .collect();
            assert!(
                lines.iter().any(|l| l.contains("Authentication required")),
                "{shown}"
            );
            lines
        };
        let mut before = PasswordPage::new(Job::new(op::UPGRADE_RUN, Vec::new()));
        before.needed = true;
        let run = Run::fed(op::UPGRADE_RUN, "repo", sudo::PROMPT);
        let within = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
        assert_eq!(card(Box::new(before)), card(Box::new(within)));
    }

    /// sudo is asked whether it needs the password, and then whether it takes it: each wait shows
    /// only once it lasts, the typed letters stay meanwhile.
    #[test]
    fn the_password_card_waits_without_flashing() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let long_ago = Instant::now() - Duration::from_secs(1);
        let (mut probing, mut term) = app();
        probing.ctx.grace = Duration::from_millis(300);
        let mut page = PasswordPage::new(Job::new(op::UPGRADE_RUN, Vec::new()));
        page.probe = Some(Run::fed(op::SYSTEM_NOPASSWD, "", ""));
        open(&mut probing, Box::new(page));
        assert!(!screen(&mut probing, &mut term).contains("Authentication required"));
        probing.ctx.shown.1 = long_ago;
        assert!(screen(&mut probing, &mut term).contains("Checking"));

        let (mut app, mut term) = app();
        app.ctx.grace = Duration::from_millis(300);
        let mut page = PasswordPage::new(Job::new(op::UPGRADE_RUN, Vec::new()));
        page.needed = true;
        page.auth = Some(Run::fed(op::SYSTEM_AUTH, "", ""));
        page.checking = Some((Instant::now(), 4));
        open(&mut app, Box::new(page));
        let mask = app.ctx.style.glyphs.mask.repeat(4);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains(&mask) && !shown.contains("Checking"),
            "{shown}"
        );
    }

    #[test]
    fn backspace_in_the_password_box_never_leaves_it() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut page = PasswordPage::new(Job::new(op::SYSTEM_REBOOT, Vec::new()));
        page.needed = true;
        open(&mut app, Box::new(page));
        press(&mut app, &[Key::Char('x'), Key::Backspace, Key::Backspace]);
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Password"), "{shown}");
    }

    #[test]
    fn the_password_box_names_what_it_takes_until_something_is_typed() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut page = PasswordPage::new(Job::new(op::SYSTEM_REBOOT, Vec::new()));
        page.needed = true;
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        let (y, line) = shown
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("Password"))
            .unwrap_or_else(|| panic!("{shown}"));
        let x = crate::ui::draw::width(&line[..line.find("Password").unwrap()]);
        let cell = &term.backend().buffer()[(x as u16, y as u16)];
        assert_eq!(cell.fg, app.ctx.style.muted, "dimmed: {shown}");
        press(&mut app, &[Key::Char('x')]);
        let shown = screen(&mut app, &mut term);
        assert!(
            !shown.contains("Password") && shown.contains(app.ctx.style.glyphs.mask),
            "{shown}"
        );
    }

    #[test]
    fn ctrl_c_and_q_quit_once_nothing_runs() {
        let (mut after, _) = app();
        open(
            &mut after,
            Box::new(RunPage::finished(
                Job::new(op::UPGRADE_RUN, Vec::new()),
                Ok(()),
            )),
        );
        press(&mut after, &[Key::Char('q')]);
        assert!(after.done(), "q after a run");
        let (mut asking, _) = app();
        let ask = || Box::new(PasswordPage::new(Job::new(op::SYSTEM_REBOOT, Vec::new())));
        open(&mut asking, ask());
        press(&mut asking, &[Key::Esc]);
        assert!(!asking.done(), "esc goes back");
        open(&mut asking, ask());
        press(&mut asking, &[Key::Ctrl('c')]);
        assert!(asking.done(), "ctrl+c on the password");
        let (mut starting, _) = app();
        let splash = crate::ui::pages::splash::SplashPage::new(&starting.ctx);
        open(&mut starting, Box::new(splash));
        press(&mut starting, &[Key::Ctrl('c')]);
        assert!(starting.done(), "ctrl+c on the start screen");
    }

    /// Handed over, as for the AUR helper's review, the run shows its terminal until the script
    /// takes it back, then the view chosen before.
    #[test]
    fn a_hand_over_shows_the_terminal_while_it_lasts() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let output = "Aur (1) paru-2.2.0-1\r\n";
        let shows = |handed: &[&str], tab: bool| {
            let (mut app, mut term) = crate::ui::app::tests::app();
            let run = Run::fed(op::UPGRADE_RUN, "aur", output);
            let mut page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
            page.details = tab;
            for state in handed {
                page.run(
                    script::Event::Script(format!("terminal\t{state}")),
                    &mut app.ctx,
                );
            }
            open(&mut app, Box::new(page));
            screen(&mut app, &mut term).contains(output.trim_end())
        };
        assert!(!shows(&[], false));
        assert!(shows(&["on"], false));
        assert!(!shows(&["on", "off"], false), "back to the compact view");
        assert!(
            shows(&["on", "off"], true),
            "back to the details chosen before"
        );
        let run = Run::fed(op::UPGRADE_RUN, "aur", output);
        let mut page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
        page.run(script::Event::Script("terminal\ton".into()), &mut app.ctx);
        open(&mut app, Box::new(page));
        press(&mut app, &[Key::Tab]);
        let shown = screen(&mut app, &mut term);
        assert!(
            !shown.contains(output.trim_end()),
            "tab still leads out: {shown}"
        );
    }

    /// A question in words not known here opens the terminal, where it is answered, and the
    /// run goes on in the compact view.
    #[test]
    fn a_question_in_unknown_words_opens_the_terminal_until_answered() {
        let (mut app, mut term) = app();
        let asked = "Name of the key: ";
        let mut run = Run::fed(op::UPGRADE_RUN, "aur", asked);
        run.check_at(Instant::now() + Duration::from_secs(5));
        let mut page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
        page.tick(&mut app.ctx);
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains(asked.trim_end()), "{shown}");
        tell(&mut app, script::Event::Output(b"key\r\n".to_vec()));
        let shown = screen(&mut app, &mut term);
        assert!(!shown.contains(asked.trim_end()), "{shown}");
    }

    /// Ended while its terminal shows, the run lays its outcome over the output, the keys the
    /// footer names act on what is seen.
    #[test]
    fn the_outcome_lies_over_the_terminal_where_it_shows() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut page = RunPage::finished(Job::new(op::UPGRADE_RUN, Vec::new()), Ok(()));
        page.run = Some(Run::ended(op::UPGRADE_RUN, "upgrading firefox\r\n", 0));
        page.details = true;
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("upgrading firefox") && shown.contains("Done") && shown.contains("Back"),
            "{shown}"
        );
    }

    #[test]
    fn the_wheel_scrolls_the_terminal_back_and_a_key_returns() {
        let (mut app, mut term) = app();
        let lines: String = (0..100).map(|i| format!("line {i}\r\n")).collect();
        let run = Run::fed(op::UPGRADE_RUN, "aur", &lines);
        let mut page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
        page.details = true;
        open(&mut app, Box::new(page));
        assert!(screen(&mut app, &mut term).contains("line 99"));
        press(&mut app, &[Key::WheelUp; 10]);
        let shown = screen(&mut app, &mut term);
        assert!(
            !shown.contains("line 99") && shown.contains("line 60"),
            "{shown}"
        );
        press(&mut app, &[Key::Char('q')]);
        assert!(screen(&mut app, &mut term).contains("line 99"));
    }

    /// The check names what it found, a single package too, and its reinstall asks first.
    #[test]
    fn a_check_offers_to_reinstall_what_it_found() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut page = RunPage::finished(Job::new(op::REPAIR_FILES, Vec::new()), Ok(()));
        let mut run = Run::ended(
            op::REPAIR_FILES,
            "warning: bar: /usr/bin/bar (Size mismatch)\r\n",
            0,
        );
        run.on_event(script::Event::Script("found\tbar".into()));
        page.run = Some(run);
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("1 package with altered files: bar."),
            "{shown}"
        );
        assert!(shown.contains("Reinstall"), "{shown}");
        press(&mut app, &[Key::Enter]);
        let asked = screen(&mut app, &mut term);
        assert!(asked.contains("Reinstall 1 package?"), "{asked}");
        assert!(
            asked.contains("Changes to their files are lost."),
            "{asked}"
        );
    }

    #[test]
    fn while_the_system_is_read_again_the_keys_offer_nothing_to_choose() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let _ = app.ctx.begin(None, false);
        open(
            &mut app,
            Box::new(RunPage::finished(
                Job::new(op::UPGRADE_RUN, Vec::new()),
                Ok(()),
            )),
        );
        let shown = screen(&mut app, &mut term);
        let footer = shown.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
        assert!(
            footer.contains("tab details") && !footer.contains("choose"),
            "{footer}"
        );
    }

    /// With verbose output the run shows as it is from the start, its prompt typed rather than
    /// chosen, tab changes nothing, and the outcome lies on a card over the last output.
    #[test]
    fn in_the_terminal_the_run_shows_as_it_is_and_the_outcome_lies_over_it() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        app.ctx.settings.output = Some("verbose".into());
        let asked = ":: Proceed with installation? [Y/n] ";
        let footer = |shown: &str| {
            shown
                .lines()
                .rfind(|l| !l.trim().is_empty())
                .unwrap_or("")
                .to_string()
        };
        let run = Run::fed(op::UPGRADE_RUN, "repo", asked);
        let mut page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
        page.prepare();
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains(asked.trim_end()) && !shown.contains("Yes"),
            "{shown}"
        );
        let keys = footer(&shown);
        assert!(
            keys.contains("ctrl+c stop") && !keys.contains("tab"),
            "{keys}"
        );
        press(&mut app, &[Key::Tab]);
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains(asked.trim_end()), "{shown}");
        let run = Run::fed(
            op::UPGRADE_RUN,
            "repo",
            " \x1b[90m$\x1b[0m sudo pacman -Syu\r\nupgrading firefox\r\n",
        );
        let mut page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
        page.outcome = Some((Ok(()), Instant::now() - SETTLE));
        open(&mut app, Box::new(page));
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains(" $ sudo pacman -Syu")
                && shown.contains("upgrading firefox")
                && shown.contains("Done"),
            "{shown}"
        );
        assert!(!footer(&shown).contains("tab"), "{shown}");
    }

    #[test]
    fn many_answers_scroll_inside_the_card() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, _) = app();
        let providers: String = (1..=14).map(|n| format!("{n}) provider-{n}  ")).collect();
        let asked = format!(
            ":: There are 14 providers available for x:\r\n:: Repository extra\r\n   {providers}\r\n\r\nEnter a number (default=1): "
        );
        let run = Run::fed(op::UPGRADE_RUN, "repo", &asked);
        let mut page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
        page.prepare();
        open(&mut app, Box::new(page));
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
        screen(&mut app, &mut term);
        press(&mut app, &[Key::Down; 13]);
        let shown = screen(&mut app, &mut term);
        let footer = shown.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
        assert!(footer.contains("choose"), "{shown}");
        assert!(shown.contains("provider-14"), "{shown}");
    }

    #[test]
    fn the_count_under_the_bar_says_how_many_are_through() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for (lang, total, through) in [
            (
                crate::i18n::Lang::En,
                "( 4/30)",
                "Downloading · 4 of 30 done",
            ),
            (crate::i18n::Lang::En, "(1/1)", "Downloading · 1 of 1 done"),
            (
                crate::i18n::Lang::De,
                "( 4/30)",
                "Wird heruntergeladen · 4 von 30 fertig",
            ),
        ] {
            crate::i18n::set(lang);
            let (mut app, mut term) = app();
            let fed =
                format!(":: Retrieving packages...\r\n Total {total}  1.5 MiB  [##--]  40%\r\n");
            let run = Run::fed(op::UPGRADE_RUN, "repo", &fed);
            let page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
            open(&mut app, Box::new(page));
            let shown = screen(&mut app, &mut term);
            assert!(shown.contains(through), "{shown}");
        }
        // The narrowest window keeps the count whole.
        let (mut app, _) = app();
        let fed = "(45/45) checking package integrity  [####] 100%\r\n";
        let run = Run::fed(op::UPGRADE_RUN, "repo", fed);
        let page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
        open(&mut app, Box::new(page));
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 12)).unwrap();
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("…e werden geprüft · 45 von 45 fertig"),
            "{shown}"
        );
        crate::i18n::set(crate::i18n::Lang::En);
    }

    /// The terminal ends with the outcome, so it shows that nothing more comes.
    #[test]
    fn the_terminal_ends_with_the_outcome() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        for (code, outcome) in [(0, "Done"), (1, "Failed")] {
            let (mut app, mut term) = app();
            let run = Run::ended(op::UPGRADE_RUN, "Upgraded\r\n", code);
            let page = RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run);
            open(&mut app, Box::new(page));
            app.handle(Msg::Tick);
            press(&mut app, &[Key::Tab]);
            let shown = screen(&mut app, &mut term);
            let lines: Vec<&str> = shown.lines().collect();
            let at = |text| lines.iter().position(|l| l.contains(text));
            let printed = at("Upgraded").expect("the output");
            assert_eq!(at(outcome), Some(printed + 2), "{shown}");
        }
    }

    #[test]
    fn a_run_asking_for_the_password_takes_no_empty_one() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        sudo::forget();
        let (mut app, mut term) = app();
        let run = Run::fed(op::UPGRADE_RUN, "repo", sudo::PROMPT);
        assert_eq!(run.prompt, Some(Prompt::Password));
        open(
            &mut app,
            Box::new(RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run)),
        );
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("Password"),
            "the empty box names it: {shown}"
        );
        press(&mut app, &[Key::Enter]);
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Password required."), "{shown}");
    }

    #[test]
    fn a_menu_of_the_helper_names_what_enter_keeps() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let run = Run::fed(op::UPGRADE_RUN, "aur", "==> ");
        assert!(matches!(run.prompt, Some(Prompt::Text { .. })));
        open(
            &mut app,
            Box::new(RunPage::running(Job::new(op::UPGRADE_RUN, Vec::new()), run)),
        );
        let shown = screen(&mut app, &mut term);
        assert!(shown.contains("Empty for default"), "{shown}");
    }
}
