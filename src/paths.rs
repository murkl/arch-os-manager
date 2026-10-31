//! Where the program keeps its files: `~/.arch-os`, or `$SM_HOME` for a throwaway run.

use std::path::PathBuf;
use std::sync::OnceLock;

static HOME: OnceLock<PathBuf> = OnceLock::new();

pub fn home() -> &'static PathBuf {
    HOME.get_or_init(|| match std::env::var_os("SM_HOME") {
        // Unit tests share one process, and the first to ask fixes the folder for all.
        _ if cfg!(test) => std::env::temp_dir().join(format!("sm-test-{}", std::process::id())),
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".arch-os"),
    })
}

pub fn settings() -> PathBuf {
    home().join("settings.yaml")
}

pub fn state() -> PathBuf {
    home().join("state.yaml")
}

pub fn themes() -> PathBuf {
    home().join("themes")
}

pub fn log() -> PathBuf {
    home().join("logs").join("system-manager.log")
}

pub fn news_archive() -> PathBuf {
    home().join("news.json")
}

static PROGRAM: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// This program where it lay at the start. An update replaces the file while it runs, and the
/// kernel then names the running one as deleted, while the new one lies at the old path.
pub fn program() -> std::io::Result<PathBuf> {
    PROGRAM
        .get_or_init(|| std::env::current_exe().map_err(|e| e.to_string()))
        .clone()
        .map_err(std::io::Error::other)
}

/// Writes beside the target and renames over it, so a crash never leaves half a file. The copy
/// is named by the process: the window and the timer may write the same file at once.
pub fn write_atomic(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    // A link, as dotfiles keep settings, is written through.
    let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("{}.new", std::process::id()));
    let written = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(content).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whichever test asks first, none writes into the real home.
    #[test]
    fn tests_keep_out_of_the_real_home() {
        assert!(
            home().starts_with(std::env::temp_dir()),
            "{home:?}",
            home = home()
        );
    }

    #[test]
    fn the_program_is_where_it_lay_at_the_start() {
        assert_eq!(program().unwrap(), std::env::current_exe().unwrap());
    }

    #[test]
    fn an_atomic_write_replaces_the_file_and_leaves_nothing_beside_it() {
        let dir = std::env::temp_dir().join(format!("sm-atomic-{}", std::process::id()));
        let path = dir.join("state.yaml");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        // A folder in the way: the copy is written, the rename refused.
        std::fs::create_dir_all(dir.join("taken/inside")).unwrap();
        assert!(write_atomic(&dir.join("taken"), b"x").is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_atomic_write_through_a_link_keeps_the_link() {
        let dir = std::env::temp_dir().join(format!("sm-atomic-link-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        let (real, link) = (
            dir.join("dotfiles/settings.yaml"),
            dir.join("settings.yaml"),
        );
        std::fs::write(&real, "one").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, b"two").unwrap();
        assert!(link.symlink_metadata().unwrap().is_symlink());
        assert_eq!(std::fs::read(&real).unwrap(), b"two");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
