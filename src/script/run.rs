//! One interactive run: its terminal, what it does, what it asks, how it ended.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::lines::Lines;
use super::session::{Event, Session};
use super::{Failure, Op, Progress, Prompt, prompt, sudo};
use crate::i18n::t;

const KEEP: usize = 300;
/// Output can trail the exit by a moment. The result waits for it.
const SETTLE: Duration = Duration::from_millis(250);
/// Lines the terminal keeps above its screen.
const HISTORY: usize = 1000;
/// How long an unfinished line stands still before it counts as a question in unknown words.
/// Output pauses mid-line for a moment at most, a program waits for its answer far longer.
const QUIET: Duration = Duration::from_secs(1);

pub struct Run {
    pub op: Op,
    session: Session,
    screen: vt100::Parser,
    lines: Lines,
    recent: VecDeque<String>,
    pub progress: Progress,
    /// The sentence of the step lib.sh announced last.
    pub step: Option<String>,
    /// What lib.sh named as found, in order.
    pub found: Vec<String>,
    /// A question waiting for the person.
    pub prompt: Option<Prompt>,
    /// sudo refused the last password.
    pub rejected: bool,
    /// Verbose output: every prompt is typed.
    verbose: bool,
    /// The script handed the terminal to the person, as an AUR helper's review needs, until it
    /// takes it back. Every prompt is typed meanwhile.
    pub handed: bool,
    fail: Option<String>,
    exit: Option<(i32, Instant)>,
    /// The unfinished line last answered, so it is answered once.
    answered: Option<String>,
    /// When output came last.
    heard: Instant,
    /// The unfinished line that asks in words not known here, until a line ends.
    asked: Option<String>,
}

impl Run {
    pub fn start(
        op: Op,
        args: &[String],
        size: (u16, u16),
        sink: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Run, Failure> {
        crate::info!("run {op} {args:?}");
        let session = Session::start(op, args, size, sink).map_err(|e| Failure::new(op, e))?;
        let mut run = Run::with(op, session, size);
        run.verbose = super::terminal();
        Ok(run)
    }

    /// A run that has printed `output` after announcing `step`, for pictures of its page. The
    /// program behind it is `true`, and its end is never told.
    #[cfg(test)]
    pub fn fed(op: Op, step: &str, output: &str) -> Run {
        let session = Session::spawn(std::process::Command::new("true"), (24, 80), |_| {})
            .expect("true runs");
        let mut run = Run::with(op, session, (24, 80));
        run.on_event(Event::Script(format!("step\t{step}")));
        run.on_event(Event::Output(output.as_bytes().to_vec()));
        run
    }

    /// A run that printed `output` and ended with `code`, its last output already arrived.
    #[cfg(test)]
    pub fn ended(op: Op, output: &str, code: i32) -> Run {
        let session = Session::spawn(std::process::Command::new("true"), (24, 80), |_| {})
            .expect("true runs");
        let mut run = Run::with(op, session, (24, 80));
        run.on_event(Event::Output(output.as_bytes().to_vec()));
        run.exit = Some((code, Instant::now() - SETTLE));
        run
    }

    fn with(op: Op, session: Session, size: (u16, u16)) -> Run {
        Run {
            op,
            session,
            screen: vt100::Parser::new(size.0.max(1), size.1.max(1), HISTORY),
            lines: Lines::default(),
            recent: VecDeque::new(),
            progress: Progress::default(),
            step: None,
            found: Vec::new(),
            prompt: None,
            rejected: false,
            verbose: false,
            handed: false,
            fail: None,
            exit: None,
            answered: None,
            heard: Instant::now(),
            asked: None,
        }
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Output(bytes) => {
                self.heard = Instant::now();
                self.screen.process(&bytes);
                let done = self.lines.feed(&bytes);
                // A finished line closes the question answered: the same may come again.
                if !done.is_empty() {
                    self.answered = None;
                    self.asked = None;
                }
                for line in done {
                    if line.contains(sudo::REJECTED) {
                        sudo::forget();
                        self.rejected = true;
                    }
                    crate::log::write("OUT", &format!("{}| {line}", self.op));
                    self.progress.feed(&line);
                    if self.recent.len() == KEEP {
                        self.recent.pop_front();
                    }
                    self.recent.push_back(line);
                }
                self.check_prompt();
            }
            Event::Script(line) => {
                if let Some(key) = line.strip_prefix("step\t") {
                    crate::info!("{}: step {key}", self.op);
                    self.step = self.op.meta().steps.get(key).map(|s| t(s).to_string());
                } else if let Some(state) = line.strip_prefix("terminal\t") {
                    self.handed = state == "on";
                } else if let Some(item) = line.strip_prefix("found\t") {
                    self.found.push(item.to_string());
                } else if line.starts_with("fail\t") && self.fail.is_none() {
                    self.fail = Some(line);
                } else if line == "refused" {
                    sudo::forget();
                    self.rejected = true;
                }
            }
            Event::Exit(code) => {
                crate::info!("{}: exit {code}", self.op);
                self.exit = Some((code, Instant::now()));
                self.prompt = None;
                self.handed = false;
                self.asked = None;
            }
        }
    }

