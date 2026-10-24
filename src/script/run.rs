//! One interactive run: its terminal, what it does, what it asks, how it ended.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::lines::Lines;
use super::session::{Event, Session};
use super::{Failure, Op, Progress, Prompt, sudo};
use crate::i18n::t;

const KEEP: usize = 300;
/// Output can trail the exit by a moment. The result waits for it.
const SETTLE: Duration = Duration::from_millis(250);

pub struct Run {
    pub op: Op,
    session: Session,
    screen: vt100::Parser,
    lines: Lines,
    recent: VecDeque<String>,
    pub progress: Progress,
    /// The sentence of the step lib.sh announced last.
    pub step: Option<String>,
    /// A question waiting for the person.
    pub prompt: Option<Prompt>,
    /// sudo refused the last password.
    pub rejected: bool,
    /// Verbose output: every prompt is typed.
    terminal: bool,
    fail: Option<String>,
    exit: Option<(i32, Instant)>,
    /// The unfinished line last answered, so it is answered once.
    answered: Option<String>,
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
        run.terminal = super::terminal();
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
            screen: vt100::Parser::new(size.0.max(1), size.1.max(1), 0),
            lines: Lines::default(),
            recent: VecDeque::new(),
            progress: Progress::default(),
            step: None,
            prompt: None,
            rejected: false,
            terminal: false,
            fail: None,
            exit: None,
            answered: None,
        }
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Output(bytes) => {
                self.screen.process(&bytes);
                let done = self.lines.feed(&bytes);
                // A finished line closes the question answered: the same may come again.
                if !done.is_empty() {
                    self.answered = None;
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
                } else if line.starts_with("fail\t") && self.fail.is_none() {
                    self.fail = Some(line);
                }
            }
            Event::Exit(code) => {
                crate::info!("{}: exit {code}", self.op);
                self.exit = Some((code, Instant::now()));
                self.prompt = None;
            }
        }
    }

    /// Looks at the unfinished line, on every tick too, since sudo silences echo just before asking.
    pub fn check_prompt(&mut self) {
        if self.exit.is_some() {
            return;
        }
        let current = self.lines.current();
        if self.answered.as_ref() == Some(&current) {
            return;
        }
        let recent: Vec<String> = self.recent.iter().rev().take(12).rev().cloned().collect();
        self.prompt = Prompt::detect(&current, &recent);
        match &self.prompt {
            // Asked again right after an answer, sudo refused it, also where its words for
            // that are its own. The password goes to the person, never twice unasked.
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
            Some(Prompt::Password) => {
                if let Some(password) = sudo::get()
                    && !self.session.echoes()
                {
                    self.send(&password, current);
                }
            }
            Some(p) if !self.terminal => {
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

    /// Keys typed into the details view go straight to the terminal.
    pub fn type_bytes(&self, bytes: &[u8]) {
        self.session.write(bytes);
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
        run.terminal = true;
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
    /// question coming again: the held password is not tried twice.
    #[test]
    fn asks_the_person_after_a_refused_password_however_sudo_words_it() {
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for refusal in [
            sudo::REJECTED,
            "Have you considered trying to match wits with a rutabaga?",
        ] {
            sudo::set("wrong".into());
            let (tx, rx) = mpsc::channel();
            let mut cmd = std::process::Command::new("bash");
            cmd.arg("-c")
                .arg(r#"for i in 1 2 3; do stty -echo; printf '%s' "$SUDO_PROMPT"; read -r -t 4 pw || exit 3; stty echo; echo; [ "$pw" = right ] && { echo "ok after $i"; exit 0; }; echo "$REFUSAL"; done; exit 1"#)
                .env("SUDO_PROMPT", sudo::PROMPT)
                .env("REFUSAL", refusal);
            let session = Session::spawn(cmd, (24, 80), move |e| drop(tx.send(e))).unwrap();
            let mut run = Run::with(crate::script::op::SYSTEM_AUTH, session, (24, 80));
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut asked = false;
            while run.result().is_none() && Instant::now() < deadline {
                if let Ok(e) = rx.recv_timeout(Duration::from_millis(20)) {
                    run.on_event(e);
                }
                run.check_prompt();
                if run.rejected && run.prompt == Some(Prompt::Password) && !asked {
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

    #[test]
    fn hands_the_password_over_only_while_echo_is_off() {
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        sudo::set("s3cret word".into());
        let (_, out) = drive(
            r#"stty -echo; printf '%s' "$SUDO_PROMPT"; read -r pw; stty echo; echo; [ "$pw" = "s3cret word" ] && echo accepted"#,
        );
        sudo::forget();
        assert!(out.contains("accepted"), "{out}");
        assert!(
            !out.contains("s3cret"),
            "the password reached the output: {out}"
        );
    }

    /// Asked again after the command's own output, as once its timestamp ran out, sudo gets the
    /// held password again. Right after an answer, the same question is a refusal.
    #[test]
    fn answers_sudo_again_once_other_output_came_between() {
        let _held = sudo::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        sudo::set("pw".into());
        let (_, out) = drive(
            r#"stty -echo; for i in 1 2; do printf '%s' "$SUDO_PROMPT"; read -r p; echo; [ "$p" = pw ] && echo "got $i"; printf 'a\nb\nc\n'; done; stty echo"#,
        );
        sudo::forget();
        assert!(out.contains("got 1") && out.contains("got 2"), "{out}");
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
