//! One interactive run: its terminal, what it does, what it asks, how it ended.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::lines::Lines;
use super::session::{Event, Session};
use super::{Failure, Op, Progress, Prompt, sudo};
use crate::i18n::t;

const KEEP: usize = 300;
/// Output can trail the exit by a moment; the result waits for it.
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
        Ok(Run::with(op, session, size))
    }

    /// A run that has printed `output` after announcing `step`, for pictures of its page; the
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
            fail: None,
            exit: None,
            answered: None,
        }
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Output(bytes) => {
                self.screen.process(&bytes);
                for line in self.lines.feed(&bytes) {
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

    /// Looks at the unfinished line; on every tick too, since sudo silences echo just before asking.
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
            Some(Prompt::Password) => {
                if let Some(password) = sudo::get()
                    && !self.session.echoes()
                {
                    self.send(&password, current);
                }
            }
            Some(p) => {
                if let Some(answer) = p.automatic() {
                    crate::info!("{}: answered {answer:?} to {current:?}", self.op);
                    self.send(answer, current);
                }
            }
            None => {}
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
            // A terminal that echoes would print the password into the output and the log.
            if self.session.echoes() {
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

    /// A run of `script` in bash, fed until it ends; returns what it printed.
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

    #[test]
    fn hands_the_password_over_only_while_echo_is_off() {
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
        let (run, _) = drive("printf 'fail\\t7\\tsystem/script.sh\\t4\\tfalse\\n' >&3; exit 1");
        let failure = run.result().unwrap().unwrap_err();
        assert_eq!(failure.code, Some(1));
        assert!(
            failure.detail.starts_with("system/script.sh:4:"),
            "{}",
            failure.detail
        );
    }
}
