//! The command line: the interface, or one of three answers for installers, scripts and timers.

use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::Duration;

use crate::data::features::Features;
use crate::data::updates::{self, Pending, Source};
use crate::i18n::{self, Lang, t, tn};
use crate::script::{self, Failure, op, query, query_within};
use crate::settings::Settings;
use crate::state::State;
use crate::ui::Target;

pub const PAGES: [&str; 12] = [
    "upgrade", "news", "packages", "services", "log", "cleanup", "pacnew", "mirrors", "firmware",
    "repair", "settings", "features",
];

/// The command line in two groups, each row an argument and what it does. The help and the
/// about page show them.
pub fn commands() -> [(&'static str, Vec<(&'static str, String)>); 2] {
    let s = |text: &str| text.to_string();
    [
        (
            t("Interface"),
            vec![
                ("system-manager", s("[--window] [<page>]")),
                ("<page>", PAGES.join(", ")),
            ],
        ),
        (
            t("Commands"),
            vec![
                ("init", s(t("Sets up the menu entry and the notification"))),
                (
                    "check",
                    s(t("Prints pending updates (exit code 2 without any)")),
                ),
                (
                    "notify [--new]",
                    s(t("Announces pending updates, with --new only new ones")),
                ),
            ],
        ),
    ]
}

fn help() -> String {
    let mut help = String::new();
    for (title, rows) in commands() {
        help += &format!("{title}\n");
        for (arg, what) in rows {
            help += &format!("  {arg:<16} {what}\n");
        }
        help += "\n";
    }
    help
}

#[derive(Debug, PartialEq)]
enum Ask {
    Help,
    Version,
    Start {
        window: bool,
        target: Option<Target>,
        command: Option<&'static str>,
        only_new: bool,
    },
}

