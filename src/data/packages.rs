//! Installed packages, repository search results and package details from pacman.

use std::collections::HashSet;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub explicit: bool,
    /// In no configured repository: from the AUR or built by hand.
    pub foreign: bool,
}

pub fn parse_installed(out: &str) -> Vec<Installed> {
    let mut all: Vec<Installed> = Vec::new();
    let (mut explicit, mut foreign) = (Vec::new(), Vec::new());
    for line in out.lines() {
        let mut f = line.split('\t');
        match (f.next(), f.next(), f.next()) {
            (Some("pkg"), Some(name), Some(version)) => all.push(Installed {
                name: name.into(),
                version: version.into(),
                ..Installed::default()
            }),
            (Some("explicit"), Some(name), _) => explicit.push(name),
            (Some("foreign"), Some(name), _) => foreign.push(name),
            _ => {}
        }
    }
    let (explicit, foreign): (HashSet<&str>, HashSet<&str>) = (
        explicit.into_iter().collect(),
        foreign.into_iter().collect(),
    );
    for p in &mut all {
        p.explicit = explicit.contains(p.name.as_str());
        p.foreign = foreign.contains(p.name.as_str());
    }
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub repo: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub installed: bool,
}

/// `pacman -Ss`: a `repo/name version [installed]` line, then the indented description.
pub fn parse_search(out: &str) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for line in out.lines() {
        if let Some(description) = line.strip_prefix("    ") {
            if let Some(last) = found.last_mut() {
                last.description = description.trim().to_string();
            }
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(id), Some(version)) = (words.next(), words.next()) else {
            continue;
        };
        let Some((repo, name)) = id.split_once('/') else {
            continue;
        };
        found.push(Found {
            repo: repo.into(),
            name: name.into(),
            version: version.into(),
            description: String::new(),
            installed: line.contains("[installed"),
        });
    }
    found
}

/// `pacman -Si` and `-Qi`: `Key : value`, a value continued on indented lines.
pub fn parse_info(out: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in out.lines() {
        match line.split_once(" : ") {
            Some((key, value)) if !line.starts_with(' ') => {
                fields.push((key.trim().into(), value.trim().into()))
            }
            _ => {
                if let Some(last) = fields.last_mut()
                    && !line.trim().is_empty()
                {
                    last.1 = format!("{} {}", last.1, line.trim());
                }
            }
        }
    }
    fields
}

pub fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .filter(|v| *v != "None")
}

/// What someone typed, as a literal for pacman's regular expression search.
pub fn escape_regex(q: &str) -> String {
    q.chars().fold(String::new(), |mut s, c| {
        if r"\.+*?()|[]{}^$".contains(c) {
            s.push('\\');
        }
        s.push(c);
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_the_three_installed_lists() {
        let p = parse_installed(
            "pkg\tzsh\t5.9-5\npkg\tparu\t2.1.0-1\nexplicit\tzsh\nexplicit\tparu\nforeign\tparu\n",
        );
        assert_eq!(
            p[0],
            Installed {
                name: "paru".into(),
                version: "2.1.0-1".into(),
                explicit: true,
                foreign: true
            }
        );
        assert_eq!(
            p[1],
            Installed {
                name: "zsh".into(),
                version: "5.9-5".into(),
                explicit: true,
                foreign: false
            }
        );
    }

    #[test]
    fn reads_search_results_with_their_descriptions() {
        let out = "extra/flatpak 1:1.18.4-1 [installed]\n    Linux application sandboxing\nextra/fcitx5-nord 0.0.0.20210727-2\n    Fcitx5 theme based on Nord color\n";
        let f = parse_search(out);
        assert_eq!(f.len(), 2);
        assert!(f[0].installed && !f[1].installed);
        assert_eq!(
            (
                f[1].repo.as_str(),
                f[1].name.as_str(),
                f[1].description.as_str()
            ),
            ("extra", "fcitx5-nord", "Fcitx5 theme based on Nord color")
        );
    }

    #[test]
    fn reads_details_with_continued_values() {
        let out = "Repository      : extra\nName            : flatpak\nDepends On      : appstream  bash\n                  curl\nOptional Deps   : None\nBuild Date      : Wed Sep 30 01:54:44 2026\n";
        let f = parse_info(out);
        assert_eq!(field(&f, "Depends On"), Some("appstream  bash curl"));
        assert_eq!(field(&f, "Optional Deps"), None);
        assert_eq!(field(&f, "Build Date"), Some("Wed Sep 30 01:54:44 2026"));
    }

    #[test]
    fn escapes_what_pacman_reads_as_a_pattern() {
        assert_eq!(escape_regex("c++"), r"c\+\+");
        assert_eq!(escape_regex("lib32-*"), r"lib32-\*");
    }
}
