//! Where the program keeps its files: `~/.arch-os`, or `$SM_HOME` for a throwaway run.

use std::path::PathBuf;
use std::sync::OnceLock;

static HOME: OnceLock<PathBuf> = OnceLock::new();

pub fn home() -> &'static PathBuf {
    HOME.get_or_init(|| match std::env::var_os("SM_HOME") {
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

pub fn news_cache() -> PathBuf {
    home().join("cache").join("news.xml")
}

/// Writes beside the target and renames over it, so a crash never leaves half a file. The copy
/// is named by the process: the window and the timer may write the same file at once.
pub fn write_atomic(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
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
}
