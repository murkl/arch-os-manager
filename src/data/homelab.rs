//! The homelab in `~/.dotfiles/homelab`: `homelab.yaml` names the servers with the address of
//! their Cup, each a folder with a folder per stack. What runs there says each server's Cup.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::i18n::{t, tf, tn};
use crate::script::{Failure, op, query};
use crate::ui::Tone;

/// A check older than this no longer tells the state.
const STALE: i64 = 48 * 3600;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Homelab {
    pub servers: Vec<Server>,
    /// Why homelab.yaml could not be read.
    pub problem: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub name: String,
    pub cup: String,
    pub stacks: Vec<Stack>,
    /// The secrets of the server by their path in its folder, without `.age`.
    pub secrets: Vec<String>,
    /// What its Cup said, or why it did not answer.
    pub report: Result<Report, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    pub name: String,
    /// Every image its services run, as Docker names them.
    pub images: Vec<String>,
    /// Those only a compose profile starts, which need not run.
    pub optional: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    servers: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    cup: String,
}

#[derive(Deserialize)]
struct Compose {
    #[serde(default)]
    services: BTreeMap<String, Service>,
}

#[derive(Deserialize)]
struct Service {
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    profiles: Vec<String>,
}

impl Homelab {
    /// The homelab of the dotfiles, each server asked through its Cup.
    pub fn load(dir: &Path) -> Result<Homelab, Failure> {
        let mut lab = Homelab::read(&dir.join("homelab"));
        for server in &mut lab.servers {
            server.report = query(op::DOTFILES_HOMELAB_CUP, &[&server.cup])
                .map_err(|f| f.headline())
                .and_then(|json| Report::parse(&json));
        }
        Ok(lab)
    }

    /// What the files say, before any Cup is asked.
    fn read(dir: &Path) -> Homelab {
        let src = std::fs::read_to_string(dir.join("homelab.yaml")).unwrap_or_default();
        if src.trim().is_empty() {
            return Homelab::default();
        }
        let file: File = match serde_saphyr::from_str(&src) {
            Ok(file) => file,
            Err(e) => {
                return Homelab {
                    servers: Vec::new(),
                    problem: Some(format!("homelab.yaml: {e}")),
                };
            }
        };
        let mut problem = None;
        let mut servers = Vec::new();
        for e in file.servers {
            let plain = !e.name.is_empty()
                && e.name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
                && !e.name.starts_with(['.', '-']);
            let web = e.cup.starts_with("http://") || e.cup.starts_with("https://");
            if !plain || !web || !dir.join(&e.name).is_dir() {
                problem.get_or_insert(tf(
                    "homelab.yaml: {name} needs a folder beside it and an http or https address of its Cup",
                    &[("name", &e.name)],
                ));
                continue;
            }
            servers.push(Server {
                stacks: stacks(&dir.join(&e.name), &mut problem),
                secrets: secrets(&dir.join(&e.name)),
                name: e.name,
                cup: e.cup,
                report: Err(String::new()),
            });
        }
        Homelab { servers, problem }
    }
}

/// The stacks of a server: each folder with a compose.yaml, by name.
fn stacks(server: &Path, problem: &mut Option<String>) -> Vec<Stack> {
    let mut names: Vec<String> = std::fs::read_dir(server)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| {
            let file = server.join(&name).join("compose.yaml");
            let src = std::fs::read_to_string(&file).ok()?;
            let compose: Compose = serde_saphyr::from_str(&src)
                .map_err(|e| {
                    problem.get_or_insert(format!("{name}/compose.yaml: {e}"));
                })
                .unwrap_or(Compose {
                    services: BTreeMap::new(),
                });
            let mut stack = Stack {
                name,
                ..Stack::default()
            };
            let mut needed = Vec::new();
            for service in compose.services.values() {
                let Some(image) = &service.image else {
                    continue;
                };
                let image = familiar(image);
                if !stack.images.contains(&image) {
                    stack.images.push(image.clone());
                }
                if service.profiles.is_empty() {
                    needed.push(image);
                }
            }
            stack.optional = stack
                .images
                .iter()
                .filter(|i| !needed.contains(i))
                .cloned()
                .collect();
            Some(stack)
        })
        .collect()
}

