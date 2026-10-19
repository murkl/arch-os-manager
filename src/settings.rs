//! `~/.arch-os/settings.yaml`: only what someone chose. An absent key is the default.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// The usage under the header of the start page, on unless switched off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aur_helper: Option<String>,
    /// The entry in the app menu, which opens the window, on unless switched off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_menu: Option<bool>,
    /// How often new updates are announced, one of `NOTIFY`, hourly unless chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
    /// What of Flatpak this program handles, one of `FLATPAK`, all unless chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flatpak: Option<String>,
    /// Journal messages like one of these, numbers aside, are left out of the log page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_log: Vec<String>,
}

pub const AUR_HELPERS: [&str; 2] = ["paru", "yay"];
/// How often updates are announced, `off` for never. The first is the default.
pub const NOTIFY: [&str; 4] = ["hourly", "daily", "weekly", "off"];
/// What of Flatpak is handled: updates and the clean-up, the clean-up only, or nothing, as where
/// another app store updates it. The first is the default.
pub const FLATPAK: [&str; 3] = ["all", "cleanup", "off"];

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
        if let Some(n) = &s.notify
            && !NOTIFY.contains(&n.as_str())
        {
            return Err(format!("notify: {n} is not one of {}", NOTIFY.join(", ")));
        }
        if let Some(f) = &s.flatpak
            && !FLATPAK.contains(&f.as_str())
        {
            return Err(format!("flatpak: {f} is not one of {}", FLATPAK.join(", ")));
        }
        Ok(s)
    }

    /// How often updates are announced, as one of `NOTIFY`.
    /// The time between two announcements, which the timer keeps and a notification waits for an
    /// answer: as `notify` says, an hour where the timer is off.
    pub fn every(&self) -> std::time::Duration {
        let hours = match self.notify() {
            "daily" => 24,
            "weekly" => 7 * 24,
            _ => 1,
        };
        std::time::Duration::from_secs(hours * 3600)
    }

    pub fn notify(&self) -> &'static str {
        NOTIFY
            .iter()
            .copied()
            .find(|n| Some(*n) == self.notify.as_deref())
            .unwrap_or(NOTIFY[0])
    }

    /// What of Flatpak is handled, as one of `FLATPAK`.
    pub fn flatpak(&self) -> &'static str {
        FLATPAK
            .iter()
            .copied()
            .find(|f| Some(*f) == self.flatpak.as_deref())
            .unwrap_or(FLATPAK[0])
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
            usage: Some(false),
            aur_helper: Some("yay".into()),
            app_menu: Some(false),
            notify: Some("daily".into()),
            flatpak: Some("cleanup".into()),
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
        assert!(Settings::parse("notify: often").is_err());
        assert!(Settings::parse("flatpak: updates").is_err());
        assert_eq!(
            Settings::parse("notify: weekly").unwrap().notify(),
            "weekly"
        );
        assert_eq!(Settings::default().notify(), "hourly");
        assert_eq!(Settings::parse("flatpak: off").unwrap().flatpak(), "off");
        assert_eq!(Settings::default().flatpak(), "all");
    }
}
