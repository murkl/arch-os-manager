//! The versions a package can be put back to: in the cache, or in the Arch Linux Archive.

use super::vercmp::vercmp;

#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub version: String,
    /// A path in the cache or an address in the archive, which pacman -U is given.
    pub location: String,
    pub cached: bool,
}

/// versions.sh output for one package, newest first, a version in the cache preferred.
pub fn parse(out: &str, name: &str) -> Vec<Version> {
    let mut all: Vec<Version> = out
        .lines()
        .filter_map(|l| {
            let (kind, location) = l.split_once('\t')?;
            let file = unescape(location.rsplit('/').next()?);
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

/// An address's `%3A`, `%2B` and kin as the characters they stand for.
fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let code = (bytes[i] == b'%')
            .then(|| s.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match code {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

    /// As the archive names them: a `+` comes as `%2B`, in the version and in the name.
    #[test]
    fn reads_what_the_archive_escapes() {
        let out = "cache\t/var/cache/pacman/pkg/glibc-2.44+r50+g1848099f063e-1-x86_64.pkg.tar.zst\n\
                   archive\thttps://archive.archlinux.org/packages/g/glibc/glibc-2.44%2Br50%2Bg1848099f063e-1-x86_64.pkg.tar.zst\n\
                   archive\thttps://archive.archlinux.org/packages/g/glibc/glibc-2.44%2Br3%2Bg0a2d6ed0bb8b-1-x86_64.pkg.tar.zst\n";
        let v = parse(out, "glibc");
        let versions: Vec<&str> = v.iter().map(|x| x.version.as_str()).collect();
        assert_eq!(
            versions,
            ["2.44+r50+g1848099f063e-1", "2.44+r3+g0a2d6ed0bb8b-1"]
        );
        assert!(v[0].cached);
        let sigc = "archive\thttps://archive.archlinux.org/packages/l/libsigc++/libsigc%2B%2B-2.12.1-2-x86_64.pkg.tar.zst\n";
        assert_eq!(parse(sigc, "libsigc++")[0].version, "2.12.1-2");
        assert_eq!(unescape("a%2"), "a%2");
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
