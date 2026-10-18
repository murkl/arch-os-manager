//! `~/.arch-os/state.yaml`: what the program remembers between runs.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct State {
    /// Links of the news already read. Absent until the news were first loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub news_read: Option<Vec<String>>,
    /// The updates the last notification was about, so the same ones are not announced twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notified: Vec<String>,
}

impl State {
    pub fn load() -> State {
        State::load_from(&crate::paths::state())
    }

    /// Changes the file as it is now, so what the interface and the timer each wrote stays.
    pub fn update(change: impl FnOnce(&mut State)) -> State {
        State::update_at(&crate::paths::state(), change)
    }

    /// The file as it is, or nothing remembered where it is missing or does not parse.
    fn load_from(path: &Path) -> State {
        let Ok(text) = std::fs::read_to_string(path) else {
            return State::default();
        };
        serde_saphyr::from_str(&text).unwrap_or_else(|e| {
            crate::warn!("state.yaml is not used: {e}");
            State::default()
        })
    }

    fn update_at(path: &Path, change: impl FnOnce(&mut State)) -> State {
        let mut state = State::load_from(path);
        change(&mut state);
        let result = serde_saphyr::to_string(&state)
            .map_err(std::io::Error::other)
            .and_then(|yaml| crate::paths::write_atomic(path, yaml.as_bytes()));
        if let Err(e) = result {
            crate::warn!("state not saved: {e}");
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_update_keeps_what_another_process_wrote_meanwhile() {
        let dir = std::env::temp_dir().join(format!("sm-state-{}", std::process::id()));
        let path = dir.join("state.yaml");
        let ui = State::update_at(&path, |s| s.news_read = Some(vec!["a".into()]));
        State::update_at(&path, |s| s.notified = vec!["linux 7.2".into()]);
        let later = State::update_at(&path, |s| {
            s.news_read.get_or_insert_with(Vec::new).push("b".into())
        });
        assert_eq!(ui.notified, Vec::<String>::new());
        assert_eq!(later.notified, ["linux 7.2"]);
        assert_eq!(later.news_read, Some(vec!["a".into(), "b".into()]));
        std::fs::write(&path, "news-read: [unclosed").unwrap();
        assert_eq!(State::load_from(&path), State::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
