//! The machine at a glance: name, a pending restart, the last upgrade, what runs.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct System {
    pub hostname: String,
    /// The running kernel's modules are gone: a kernel update waits for a restart.
    pub reboot: bool,
    pub last_upgrade: Option<jiff::Timestamp>,
    pub kernel: String,
    pub booted: Option<jiff::Timestamp>,
    pub pacman: String,
    /// The desktop of the session, and the session's kind: wayland, x11, tty.
    pub desktop: Option<String>,
    pub session: Option<String>,
}

impl System {
    pub fn parse(out: &str) -> System {
        let mut s = System::default();
        for line in out.lines() {
            match line.split_once('\t') {
                Some(("hostname", v)) => s.hostname = v.to_string(),
                Some(("reboot", v)) => s.reboot = v == "yes",
                // pacman.log writes 2026-10-10T22:21:45+0200.
                Some(("last-upgrade", v)) => {
                    s.last_upgrade = jiff::Timestamp::strptime("%Y-%m-%dT%H:%M:%S%z", v).ok()
                }
                Some(("kernel", v)) => s.kernel = v.to_string(),
                Some(("booted", v)) => {
                    s.booted = v
                        .parse()
                        .ok()
                        .and_then(|secs| jiff::Timestamp::from_second(secs).ok())
                }
                Some(("pacman", v)) => s.pacman = v.to_string(),
                Some(("desktop", v)) => s.desktop = Some(v.to_string()),
                Some(("session", v)) => s.session = Some(v.to_string()),
                _ => {}
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pacman_log_time() {
        let s = System::parse(concat!(
            "hostname\tzeus\nreboot\tyes\nlast-upgrade\t2026-10-10T22:21:45+0200\n",
            "kernel\t7.2.9-zen1-1-zen\nbooted\t1792130400\npacman\t7.1.0.r9.g54d9411-1\n",
            "desktop\tGNOME 51.1\nsession\twayland\n"
        ));
        assert_eq!(s.hostname, "zeus");
        assert!(s.reboot);
        assert_eq!(s.last_upgrade.unwrap().to_string(), "2026-10-10T20:21:45Z");
        assert_eq!(s.kernel, "7.2.9-zen1-1-zen");
        assert_eq!(s.booted.unwrap().as_second(), 1792130400);
        assert_eq!(s.pacman, "7.1.0.r9.g54d9411-1");
        assert_eq!(
            (s.desktop.as_deref(), s.session.as_deref()),
            (Some("GNOME 51.1"), Some("wayland"))
        );
    }
}
