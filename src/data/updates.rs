//! Pending updates, as updates.sh lists them.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Repo,
    Aur,
    Flatpak,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub source: Source,
    pub name: String,
    /// Empty where the source does not say, as for Flatpak.
    pub old: String,
    pub new: String,
}

pub fn parse(out: &str) -> Vec<Update> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let source = match *f.first()? {
                "repo" => Source::Repo,
                "aur" => Source::Aur,
                "flatpak" => Source::Flatpak,
                _ => return None,
            };
            Some(Update {
                source,
                name: f.get(1)?.to_string(),
                old: f.get(2).unwrap_or(&"").to_string(),
                new: f.get(3).unwrap_or(&"").to_string(),
            })
        })
        .collect()
}

/// As checkupdates prints them, for scripts and shell extensions reading `system-manager check`.
pub fn checkupdates_format(updates: &[Update]) -> String {
    updates
        .iter()
        .map(|u| format!("{} {} -> {}\n", u.name, u.old, u.new))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_all_sources_including_a_flatpak_without_version() {
        let u = parse(
            "repo\tlinux-zen\t7.2.8.zen1-1\t7.2.9.zen1-1\naur\tparu\t2.0.0-1\t2.1.0-1\nflatpak\tcom.x.App\t\t\n",
        );
        assert_eq!(u.len(), 3);
        assert_eq!(
            u[0],
            Update {
                source: Source::Repo,
                name: "linux-zen".into(),
                old: "7.2.8.zen1-1".into(),
                new: "7.2.9.zen1-1".into()
            }
        );
        assert_eq!((u[2].source, u[2].new.as_str()), (Source::Flatpak, ""));
        assert_eq!(
            checkupdates_format(&u[..1]),
            "linux-zen 7.2.8.zen1-1 -> 7.2.9.zen1-1\n"
        );
    }
}
