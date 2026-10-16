//! The Arch OS Manager 1.x, taken over once: its settings into settings.yaml, then what it left
//! removed. scripts/migrate/script.sh says how to remove this.

use crate::script::{op, query};
use crate::settings::Settings;

/// What the old manager left, and its settings that have a counterpart.
#[derive(Debug, Default, PartialEq)]
pub struct Legacy {
    pub left: Vec<String>,
    pub notify: Option<bool>,
    pub aur_helper: Option<String>,
}

impl Legacy {
    pub fn parse(out: &str) -> Legacy {
        let mut l = Legacy::default();
        for line in out.lines() {
            match line.split_once('\t') {
                Some(("left", path)) => l.left.push(path.to_string()),
                Some(("notify", v)) => l.notify = Some(v == "true"),
                Some(("aur-helper", v)) => l.aur_helper = Some(v.to_string()),
                _ => {}
            }
        }
        l
    }

    /// Its settings, where settings.yaml has none of its own.
    pub fn take_over(&self, settings: &mut Settings) {
        if settings.notify.is_none() {
            settings.notify = self.notify.and_then(crate::settings::switch);
        }
        if settings.aur_helper.is_none() {
            settings.aur_helper = self.aur_helper.clone();
        }
    }
}

/// Takes the old manager over where it left something. The settings are written before any of
/// it goes; what fails is logged, and the next start tries again.
pub fn run() {
    let home = crate::paths::home().display().to_string();
    let legacy = match query(op::MIGRATE_READ, &[&home]) {
        Ok(out) => Legacy::parse(&out),
        Err(f) => {
            crate::warn!("migrate: {}", f.detail);
            return;
        }
    };
    if legacy.left.is_empty() {
        return;
    }
    let (mut settings, error) = Settings::load();
    if let Some(e) = error {
        crate::warn!("migrate: settings.yaml is not used: {e}");
        return;
    }
    legacy.take_over(&mut settings);
    if let Err(e) = settings.save() {
        crate::warn!("migrate: settings.yaml not written: {e}");
        return;
    }
    match query(op::MIGRATE_CLEAN, &[&home]) {
        Ok(_) => crate::info!("migrate: took over {}", legacy.left.join(", ")),
        Err(f) => crate::warn!("migrate: {}", f.detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_over_only_what_was_not_chosen_anew() {
        let legacy = Legacy::parse(
            "left\t/h/bin\nleft\t/h/config/settings.conf\nnotify\tfalse\naur-helper\tyay\n",
        );
        assert_eq!(legacy.left, ["/h/bin", "/h/config/settings.conf"]);
        let mut fresh = Settings::default();
        legacy.take_over(&mut fresh);
        assert_eq!(
            (fresh.notify, fresh.aur_helper.as_deref()),
            (Some(false), Some("yay"))
        );
        let mut chosen = Settings {
            aur_helper: Some("paru".into()),
            ..Settings::default()
        };
        legacy.take_over(&mut chosen);
        assert_eq!(chosen.aur_helper.as_deref(), Some("paru"));
        assert_eq!(Legacy::parse(""), Legacy::default());
    }
}
