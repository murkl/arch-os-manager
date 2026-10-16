//! The System Manager: a runtime for the scripts in `scripts/`, as a terminal or window interface.

pub mod cli;
pub mod data;
pub mod i18n;
pub mod integration;
pub mod log;
pub mod migrate;
pub mod paths;
pub mod script;
pub mod settings;
pub mod state;
mod term;
pub mod theme;
mod ui;
pub mod version;
mod window;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}
