//! Services of the system and of the user, from systemctl's JSON.

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub struct Service {
    pub user: bool,
    pub unit: String,
    pub description: String,
    /// active, inactive, failed, activating; empty for a unit file never loaded.
    pub active: String,
    pub sub: String,
    /// enabled, disabled, static, masked, …; empty for a unit without a file.
    pub enabled: String,
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
    active: String,
    #[serde(default)]
    sub: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct File {
    unit_file: String,
    state: String,
}

/// services.sh output: loaded units and unit files, joined; failed first, then running.
pub fn parse(out: &str, user: bool) -> Result<Vec<Service>, String> {
    let (mut units, mut files): (Vec<Unit>, Vec<File>) = (Vec::new(), Vec::new());
    for line in out.lines() {
        match line.split_once('\t') {
            Some(("units", json)) => {
                units = serde_json::from_str(json).map_err(|e| e.to_string())?
            }
            Some(("files", json)) => {
                files = serde_json::from_str(json).map_err(|e| e.to_string())?
            }
            _ => {}
        }
    }
    let state = |unit: &str| {
        files
            .iter()
            .find(|f| f.unit_file == unit)
            .map(|f| f.state.clone())
            .unwrap_or_default()
    };
    let mut all: Vec<Service> = units
        .iter()
        .map(|u| Service {
            user,
            unit: u.unit.clone(),
            description: u.description.clone(),
            active: u.active.clone(),
            sub: u.sub.clone(),
            enabled: state(&u.unit),
        })
        .collect();
    // A template is no service until it has an instance.
    for f in files.iter().filter(|f| !f.unit_file.ends_with("@.service")) {
        if !all.iter().any(|s| s.unit == f.unit_file) {
            all.push(Service {
                user,
                unit: f.unit_file.clone(),
                description: String::new(),
                active: String::new(),
                sub: String::new(),
                enabled: f.state.clone(),
            });
        }
    }
    all.sort_by_key(|s| (!s.failed(), !s.running(), s.unit.clone()));
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
