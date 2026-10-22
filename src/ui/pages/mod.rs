//! The pages.

pub mod ask;
pub mod cleanup;
pub mod dotfiles;
pub mod features;
pub mod firmware;
pub mod home;
pub mod homelab;
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

use super::Does;

/// The footer of a plain list.
pub fn list_hints(filter: bool) -> Vec<(&'static str, Does)> {
    if filter {
        return vec![
            ("↑↓", Does::Choose),
            ("⏎", Does::Open),
            ("esc", Does::CloseFilter),
        ];
    }
    vec![
        ("↑↓", Does::Choose),
        ("⏎", Does::Open),
        ("f", Does::Filter),
        ("esc", Does::Back),
    ]
}