    /// Looks at the unfinished line, on every tick too, since sudo silences echo just before asking
    /// and a question in unknown words shows only by standing still.
    pub fn check_prompt(&mut self) {
        self.check_at(Instant::now());
    }

    /// As `check_prompt` at `now`, which a test sets.
    pub fn check_at(&mut self, now: Instant) {
        if self.exit.is_some() {
            return;
        }
        let current = self.lines.current();
        // What the person types after it belongs to the question.
        if self.asked.as_ref().is_some_and(|q| !current.starts_with(q)) {
            self.asked = None;
        }
        if self.answered.as_ref() == Some(&current) {
            return;
        }
        let recent: Vec<String> = self.recent.iter().rev().take(12).rev().cloned().collect();
        self.prompt = Prompt::detect(&current, &recent);
        // On a terminal shown anyway the person sees every question.
        if !self.verbose
            && !self.handed
            && self.prompt.is_none()
            && self.asked.is_none()
            && prompt::asks(&current)
            && now.saturating_duration_since(self.heard) >= QUIET
        {
            crate::info!("{}: waits on {current:?}", self.op);
            self.asked = Some(current.clone());
        }
        match &self.prompt {
            // Asked again right after an answer, sudo refused it, also where its words for
            // that are its own. The held password reaches sudo through a pipe only, see
            // session.rs, so the person answers the question.
            Some(Prompt::Password)
                if recent
                    .iter()
                    .rev()
                    .take(3)
                    .any(|l| l.ends_with(sudo::PROMPT)) =>
            {
                sudo::forget();
                self.rejected = true;
            }
            Some(p) if !self.verbose && !self.handed => {
                if let Some(answer) = p.automatic() {
                    crate::info!("{}: answered {answer:?} to {current:?}", self.op);
                    self.send(answer, current);
                }
            }
            _ => {}
        }
    }

    fn send(&mut self, text: &str, line: String) {
        self.session.write(text.as_bytes());
        self.session.write(b"\r");
        self.answered = Some(line);
        self.prompt = None;
    }

    /// The person's answer to the open question.
    pub fn answer(&mut self, text: &str) {
        let line = self.lines.current();
        if self.prompt == Some(Prompt::Password) {
            // An empty one counts as a failed attempt towards the lock of pam_faillock. A
            // terminal that echoes would print it into the output and the log.
            if text.is_empty() || self.session.echoes() {
                return;
            }
            sudo::set(text.to_string());
            self.rejected = false;
        } else {
            crate::info!("{}: answered {text:?} to {line:?}", self.op);
        }
        self.send(text, line);
    }

