//! The Arch OS Manager 1.x, taken over once: its settings into settings.yaml, then what it left
//! removed. scripts/migrate/migrate.sh says how to remove this.

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

    /// Its settings, where settings.yaml has none of its own. Returns whether any was taken.
    pub fn take_over(&self, settings: &mut Settings) -> bool {
        let mut taken = false;
        if settings.notify.is_none() && self.notify == Some(false) {
            settings.notify = Some("off".into());
            taken = true;
        }
        if settings.aur_helper.is_none() && self.aur_helper.is_some() {
            settings.aur_helper = self.aur_helper.clone();
            taken = true;
        }
        taken
    }
}

fn home() -> String {
    crate::paths::home().display().to_string()
}

/// What the old manager left, where it left something. What fails is logged, and the next
/// start reads again.
pub fn read() -> Option<Legacy> {
    let legacy = match query(op::MIGRATE_READ, &[&home()]) {
        Ok(out) => Legacy::parse(&out),
        Err(f) => {
            crate::warn!("migrate: {}", f.detail);
            return None;
        }
    };
    (!legacy.left.is_empty()).then_some(legacy)
}

/// Removes what it left, once its settings are written.
pub fn clean(legacy: &Legacy) {
    match query(op::MIGRATE_CLEAN, &[&home()]) {
        Ok(_) => crate::info!("migrate: took over {}", legacy.left.join(", ")),
        Err(f) => crate::warn!("migrate: {}", f.detail),
    }
}

/// Takes the old manager over at once, for `init`. The interface does it behind the splash.
pub fn run() {
    let Some(legacy) = read() else {
        return;
    };
    let (mut settings, error) = Settings::load();
    if let Some(e) = error {
        crate::warn!("migrate: settings.yaml ignored: {e}");
        return;
    }
    if legacy.take_over(&mut settings)
        && let Err(e) = settings.save()
    {
        crate::warn!("migrate: settings.yaml not written: {e}");
        return;
    }
    clean(&legacy);
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
        assert!(legacy.take_over(&mut fresh));
        assert_eq!(
            (fresh.notify.as_deref(), fresh.aur_helper.as_deref()),
            (Some("off"), Some("yay"))
        );
        let mut chosen = Settings {
            aur_helper: Some("paru".into()),
            ..Settings::default()
        };
        assert!(legacy.take_over(&mut chosen), "the notification is taken");
        assert_eq!(chosen.aur_helper.as_deref(), Some("paru"));
        assert!(!legacy.take_over(&mut chosen), "nothing left to take");
        assert_eq!(Legacy::parse(""), Legacy::default());
    }
}
