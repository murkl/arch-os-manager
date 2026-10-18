//! Everything that can be opened, from the start page, the search, or the command line.

use super::list::Row;
use super::pages;
use super::{Ctx, Page, Tone};
use crate::i18n::t;
use crate::script::op;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    Upgrade,
    Reboot,
    News,
    Packages,
    Services,
    Log,
    Cleanup,
    Pacnew,
    Mirrors,
    Firmware,
    Repair,
    Settings,
    Language,
    Theme,
    Usage,
    AurHelper,
    Notifications,
    Desktop,
    Hidden,
    Features,
    About,
}

/// The start page, group by group.
pub const HOME: &[&[Target]] = &[
    &[Target::Upgrade, Target::Reboot, Target::News],
    &[Target::Packages, Target::Services, Target::Log],
    &[
        Target::Cleanup,
        Target::Pacnew,
        Target::Mirrors,
        Target::Firmware,
        Target::Repair,
    ],
    &[Target::Settings],
];

pub const SETTINGS: &[Target] = &[
    Target::Language,
    Target::Theme,
    Target::Usage,
    Target::AurHelper,
    Target::Notifications,
    Target::Desktop,
    Target::Hidden,
    Target::Features,
    Target::About,
];

pub const ALL: &[Target] = &[
    Target::Upgrade,
    Target::Reboot,
    Target::News,
    Target::Packages,
    Target::Services,
    Target::Log,
    Target::Cleanup,
    Target::Pacnew,
    Target::Mirrors,
    Target::Firmware,
    Target::Repair,
    Target::Settings,
    Target::Language,
    Target::Theme,
    Target::Usage,
    Target::AurHelper,
    Target::Notifications,
    Target::Desktop,
    Target::Hidden,
    Target::Features,
    Target::About,
];

