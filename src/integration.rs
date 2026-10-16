//! The app menu entry, the update notification and the update indicator, kept as the
//! settings and the optional packages say.

use crate::data::features::Features;
use crate::script::{self, Failure, op, query};
use crate::settings::Settings;

/// What is in place after `apply`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Applied {
    pub entry: bool,
    pub timer: bool,
    pub indicator: bool,
}

/// Writes or removes the entry, its icon and the timer, for this program where it lies now.
/// Both need a desktop and the timer libnotify; without them the settings wait. The update
/// indicator, where it is installed, checks and updates through this program.
pub fn apply(settings: &Settings, features: &Features) -> Result<Applied, Failure> {
    let exe =
        std::env::current_exe().map_err(|e| Failure::new(op::DESKTOP_APPLY, e.to_string()))?;
    let desktop = features.lacks("desktop").is_none();
    let applied = Applied {
        entry: settings.app_menu.unwrap_or(true) && desktop,
        timer: settings.notify.unwrap_or(true)
            && desktop
            && script::available(op::DESKTOP_NOTIFY, features),
        indicator: features.ok("arch-update"),
    };
    let on = |b: bool| if b { "on" } else { "off" };
    query(
        op::DESKTOP_APPLY,
        &[
            &exe.display().to_string(),
            on(applied.entry),
            on(applied.timer),
            on(applied.indicator),
        ],
    )?;
    Ok(applied)
}
