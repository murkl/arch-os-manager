//! `~/.arch-os/settings.yaml`: only what someone chose. An absent key is the default.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::data::cleanup::Part;

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
    /// How a run shows, one of `RUNS`, the overview unless chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<String>,
    /// The entry in the app menu, which opens the window, on unless switched off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_menu: Option<bool>,
    /// How often new updates are announced, one of `NOTIFY`, hourly unless chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
    /// Flatpak's updates, on unless switched off.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "flatpak"
    )]
    pub flatpak: Option<bool>,
    /// The parts of the clean-up taken otherwise than `Part::default` says, by id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cleanup: BTreeMap<String, bool>,
    /// Journal messages like one of these, numbers aside, are left out of the log page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_log: Vec<String>,
    /// The dotfiles in ~/.dotfiles, off unless switched on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dotfiles: Option<bool>,
    /// The profile of the dotfiles, the host's name unless chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dotfiles_profile: Option<String>,
    /// The homelab of the dotfiles, off unless switched on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homelab: Option<bool>,
}

pub const AUR_HELPERS: [&str; 2] = ["paru", "yay"];
/// How often updates are announced, `off` for never. The first is the default.
pub const NOTIFY: [&str; 4] = ["hourly", "daily", "weekly", "off"];
/// How a run shows: as the overview, or in the terminal only, where the person answers every
/// question. The first is the default.
pub const RUNS: [&str; 2] = ["overview", "terminal"];

/// A switch stored only as an exception: absent is on.
pub fn switch(on: bool) -> Option<bool> {
    (!on).then_some(false)
}

/// `flatpak` as a switch, or as 2.1 and earlier wrote it: `all` for on, `cleanup` and `off` for
/// off. Their `off` left Flatpak out of the clean-up too.
#[derive(Deserialize)]
#[serde(untagged)]
enum Flatpak {
    Switch(bool),
    Named(String),
}

fn flatpak<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Flatpak::deserialize(d)? {
        Flatpak::Switch(on) => Ok(switch(on)),
        Flatpak::Named(n) => match n.as_str() {
            "all" => Ok(None),
            "cleanup" | "off" => Ok(Some(false)),
            _ => Err(serde::de::Error::custom(format!(
                "flatpak: {n} is not true or false"
            ))),
        },
    }
}

