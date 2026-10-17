//! What the scripts and the kernel print, read into values. Pure functions, tested against real
//! output.

pub mod aur;
pub mod cleanup;
pub mod features;
pub mod firmware;
pub mod journal;
pub mod mirrors;
pub mod news;
pub mod packages;
pub mod services;
pub mod system;
pub mod updates;
pub mod usage;
pub mod vercmp;
pub mod versions;

use crate::i18n::{t, tf};

/// `8.08 MiB` as bytes, as pacman prints sizes.
pub fn parse_size(s: &str) -> u64 {
    let mut parts = s.split_whitespace();
    let value: f64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let unit = match parts.next().unwrap_or("B") {
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    (value * unit).round() as u64
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// How long ago, in whole days.
pub fn ago(then: jiff::Timestamp) -> String {
    let days = (jiff::Timestamp::now().as_second() - then.as_second()).max(0) / 86_400;
    match days {
        0 => t("today").to_string(),
        1 => t("yesterday").to_string(),
        n => tf("{n} days ago", &[("n", &n)]),
    }
}

/// A local date as `2026-10-15`.
pub fn date(then: jiff::Timestamp) -> String {
    then.to_zoned(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%d")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_round_trip_in_pacman_units() {
        assert_eq!(parse_size("8.08 MiB"), 8_472_494);
        assert_eq!(parse_size("512.00 B"), 512);
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(8_472_494), "8.1 MiB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }
}
