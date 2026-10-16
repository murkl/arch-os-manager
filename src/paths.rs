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

/// Writes beside the target and renames over it, so a crash never leaves half a file.
pub fn write_atomic(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("new");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}