/// The arguments after the program's name, or why they ask for nothing it does.
fn parse(args: impl IntoIterator<Item = String>) -> Result<Ask, String> {
    let (mut window, mut target, mut command, mut only_new) = (false, None, None, false);
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Ask::Help),
            "-V" | "--version" => return Ok(Ask::Version),
            "-w" | "--window" => window = true,
            "--new" => only_new = true,
            "init" => command = Some("init"),
            "check" => command = Some("check"),
            "notify" => command = Some("notify"),
            page => match Target::from_id(page) {
                Some(t) => target = Some(t),
                None => return Err(format!("unknown argument {page}")),
            },
        }
    }
    if only_new && command != Some("notify") {
        return Err("--new goes with notify".into());
    }
    Ok(Ask::Start {
        window,
        target,
        command,
        only_new,
    })
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The renewal lib.sh starts for a run, never a person's command.
    if let [command, shell, every, script] = args.as_slice()
        && command == "sudo-renew"
        && let (Ok(shell), Ok(every)) = (shell.parse(), every.parse())
    {
        return script::sudo::renew(shell, Duration::from_secs(every), script.as_ref());
    }
    let (window, target, command, only_new) = match parse(args) {
        Ok(Ask::Help) => {
            print!("{}", help());
            return ExitCode::SUCCESS;
        }
        Ok(Ask::Version) => {
            println!("system-manager {}", crate::version::version());
            return ExitCode::SUCCESS;
        }
        Ok(Ask::Start {
            window,
            target,
            command,
            only_new,
        }) => (window, target, command, only_new),
        Err(why) => {
            eprint!("system-manager: {why}\n\n{}", help());
            return ExitCode::from(64);
        }
    };
    if rustix::process::geteuid().is_root() && refuses_root(command) {
        eprintln!("system-manager: runs as the user it serves and asks for the password itself");
        return ExitCode::FAILURE;
    }
    if !crate::paths::home().is_absolute() {
        eprintln!("system-manager: HOME names no folder for its files");
        return ExitCode::FAILURE;
    }
    if let Err(e) = crate::log::init(&crate::paths::log()) {
        eprintln!(
            "system-manager: no log at {}: {e}",
            crate::paths::log().display()
        );
    }
    // Found before an update can replace the file.
    if let Err(e) = crate::paths::program() {
        crate::warn!("no path of this program: {e}");
    }
    crate::info!(
        "start {} {:?}",
        crate::version::version(),
        std::env::args().skip(1).collect::<Vec<_>>()
    );
    let code = match command {
        Some("init") => {
            crate::migrate::run();
            init()
        }
        Some("check") => check(),
        Some("notify") => notify(only_new),
        _ => {
            let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            let result = if window || !terminal {
                crate::window::run(target)
            } else {
                crate::term::run(target)
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    crate::error!("{e}");
                    eprintln!("system-manager: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    };
    // What was chosen right before quitting may still be on its way to the disk.
    if !crate::ui::settle(WRITES) {
        crate::warn!(
            "settings or state still being written after {} s",
            WRITES.as_secs()
        );
        eprintln!(
            "system-manager: settings or state still being written after {} s",
            WRITES.as_secs()
        );
    }
    script::cleanup();
    if !crate::log::finish(WRITES) {
        eprintln!(
            "system-manager: the log is still being written after {} s",
            WRITES.as_secs()
        );
    }
    code
}

/// How long the end waits for the settings and the state to be written.
const WRITES: Duration = Duration::from_secs(10);

/// Whether a command refuses root: makepkg does, and the files it writes would be root's in the
/// user's home. Only reading the updates is the same for root.
fn refuses_root(command: Option<&str>) -> bool {
    command != Some("check")
}

/// The settings in force and the optional packages there, as a start of the interface reads them.
fn setup() -> Result<(Settings, Features), Failure> {
    let (settings, _) = Settings::load();
    i18n::set(
        settings
            .language
            .as_deref()
            .and_then(Lang::from_code)
            .unwrap_or_else(Lang::detect),
    );
    let features = Features::load()?;
    script::set_aur(features.aur_helper(settings.aur_helper.as_deref()));
    script::set_aur_review(settings.aur_review.unwrap_or(true));
    script::set_flatpak(features.flatpak(settings.flatpak.unwrap_or(true)));
    Ok((settings, features))
}

fn init() -> ExitCode {
    let applied =
        setup().and_then(|(settings, features)| crate::integration::apply(&settings, &features));
    match applied {
        Ok(a) => {
            let on = |b: bool| if b { "on" } else { "off" };
            println!(
                "app menu entry: {}\nupdate notification: {}\nupdate indicator: {}",
                on(a.entry),
                a.timer,
                on(a.indicator)
            );
            ExitCode::SUCCESS
        }
        Err(f) => {
            eprintln!("system-manager: {}: {}", f.headline(), f.detail);
            ExitCode::FAILURE
        }
    }
}

/// How long a check waits for an upgrade of this user to end.
const UPGRADE: Duration = Duration::from_secs(60 * 60);

/// The pending updates, with the AUR helper the settings choose, once an upgrade of this user has
/// ended.
fn pending(features: &Features) -> Result<Pending, Failure> {
    if let Some(p) = script::missing(op::UPGRADE_LIST, features) {
        return Err(Failure::new(op::UPGRADE_LIST, format!("needs {p}")));
    }
    // The query outlasts the script's own wait, which says why it gave up.
    let wait = UPGRADE.as_secs().to_string();
    query_within(
        op::UPGRADE_IDLE,
        &[&wait],
        UPGRADE + Duration::from_secs(60),
    )?;
    Ok(updates::parse(&query(op::UPGRADE_LIST, &[])?))
}

fn check() -> ExitCode {
    match setup().and_then(|(_, features)| pending(&features)) {
        Ok(p) => {
            print!("{}", updates::checkupdates_format(&p.updates));
            for source in &p.failed {
                let name = match source {
                    Source::Repo => "the repositories",
                    Source::Aur => "the AUR",
                    Source::Flatpak => "Flatpak",
                };
                eprintln!("system-manager: {name} could not be asked");
            }
            match (p.updates.is_empty(), p.failed.is_empty()) {
                (false, _) => ExitCode::SUCCESS,
                (true, true) => ExitCode::from(2),
                (true, false) => ExitCode::FAILURE,
            }
        }
        Err(f) => {
            eprintln!("system-manager: {}: {}", f.headline(), f.detail);
            ExitCode::FAILURE
        }
    }
}

/// What a notification came to: the handle the script gave it, and the answer.
#[derive(Debug, Default, PartialEq)]
struct Answer {
    shown: Option<String>,
    open: bool,
    /// The time ran out unanswered, and it still stands.
    standing: bool,
}

impl Answer {
    fn parse(out: &str) -> Answer {
        let mut answer = Answer::default();
        for line in out.lines() {
            match line.split_once('\t') {
                Some(("shown", handle)) => answer.shown = Some(handle.to_string()),
                Some(("chosen", action)) => answer.open |= action == "open",
                _ => answer.standing |= line == "standing",
            }
        }
        answer
    }

    /// What it leaves in the state: the updates it announced, and itself where it still stands.
    /// One another run showed meanwhile stays.
    fn settle(&self, state: &mut State, now: Vec<String>) {
        state.notified = now;
        match &self.shown {
            Some(handle) if self.standing => state.notification = Some(handle.clone()),
            shown if state.notification == *shown => state.notification = None,
            _ => {}
        }
    }
}

/// What `notify` prints instead of a notification, when it shows none.
fn quiet(pending: usize, fresh: bool, only_new: bool) -> Option<String> {
    if pending == 0 {
        Some("updates: none".into())
    } else if only_new && !fresh {
        Some(format!("updates: {pending}, announced before"))
    } else {
        None
    }
}

/// Whether the updates are news: one not announced yet, or a notification still standing, which
/// they bring up to date.
fn fresh(now: &[String], state: &State) -> bool {
    now.iter().any(|u| !state.notified.contains(u)) || state.notification.is_some()
}

fn notify(only_new: bool) -> ExitCode {
    let read = setup().and_then(|(settings, features)| Ok((settings, pending(&features)?)));
    let (settings, list, complete) = match read {
        Ok((settings, p)) => (settings, p.updates, p.failed.is_empty()),
        Err(f) => {
            crate::warn!("notify: {}", f.detail);
            return ExitCode::FAILURE;
        }
    };
    let now: Vec<String> = list
        .iter()
        .map(|u| format!("{} {}", u.name, u.new))
        .collect();
    let state = State::load();
    if list.is_empty() && complete {
        crate::integration::withdraw();
    }
    if let Some(status) = quiet(list.len(), fresh(&now, &state), only_new) {
        // Nothing pending forgets what was announced, unless a source could not tell.
        if list.is_empty() && complete {
            State::update(|s| s.notified = now);
        }
        crate::info!("notify: {status}");
        println!("{status}");
        return ExitCode::SUCCESS;
    }
    let text = tn(
        "{n} update available.",
        "{n} updates available.",
        list.len(),
        &[],
    );
    // It waits for a click until the timer is due again, which then takes its place. The script
    // stops waiting by itself, the limit catches one that does not.
    let every = settings.every();
    let shown = script::query_lines(
        op::DESKTOP_NOTIFY,
        &[
            t("System update"),
            &text,
            // The button opens the window.
            if crate::window::can_open() {
                t("Update")
            } else {
                ""
            },
            &every.as_secs().to_string(),
            state.notification.as_deref().unwrap_or_default(),
        ],
        every + Duration::from_secs(60),
        // Kept as soon as it shows, so the manager can close it while it waits.
        |line| {
            if let Some(handle) = Answer::parse(line).shown {
                State::update(|s| s.notification = Some(handle));
            }
        },
    );
    let answer = shown.as_deref().map(Answer::parse);
    // Remembered only once shown, so one the desktop missed comes again.
    if let Ok(answer) = &answer {
        State::update(|s| answer.settle(s, now));
    }
    match answer {
        Ok(answer) if answer.open => {
            let exe = crate::paths::program()
                .unwrap_or_default()
                .display()
                .to_string();
            if let Err(f) = query(op::DESKTOP_OPEN, &[&exe, Target::Upgrade.id()]) {
                crate::warn!("notify: {}", f.detail);
            }
            ExitCode::SUCCESS
        }
        Ok(_) => ExitCode::SUCCESS,
        Err(f) => {
            crate::warn!("notify: {}", f.detail);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_help_lines_up_every_description_under_its_group() {
        let help = help();
        let rows: Vec<&str> = help.lines().filter(|l| l.starts_with("  ")).collect();
        assert!(rows.len() == 5, "{help}");
        for line in rows {
            let (name, text) = line.split_at(19);
            assert!(name.ends_with(' ') && !text.starts_with(' '), "{line:?}");
        }
        assert!(help.starts_with("Interface\n  system-manager   [--window] [<page>]\n"));
        assert!(!help.contains("--version"));
    }

    #[test]
    fn reads_what_the_command_line_asks() {
        let ask = |args: &[&str]| parse(args.iter().map(|a| a.to_string()));
        assert_eq!(ask(&["--version", "nonsense"]), Ok(Ask::Version));
        assert_eq!(ask(&["-h"]), Ok(Ask::Help));
        assert_eq!(
            ask(&["--window", "news"]),
            Ok(Ask::Start {
                window: true,
                target: Target::from_id("news"),
                command: None,
                only_new: false
            })
        );
        assert_eq!(
            ask(&["notify", "--new"]),
            Ok(Ask::Start {
                window: false,
                target: None,
                command: Some("notify"),
                only_new: true
            })
        );
        assert_eq!(
            ask(&["check", "--new"]),
            Err("--new goes with notify".into())
        );
        assert_eq!(ask(&["neews"]), Err("unknown argument neews".into()));
        for page in PAGES {
            assert!(Target::from_id(page).is_some(), "{page}");
        }
    }

    #[test]
    fn root_may_only_check() {
        assert!(!refuses_root(Some("check")));
        for command in [None, Some("init"), Some("notify")] {
            assert!(refuses_root(command), "{command:?}");
        }
    }

    #[test]
    fn a_notification_still_standing_is_brought_up_to_date_as_if_new() {
        let now = ["linux 7.2".to_string()];
        let mut state = State {
            notified: now.to_vec(),
            ..State::default()
        };
        assert!(!fresh(&now, &state), "announced before");
        state.notification = Some("bus :1.5 7".into());
        assert!(fresh(&now, &state));
        assert_eq!(
            Answer::parse("shown\tbus :1.5 7\nstanding\n"),
            Answer {
                shown: Some("bus :1.5 7".into()),
                open: false,
                standing: true
            }
        );
        assert!(Answer::parse("shown\tbus :1.5 7\nchosen\topen\n").open);
        assert_eq!(
            Answer::parse(""),
            Answer::default(),
            "closed without an answer"
        );
    }

    #[test]
    fn a_notification_that_ends_forgets_only_itself() {
        let now = vec!["linux 7.2".to_string()];
        let answered = Answer {
            shown: Some("bus :1.5 7".into()),
            ..Answer::default()
        };
        let mut state = State {
            notification: Some("bus :1.5 8".into()),
            ..State::default()
        };
        answered.settle(&mut state, now.clone());
        assert_eq!(
            state.notification.as_deref(),
            Some("bus :1.5 8"),
            "shown meanwhile by another run"
        );
        state.notification = Some("bus :1.5 7".into());
        answered.settle(&mut state, now.clone());
        assert_eq!((state.notification, state.notified), (None, now.clone()));
        let standing = Answer {
            standing: true,
            ..answered
        };
        let mut state = State::default();
        standing.settle(&mut state, now);
        assert_eq!(state.notification.as_deref(), Some("bus :1.5 7"));
    }

    #[test]
    fn notify_by_hand_shows_what_is_pending_and_the_timer_only_what_is_new() {
        assert_eq!(quiet(0, false, false).as_deref(), Some("updates: none"));
        assert_eq!(quiet(0, false, true).as_deref(), Some("updates: none"));
        assert_eq!(quiet(6, false, false), None);
        assert_eq!(quiet(6, true, true), None);
        assert_eq!(
            quiet(6, false, true).as_deref(),
            Some("updates: 6, announced before")
        );
    }
}
