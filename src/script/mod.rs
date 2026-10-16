//! The scripts in `scripts/`, the only code that touches the system, and how they run.
//!
//! A script is a folder: `script.sh` with one `op_<name>` function per operation, and
//! `script.yaml` declaring them. `lib.sh` beside the folders holds what they share.

mod lines;
mod progress;
mod prompt;
mod query;
mod run;
mod session;
pub mod sudo;

pub use progress::{Phase, Progress};
pub use prompt::Prompt;
pub use query::{query, query_within};
pub use run::Run;
pub use session::Event;

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::os::fd::{BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::i18n::t;

/// An operation of a script: `update run`, `packages install`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Op {
    pub script: &'static str,
    pub name: &'static str,
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} {}", self.script, self.name)
    }
}

macro_rules! ops {
    ($($konst:ident = $script:literal $name:literal),* $(,)?) => {
        $(pub const $konst: Op = Op { script: $script, name: $name };)*
        /// Every operation, so a test can hold the list against the folders.
        pub const ALL: &[Op] = &[$($konst),*];
    };
}

/// The operations by name; one the scripts lack fails the tests, not a run.
pub mod op {
    use super::Op;
    ops! {
        CLEANUP_PLAN = "cleanup" "plan", CLEANUP_RUN = "cleanup" "run",
        DESKTOP_APPLY = "desktop" "apply", DESKTOP_NOTIFY = "desktop" "notify", DESKTOP_OPEN = "desktop" "open",
        DESKTOP_EXTENSION_INSTALL = "desktop" "extension-install",
        DESKTOP_EXTENSION_REMOVE = "desktop" "extension-remove",
        FEATURES_PROBE = "features" "probe", FEATURES_INSTALL = "features" "install",
        FEATURES_REMOVE = "features" "remove",
        FIRMWARE_LIST = "firmware" "list", FIRMWARE_UPDATE = "firmware" "update",
        LOG_ERRORS = "log" "errors",
        MIGRATE_READ = "migrate" "read", MIGRATE_CLEAN = "migrate" "clean",
        MIRRORS_LIST = "mirrors" "list", MIRRORS_RANK = "mirrors" "rank",
        NEWS_FETCH = "news" "fetch",
        PACKAGES_INSTALLED = "packages" "installed", PACKAGES_SEARCH = "packages" "search",
        PACKAGES_AUR_SEARCH = "packages" "aur-search", PACKAGES_AUR_INFO = "packages" "aur-info",
        PACKAGES_INFO = "packages" "info", PACKAGES_VERSIONS = "packages" "versions",
        PACKAGES_PKGBUILD = "packages" "pkgbuild", PACKAGES_INSTALL = "packages" "install",
        PACKAGES_REMOVE = "packages" "remove", PACKAGES_DOWNGRADE = "packages" "downgrade",
        PACNEW_LIST = "pacnew" "list", PACNEW_DIFF = "pacnew" "diff", PACNEW_TAKE = "pacnew" "take",
        PACNEW_KEEP = "pacnew" "keep", PACNEW_MERGE = "pacnew" "merge",
        REPAIR_RUN = "repair" "run",
        SERVICES_LIST = "services" "list", SERVICES_LOG = "services" "log",
        SERVICES_SYSTEM = "services" "system", SERVICES_USER = "services" "user",
        SYSTEM_STATE = "system" "state", SYSTEM_NOPASSWD = "system" "nopasswd",
        SYSTEM_AUTH = "system" "auth", SYSTEM_REBOOT = "system" "reboot",
        UPDATE_LIST = "update" "list", UPDATE_RUN = "update" "run",
    }
}

/// `script.yaml`: the operations, and what the whole script shares.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    #[serde(default)]
    pub ops: BTreeMap<String, OpMeta>,
    /// Exit codes from 10 on: headline, then explanation.
    #[serde(default)]
    pub errors: BTreeMap<i32, String>,
    /// Output a failure is recognised by; lib.yaml's apply to every script.
    #[serde(default)]
    pub hints: BTreeMap<String, Hint>,
    /// features only: the optional packages by kind, such as `aur-helper`, then by name.
    #[serde(default)]
    pub probe: BTreeMap<String, BTreeMap<String, Probe>>,
    /// features only: what a package may need of the system, with why it is not offered
    /// without; `fact_<name>` in the script tells whether it is there.
    #[serde(default)]
    pub facts: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpMeta {
    pub title: String,
    /// Asked before the run: the question, then what happens.
    #[serde(default)]
    pub confirm: Option<String>,
    /// Calls sudo, so the password is asked before the run.
    #[serde(default)]
    pub root: bool,
    /// Optional packages it cannot run without.
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub steps: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hint {
    /// Headline, then explanation.
    pub text: String,
    /// The page that fixes it, by the id the command line knows it by.
    #[serde(default)]
    pub offer: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// The call whose answer proves the package works.
    pub command: Vec<String>,
    /// What the package adds, for the page that lists them.
    pub adds: String,
    /// The fact it is offered on.
    #[serde(default)]
    pub needs: Option<String>,
}

fn file(path: &str) -> &'static [u8] {
    crate::embedded::SCRIPTS
        .iter()
        .find(|(f, _)| *f == path)
        .map(|(_, b)| *b)
        .unwrap_or_else(|| panic!("scripts/{path} is not embedded"))
}

