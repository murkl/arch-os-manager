//! Answers of the AUR's RPC interface.

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Package {
    pub name: String,
    pub package_base: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "URL", default)]
    pub url: Option<String>,
    #[serde(default)]
    pub num_votes: u32,
    #[serde(default)]
    pub popularity: f64,
    /// When it was flagged out of date, seconds since the epoch.
    #[serde(default)]
    pub out_of_date: Option<i64>,
    #[serde(default)]
    pub maintainer: Option<String>,
    #[serde(default)]
    pub last_modified: i64,
    #[serde(default)]
    pub depends: Vec<String>,
}

#[derive(Deserialize)]
struct Response {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    results: Vec<Package>,
}

/// One JSON answer per line, as aur.sh prints them.
pub fn parse(out: &str) -> Result<Vec<Package>, String> {
    let mut all = Vec::new();
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        let r: Response = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if r.kind == "error" {
            return Err(r.error.unwrap_or_default());
        }
        all.extend(r.results);
    }
    Ok(all)
}

/// Of `names`, those the AUR did not answer with.
pub fn unknown(names: &[String], found: &[Package]) -> Vec<String> {
    names
        .iter()
        .filter(|n| !found.iter().any(|p| &p.name == *n))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_results_across_answers_and_reports_errors() {
        let a = r#"{"resultcount":1,"results":[{"Name":"paru","PackageBase":"paru","Version":"2.1.0-1","Description":"Feature packed AUR helper","URL":"https://github.com/morganamilo/paru","NumVotes":1000,"Popularity":30.5,"OutOfDate":null,"Maintainer":"Morganamilo","LastModified":1760000000,"Depends":["git","pacman"]}],"type":"multiinfo","version":5}"#;
        let b = r#"{"resultcount":0,"results":[],"type":"multiinfo","version":5}"#;
        let p = parse(&format!("{a}\n{b}\n")).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(
            (p[0].num_votes, p[0].out_of_date, p[0].depends.len()),
            (1000, None, 2)
        );
        let e = r#"{"error":"Too many package results.","resultcount":0,"results":[],"type":"error","version":5}"#;
        assert_eq!(parse(e).unwrap_err(), "Too many package results.");
    }

    #[test]
    fn a_name_the_aur_does_not_answer_is_unknown() {
        let found = parse(r#"{"resultcount":1,"results":[{"Name":"paru","PackageBase":"paru","Version":"2.1.0-1"}],"type":"multiinfo","version":5}"#).unwrap();
        let names = ["gone".to_string(), "paru".into(), "old".into()];
        assert_eq!(unknown(&names, &found), ["gone", "old"]);
        assert!(unknown(&names[1..2], &found).is_empty());
    }
}
