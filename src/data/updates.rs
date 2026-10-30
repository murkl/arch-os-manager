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
    /// The package, or the Flatpak's id, as `id//branch` where it is pending in two branches.
    pub name: String,
    /// Empty where the source does not say.
    pub old: String,
    pub new: String,
    /// The name Flatpak shows, and the branch. Empty for a package.
    pub title: String,
    pub branch: String,
}

/// What updates.sh found, and the sources it could not ask.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pending {
    pub updates: Vec<Update>,
    pub failed: Vec<Source>,
}

pub fn parse(out: &str) -> Pending {
    let source = |s: &str| match s {
        "repo" => Some(Source::Repo),
        "aur" => Some(Source::Aur),
        "flatpak" => Some(Source::Flatpak),
        _ => None,
    };
    let mut pending = Pending::default();
    for line in out.lines() {
        match line.split('\t').collect::<Vec<_>>()[..] {
            ["failed", s] => pending.failed.extend(source(s)),
            [s, name, ref rest @ ..] => {
                if let Some(source) = source(s) {
                    let field = |i: usize| rest.get(i).unwrap_or(&"").to_string();
                    pending.updates.push(Update {
                        source,
                        name: name.to_string(),
                        old: field(0),
                        new: field(1),
                        title: field(2),
                        branch: field(3),
                    });
                }
            }
            _ => {}
        }
    }
    pending
}

/// checkupdates format for `system-manager check`, an AUR package as `aur/name`, a Flatpak as
/// `flatpak/id`. An unknown version is `?`, as arch-update counts only lines with a word on each
/// side of the arrow.
pub fn checkupdates_format(updates: &[Update]) -> String {
    fn known(v: &str) -> &str {
        if v.is_empty() { "?" } else { v }
    }
    updates
        .iter()
        .map(|u| {
            let source = match u.source {
                Source::Repo => "",
                Source::Aur => "aur/",
                Source::Flatpak => "flatpak/",
            };
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
        let pending = parse(
            "repo\tlinux-zen\t7.2.8.zen1-1\t7.2.9.zen1-1\naur\tparu\t2.0.0-1\t2.1.0-1\nflatpak\tcom.x.App\t\t\tApp One\tstable\n",
        );
        assert!(pending.failed.is_empty());
        let u = pending.updates;
        assert_eq!(u.len(), 3);
        assert_eq!(
            u[0],
            Update {
                source: Source::Repo,
                name: "linux-zen".into(),
                old: "7.2.8.zen1-1".into(),
                new: "7.2.9.zen1-1".into(),
                title: String::new(),
                branch: String::new(),
            }
        );
        let app = &u[2];
        assert_eq!((app.source, app.new.as_str()), (Source::Flatpak, ""));
        assert_eq!(
            (app.title.as_str(), app.branch.as_str()),
            ("App One", "stable")
        );
        assert_eq!(
            checkupdates_format(&u),
            "linux-zen 7.2.8.zen1-1 -> 7.2.9.zen1-1\naur/paru 2.0.0-1 -> 2.1.0-1\nflatpak/com.x.App ? -> ?\n"
        );
    }

    #[test]
    fn a_source_that_could_not_be_asked_leaves_the_others() {
        let pending = parse("repo\tlinux\t1-1\t2-1\nfailed\taur\nfailed\tflatpak\n");
        assert_eq!(pending.updates.len(), 1);
        assert_eq!(pending.failed, [Source::Aur, Source::Flatpak]);
    }
}
