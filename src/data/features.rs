//! Which optional packages are there and working, and what of the system they need.

use std::collections::BTreeSet;

use crate::script::{Failure, meta, op, query};
use crate::settings::AUR_HELPERS;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Working, with the installed version where pacman knows it.
    Ok(String),
    Missing,
    /// Installed, but it did not answer.
    Broken,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Features {
    pub list: Vec<(String, Status)>,
    /// The facts of the system that hold, such as a desktop.
    pub facts: BTreeSet<String>,
}

impl Features {
    pub fn load() -> Result<Features, Failure> {
        let m = meta("features");
        let args: Vec<String> = m
            .probe
            .iter()
            .map(|(p, probe)| format!("{p}={}", probe.command.join(" ")))
            .chain(m.facts.keys().map(|f| format!("@{f}")))
            .collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        Ok(Features::parse(&query(op::FEATURES_PROBE, &args)?))
    }

    pub fn parse(out: &str) -> Features {
        let mut features = Features::default();
        for line in out.lines() {
            let mut f = line.split('\t');
            let (Some(name), Some(status)) = (f.next(), f.next()) else {
                continue;
            };
            if let Some(fact) = name.strip_prefix('@') {
                if status == "ok" {
                    features.facts.insert(fact.to_string());
                }
                continue;
            }
            let status = match status {
                "ok" => Status::Ok(f.next().unwrap_or_default().to_string()),
                "broken" => Status::Broken,
                _ => Status::Missing,
            };
            features.list.push((name.to_string(), status));
        }
        features
    }

    /// Why something that needs a fact is not offered, untranslated; none while it holds.
    pub fn lacks(&self, fact: &str) -> Option<&'static str> {
        if self.facts.contains(fact) {
            return None;
        }
        meta("features").facts.get(fact).map(String::as_str)
    }

    /// Why a package is not offered, untranslated; none when it is.
    pub fn unmet(&self, package: &str) -> Option<&'static str> {
        let need = meta("features").probe.get(package)?.needs.as_deref()?;
        self.lacks(need)
    }

    pub fn ok(&self, package: &str) -> bool {
        self.list
            .iter()
            .any(|(p, s)| p == package && matches!(s, Status::Ok(_)))
    }

    /// The helper scripts get: the chosen one if it works, else the first that does.
    pub fn aur_helper(&self, chosen: Option<&str>) -> Option<&'static str> {
        match chosen {
            Some("none") => None,
            Some(h) => AUR_HELPERS.into_iter().find(|x| *x == h && self.ok(x)),
            None => AUR_HELPERS.into_iter().find(|x| self.ok(x)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pkgbuild_offers_exactly_the_probed_packages() {
        let pkgbuild = include_str!("../../aur/PKGBUILD");
        let block = pkgbuild
            .split("optdepends=(")
            .nth(1)
            .and_then(|b| b.split(')').next())
            .unwrap();
        let mut offered: Vec<&str> = block
            .lines()
            .filter_map(|l| {
                l.trim()
                    .strip_prefix('\'')
                    .and_then(|l| l.split(':').next())
            })
            .filter(|p| *p != "ttf-nerd-fonts-symbols")
            .collect();
        offered.sort();
        let probed: Vec<&str> = meta("features")
            .probe
            .iter()
            .filter(|(_, p)| !p.extension)
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(offered, probed);
        assert!(pkgbuild.contains(&format!("url='{}'", env!("CARGO_PKG_REPOSITORY"))));
    }

    #[test]
    fn offers_a_package_only_where_what_it_needs_holds() {
        let on_a_desktop = Features::parse("meld\tmissing\n@desktop\tok\n@gnome\tmissing\n");
        assert_eq!(on_a_desktop.unmet("meld"), None);
        assert_eq!(on_a_desktop.unmet("fwupd"), None);
        assert_eq!(on_a_desktop.lacks("gnome"), Some("needs GNOME"));
        assert!(on_a_desktop.list.iter().all(|(p, _)| !p.starts_with('@')));
        let headless = Features::parse("meld\tmissing\n@desktop\tmissing\n");
        assert_eq!(headless.unmet("meld"), Some("needs a desktop"));
    }

    #[test]
    fn reads_the_probe_and_picks_paru_first() {
        let f =
            Features::parse("paru\tok\t2.1.0-1\nyay\tok\t12.5.0-1\nmeld\tmissing\nfwupd\tbroken\n");
        assert!(f.ok("paru") && f.ok("yay") && !f.ok("meld") && !f.ok("fwupd"));
        assert_eq!(f.aur_helper(None), Some("paru"));
        assert_eq!(f.aur_helper(Some("yay")), Some("yay"));
        assert_eq!(f.aur_helper(Some("none")), None);
        let only_yay = Features::parse("paru\tmissing\nyay\tok\t12\n");
        assert_eq!(only_yay.aur_helper(Some("paru")), None);
        assert_eq!(only_yay.aur_helper(None), Some("yay"));
    }
}
