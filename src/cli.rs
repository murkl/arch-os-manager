//! The command line: the interface, or one of three answers for installers, scripts and timers.

use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::Duration;

use crate::data::features::Features;
use crate::data::updates::{self, Pending, Source};
use crate::i18n::{self, Lang, t, tn};
use crate::script::{self, Failure, op, query};
use crate::settings::Settings;
use crate::state::State;
use crate::ui::Target;

/// The pages the command line opens on.
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
                (
                    "init",
                    s(t("Puts the menu entry and the notification in place")),
                ),
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

/// What the command line asks for.
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
    let (window, target, command, only_new) = match parse(std::env::args().skip(1)) {
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
    script::cleanup();
    code
}

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

/// The pending updates, with the AUR helper the settings choose.
fn pending() -> Result<Pending, Failure> {
    let (_, features) = setup()?;
    if let Some(p) = script::missing(op::UPDATE_LIST, &features) {
        return Err(Failure::new(op::UPDATE_LIST, format!("needs {p}")));
    }
    Ok(updates::parse(&query(op::UPDATE_LIST, &[])?))
}

fn check() -> ExitCode {
    match pending() {
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

/// How long a notification waits for a click. The timer comes back within the hour anyway.
const ANSWER: Duration = Duration::from_secs(3600);

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

fn notify(only_new: bool) -> ExitCode {
    let (list, complete) = match pending() {
        Ok(p) => (p.updates, p.failed.is_empty()),
        Err(f) => {
            crate::warn!("notify: {}", f.detail);
            return ExitCode::FAILURE;
        }
    };
    let now: Vec<String> = list
        .iter()
        .map(|u| format!("{} {}", u.name, u.new))
        .collect();
    let notified = State::load().notified;
    let fresh = now.iter().any(|u| !notified.contains(u));
    if let Some(status) = quiet(list.len(), fresh, only_new) {
        // Nothing pending forgets what was announced, unless a source could not tell.
        if list.is_empty() && complete {
            State::update(|s| s.notified = now);
        }
        crate::info!("notify: {status}");
        println!("{status}");
        return ExitCode::SUCCESS;
    }
    let text = tn(
        "{n} update is ready.",
        "{n} updates are ready.",
        list.len(),
        &[],
    );
    // The script stops waiting by itself. The limit catches one that does not.
    let shown = script::query_within(
        op::DESKTOP_NOTIFY,
        &[
            t("System update"),
            &text,
            t("Update"),
            &ANSWER.as_secs().to_string(),
        ],
        ANSWER + Duration::from_secs(60),
    );
    // Remembered only once shown, so one the desktop missed comes again.
    if shown.is_ok() {
        State::update(|s| s.notified = now);
    }
    match shown {
        Ok(action) if action.trim() == "open" => {
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