/// The secrets of a server by their path, `.age` left off, by name.
fn secrets(server: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut dirs = vec![server.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                dirs.push(path);
            } else if let Some(rel) = path
                .strip_prefix(server)
                .ok()
                .and_then(|p| p.to_str())
                .and_then(|p| p.strip_suffix(".age"))
            {
                found.push(rel.to_string());
            }
        }
    }
    found.sort();
    found
}

/// An image as Docker reports it: without `docker.io/library/`, with its tag.
fn familiar(image: &str) -> String {
    let image = image
        .trim_start_matches("docker.io/")
        .trim_start_matches("library/");
    match tag(image) {
        Some(_) => image.to_string(),
        None => format!("{image}:latest"),
    }
}

fn tag(image: &str) -> Option<&str> {
    let (_, t) = image.rsplit_once(':')?;
    (!t.contains('/')).then_some(t)
}

fn repository(image: &str) -> &str {
    match tag(image) {
        Some(t) => &image[..image.len() - t.len() - 1],
        None => image,
    }
}

/// What a server's Cup says, as its API answers.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Report {
    images: Vec<Image>,
    last_updated: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Image {
    reference: String,
    #[serde(default)]
    in_use: Option<bool>,
    #[serde(default)]
    result: Outcome,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Outcome {
    #[serde(default)]
    has_update: Option<bool>,
    #[serde(default)]
    info: Option<Info>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Info {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    new_tag: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Current,
    Update,
    Unchecked,
    Unknown,
    Absent,
}

impl Report {
    pub fn parse(json: &str) -> Result<Report, String> {
        serde_json::from_str(json).map_err(|e| format!("Cup: {e}"))
    }

    /// Seconds since Cup last checked, at `now`.
    fn age(&self, now: jiff::Timestamp) -> Option<i64> {
        let at: jiff::Timestamp = self.last_updated.parse().ok()?;
        Some(now.as_second() - at.as_second())
    }

    fn image(&self, reference: &str) -> Option<&Image> {
        self.images.iter().find(|i| i.reference == reference)
    }

    fn state(&self, reference: &str, now: jiff::Timestamp) -> State {
        let Some(img) = self.image(reference).filter(|i| i.in_use == Some(true)) else {
            return State::Absent;
        };
        match img.result.has_update {
            _ if img.result.error.is_some() => State::Unknown,
            None => State::Unknown,
            _ if self.age(now).is_none_or(|a| a > STALE) => State::Unchecked,
            Some(true) if newer(img).is_none() => State::Update,
            _ => State::Current,
        }
    }
}

/// A newer tag to raise the compose.yaml to, as Cup names one.
fn newer(img: &Image) -> Option<&str> {
    img.result
        .info
        .as_ref()
        .filter(|i| i.kind == "version")
        .and_then(|i| i.new_tag.as_deref())
}

/// A stack as its line tells it: running of needed images, then what most needs saying.
struct Assessment {
    running: usize,
    need: usize,
    extra: Option<String>,
    tone: Tone,
    /// An update or a newer version waits, whatever else the tone says.
    update: bool,
}

fn assess(stack: &Stack, report: &Report, now: jiff::Timestamp) -> Assessment {
    let (mut need, mut running) = (0, 0);
    let mut states = Vec::new();
    for image in &stack.images {
        let optional = stack.optional.contains(image);
        need += usize::from(!optional);
        let state = report.state(image, now);
        if state != State::Absent {
            running += usize::from(!optional);
            states.push((image, state));
        }
    }
    if need > 0 && running == 0 {
        return Assessment {
            running,
            need,
            extra: None,
            tone: Tone::Error,
            update: false,
        };
    }
    let available = states.iter().find_map(|(image, state)| {
        let tag = report.image(image).and_then(newer)?;
        matches!(state, State::Current | State::Unchecked).then(|| {
            let repo = repository(image);
            let name = repo.rsplit('/').next().unwrap_or(repo);
            tf("{image} available", &[("image", &format!("{name}:{tag}"))])
        })
    });
    let has = |s: State| states.iter().any(|(_, x)| *x == s);
    let update = has(State::Update) || available.is_some();
    let (extra, mut tone) = if has(State::Update) {
        (Some(t("update").to_string()), Tone::Accent)
    } else if let Some(a) = available {
        (Some(a), Tone::Accent)
    } else if has(State::Unknown) {
        (Some(t("unknown").to_string()), Tone::Muted)
    } else if has(State::Unchecked) {
        (Some(t("unchecked").to_string()), Tone::Warn)
    } else {
        (None, Tone::Ok)
    };
    if running < need {
        tone = Tone::Warn;
    }
    Assessment {
        running,
        need,
        extra,
        tone,
        update,
    }
}

/// A stack in a line: running of needed images, then what most needs saying, apart by `sep`.
pub fn status(
    stack: &Stack,
    report: Option<&Report>,
    now: jiff::Timestamp,
    sep: &str,
) -> (String, Tone) {
    let Some(report) = report.filter(|_| !stack.images.is_empty()) else {
        return (t("unknown").into(), Tone::Muted);
    };
    let a = assess(stack, report, now);
    let text = format!("{}/{}", a.running, a.need);
    match a.extra {
        Some(extra) => (format!("{text} {sep} {extra}"), a.tone),
        None => (text, a.tone),
    }
}

/// How many stacks of a server wait for an update, none where its Cup did not answer.
pub fn waiting(server: &Server, now: jiff::Timestamp) -> Option<usize> {
    let report = server.report.as_ref().ok()?;
    Some(
        server
            .stacks
            .iter()
            .filter(|s| !s.images.is_empty())
            .filter(|s| assess(s, report, now).update)
            .count(),
    )
}

/// A server in a line: running of needed images of all its stacks, and how many wait for an
/// update, apart by `sep`.
pub fn summary(server: &Server, now: jiff::Timestamp, sep: &str) -> (String, Tone) {
    let Ok(report) = &server.report else {
        return (t("not reached").into(), Tone::Muted);
    };
    let all: Vec<Assessment> = server
        .stacks
        .iter()
        .filter(|s| !s.images.is_empty())
        .map(|s| assess(s, report, now))
        .collect();
    let running: usize = all.iter().map(|a| a.running).sum();
    let need: usize = all.iter().map(|a| a.need).sum();
    let updates = all.iter().filter(|a| a.update).count();
    let worst = [Tone::Error, Tone::Warn, Tone::Accent]
        .into_iter()
        .find(|t| all.iter().any(|a| a.tone == *t))
        .unwrap_or(Tone::Ok);
    let text = format!("{running}/{need}");
    match updates {
        0 => (text, worst),
        n => (
            format!("{text} {sep} {}", tn("{n} update", "{n} updates", n, &[])),
            worst,
        ),
    }
}

/// Each image of a stack as repository and tag, its state, and the tone of the state.
pub fn images(
    stack: &Stack,
    report: Option<&Report>,
    now: jiff::Timestamp,
) -> Vec<(String, String, Tone)> {
    stack
        .images
        .iter()
        .map(|image| {
            let (state, tone) = match report {
                None => (t("unknown").to_string(), Tone::Muted),
                Some(r) => match (r.state(image, now), r.image(image).and_then(newer)) {
                    (State::Current | State::Unchecked, Some(tag)) => {
                        (tf("{image} available", &[("image", &tag)]), Tone::Accent)
                    }
                    (State::Absent, _) if stack.optional.contains(image) => {
                        (t("optional, not running").into(), Tone::Muted)
                    }
                    (State::Absent, _) => (t("not running").into(), Tone::Error),
                    (State::Current, _) => (t("up to date").into(), Tone::Ok),
                    (State::Update, _) => (t("update").into(), Tone::Accent),
                    (State::Unchecked, _) => (t("unchecked").into(), Tone::Warn),
                    (State::Unknown, _) => (t("unknown").into(), Tone::Muted),
                },
            };
            (image.clone(), state, tone)
        })
        .collect()
}

/// When Cup last checked, in a sentence.
pub fn checked(report: &Report, now: jiff::Timestamp) -> String {
    let Some(age) = report.age(now) else {
        return t("Cup names no check.").into();
    };
    if age < 3600 {
        tf("Cup checked {n} min ago.", &[("n", &(age / 60))])
    } else if age < STALE {
        tf("Cup checked {n} h ago.", &[("n", &(age / 3600))])
    } else {
        tf("Cup checked {n} days ago.", &[("n", &(age / 86400))])
    }
}

/// A homelab as the screenshots and the tests of the pages show it.
#[cfg(test)]
pub fn example() -> Homelab {
    // Checked an hour ago, whenever the example is shown.
    let at = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(1);
    let image = |reference: &str, update: bool| {
        format!(
            r#"{{"reference":"{reference}","in_use":true,"result":{{"has_update":{update},"info":null,"error":null}}}}"#
        )
    };
    let images = [
        image("ghcr.io/sergi0g/cup:v3.5.1", false),
        image("ghcr.io/linuxserver/socket-proxy:3.4.7", false),
        image("jellyfin/jellyfin:10", true),
    ];
    let report = Report::parse(&format!(
        r#"{{"metrics":{{}},"last_updated":"{at}","images":[{}]}}"#,
        images.join(",")
    ))
    .unwrap();
    Homelab {
        servers: vec![Server {
            name: "nas.fritz.box".into(),
            cup: "http://nas.fritz.box:8000".into(),
            stacks: vec![
                Stack {
                    name: "cup".into(),
                    images: vec![
                        "ghcr.io/sergi0g/cup:v3.5.1".into(),
                        "ghcr.io/linuxserver/socket-proxy:3.4.7".into(),
                    ],
                    optional: Vec::new(),
                },
                Stack {
                    name: "jellyfin".into(),
                    images: vec!["jellyfin/jellyfin:10".into()],
                    optional: Vec::new(),
                },
            ],
            secrets: vec!["traefik/.env".into()],
            report: Ok(report),
        }],
        problem: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> jiff::Timestamp {
        "2026-10-18T12:00:00Z".parse().unwrap()
    }

    fn report(images: &str, at: &str) -> Report {
        Report::parse(&format!(
            r#"{{"metrics":{{}},"images":[{images}],"last_updated":"{at}"}}"#
        ))
        .unwrap()
    }

    fn image(reference: &str, in_use: bool, has_update: bool, new_tag: Option<&str>) -> String {
        let info = match new_tag {
            Some(t) => format!(r#"{{"type":"version","new_tag":"{t}"}}"#),
            None => "null".into(),
        };
        format!(
            r#"{{"reference":"{reference}","in_use":{in_use},"result":{{"has_update":{has_update},"info":{info},"error":null}}}}"#
        )
    }

    fn stack(images: &[&str], optional: &[&str]) -> Stack {
        Stack {
            name: "s".into(),
            images: images.iter().map(|s| s.to_string()).collect(),
            optional: optional.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_stack_counts_what_runs_and_names_what_waits() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let fresh = "2026-10-18T11:00:00Z";
        let r = report(
            &[
                image("a:1", true, false, None),
                image("b:2", true, true, None),
            ]
            .join(","),
            fresh,
        );
        assert_eq!(
            status(&stack(&["a:1", "b:2"], &[]), Some(&r), now(), "·"),
            ("2/2 · update".into(), Tone::Accent)
        );
        let r = report(&image("a:1", true, true, Some("1.1")), fresh);
        assert_eq!(
            status(&stack(&["a:1", "c:3"], &["c:3"]), Some(&r), now(), "·"),
            ("1/1 · a:1.1 available".into(), Tone::Accent)
        );
        let r = report(&image("a:1", false, false, None), fresh);
        assert_eq!(
            status(&stack(&["a:1"], &[]), Some(&r), now(), "·"),
            ("0/1".into(), Tone::Error)
        );
        let old = report(&image("a:1", true, false, None), "2026-10-10T00:00:00Z");
        assert_eq!(
            status(&stack(&["a:1"], &[]), Some(&old), now(), "·"),
            ("1/1 · unchecked".into(), Tone::Warn)
        );
        assert_eq!(
            status(&stack(&["a:1"], &[]), None, now(), "·"),
            ("unknown".into(), Tone::Muted)
        );
        assert_eq!(checked(&old, now()), "Cup checked 8 days ago.");
        let r = report(
            &[
                image("a:1", true, true, None),
                image("b:2", true, false, None),
            ]
            .join(","),
            fresh,
        );
        let server = Server {
            name: "h".into(),
            cup: "http://h:8000".into(),
            stacks: vec![stack(&["a:1"], &[]), stack(&["b:2"], &[]), stack(&[], &[])],
            secrets: Vec::new(),
            report: Ok(r),
        };
        assert_eq!(
            summary(&server, now(), "·"),
            ("2/2 · 1 update".into(), Tone::Accent)
        );
        assert_eq!(waiting(&server, now()), Some(1));
        let gone = Server {
            report: Err("x".into()),
            ..server
        };
        assert_eq!(
            summary(&gone, now(), "·"),
            ("not reached".into(), Tone::Muted)
        );
        assert_eq!(waiting(&gone, now()), None);
    }

    #[test]
    fn the_homelab_reads_its_servers_stacks_and_secrets() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let dir = std::env::temp_dir().join(format!("sm-homelab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |path: &str, text: &str| {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "homelab.yaml",
            "servers:\n  - name: nas\n    cup: http://nas:8000\n  - name: gone\n    cup: http://gone:8000\n",
        );
        write(
            "nas/media/compose.yaml",
            "services:\n  app:\n    image: docker.io/library/postgres:18\n  extra:\n    image: x/y\n    profiles: [extra]\n",
        );
        write("nas/notes/README.md", "no stack\n");
        write("nas/traefik/.env.age", "sealed");
        write("nas/adguard/conf/AdGuardHome.yaml.age", "sealed");
        let lab = Homelab::read(&dir);
        assert_eq!(
            lab.servers.len(),
            1,
            "a server without a folder is left out"
        );
        assert!(lab.problem.as_deref().unwrap().contains("gone"), "{lab:?}");
        let nas = &lab.servers[0];
        assert_eq!(nas.stacks.len(), 1);
        assert_eq!(nas.stacks[0].images, ["postgres:18", "x/y:latest"]);
        assert_eq!(nas.stacks[0].optional, ["x/y:latest"]);
        assert_eq!(
            nas.secrets,
            ["adguard/conf/AdGuardHome.yaml", "traefik/.env"]
        );
        write("homelab.yaml", "server: []\n");
        assert!(
            Homelab::read(&dir)
                .problem
                .unwrap()
                .contains("unknown field")
        );
        write(
            "homelab.yaml",
            "servers:\n  - name: nas\n    cup: ftp://nas\n",
        );
        assert!(Homelab::read(&dir).servers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_template_of_cup_is_a_stack_the_homelab_reads() {
        let template = crate::embedded::SCRIPTS
            .iter()
            .find(|(f, _)| *f == "dotfiles/homelab/cup/compose.yaml")
            .unwrap()
            .1;
        let compose: Compose =
            serde_saphyr::from_str(std::str::from_utf8(template).unwrap()).unwrap();
        let images: Vec<String> = compose
            .services
            .values()
            .filter_map(|s| s.image.as_deref().map(familiar))
            .collect();
        assert!(
            images.iter().all(|i| tag(i).is_some_and(|t| t != "latest")),
            "pinned: {images:?}"
        );
    }
}
