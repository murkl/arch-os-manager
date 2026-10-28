//! The dotfiles in chezmoi's source folder: what their script reads of the repository and of
//! chezmoi against this machine.

use std::path::PathBuf;

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

/// What chezmoi said of this machine.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Chezmoi {
    /// Its configuration comes from a template it never made here: a setup asks for it.
    Unconfigured,
    /// A template of the dotfiles does not render, as its details say.
    Failed,
    #[default]
    Ready,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub dir: PathBuf,
    pub host: String,
    pub repo: Repo,
    pub remote: Option<String>,
    /// Commits here and not in the repository, and the other way round.
    pub ahead: usize,
    pub behind: usize,
    /// Changed files of the dotfiles not committed.
    pub changes: usize,
    pub chezmoi: Chezmoi,
    pub files: usize,
    pub secrets: usize,
    /// Files and scripts a setup would apply.
    pub due: usize,
    /// Files changed here since chezmoi wrote them, for a backup.
    pub changed: usize,
}

impl State {
    pub fn parse(out: &str) -> State {
        let mut s = State::default();
        for line in out.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let n = |i: usize| f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
            match f.as_slice() {
                ["dir", v] => s.dir = PathBuf::from(v),
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
                ["config", "missing"] => s.chezmoi = Chezmoi::Unconfigured,
                ["status", "failed"] => s.chezmoi = Chezmoi::Failed,
                ["files", _] => s.files = n(1),
                ["secrets", _] => s.secrets = n(1),
                ["due", _] => s.due = n(1),
                ["changed", _] => s.changed = n(1),
                _ => {}
            }
        }
        s
    }

    /// Whether a backup has anything to take in, commit or upload.
    pub fn unsaved(&self) -> bool {
        self.changed > 0 || self.changes > 0 || self.ahead > 0
    }
}

/// What the dotfiles keep in the home folder, as their script lists it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kept {
    File(String),
    /// A folder holding nothing but files of the dotfiles, its path ending in a slash.
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

/// The dotfiles as the start reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct Dotfiles {
    pub state: State,
}

impl Dotfiles {
    pub fn load() -> Result<Dotfiles, Failure> {
        Ok(Dotfiles {
            state: State::parse(&query(op::DOTFILES_STATE, &[])?),
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
            "dir\t/home/tux/.local/share/chezmoi\nhost\ttux-notebook\nrepo\tready\n",
            "remote\tgit@github.com:tux/dotfiles.git\nahead\t0\nbehind\t0\nchanges\t2\n",
            "files\t31\nsecrets\t3\ndue\t0\nchanged\t0\n",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_state() {
        let s = State::parse(concat!(
            "dir\t/home/tux/.local/share/chezmoi\nhost\ttux\nrepo\tready\n",
            "remote\tgit@github.com:tux/dotfiles.git\nahead\t1\nbehind\t2\nchanges\t3\n",
            "files\t12\nsecrets\t3\ndue\t2\nchanged\t1\n",
        ));
        assert_eq!(s.dir, PathBuf::from("/home/tux/.local/share/chezmoi"));
        assert_eq!((s.host.as_str(), s.repo), ("tux", Repo::Ready));
        assert_eq!(s.remote.as_deref(), Some("git@github.com:tux/dotfiles.git"));
        assert_eq!((s.ahead, s.behind, s.changes), (1, 2, 3));
        assert_eq!((s.files, s.secrets, s.due, s.changed), (12, 3, 2, 1));
        assert_eq!(s.chezmoi, Chezmoi::Ready);
        assert!(s.unsaved());
    }

    #[test]
    fn a_missing_repository_and_an_unconfigured_chezmoi_say_so() {
        let s = State::parse("dir\t/x\nrepo\tmissing\n");
        assert_eq!(s.repo, Repo::Missing);
        assert!(!s.unsaved());
        let s = State::parse("repo\tready\nchanges\t0\nconfig\tmissing\n");
        assert_eq!(s.chezmoi, Chezmoi::Unconfigured);
        let s = State::parse("repo\tready\nstatus\tfailed\n");
        assert_eq!(s.chezmoi, Chezmoi::Failed);
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
}
