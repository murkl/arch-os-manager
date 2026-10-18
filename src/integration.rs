//! The app menu entry, the update notification and the update indicator, kept as the
//! settings and the optional packages say.

use crate::data::features::Features;
use crate::script::{self, Failure, op, query};
use crate::settings::Settings;

/// What is in place after `apply`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Applied {
    pub entry: bool,
    /// How often the timer announces, or `off`.
    pub timer: &'static str,
    pub indicator: bool,
}

/// Writes or removes the entry, its icon and the timer, for this program where it lies now.
/// The update indicator, where it is installed, checks and updates through this program.
pub fn apply(settings: &Settings, features: &Features) -> Result<Applied, Failure> {
    let exe =
        crate::paths::program().map_err(|e| Failure::new(op::DESKTOP_APPLY, e.to_string()))?;
    // Removed while running: an entry would point at nothing.
    if !exe.is_file() {
        let gone = format!("{} is gone", exe.display());
        return Err(Failure::new(op::DESKTOP_APPLY, gone));
    }
    let applied = plan(settings, features);
    let on = |b: bool| if b { "on" } else { "off" };
    query(
        op::DESKTOP_APPLY,
        &[
            &exe.display().to_string(),
            on(applied.entry),
            applied.timer,
            on(applied.indicator),
        ],
    )?;
    Ok(applied)
}

/// What the settings ask for, as far as the system offers it. The entry opens the window, so it
/// needs a Wayland session, the timer a desktop and libnotify. Without them the settings wait.
fn plan(settings: &Settings, features: &Features) -> Applied {
    let desktop = features.lacks("desktop").is_none();
    Applied {
        entry: settings.app_menu.unwrap_or(true) && features.lacks("wayland").is_none(),
        timer: match desktop && script::available(op::DESKTOP_NOTIFY, features) {
            true => settings.notify(),
            false => "off",
        },
        indicator: features.ok("gnome-arch-update"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_apply_as_far_as_the_system_offers() {
        let gnome = Features::parse(
            "libnotify\tok\t0.8.8-1\ngnome-arch-update\tok\t\n@desktop\tok\n@wayland\tok\n@gnome\tok\n",
        );
        let daily = Settings {
            notify: Some("daily".into()),
            ..Settings::default()
        };
        assert_eq!(
            plan(&daily, &gnome),
            Applied {
                entry: true,
                timer: "daily",
                indicator: true
            }
        );
        let off = Settings {
            app_menu: Some(false),
            ..Settings::default()
        };
        assert!(!plan(&off, &gnome).entry);
        // An X11 desktop without libnotify: no window to open, nothing to show a notice.
        let x11 = Features::parse("libnotify\tmissing\n@desktop\tok\n@wayland\tmissing\n");
        assert_eq!(
            plan(&daily, &x11),
            Applied {
                entry: false,
                timer: "off",
                indicator: false
            }
        );
    }
}
