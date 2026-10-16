//! `~/.arch-os/settings.yaml`: only what someone chose. An absent key is the default.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aur_helper: Option<String>,
    /// The entry in the app menu, which opens the window; on unless switched off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_menu: Option<bool>,
    /// The hourly announcement of new updates; on unless switched off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<bool>,
    /// Journal messages like one of these, numbers aside, are left out of the log page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_log: Vec<String>,
}

pub const AUR_HELPERS: [&str; 2] = ["paru", "yay"];

/// A switch stored only as an exception: absent is on.
pub fn switch(on: bool) -> Option<bool> {
    (!on).then_some(false)
}

impl Settings {
    pub fn parse(src: &str) -> Result<Settings, String> {
        if src.trim().is_empty() {
            return Ok(Settings::default());
        }
        let s: Settings = serde_saphyr::from_str(src).map_err(|e| e.to_string())?;
        if let Some(l) = &s.language
            && crate::i18n::Lang::from_code(l).is_none()
        {
            return Err(format!("language: {l} is not one of en, de"));
        }
        if let Some(h) = &s.aur_helper
            && h != "none"
            && !AUR_HELPERS.contains(&h.as_str())
        {
            return Err(format!("aur-helper: {h} is not one of paru, yay, none"));
        }
        Ok(s)
    }

    /// The settings, or the defaults plus why the file was not usable.
    pub fn load() -> (Settings, Option<String>) {
        match std::fs::read_to_string(crate::paths::settings()) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
            Err(e) => (Settings::default(), Some(e.to_string())),
            Ok(src) => match Settings::parse(&src) {
                Ok(s) => (s, None),
                Err(e) => (Settings::default(), Some(e)),
            },
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let yaml = serde_saphyr::to_string(self).map_err(std::io::Error::other)?;
        let yaml = if yaml.trim() == "{}" {
            String::new()
        } else {
            yaml
        };
        crate::paths::write_atomic(&crate::paths::settings(), yaml.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_default() {
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
    }

    #[test]
    fn round_trips() {
        let s = Settings {
            language: Some("de".into()),
            theme: Some("nord".into()),
            aur_helper: Some("yay".into()),
            app_menu: Some(false),
            notify: None,
            hidden_log: vec!["ACPI Error: with space".into()],
        };
        let yaml = serde_saphyr::to_string(&s).unwrap();
        assert_eq!(Settings::parse(&yaml).unwrap(), s);
    }

    #[test]
    fn rejects_unknown_keys_and_values() {
        assert!(Settings::parse("colour: red").is_err());
        assert!(Settings::parse("language: fr").is_err());
        assert!(Settings::parse("aur-helper: pikaur").is_err());
    }
}