pub fn parse_meta(src: &[u8]) -> Result<Meta, String> {
    let src = std::str::from_utf8(src).map_err(|e| e.to_string())?;
    serde_saphyr::from_str(src).map_err(|e| e.to_string())
}

/// The yaml of a script, by its folder; `lib` is lib.yaml.
pub fn meta(script: &str) -> &'static Meta {
    static METAS: OnceLock<Mutex<HashMap<String, &'static Meta>>> = OnceLock::new();
    let mut metas = METAS.get_or_init(Default::default).lock().unwrap();
    if let Some(m) = metas.get(script) {
        return m;
    }
    let path = if script == "lib" {
        "lib.yaml".to_string()
    } else {
        format!("{script}/script.yaml")
    };
    let parsed = parse_meta(file(&path)).unwrap_or_else(|e| panic!("scripts/{path}: {e}"));
    let leaked: &'static Meta = Box::leak(Box::new(parsed));
    metas.insert(script.to_string(), leaked);
    leaked
}

impl Op {
    pub fn meta(self) -> &'static OpMeta {
        meta(self.script)
            .ops
            .get(self.name)
            .unwrap_or_else(|| panic!("{self} is not in its script.yaml"))
    }

    pub fn title(self) -> &'static str {
        t(&self.meta().title)
    }
}

/// Whether the optional packages an operation needs are all there.
pub fn available(op: Op, features: &crate::data::features::Features) -> bool {
    op.meta().needs.iter().all(|p| features.ok(p))
}

static AUR: Mutex<String> = Mutex::new(String::new());

/// The AUR helper scripts get as `$SM_AUR`; empty for none.
pub fn set_aur(helper: Option<&str>) {
    *AUR.lock().unwrap() = helper.unwrap_or_default().to_string();
}

pub fn aur() -> Option<String> {
    let helper = AUR.lock().unwrap().clone();
    (!helper.is_empty()).then_some(helper)
}

static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// The embedded scripts written out once per process, readable by this user only.
fn dir() -> Result<&'static Path, String> {
    DIR.get_or_init(|| {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("system-manager-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        for (name, bytes) in crate::embedded::SCRIPTS {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&path, bytes).map_err(|e| format!("{name}: {e}"))?;
        }
        Ok(dir)
    })
    .as_ref()
    .map(PathBuf::as_path)
    .map_err(Clone::clone)
}

pub fn cleanup() {
    if let Some(Ok(dir)) = DIR.get() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn command(op: Op, args: &[String]) -> Result<Command, String> {
    let mut cmd = Command::new("bash");
    cmd.arg("--noprofile")
        .arg("--norc")
        .arg(dir()?.join(op.script).join("script.sh"))
        .arg(op.name)
        .args(args);
    cmd.env("LC_ALL", "C.UTF-8")
        .env("SM_AUR", AUR.lock().unwrap().as_str())
        .env("SUDO_PROMPT", sudo::PROMPT)
        .env_remove("BASH_ENV")
        .env_remove("ENV");
    Ok(cmd)
}

/// Puts `fd` at 3 in the child, where lib.sh writes its events. Runs between fork and exec.
fn attach_fd3(fd: RawFd) -> std::io::Result<()> {
    // SAFETY: fd stays open in the parent until spawn returns; 3 is ours to replace in the child.
    unsafe {
        let source = BorrowedFd::borrow_raw(fd);
        if fd == 3 {
            rustix::io::fcntl_setfd(source, rustix::io::FdFlags::empty())?;
        } else {
            let mut three = std::mem::ManuallyDrop::new(OwnedFd::from_raw_fd(3));
            rustix::io::dup2(source, &mut three)?;
        }
    }
    Ok(())
}

fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    Ok(rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?)
}

/// The line `n` of a script as written, for a failure in a pipeline, where bash names only
/// the last command.
fn source_line(file: &str, n: usize) -> Option<String> {
    let (_, body) = crate::embedded::SCRIPTS.iter().find(|(f, _)| *f == file)?;
    std::str::from_utf8(body)
        .ok()?
        .lines()
        .nth(n.checked_sub(1)?)
        .map(|l| l.trim().to_string())
}

