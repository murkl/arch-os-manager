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
    /// Empty where the source does not say.
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

/// As checkupdates prints them, for scripts and shell extensions reading `system-manager check`;
/// an AUR package as `aur/name`, so a menu built from it tells where it comes from. An unknown
/// version is `?`: arch-update counts only lines with a word on each side of the arrow.
pub fn checkupdates_format(updates: &[Update]) -> String {
    fn known(v: &str) -> &str {
        if v.is_empty() { "?" } else { v }
    }
    updates
        .iter()
        .map(|u| {
            let source = if u.source == Source::Aur { "aur/" } else { "" };
            format!(
                "{source}{} {} -> {}\n",
                u.name,
                known(&u.old),
                known(&u.new)
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_all_sources_and_prints_an_unknown_version_as_a_question_mark() {
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
            checkupdates_format(&u),
            "linux-zen 7.2.8.zen1-1 -> 7.2.9.zen1-1\naur/paru 2.0.0-1 -> 2.1.0-1\ncom.x.App ? -> ?\n"
        );
    }
}