impl Settings {
    pub fn parse(src: &str) -> Result<Settings, String> {
        if src.trim().is_empty() {
            return Ok(Settings::default());
        }
        let mut s: Settings = serde_saphyr::from_str(src).map_err(|e| e.to_string())?;
        // As text: untyped, YAML reads off as false.
        #[derive(Deserialize)]
        struct Before {
            #[serde(default)]
            flatpak: Option<String>,
        }
        if let Ok(Before {
            flatpak: Some(before),
        }) = serde_saphyr::from_str(src)
            && before == "off"
        {
            s.cleanup.entry(Part::Flatpak.id().into()).or_insert(false);
        }
        if let Some(part) = s.cleanup.keys().find(|p| Part::from_id(p).is_none()) {
            let ids: Vec<&str> = Part::ALL.into_iter().map(Part::id).collect();
            return Err(format!("cleanup: {part} is not one of {}", ids.join(", ")));
        }
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
        if let Some(r) = &s.runs
            && !RUNS.contains(&r.as_str())
        {
            return Err(format!("runs: {r} is not one of {}", RUNS.join(", ")));
        }
        if let Some(p) = &s.dotfiles_profile
            && (p.is_empty() || p.starts_with('.') || p.contains('/'))
        {
            return Err(format!(
                "dotfiles-profile: {p} is no folder name, without / and not hidden"
            ));
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

    /// How a run shows, as one of `RUNS`.
    pub fn runs(&self) -> &'static str {
        RUNS.iter()
            .copied()
            .find(|r| Some(*r) == self.runs.as_deref())
            .unwrap_or(RUNS[0])
    }

    /// Whether a run shows in the terminal only, where the person answers every question.
    pub fn terminal(&self) -> bool {
        self.runs() == "terminal"
    }

    /// Whether the dotfiles are switched on.
    pub fn dotfiles(&self) -> bool {
        self.dotfiles.unwrap_or(false)
    }

    /// Whether the homelab is switched on, which lives in the dotfiles.
    pub fn homelab(&self) -> bool {
        self.dotfiles() && self.homelab.unwrap_or(false)
    }

    /// Whether the clean-up takes a part, as chosen or by default.
    pub fn cleans(&self, part: Part) -> bool {
        self.cleanup
            .get(part.id())
            .copied()
            .unwrap_or(part.default())
    }

    /// Takes a part into the clean-up or out of it, stored only where that is not the default.
    pub fn set_cleans(&mut self, part: Part, on: bool) {
        if on == part.default() {
            self.cleanup.remove(part.id());
        } else {
            self.cleanup.insert(part.id().into(), on);
        }
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
            runs: Some("terminal".into()),
            app_menu: Some(false),
            notify: Some("daily".into()),
            flatpak: Some(false),
            cleanup: BTreeMap::from([("docker".into(), true), ("flatpak".into(), false)]),
            hidden_log: vec!["ACPI Error: with space".into()],
            dotfiles: Some(true),
            dotfiles_profile: Some("work laptop".into()),
            homelab: Some(true),
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
        assert!(Settings::parse("runs: console").is_err());
        assert!(Settings::parse("flatpak: updates").is_err());
        assert!(Settings::parse("cleanup: {trash: true}").is_err());
        assert!(Settings::parse("cleanup: {docker: maybe}").is_err());
        assert!(Settings::parse("dotfiles: maybe").is_err());
        assert!(Settings::parse("dotfiles-profile: ../x").is_err());
        assert!(Settings::parse("dotfiles-profile: .git").is_err());
        assert!(Settings::parse("dotfiles-profile: ''").is_err());
        assert!(!Settings::default().dotfiles());
        assert!(Settings::parse("dotfiles: true").unwrap().dotfiles());
        assert!(
            !Settings::parse("homelab: true").unwrap().homelab(),
            "only with the dotfiles"
        );
        assert!(
            Settings::parse("dotfiles: true\nhomelab: true")
                .unwrap()
                .homelab()
        );
        assert_eq!(
            Settings::parse("notify: weekly").unwrap().notify(),
            "weekly"
        );
        assert_eq!(Settings::default().notify(), "hourly");
        assert!(Settings::parse("runs: terminal").unwrap().terminal());
        assert!(!Settings::default().terminal());
    }

    #[test]
    fn flatpak_as_2_1_wrote_it_keeps_its_meaning() {
        let read = |src: &str| {
            let s = Settings::parse(src).unwrap();
            (s.flatpak, s.cleans(Part::Flatpak))
        };
        assert_eq!(read(""), (None, true));
        assert_eq!(read("flatpak: all"), (None, true));
        assert_eq!(read("flatpak: cleanup"), (Some(false), true));
        assert_eq!(read("flatpak: off"), (Some(false), false));
        assert_eq!(read("flatpak: false"), (Some(false), true));
        assert_eq!(read("flatpak: true"), (None, true));
        assert_eq!(
            read("flatpak: off\ncleanup: {flatpak: true}"),
            (Some(false), true)
        );
        let saved = serde_saphyr::to_string(&Settings::parse("flatpak: off").unwrap()).unwrap();
        assert_eq!(Settings::parse(&saved).unwrap().flatpak, Some(false));
        assert!(!saved.contains("off"), "{saved}");
    }

    #[test]
    fn a_part_is_stored_only_where_it_is_not_the_default() {
        let mut s = Settings::default();
        assert!(s.cleans(Part::Orphans) && !s.cleans(Part::Docker));
        s.set_cleans(Part::Docker, true);
        s.set_cleans(Part::Orphans, false);
        assert_eq!(
            s.cleanup,
            BTreeMap::from([("docker".into(), true), ("orphans".into(), false)])
        );
        s.set_cleans(Part::Docker, false);
        s.set_cleans(Part::Orphans, true);
        assert!(s.cleanup.is_empty());
    }
}
