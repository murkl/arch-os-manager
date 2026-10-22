//! The dotfiles in `~/.dotfiles`: what their script reads of them against this machine, the groups
//! of the profile and the profiles there are.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::i18n::tf;
use crate::script::{Failure, op, query};

/// The repository, as the script finds it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Repo {
    #[default]
    Missing,
    /// Something else lies there: a bare repository or a folder inside another.
    Invalid,
    Ready,
}

/// Whether this machine ran a script of the profile as it is now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ran {
    Applied,
    Changed,
    /// Named in groups.txt, but the file is gone.
    Missing,
}

impl Ran {
    fn parse(s: &str) -> Ran {
        match s {
            "applied" => Ran::Applied,
            "missing" => Ran::Missing,
            _ => Ran::Changed,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub dir: PathBuf,
    pub profile: String,
    /// The host's name, the profile unless another is chosen.
    pub host: String,
    pub repo: Repo,
    pub remote: Option<String>,
    /// Commits here and not in the repository, and the other way round.
    pub ahead: usize,
    pub behind: usize,
    /// Changed files not committed.
    pub changes: usize,
    /// The profile has no folder yet.
    pub new: bool,
    pub files: usize,
    /// Files of the profile whose link in the home folder is missing.
    pub links: usize,
    pub secrets: usize,
    /// Where the profile has a key, whether this machine holds it unlocked.
    pub unlocked: Option<bool>,
    /// init.sh, and when it ran here last.
    pub init: Option<(Ran, Option<jiff::Timestamp>)>,
    /// The groups switched on, by script.
    pub groups: Vec<(String, Ran)>,
    /// Secrets a setup would write.
    pub due_secrets: usize,
    /// GNOME settings or extensions the profile brings that this machine has not loaded.
    pub due_gnome: bool,
    /// What a backup would save beside the changed files: `packages`, `gnome`, `secrets`.
    pub unsaved: Vec<String>,
    /// When a setup last finished here.
    pub setup: Option<jiff::Timestamp>,
    /// Folders of `~/.dotfiles-backup`.
    pub backups: Vec<String>,
    /// homelab.yaml is there.
    pub homelab: bool,
}

fn timestamp(s: &str) -> Option<jiff::Timestamp> {
    s.parse()
        .ok()
        .and_then(|secs| jiff::Timestamp::from_second(secs).ok())
}

impl State {
    pub fn parse(out: &str) -> State {
        let mut s = State::default();
        for line in out.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let n = |i: usize| f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
            match f.as_slice() {
                ["dir", v] => s.dir = PathBuf::from(v),
                ["profile", v] => s.profile = v.to_string(),
                ["host", v] => s.host = v.to_string(),
                ["repo", v] => {
                    s.repo = match *v {
                        "ready" => Repo::Ready,
                        "invalid" => Repo::Invalid,
                        _ => Repo::Missing,
                    }
                }
                ["remote", v] => s.remote = Some(v.to_string()),
                ["ahead", _] => s.ahead = n(1),
                ["behind", _] => s.behind = n(1),
                ["changes", _] => s.changes = n(1),
                ["new", _] => s.new = true,
                ["files", _] => s.files = n(1),
                ["links", _] => s.links = n(1),
                ["secrets", _] => s.secrets = n(1),
                ["key", v] => s.unlocked = Some(*v == "unlocked"),
                ["init", ran, at] => s.init = Some((Ran::parse(ran), timestamp(at))),
                ["group", script, ran] => s.groups.push((script.to_string(), Ran::parse(ran))),
                ["pending", "secrets", _] => s.due_secrets = n(2),
                ["pending", "gnome"] => s.due_gnome = true,
                ["unsaved", part] => s.unsaved.push(part.to_string()),
                ["setup", at] => s.setup = timestamp(at),
                ["backup", path] => s.backups.push(path.to_string()),
                ["homelab", _] => s.homelab = true,
                _ => {}
            }
        }
        s
    }

    /// Whether a group is switched on in the profile.
    pub fn on(&self, script: &str) -> bool {
        self.groups.iter().any(|(s, _)| s == script)
    }

    /// Whether a backup has anything to save or upload. A new profile has all of it.
    pub fn unsaved(&self) -> bool {
        self.new || self.changes > 0 || self.ahead > 0 || !self.unsaved.is_empty()
    }
}

/// A group of the profile's dotfiles.yaml: a name, a sentence, and the script that sets it up.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub script: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    groups: Option<Vec<Group>>,
}