    /// A question in words not known here waits on the terminal.
    pub fn waiting(&self) -> bool {
        self.asked.is_some()
    }

    /// Keys typed into the details view go straight to the terminal, whose view comes back from
    /// the history.
    pub fn type_bytes(&mut self, bytes: &[u8]) {
        self.screen.screen_mut().set_scrollback(0);
        self.session.write(bytes);
    }

    /// Pasted text, as one.
    pub fn paste(&mut self, text: &str) {
        let text = pasted(text, self.screen.screen().bracketed_paste());
        self.type_bytes(text.as_bytes());
    }

    /// Moves the view up the history by `lines`, down where negative. Clamped to what it holds.
    pub fn scroll(&mut self, lines: isize) {
        let screen = self.screen.screen_mut();
        screen.set_scrollback(screen.scrollback().saturating_add_signed(lines));
    }

    /// How far up the history the view stands, and how far the history reaches.
    pub fn history(&mut self) -> (usize, usize) {
        let screen = self.screen.screen_mut();
        let at = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let reach = screen.scrollback();
        screen.set_scrollback(at);
        (at, reach)
    }

    pub fn interrupt(&self) {
        crate::info!("{}: interrupted", self.op);
        self.session.write(&[0x03]);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.screen.screen_mut().set_size(rows.max(1), cols.max(1));
        self.session.resize(rows, cols);
    }

    /// Writes `line` under the output, a blank line between, bold in green or red.
    pub fn conclude(&mut self, line: &str, ok: bool) {
        let gap = match self.screen.screen().cursor_position() {
            (_, 0) => "\r\n",
            _ => "\r\n\r\n",
        };
        let colour = if ok { 32 } else { 31 };
        let text = format!("{gap}\x1b[1;{colour}m{line}\x1b[0m");
        self.screen.process(text.as_bytes());
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.screen.screen()
    }

    pub fn running(&self) -> bool {
        self.exit.is_none()
    }

    /// The outcome, once the run ended and its last output arrived.
    pub fn result(&self) -> Option<Result<(), Failure>> {
        let (code, at) = self.exit?;
        if at.elapsed() < SETTLE {
            return None;
        }
        if code == 0 {
            return Some(Ok(()));
        }
        let mut tail: Vec<String> = self.recent.iter().rev().take(40).rev().cloned().collect();
        let current = self.lines.current();
        if !current.trim().is_empty() {
            tail.push(current);
        }
        Some(Err(Failure::classify(
            self.op,
            Some(code),
            self.fail.as_deref(),
            tail,
        )))
    }
}

