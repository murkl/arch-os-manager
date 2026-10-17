//! The mirror list pacman downloads from.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mirrors {
    pub changed: Option<jiff::Timestamp>,
    /// reflector.timer ranks them weekly.
    pub timer: bool,
    pub servers: Vec<String>,
}

impl Mirrors {
    pub fn parse(out: &str) -> Mirrors {
        let mut m = Mirrors::default();
        for line in out.lines() {
            match line.split_once('\t') {
                Some(("changed", v)) => {
                    m.changed = v
                        .parse()
                        .ok()
                        .and_then(|s| jiff::Timestamp::from_second(s).ok())
                }
                Some(("timer", v)) => m.timer = v == "on",
                Some(("server", v)) => m.servers.push(host(v)),
                _ => {}
            }
        }
        m
    }
}

/// `https://mirror.example.org/archlinux/$repo/os/$arch` as `mirror.example.org`.
fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hosts_and_age() {
        let m = Mirrors::parse(
            "changed\t1760000000\ntimer\ton\nserver\thttps://ftp.fau.de/archlinux/$repo/os/$arch\n",
        );
        assert!(m.timer && m.changed.is_some());
        assert_eq!(m.servers, ["ftp.fau.de"]);
    }
}
