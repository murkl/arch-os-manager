//! The machine at a glance: name, a pending restart, what runs.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct System {
    pub hostname: String,
    /// The running kernel's modules are gone: a kernel update waits for a restart.
    pub reboot: bool,
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
    fn reads_the_state() {
        let s = System::parse(concat!(
            "hostname\ttux-notebook\nreboot\tyes\n",
            "kernel\t7.2.9-zen1-1-zen\nbooted\t1792130400\npacman\t7.1.0.r9.g54d9411-1\n",
            "desktop\tGNOME 51.1\nsession\twayland\n"
        ));
        assert_eq!(s.hostname, "tux-notebook");
        assert!(s.reboot);
        assert_eq!(s.kernel, "7.2.9-zen1-1-zen");
        assert_eq!(s.booted.unwrap().as_second(), 1792130400);
        assert_eq!(s.pacman, "7.1.0.r9.g54d9411-1");
        assert_eq!(
            (s.desktop.as_deref(), s.session.as_deref()),
            (Some("GNOME 51.1"), Some("wayland"))
        );
    }
}