/// Whether a script path is a plain one inside the profile ending in .sh, as the script of the
/// dotfiles takes it: no shell in a value of the yaml.
fn plain(script: &str) -> bool {
    script.ends_with(".sh")
        && !script.starts_with('/')
        && script
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
        && script
            .split('/')
            .all(|part| !part.is_empty() && part != "..")
}

/// The groups of a profile's dotfiles.yaml, each script checked to be a file of the profile. None
/// without the file.
pub fn groups(profile: &Path) -> Result<Vec<Group>, String> {
    let path = profile.join("dotfiles.yaml");
    let src = match std::fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("dotfiles.yaml: {e}")),
    };
    parse_groups(&src, |script| profile.join(script).is_file())
}

fn parse_groups(src: &str, exists: impl Fn(&str) -> bool) -> Result<Vec<Group>, String> {
    if src.trim().is_empty() {
        return Ok(Vec::new());
    }
    let file: File = serde_saphyr::from_str(src).map_err(|e| format!("dotfiles.yaml: {e}"))?;
    let groups = file.groups.unwrap_or_default();
    for (i, g) in groups.iter().enumerate() {
        let script: &[(&str, &dyn std::fmt::Display)] = &[("script", &g.script)];
        if g.name.trim().is_empty() {
            return Err(tf(
                "dotfiles.yaml: group {n} has no name",
                &[("n", &(i + 1))],
            ));
        }
        if !plain(&g.script) {
            return Err(tf(
                "dotfiles.yaml: {script} is no path of the profile ending in .sh",
                script,
            ));
        }
        if !exists(&g.script) {
            return Err(tf("dotfiles.yaml: {script} does not exist", script));
        }
        if groups[..i].iter().any(|o| o.script == g.script) {
            return Err(tf("dotfiles.yaml: {script} sets up two groups", script));
        }
    }
    Ok(groups)
}

/// What a profile holds, one of them at least, as the script of the dotfiles shapes it.
const PARTS: [&str; 8] = [
    "init.sh",
    "dotfiles.yaml",
    "home",
    "secrets",
    "key.age",
    "packages",
    "gnome",
    "groups.txt",
];

/// The profiles of the dotfiles: every folder at their top holding a part of a profile, by name.
/// Another folder there, as the homelab or pictures kept along, is none.
pub fn profiles(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| PARTS.iter().any(|part| e.path().join(part).exists()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect();
    found.sort();
    found
}

/// What the dotfiles keep in the home folder, as their script lists it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kept {
    File(String),
    /// A folder holding nothing but links of the dotfiles, its path ending in a slash.
    Folder(String),
    Secret(String),
}

impl Kept {
    pub fn path(&self) -> &str {
        match self {
            Kept::File(p) | Kept::Folder(p) | Kept::Secret(p) => p,
        }
    }
}

pub fn parse_kept(out: &str) -> Vec<Kept> {
    out.lines()
        .filter_map(|line| match line.split_once('\t') {
            Some(("file", p)) => Some(Kept::File(p.to_string())),
            Some(("dir", p)) => Some(Kept::Folder(p.to_string())),
            Some(("secret", p)) => Some(Kept::Secret(p.to_string())),
            _ => None,
        })
        .collect()
}

/// The dotfiles as the start reads them: the state, the groups of the profile and the profiles.
#[derive(Clone, Debug, PartialEq)]
pub struct Dotfiles {
    pub state: State,
    pub groups: Result<Vec<Group>, String>,
    pub profiles: Vec<String>,
}

