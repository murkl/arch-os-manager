//! What the clean-up would remove, as cleanup.sh lists it.

use super::parse_size;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub orphans: Vec<(String, u64)>,
    pub cache: Vec<(String, u64)>,
    /// Kernel packages kept for a recovery from a snapshot.
    pub kept: Vec<String>,
    pub aur: Vec<(String, u64)>,
    /// Unused Flatpak runtimes are looked for during the clean-up itself.
    pub flatpak: bool,
}

impl Plan {
    pub fn parse(out: &str) -> Plan {
        let mut plan = Plan::default();
        for line in out.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            match f.as_slice() {
                ["orphan", name, size] => plan.orphans.push((name.to_string(), parse_size(size))),
                ["cache", path, bytes] => plan
                    .cache
                    .push((path.to_string(), bytes.parse().unwrap_or(0))),
                ["keep", key] => plan.kept.push(key.to_string()),
                ["aur", path, bytes] => plan
                    .aur
                    .push((path.to_string(), bytes.parse().unwrap_or(0))),
                ["flatpak"] => plan.flatpak = true,
                _ => {}
            }
        }
        plan
    }

    pub fn bytes(&self) -> u64 {
        self.orphans
            .iter()
            .chain(&self.cache)
            .chain(&self.aur)
            .map(|(_, b)| b)
            .sum()
    }

    /// Nothing is known to be removable; Flatpak alone may still find something.
    pub fn is_empty(&self) -> bool {
        self.orphans.is_empty() && self.cache.is_empty() && self.aur.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_kind_and_sums_the_space() {
        let p = Plan::parse(
            "orphan\tfoo\t1.00 MiB\ncache\t/var/cache/pacman/pkg/a.pkg.tar.zst\t1024\nkeep\tlinux-zen-7.2.9.zen1-1\naur\t/home/u/.cache/paru/clone/x\t2048\nflatpak\n",
        );
        assert_eq!(p.bytes(), 1024 * 1024 + 1024 + 2048);
        assert_eq!(p.kept, ["linux-zen-7.2.9.zen1-1"]);
        assert!(p.flatpak && !p.is_empty());
        assert!(Plan::parse("flatpak\n").is_empty());
    }
}