/// Pasted text as a terminal hands it on: a line break as enter, no other control character,
/// marked as pasted where the program asks for that. Without escape no text ends the mark early.
fn pasted(text: &str, bracketed: bool) -> String {
    let text: String = text
        .replace("\r\n", "\r")
        .replace('\n', "\r")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\r' | '\t'))
        .collect();
    match bracketed {
        true => format!("\x1b[200~{text}\x1b[201~"),
        false => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A run of `script` in bash, fed until it ends. Returns what it printed.
    fn drive(script: &str) -> (Run, String) {
        let (tx, rx) = mpsc::channel();
        let mut cmd = std::process::Command::new("bash");
        cmd.arg("-c").arg(script).env("SUDO_PROMPT", sudo::PROMPT);
        let session = Session::spawn(cmd, (24, 80), move |e| drop(tx.send(e))).unwrap();
        let mut run = Run::with(crate::script::op::SYSTEM_STATE, session, (24, 80));
        let deadline = Instant::now() + Duration::from_secs(10);
        while run.result().is_none() && Instant::now() < deadline {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(20)) {
                run.on_event(e);
            }
            run.check_prompt();
        }
        let text: Vec<String> = run.recent.iter().cloned().collect();
        (run, text.join("\n"))
    }

    /// While the script hands over, no question is answered for the person. Once it takes the
    /// terminal back, the confirmation given before the run answers again.
    #[test]
    fn during_the_hand_over_the_person_answers_every_question() {
        let (tx, rx) = mpsc::channel();
        let mut cmd = std::process::Command::new("bash");
        cmd.arg("-c").arg(
            r#"read -r -t 2 -p ":: Proceed with installation? [Y/n] " a || { echo; echo unanswered; }"#,
        );
        let session = Session::spawn(cmd, (24, 80), move |e| drop(tx.send(e))).unwrap();
        let mut run = Run::with(crate::script::op::UPGRADE_RUN, session, (24, 80));
        run.on_event(Event::Script("terminal\ton".into()));
        assert!(run.handed);
        let deadline = Instant::now() + Duration::from_secs(10);
        while run.result().is_none() && Instant::now() < deadline {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(20)) {
                run.on_event(e);
            }
            run.check_prompt();
        }
        assert!(
            run.recent.iter().any(|l| l == "unanswered"),
            "{:?}",
            run.recent
        );
        let mut run = Run::fed(crate::script::op::UPGRADE_RUN, "aur", "");
        run.on_event(Event::Script("terminal\ton".into()));
        run.on_event(Event::Script("terminal\toff".into()));
        assert!(!run.handed);
        run.on_event(Event::Output(
            b":: Proceed with installation? [Y/n] ".to_vec(),
        ));
        assert_eq!(run.prompt, None, "answered");
        assert!(run.answered.is_some());
    }

    /// A question in words not known here shows once its line stands still: a bar that does,
    /// typing after it, and a finished line do not.
    #[test]
    fn a_question_in_unknown_words_waits_once_its_line_stands_still() {
        let mut run = Run::fed(crate::script::op::UPGRADE_RUN, "aur", "Name of the key: ");
        let at = run.heard;
        run.check_at(at);
        assert!(!run.waiting(), "not yet still");
        run.check_at(at + QUIET);
        assert!(run.waiting());
        run.on_event(Event::Output(b"ab".to_vec()));
        run.check_at(at + QUIET);
        assert!(run.waiting(), "typed after the question");
        run.on_event(Event::Output(b"\r\n".to_vec()));
        run.check_at(Instant::now() + 2 * QUIET);
        assert!(!run.waiting(), "answered");
        for line in [
            "( 1/2) upgrading firefox   [######------]  50%",
            "Downloading\u{2026}  [*******     ]",
            "Proceed? [Y/n] ",
        ] {
            let mut run = Run::fed(crate::script::op::UPGRADE_RUN, "repo", line);
            run.check_at(run.heard + 10 * QUIET);
            assert!(!run.waiting(), "{line}");
        }
        let mut run = Run::fed(crate::script::op::UPGRADE_RUN, "aur", "");
        run.on_event(Event::Script("terminal\ton".into()));
        run.on_event(Event::Output(b":".to_vec()));
        run.check_at(run.heard + 10 * QUIET);
        assert!(!run.waiting(), "handed over, the terminal shows anyway");
    }

    #[test]
    fn the_history_keeps_what_scrolled_off_and_a_key_returns_to_the_screen() {
        let lines: String = (0..100).map(|i| format!("line {i}\r\n")).collect();
        let mut run = Run::fed(crate::script::op::UPGRADE_RUN, "aur", &lines);
        let (at, reach) = run.history();
        assert_eq!((at, reach), (0, 77));
        run.scroll(3);
        assert_eq!(run.history(), (3, 77));
        assert!(
            run.screen().contents().starts_with("line 74"),
            "{}",
            run.screen().contents()
        );
        run.scroll(-10);
        assert_eq!(run.history().0, 0);
        run.scroll(3);
        run.type_bytes(b"q");
        assert_eq!(run.history().0, 0);
    }

    #[test]
    fn pasted_text_breaks_lines_as_enter_and_carries_no_escape() {
        assert_eq!(pasted("a\r\nb\nc\td", false), "a\rb\rc\td");
        assert_eq!(
            pasted("x\x1b[201~rm", true),
            "\x1b[200~x[201~rm\x1b[201~",
            "no end mark from the text"
        );
    }

    #[test]
    fn keeps_what_the_script_found_in_order() {
        let mut run = Run::fed(crate::script::op::REPAIR_FILES, "check", "");
        for line in ["found\tbar", "found\tfoo"] {
            run.on_event(Event::Script(line.into()));
        }
        assert_eq!(run.found, ["bar", "foo"]);
    }

    #[test]
    fn answers_the_confirmation_given_before_the_run() {
        let (run, out) =
            drive(r#"read -r -p ":: Proceed with installation? [Y/n] " a; echo "answer=$a""#);
        assert!(out.contains("answer=y"), "{out}");
        assert_eq!(run.result(), Some(Ok(())));
    }

    /// With verbose output every prompt is typed, also what was confirmed before the run.
    #[test]
    fn in_the_terminal_leaves_every_question_to_the_person() {
        let (tx, rx) = mpsc::channel();
        let mut cmd = std::process::Command::new("bash");
        cmd.arg("-c").arg(
            r#"read -r -t 2 -p ":: Proceed with installation? [Y/n] " a || { echo; echo unanswered; }"#,
        );
        let session = Session::spawn(cmd, (24, 80), move |e| drop(tx.send(e))).unwrap();
        let mut run = Run::with(crate::script::op::UPGRADE_RUN, session, (24, 80));
        run.verbose = true;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut asked = false;
        while run.result().is_none() && Instant::now() < deadline {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(20)) {
                run.on_event(e);
            }
            run.check_prompt();
            asked |= matches!(run.prompt, Some(Prompt::YesNo { yes: true, .. }));
        }
        assert!(asked, "the question is still seen: {:?}", run.recent);
        assert!(
            run.recent.iter().any(|l| l == "unanswered"),
            "{:?}",
            run.recent
        );
    }

    #[test]
    fn answers_the_same_confirmation_each_time_it_comes() {
        let (_, out) = drive(
            r#"for i in 1 2; do read -r -p ":: Proceed with installation? [Y/n] " a; echo "answer$i=$a"; done"#,
        );
        assert!(
            out.contains("answer1=y") && out.contains("answer2=y"),
            "{out}"
        );
    }

    /// A refusal in other words than sudo's own, as `Defaults insults` gives, is told by the
    /// question coming again right after the person's answer.
    #[test]
    fn asks_the_person_after_a_refused_password_however_sudo_words_it() {
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for refusal in [
            sudo::REJECTED,
            "Have you considered trying to match wits with a rutabaga?",
        ] {
            sudo::forget();
            let (tx, rx) = mpsc::channel();
            let mut cmd = std::process::Command::new("bash");
            cmd.arg("-c")
                .arg(r#"for i in 1 2 3; do stty -echo; printf '%s' "$SUDO_PROMPT"; read -r -t 4 pw || exit 3; stty echo; echo; [ "$pw" = right ] && { echo "ok after $i"; exit 0; }; echo "$REFUSAL"; done; exit 1"#)
                .env("SUDO_PROMPT", sudo::PROMPT)
                .env("REFUSAL", refusal);
            let session = Session::spawn(cmd, (24, 80), move |e| drop(tx.send(e))).unwrap();
            let mut run = Run::with(crate::script::op::SYSTEM_AUTH, session, (24, 80));
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut tried, mut asked) = (false, false);
            while run.result().is_none() && Instant::now() < deadline {
                if let Ok(e) = rx.recv_timeout(Duration::from_millis(20)) {
                    run.on_event(e);
                }
                run.check_prompt();
                if run.prompt == Some(Prompt::Password) && !tried {
                    tried = true;
                    run.answer("wrong");
                } else if run.rejected && run.prompt == Some(Prompt::Password) && !asked {
                    asked = true;
                    run.answer("right");
                }
            }
            sudo::forget();
            assert!(asked, "never asked again: {:?}", run.recent);
            assert!(
                run.recent.iter().any(|l| l == "ok after 2"),
                "{refusal}: {:?}",
                run.recent
            );
        }
    }

    /// The held password goes to sudo through a pipe only: any program of the run can print
    /// sudo's prompt with echo off, and gets nothing.
    #[test]
    fn never_types_the_held_password_into_the_terminal() {
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        sudo::set("s3cret word".into());
        let (_, out) = drive(
            r#"stty -echo; printf '%s' "$SUDO_PROMPT"; read -r -t 2 pw || { echo; echo unanswered; }; [ -n "${pw:-}" ] && echo "got $pw""#,
        );
        sudo::forget();
        assert!(out.contains("unanswered"), "{out}");
        assert!(!out.contains("s3cret"), "{out}");
    }

    /// lib.sh tells of a password sudo refused from its pipe: it is forgotten, so no later run
    /// tries it, and the person answers.
    #[test]
    fn a_password_refused_from_the_pipe_is_forgotten() {
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        sudo::set("old".into());
        let mut run = Run::fed(crate::script::op::UPGRADE_RUN, "repo", "");
        run.on_event(Event::Script("refused".into()));
        assert!(run.rejected && sudo::get().is_none());
    }

    #[test]
    fn never_hands_an_empty_password_to_sudo() {
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        sudo::set(String::new());
        assert_eq!(sudo::get(), None);
        let (tx, rx) = mpsc::channel();
        let mut cmd = std::process::Command::new("bash");
        cmd.arg("-c")
            .arg(r#"stty -echo; printf '%s' "$SUDO_PROMPT"; read -r -t 2 pw || { echo; echo unanswered; }"#)
            .env("SUDO_PROMPT", sudo::PROMPT);
        let session = Session::spawn(cmd, (24, 80), move |e| drop(tx.send(e))).unwrap();
        let mut run = Run::with(crate::script::op::SYSTEM_AUTH, session, (24, 80));
        let deadline = Instant::now() + Duration::from_secs(10);
        while run.result().is_none() && Instant::now() < deadline {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(20)) {
                run.on_event(e);
            }
            if run.prompt == Some(Prompt::Password) {
                run.answer("");
            }
        }
        assert!(run.recent.iter().any(|l| l == "unanswered"));
    }

    #[test]
    fn leaves_a_question_it_does_not_know_to_the_person() {
        let (tx, rx) = mpsc::channel();
        let mut cmd = std::process::Command::new("bash");
        cmd.arg("-c").arg(r#"read -r -p ":: foo and bar are in conflict. Remove bar? [y/N] " a; echo "answer=$a""#);
        let session = Session::spawn(cmd, (24, 80), move |e| drop(tx.send(e))).unwrap();
        let mut run = Run::with(crate::script::op::SYSTEM_STATE, session, (24, 80));
        let deadline = Instant::now() + Duration::from_secs(10);
        while run.prompt.is_none() && Instant::now() < deadline {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(20)) {
                run.on_event(e);
            }
        }
        assert!(
            matches!(run.prompt, Some(Prompt::YesNo { yes: false, .. })),
            "{:?}",
            run.prompt
        );
        run.answer("y");
        while run.result().is_none() && Instant::now() < deadline {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(20)) {
                run.on_event(e);
            }
        }
        assert!(run.recent.iter().any(|l| l == "answer=y"));
    }

    #[test]
    fn reports_the_exit_code_and_the_trap_line() {
        let (run, _) = drive("printf 'fail\\t7\\tsystem/system.sh\\t4\\tfalse\\n' >&3; exit 1");
        let failure = run.result().unwrap().unwrap_err();
        assert_eq!(failure.code, Some(1));
        assert!(
            failure.detail.starts_with("system/system.sh:4:"),
            "{}",
            failure.detail
        );
    }
}
