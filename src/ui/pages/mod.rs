//! The pages.

pub mod cleanup;
pub mod features;
pub mod firmware;
pub mod home;
pub mod log;
pub mod mirrors;
pub mod news;
pub mod packages;
pub mod pacnew;
pub mod repair;
pub mod run;
pub mod services;
pub mod settings;
pub mod splash;
pub mod upgrade;

use crate::i18n::t;

/// The footer of a plain list.
pub fn list_hints(filter: bool) -> Vec<(&'static str, String)> {
    if filter {
        return vec![
            ("↑↓", t("choose").to_string()),
            ("⏎", t("open").to_string()),
            ("esc", t("close filter").to_string()),
        ];
    }
    vec![
        ("↑↓", t("choose").to_string()),
        ("⏎", t("open").to_string()),
        ("f", t("filter").to_string()),
        ("esc", t("back").to_string()),
    ]
}
