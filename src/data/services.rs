//! Services of the system and of the user, from systemctl's JSON.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub struct Service {
    pub user: bool,
    pub unit: String,
    pub description: String,
    /// active, inactive, failed, activating, empty for a unit file never loaded.
    pub active: String,
    /// enabled, disabled, static, masked, …, empty for a unit without a file.
    pub enabled: String,
    /// The graphical session cannot do without it, as the display manager and the bus.
    pub session: bool,
}

impl Service {
    pub fn failed(&self) -> bool {
        self.active == "failed"
    }

    pub fn running(&self) -> bool {
        self.active == "active"
    }
}

#[derive(Deserialize)]
struct Unit {
    unit: String,
    #[serde(default)]
    load: String,
    #[serde(default)]
    active: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct File {
    unit_file: String,
    state: String,
}

/// services.sh output: loaded units and unit files, joined, failed first, then running.
pub fn parse(out: &str, user: bool) -> Result<Vec<Service>, String> {
    let (mut units, mut files): (Vec<Unit>, Vec<File>) = (Vec::new(), Vec::new());
    let mut session = Vec::new();
    for line in out.lines() {
        match line.split_once('\t') {
            Some(("session", unit)) => session.push(unit),
            Some(("units", json)) => {
                units = serde_json::from_str(json).map_err(|e| e.to_string())?
            }
            Some(("files", json)) => {
                files = serde_json::from_str(json).map_err(|e| e.to_string())?
            }
            _ => {}
        }
    }
    let state: HashMap<&str, &str> = files
        .iter()
        .map(|f| (f.unit_file.as_str(), f.state.as_str()))
        .collect();
    // A unit only named somewhere, with no file behind it, is nothing to start.
    let mut all: Vec<Service> = units
        .iter()
        .filter(|u| u.load != "not-found")
        .map(|u| Service {
            user,
            unit: u.unit.clone(),
            description: u.description.clone(),
            active: u.active.clone(),
            enabled: state.get(u.unit.as_str()).unwrap_or(&"").to_string(),
            session: session.contains(&u.unit.as_str()),
        })
        .collect();
    // A template is no service until it has an instance, an alias is listed under its unit.
    for f in files
        .iter()
        .filter(|f| !f.unit_file.ends_with("@.service") && f.state != "alias")
    {
        if !all.iter().any(|s| s.unit == f.unit_file) {
            all.push(Service {
                user,
                unit: f.unit_file.clone(),
                description: String::new(),
                active: String::new(),
                enabled: f.state.clone(),
                session: session.contains(&f.unit_file.as_str()),
            });
        }
    }
    all.sort_by_key(|s| (!s.failed(), !s.running(), s.unit.clone()));
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As systemctl printed them on a real system.
    #[test]
    fn leaves_out_a_unit_without_a_file() {
        let out = concat!(
            r#"units	[{"unit":"auto-cpufreq.service","load":"not-found","active":"inactive","sub":"dead","description":"auto-cpufreq.service"},{"unit":"getty@tty1.service","load":"loaded","active":"inactive","sub":"dead","description":"Getty on tty1"},{"unit":"systemd-journald.service","load":"loaded","active":"active","sub":"running","description":"Journal Service"}]"#,
            "\n",
            r#"files	[{"unit_file":"getty@.service","state":"enabled","preset":"enabled"},{"unit_file":"reflector.service","state":"disabled","preset":"disabled"},{"unit_file":"systemd-journald.service","state":"static","preset":null}]"#,
            "\n"
        );
        let s = parse(out, true).unwrap();
        let names: Vec<&str> = s.iter().map(|x| x.unit.as_str()).collect();
        assert_eq!(
            names,
            [
                "systemd-journald.service",
                "getty@tty1.service",
                "reflector.service"
            ]
        );
        assert_eq!(s[0].enabled, "static");
        assert!(s[0].running() && s[0].user);
    }

    /// As systemctl printed them on a real system: the alias names the display manager.
    #[test]
    fn leaves_out_an_alias() {
        let out = concat!(
            r#"units	[{"unit":"gdm.service","load":"loaded","active":"active","sub":"running","description":"GNOME Display Manager"}]"#,
            "\n",
            r#"files	[{"unit_file":"display-manager.service","state":"alias","preset":null},{"unit_file":"gdm.service","state":"enabled","preset":"disabled"}]"#,
            "\nsession\tgdm.service\n"
        );
        let s = parse(out, false).unwrap();
        let names: Vec<&str> = s.iter().map(|x| x.unit.as_str()).collect();
        assert_eq!(names, ["gdm.service"]);
        assert!(s[0].session);
    }

    #[test]
    fn joins_units_and_files_failed_first() {
        let out = concat!(
            "units\t[{\"unit\":\"b.service\",\"load\":\"loaded\",\"active\":\"active\",\"sub\":\"running\",\"description\":\"B\"},",
            "{\"unit\":\"c.service\",\"load\":\"loaded\",\"active\":\"failed\",\"sub\":\"failed\",\"description\":\"C\"}]\n",
            "files\t[{\"unit_file\":\"a.service\",\"state\":\"disabled\",\"preset\":\"disabled\"},",
            "{\"unit_file\":\"b.service\",\"state\":\"enabled\",\"preset\":\"enabled\"},",
            "{\"unit_file\":\"getty@.service\",\"state\":\"enabled\",\"preset\":\"enabled\"}]\n"
        );
        let s = parse(out, false).unwrap();
        let names: Vec<&str> = s.iter().map(|x| x.unit.as_str()).collect();
        assert_eq!(names, ["c.service", "b.service", "a.service"]);
        assert_eq!(s[1].enabled, "enabled");
        assert!(s[0].failed());
    }

    #[test]
    fn marks_what_the_graphical_session_cannot_do_without() {
        let out = concat!(
            "units\t[{\"unit\":\"gdm.service\",\"load\":\"loaded\",\"active\":\"active\",\"description\":\"GNOME Display Manager\"},",
            "{\"unit\":\"sshd.service\",\"load\":\"loaded\",\"active\":\"active\",\"description\":\"SSH\"}]\n",
            "files\t[{\"unit_file\":\"dbus-broker.service\",\"state\":\"enabled\"}]\n",
            "session\tgdm.service\nsession\tdbus-broker.service\n"
        );
        let s = parse(out, false).unwrap();
        let session: Vec<(&str, bool)> = s.iter().map(|x| (x.unit.as_str(), x.session)).collect();
        assert_eq!(
            session,
            [
                ("gdm.service", true),
                ("sshd.service", false),
                ("dbus-broker.service", true)
            ]
        );
    }
}