impl Target {
    /// The name on the command line and the row's id.
    pub fn id(self) -> &'static str {
        match self {
            Target::Upgrade => "upgrade",
            Target::Reboot => "reboot",
            Target::News => "news",
            Target::Packages => "packages",
            Target::Services => "services",
            Target::Log => "log",
            Target::Cleanup => "cleanup",
            Target::Pacnew => "pacnew",
            Target::Mirrors => "mirrors",
            Target::Firmware => "firmware",
            Target::Repair => "repair",
            Target::Settings => "settings",
            Target::Language => "language",
            Target::Theme => "theme",
            Target::Usage => "usage",
            Target::AurHelper => "aur-helper",
            Target::Notifications => "notifications",
            Target::Desktop => "desktop",
            Target::Hidden => "hidden",
            Target::Features => "features",
            Target::About => "about",
        }
    }

    pub fn from_id(id: &str) -> Option<Target> {
        ALL.iter().copied().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::Upgrade => t("Update"),
            Target::Reboot => t("Restart"),
            Target::News => t("News"),
            Target::Packages => t("Packages"),
            Target::Services => t("Services"),
            Target::Log => t("System log"),
            Target::Cleanup => t("Clean up"),
            Target::Pacnew => t("Config updates"),
            Target::Mirrors => t("Mirrors"),
            Target::Firmware => t("Firmware"),
            Target::Repair => t("Repair"),
            Target::Settings => t("Settings"),
            Target::Language => t("Language"),
            Target::Theme => t("Theme"),
            Target::Usage => t("System usage"),
            Target::AurHelper => t("AUR helper"),
            Target::Notifications => t("Update notification"),
            Target::Desktop => t("App menu entry"),
            Target::Hidden => t("Hidden messages"),
            Target::Features => t("Optional packages"),
            Target::About => t("About"),
        }
    }

    pub fn icon(self) -> &'static str {
        use super::icon;
        match self {
            Target::Upgrade => icon::UPDATE,
            Target::Reboot => icon::RESTART,
            Target::News => icon::NEWS,
            Target::Packages => icon::PACKAGE,
            Target::Services => icon::SERVICES,
            Target::Log => icon::LOG,
            Target::Cleanup => icon::CLEANUP,
            Target::Pacnew => icon::PACNEW,
            Target::Mirrors => icon::MIRRORS,
            Target::Firmware => icon::FIRMWARE,
            Target::Repair => icon::REPAIR,
            Target::Settings => icon::SETTINGS,
            Target::Language => icon::LANGUAGE,
            Target::Theme => icon::THEME,
            Target::Usage => icon::USAGE,
            Target::AurHelper => icon::ARCH,
            Target::Notifications => icon::NOTIFICATION,
            Target::Desktop => icon::APP_MENU,
            Target::Hidden => icon::HIDDEN,
            Target::Features => icon::OPTIONAL,
            Target::About => icon::ABOUT,
        }
    }

    pub fn in_settings(self) -> bool {
        SETTINGS.contains(&self)
    }

    /// The script whose optional packages decide whether this can open.
    pub fn script(self) -> Option<crate::script::Op> {
        match self {
            Target::Pacnew => Some(op::PACNEW_LIST),
            Target::Cleanup => Some(op::CLEANUP_PLAN),
            Target::Firmware => Some(op::FIRMWARE_LIST),
            Target::Notifications => Some(op::DESKTOP_NOTIFY),
            _ => None,
        }
    }

    /// The pages to push, the settings page first for its own rows.
    pub fn open(self, ctx: &mut Ctx) -> Vec<Box<dyn Page>> {
        if let Some(script) = self.script()
            && ctx.features().is_some()
            && !ctx.available(script)
        {
            let missing = script
                .meta()
                .needs
                .iter()
                .find(|p| !ctx.features().is_some_and(|f| f.ok(p)))
                .cloned();
            return vec![Box::new(pages::features::FeaturesPage::new(missing))];
        }
        let page: Box<dyn Page> = match self {
            Target::Upgrade => Box::new(pages::upgrade::UpgradePage::new()),
            Target::Reboot => pages::run::start(
                pages::run::Job::new(op::SYSTEM_REBOOT, Vec::new()).named(self.label()),
                ctx,
            ),
            Target::News => Box::new(pages::news::NewsPage::new()),
            Target::Packages => Box::new(pages::packages::PackagesPage::new()),
            Target::Services => Box::new(pages::services::ServicesPage::new()),
            Target::Log => Box::new(pages::log::LogPage::new()),
            Target::Cleanup => Box::new(pages::cleanup::CleanupPage::new()),
            Target::Pacnew => Box::new(pages::pacnew::PacnewPage::new()),
            Target::Mirrors => Box::new(pages::mirrors::MirrorsPage::new()),
            Target::Firmware => Box::new(pages::firmware::FirmwarePage::new()),
            Target::Repair => Box::new(pages::repair::RepairPage::new()),
            Target::Settings => Box::new(pages::settings::SettingsPage::new(None)),
            Target::Features => Box::new(pages::features::FeaturesPage::new(None)),
            Target::About => Box::new(pages::settings::AboutPage::new()),
            Target::Hidden => Box::new(pages::log::HiddenPage::new()),
            Target::Language | Target::Theme | Target::AurHelper | Target::Notifications => {
                return vec![
                    Box::new(pages::settings::SettingsPage::new(Some(self))),
                    pages::settings::choice(self, ctx),
                ];
            }
            Target::Usage | Target::Desktop => {
                return vec![Box::new(pages::settings::SettingsPage::new(Some(self)))];
            }
        };
        if self.in_settings() {
            vec![
                Box::new(pages::settings::SettingsPage::new(Some(self))),
                page,
            ]
        } else {
            vec![page]
        }
    }

    /// The row in the search over everything: the label, and where it lives.
    pub fn search_row(self) -> Row<Target> {
        let place = if self.in_settings() {
            Target::Settings.label()
        } else {
            ""
        };
        Row::new(self, self.label())
            .icon(self.icon(), Tone::Normal)
            .detail(place, Tone::Muted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_every_target_is_reachable() {
        for (i, a) in ALL.iter().enumerate() {
            assert_eq!(Target::from_id(a.id()), Some(*a));
            assert!(ALL[i + 1..].iter().all(|b| b.id() != a.id()));
            let on_home = HOME.iter().any(|g| g.contains(a));
            assert!(on_home || a.in_settings(), "{a:?} is on no page");
        }
    }
}
