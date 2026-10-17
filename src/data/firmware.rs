//! Updates the firmware of the device as `fwupdmgr get-updates --json` lists them.

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub struct Firmware {
    pub device: String,
    pub current: String,
    pub new: String,
    pub summary: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Answer {
    #[serde(default)]
    devices: Vec<Device>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Device {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    releases: Vec<Release>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    #[serde(default)]
    version: String,
    #[serde(default)]
    summary: String,
}

/// Empty output is what fwupdmgr leaves when nothing is to update.
pub fn parse(out: &str) -> Result<Vec<Firmware>, String> {
    if out.trim().is_empty() {
        return Ok(Vec::new());
    }
    let a: Answer = serde_json::from_str(out).map_err(|e| e.to_string())?;
    Ok(a.devices
        .into_iter()
        .filter_map(|d| {
            let r = d.releases.into_iter().next()?;
            Some(Firmware {
                device: d.name,
                current: d.version,
                new: r.version,
                summary: r.summary,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_newest_release_per_device() {
        let out = r#"{"Devices":[{"Name":"System Firmware","Version":"1.2","Releases":[{"Version":"1.4","Summary":"BIOS"},{"Version":"1.3"}]},{"Name":"Mouse","Version":"1"}]}"#;
        assert_eq!(
            parse(out).unwrap(),
            [Firmware {
                device: "System Firmware".into(),
                current: "1.2".into(),
                new: "1.4".into(),
                summary: "BIOS".into()
            }]
        );
        assert!(parse("").unwrap().is_empty());
    }
}
