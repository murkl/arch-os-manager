//! `~/.arch-os/state.yaml`: what the program remembers between runs.

use serde::{Deserialize, Serialize};

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
        std::fs::read_to_string(crate::paths::state())
            .ok()
            .and_then(|s| serde_saphyr::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let result = serde_saphyr::to_string(self)
            .map_err(std::io::Error::other)
            .and_then(|yaml| crate::paths::write_atomic(&crate::paths::state(), yaml.as_bytes()));
        if let Err(e) = result {
            crate::warn!("state not saved: {e}");
        }
    }
}