/// Why an operation did not succeed, in the words of its yaml where it has them.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub op: Op,
    pub code: Option<i32>,
    /// Headline and explanation of a declared error or a recognised hint, untranslated.
    pub known: Option<&'static str>,
    /// The page a recognised hint names as the fix.
    pub offer: Option<&'static str>,
    /// Where it failed: file, line, command, exit code; or why it never ran.
    pub detail: String,
    /// The last lines of its output.
    pub tail: Vec<String>,
}

impl Failure {
    pub fn new(op: Op, detail: impl Into<String>) -> Failure {
        Failure {
            op,
            code: None,
            known: None,
            offer: None,
            detail: detail.into(),
            tail: Vec::new(),
        }
    }

    /// From an exit code, the first `fail` line lib.sh wrote, and the output.
    pub fn classify(op: Op, code: Option<i32>, fail: Option<&str>, tail: Vec<String>) -> Failure {
        let own = meta(op.script);
        // The last line that names a cause; the longest key it holds is the most specific.
        let hint = || {
            tail.iter().rev().find_map(|line| {
                own.hints
                    .iter()
                    .chain(&meta("lib").hints)
                    .filter(|(key, _)| line.contains(key.as_str()))
                    .max_by_key(|(key, _)| key.len())
                    .map(|(_, h)| h)
            })
        };
        let (known, offer) = match code.and_then(|c| own.errors.get(&c)) {
            Some(text) => (Some(text.as_str()), None),
            None => match hint() {
                Some(h) => (Some(h.text.as_str()), h.offer.as_deref()),
                None => (None, None),
            },
        };
        let detail = match (fail.map(|f| f.split('\t').collect::<Vec<_>>()), code) {
            (Some(f), _) if f.len() >= 5 => {
                let line = f[3]
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| source_line(f[2], n))
                    .unwrap_or(f[4].to_string());
                format!("{}:{}: {} (exit {})", f[2], f[3], line, f[1])
            }
            (_, Some(c)) => format!("{op}: exit {c}"),
            (_, None) => format!("{op}: stopped by a signal"),
        };
        Failure {
            op,
            code,
            known,
            offer,
            detail,
            tail,
        }
    }

    pub fn headline(&self) -> String {
        match self.known {
            Some(text) => t(text).lines().next().unwrap_or_default().to_string(),
            None => crate::i18n::tf("{title} failed", &[("title", &self.op.title())]),
        }
    }

    pub fn explanation(&self) -> String {
        self.known
            .map(|k| t(k).lines().skip(1).collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The folders with a script.
    fn scripts() -> Vec<String> {
        let mut v: Vec<String> = crate::embedded::SCRIPTS
            .iter()
            .filter_map(|(f, _)| f.strip_suffix("/script.sh").map(str::to_string))
            .collect();
        v.sort();
        v
    }

    fn body(script: &str) -> String {
        String::from_utf8(file(&format!("{script}/script.sh")).to_vec()).unwrap()
    }

    /// Each function of a script, by name, with its lines; comments left out.
    fn functions(script: &str) -> BTreeMap<String, Vec<String>> {
        let mut found = BTreeMap::new();
        let mut current: Option<(String, Vec<String>)> = None;
        for line in body(script).lines() {
            if let Some(name) = line.strip_suffix("() {") {
                current = Some((name.to_string(), Vec::new()));
            } else if line == "}" {
                if let Some((name, lines)) = current.take() {
                    found.insert(name, lines);
                }
            } else if let Some((_, lines)) = current.as_mut()
                && !line.trim_start().starts_with('#')
            {
                lines.push(line.to_string());
            }
        }
        found
    }

    #[test]
    fn every_folder_has_both_files_and_every_operation_a_name() {
        for s in scripts() {
            assert!(
                crate::embedded::SCRIPTS
                    .iter()
                    .any(|(f, _)| *f == format!("{s}/script.yaml")),
                "{s} has no script.yaml"
            );
        }
        let mut named: Vec<String> = op::ALL.iter().map(|o| o.to_string()).collect();
        named.sort();
        let mut declared: Vec<String> = scripts()
            .iter()
            .flat_map(|s| meta(s).ops.keys().map(move |o| format!("{s} {o}")))
            .collect();
        declared.sort();
        assert_eq!(named, declared);
    }

    #[test]
    fn every_operation_is_a_function_and_every_function_named_op_an_operation() {
        for s in scripts() {
            let mut defined: Vec<String> = functions(&s)
                .into_keys()
                .filter_map(|f| f.strip_prefix("op_").map(|o| o.replace('_', "-")))
                .collect();
            defined.sort();
            let declared: Vec<String> = meta(&s).ops.keys().cloned().collect();
            assert_eq!(defined, declared, "{s}");
        }
    }

    #[test]
    fn root_is_declared_exactly_where_the_operation_calls_sudo() {
        for o in op::ALL {
            let lines = &functions(o.script)[&format!("op_{}", o.name.replace('-', "_"))];
            let calls = lines.iter().any(|l| l.contains("sudo "));
            assert_eq!(
                o.meta().root,
                calls,
                "{o}: root is {} but sudo {}",
                o.meta().root,
                if calls { "is called" } else { "is not" }
            );
        }
        // A helper that elevates would hide it from the line above.
        for s in scripts() {
            for (name, lines) in functions(&s) {
                if !name.starts_with("op_") {
                    assert!(
                        !lines.iter().any(|l| l.contains("sudo ")),
                        "{s}: {name} calls sudo outside an operation"
                    );
                }
            }
        }
    }

    #[test]
    fn declared_errors_are_exactly_the_codes_exited_with() {
        for s in scripts() {
            let mut used: Vec<i32> = body(&s)
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .flat_map(|l| {
                    l.match_indices("exit ")
                        .map(|(i, _)| l[i + 5..].to_string())
                        .collect::<Vec<_>>()
                })
                .filter_map(|rest| {
                    rest.split(|c: char| !c.is_ascii_digit())
                        .next()
                        .and_then(|d| d.parse().ok())
                })
                .filter(|c| *c >= 10)
                .collect();
            used.sort();
            used.dedup();
            let declared: Vec<i32> = meta(&s).errors.keys().copied().collect();
            assert_eq!(used, declared, "{s}");
        }
    }

    #[test]
    fn every_script_sources_lib_first_and_hands_over_to_the_operation_last() {
        for s in scripts() {
            let text = body(&s);
            let first: Vec<&str> = text.lines().take(2).collect();
            assert_eq!(
                first,
                [
                    "# shellcheck source=../lib.sh",
                    ". \"${BASH_SOURCE[0]%/*}/../lib.sh\" || exit 1"
                ],
                "{s}"
            );
            assert_eq!(text.lines().last(), Some("operation \"$@\""), "{s}");
        }
    }

    #[test]
    fn needs_name_only_probed_packages_and_offers_only_pages() {
        for o in op::ALL {
            for p in &o.meta().needs {
                assert!(
                    crate::data::features::package(p).is_some(),
                    "{o} needs {p}, which features does not probe"
                );
            }
        }
        for s in scripts().iter().map(String::as_str).chain(["lib"]) {
            for (key, h) in &meta(s).hints {
                if let Some(page) = &h.offer {
                    assert!(
                        crate::ui::Target::from_id(page).is_some(),
                        "{s}: hint {key:?} offers {page}, no page"
                    );
                }
            }
        }
    }

    #[test]
    fn every_fact_a_package_needs_is_declared_and_told_by_a_function() {
        let features = meta("features");
        let defined = functions("features");
        for name in features.facts.keys() {
            assert!(
                defined.contains_key(&format!("fact_{name}")),
                "features declares {name} but defines no fact_{name}"
            );
        }
        for (_, p, probe) in crate::data::features::packages() {
            if let Some(need) = &probe.needs {
                assert!(
                    features.facts.contains_key(need),
                    "{p} needs {need}, which features does not declare"
                );
            }
        }
    }

    #[test]
    fn classify_prefers_the_declared_error_then_hints_then_the_trap() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let declared = Failure::classify(op::UPDATE_LIST, Some(10), None, vec![]);
        assert!(declared.known.is_some());
        let hinted = Failure::classify(
            op::UPDATE_RUN,
            Some(1),
            None,
            vec!["error: failed to init transaction (unable to lock database)".into()],
        );
        assert!(hinted.known.unwrap().contains("locked"));
        assert_eq!(hinted.offer, Some("repair"));
        let specific = Failure::classify(
            op::UPDATE_RUN,
            Some(1),
            None,
            vec!["error: failed to synchronize all databases (invalid or corrupted database (PGP signature))".into()],
        );
        assert_eq!(specific.offer, Some("mirrors"));
        let trapped = Failure::classify(
            op::PACKAGES_INSTALLED,
            Some(1),
            Some("fail\t3\tpackages/script.sh\t10\tsed 's/^/pkg/'"),
            vec![],
        );
        assert_eq!(trapped.known, None);
        assert_eq!(
            trapped.detail,
            "packages/script.sh:10: pacman -Q | sed 's/^/pkg\\t/; s/ /\\t/' (exit 3)"
        );
    }
}