impl Dotfiles {
    pub fn load() -> Result<Dotfiles, Failure> {
        let state = State::parse(&query(op::DOTFILES_STATE, &[])?);
        let (groups, profiles) = match state.repo {
            Repo::Ready => (
                groups(&state.dir.join(&state.profile)),
                profiles(&state.dir),
            ),
            _ => (Ok(Vec::new()), Vec::new()),
        };
        Ok(Dotfiles {
            state,
            groups,
            profiles,
        })
    }

    /// The remote's state fetched first, so ahead and behind tell what it holds now.
    pub fn fetch() -> Result<Dotfiles, Failure> {
        query(op::DOTFILES_FETCH, &[])?;
        Dotfiles::load()
    }
}

/// Dotfiles as the screenshots and the tests of the pages show them.
#[cfg(test)]
pub fn example() -> Dotfiles {
    Dotfiles {
        state: State::parse(concat!(
            "dir\t/home/tux/.dotfiles\nprofile\ttux-notebook\nhost\ttux-notebook\n",
            "repo\tready\nremote\tgit@github.com:tux/dotfiles.git\nahead\t0\nbehind\t0\n",
            "changes\t2\nfiles\t31\nlinks\t0\nsetup\t1792130400\ninit\tapplied\t1792130400\n",
            "group\tgroups/development.sh\tapplied\nsecrets\t3\nkey\tunlocked\n",
        )),
        groups: Ok(vec![
            Group {
                name: "Development".into(),
                description: Some("Neovim, Rust and the linters".into()),
                script: "groups/development.sh".into(),
            },
            Group {
                name: "Gaming".into(),
                description: None,
                script: "groups/gaming.sh".into(),
            },
        ]),
        profiles: vec!["tux-desktop".into(), "tux-notebook".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_state() {
        let s = State::parse(concat!(
            "dir\t/home/tux/.dotfiles\nprofile\ttux-notebook\nhost\ttux\nrepo\tready\n",
            "remote\tgit@github.com:tux/dotfiles.git\nahead\t1\nbehind\t2\nchanges\t3\n",
            "files\t12\nlinks\t2\nsetup\t1792130400\ninit\tchanged\t1792130000\n",
            "group\tgroups/gaming.sh\tapplied\ngroup\tgroups/office.sh\tmissing\n",
            "secrets\t3\nkey\tunlocked\npending\tsecrets\t1\npending\tgnome\n",
            "unsaved\tsecrets\nunsaved\tgnome\nunsaved\tpackages\n",
            "backup\t/home/tux/.dotfiles-backup/2026-10-22-151020\nhomelab\tready\n"
        ));
        assert_eq!(s.dir, PathBuf::from("/home/tux/.dotfiles"));
        assert_eq!((s.profile.as_str(), s.repo), ("tux-notebook", Repo::Ready));
        assert_eq!(s.host, "tux");
        assert_eq!(s.remote.as_deref(), Some("git@github.com:tux/dotfiles.git"));
        assert_eq!((s.ahead, s.behind, s.changes), (1, 2, 3));
        assert_eq!((s.files, s.links, s.secrets), (12, 2, 3));
        assert_eq!(s.unlocked, Some(true));
        assert_eq!(s.setup.unwrap().as_second(), 1792130400);
        let (ran, at) = s.init.unwrap();
        assert_eq!((ran, at.unwrap().as_second()), (Ran::Changed, 1792130000));
        assert!(s.on("groups/gaming.sh") && !s.on("groups/dev.sh"));
        assert_eq!(s.groups[1].1, Ran::Missing);
        assert_eq!((s.due_secrets, s.due_gnome), (1, true));
        assert_eq!(s.unsaved, ["secrets", "gnome", "packages"]);
        assert_eq!(s.backups.len(), 1);
        assert!(s.homelab);
        assert!(!s.new && s.unsaved());
    }

    #[test]
    fn a_missing_repository_and_a_new_profile_say_so() {
        let s = State::parse("dir\t/x\nprofile\tp\nrepo\tmissing\n");
        assert_eq!(s.repo, Repo::Missing);
        assert!(!s.unsaved());
        let s = State::parse("repo\tready\nchanges\t0\nnew\tprofile\n");
        assert!(s.new && s.init.is_none());
        assert!(s.unsaved(), "a new profile is backed up by nothing yet");
        // Never run here, so no time.
        let s = State::parse("init\tchanged\t\n");
        assert_eq!(s.init, Some((Ran::Changed, None)));
    }

    #[test]
    fn groups_are_checked_against_the_profile() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let there = |s: &str| s == "groups/gaming.sh" || s == "office.sh";
        let src = "groups:\n  - name: Gaming\n    description: Steam\n    script: groups/gaming.sh\n  - name: Office\n    script: office.sh\n";
        let groups = parse_groups(src, there).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].description.as_deref(), Some("Steam"));
        assert_eq!(parse_groups("groups:\n", there), Ok(Vec::new()));
        assert_eq!(parse_groups("groups: []\n", there), Ok(Vec::new()));
        assert_eq!(parse_groups("# nothing\n", there), Ok(Vec::new()));
        for (src, says) in [
            ("groups:\n  - name: G\n    script: rm -rf ~.sh\n", "no path"),
            ("groups:\n  - name: G\n    script: ../x.sh\n", "no path"),
            (
                "groups:\n  - name: G\n    script: /usr/bin/x.sh\n",
                "no path",
            ),
            ("groups:\n  - name: G\n    script: gaming.py\n", "no path"),
            (
                "groups:\n  - name: G\n    script: groups/none.sh\n",
                "does not exist",
            ),
            ("groups:\n  - name: ' '\n    script: office.sh\n", "no name"),
            (
                "groups:\n  - name: G\n    script: office.sh\n  - name: H\n    script: office.sh\n",
                "two groups",
            ),
            (
                "groups:\n  - name: G\n    run: office.sh\n",
                "unknown field",
            ),
            ("group: []\n", "unknown field"),
        ] {
            let e = parse_groups(src, there).unwrap_err();
            assert!(
                e.starts_with("dotfiles.yaml: ") && e.contains(says),
                "{src}: {e}"
            );
        }
    }

    #[test]
    fn the_template_of_a_new_profile_has_no_group_yet() {
        let template = std::str::from_utf8(
            crate::embedded::SCRIPTS
                .iter()
                .find(|(f, _)| *f == "dotfiles/profile/dotfiles.yaml")
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(parse_groups(template, |_| false), Ok(Vec::new()));
        // Its example, uncommented, is a group as the loader takes it.
        let example: String = template
            .lines()
            .filter_map(|l| l.strip_prefix("# "))
            .skip_while(|l| !l.starts_with("groups:"))
            .map(|l| format!("{l}\n"))
            .collect();
        let groups = parse_groups(&example, |s| s == "groups/gaming.sh").unwrap();
        assert_eq!(groups[0].name, "Gaming");
    }

    #[test]
    fn reads_what_the_dotfiles_keep() {
        let kept = parse_kept(
            "file\t/home/tux/.zshrc\ndir\t/home/tux/.config/nvim/\nsecret\t/home/tux/.ssh/id_ed25519\n",
        );
        assert_eq!(
            kept,
            [
                Kept::File("/home/tux/.zshrc".into()),
                Kept::Folder("/home/tux/.config/nvim/".into()),
                Kept::Secret("/home/tux/.ssh/id_ed25519".into()),
            ]
        );
        assert_eq!(kept[1].path(), "/home/tux/.config/nvim/");
    }

    #[test]
    fn profiles_are_the_folders_at_the_top() {
        let dir = std::env::temp_dir().join(format!("sm-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in [
            "work/home",
            "home-pc",
            ".git/home",
            "res/wallpaper",
            "homelab",
        ] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("home-pc/init.sh"), "").unwrap();
        std::fs::write(dir.join("homelab/homelab.yaml"), "").unwrap();
        std::fs::write(dir.join("README.md"), "").unwrap();
        assert_eq!(profiles(&dir), ["home-pc", "work"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
