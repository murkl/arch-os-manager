//! The versions a package can be put back to: in the cache, or in the Arch Linux Archive.

use super::vercmp::vercmp;

#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub version: String,
    /// A path in the cache or an address in the archive; what pacman -U is given.
    pub location: String,
    pub cached: bool,
}

/// versions.sh output for one package, newest first, a version in the cache preferred.
pub fn parse(out: &str, name: &str) -> Vec<Version> {
    let mut all: Vec<Version> = out
        .lines()
        .filter_map(|l| {
            let (kind, location) = l.split_once('\t')?;
            let file = location.rsplit('/').next()?.replace("%3A", ":");
            Some(Version {
                version: version_of(&file, name)?,
                location: location.to_string(),
                cached: kind == "cache",
            })
        })
        .collect();
    all.sort_by(|a, b| vercmp(&b.version, &a.version).then(b.cached.cmp(&a.cached)));
    all.dedup_by(|later, kept| later.version == kept.version);
    all
}

/// `name-1:2.3-4-x86_64.pkg.tar.zst` as `1:2.3-4`, and nothing for another package's file.
fn version_of(file: &str, name: &str) -> Option<String> {
    let rest = file.strip_prefix(name)?.strip_prefix('-')?;
    let stem = &rest[..rest.find(".pkg.tar.")?];
    let mut parts = stem.rsplitn(3, '-');
    let (_arch, rel, ver) = (parts.next()?, parts.next()?, parts.next()?);
    (!ver.contains('-') && !ver.is_empty() && !rel.is_empty()).then(|| format!("{ver}-{rel}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_newest_first_and_keeps_the_cached_copy() {
        let out = "cache\t/var/cache/pacman/pkg/flatpak-1:1.18.4-1-x86_64.pkg.tar.zst\n\
                   cache\t/var/cache/pacman/pkg/flatpak-builder-1.4.4-1-x86_64.pkg.tar.zst\n\
                   archive\thttps://archive.archlinux.org/packages/f/flatpak/flatpak-1%3A1.18.4-1-x86_64.pkg.tar.zst\n\
                   archive\thttps://archive.archlinux.org/packages/f/flatpak/flatpak-1%3A1.16.1-1-x86_64.pkg.tar.zst\n\
                   archive\thttps://archive.archlinux.org/packages/f/flatpak/flatpak-1.12.0-1-x86_64.pkg.tar.xz\n";
        let v = parse(out, "flatpak");
        let versions: Vec<&str> = v.iter().map(|x| x.version.as_str()).collect();
        assert_eq!(versions, ["1:1.18.4-1", "1:1.16.1-1", "1.12.0-1"]);
        assert!(v[0].cached);
    }

    #[test]
    fn leaves_out_packages_that_share_the_prefix() {
        assert_eq!(
            version_of("linux-zen-7.2.9.zen1-1-x86_64.pkg.tar.zst", "linux"),
            None
        );
        assert_eq!(
            version_of("linux-7.2.9.arch1-1-x86_64.pkg.tar.zst", "linux").as_deref(),
            Some("7.2.9.arch1-1")
        );
    }
}
