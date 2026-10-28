//! The homelab as Arcane serves it: each environment a server, each project a stack with its git
//! sync and its images.

use serde::Deserialize;

use crate::i18n::{t, tf, tn};
use crate::script::{Failure, op, query};
use crate::ui::Tone;

/// A check older than this no longer tells the state.
const STALE: i64 = 48 * 3600;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Homelab {
    pub servers: Vec<Server>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub id: String,
    pub name: String,
    /// Arcane reached it, and its stacks are as it told.
    pub online: bool,
    pub stacks: Vec<Stack>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    pub id: String,
    pub name: String,
    pub running: usize,
    pub services: usize,
    pub images: Vec<Image>,
    /// When Arcane last checked its images, none where it never did.
    pub checked: Option<jiff::Timestamp>,
    /// The git sync it is deployed from, none for a stack made in Arcane.
    pub sync: Option<Sync>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub reference: String,
    pub state: Update,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    Current,
    /// A new build of the same tag.
    Digest,
    /// A newer tag, which the compose file must name.
    Version(String),
    /// The check failed or never ran.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sync {
    pub id: String,
    /// The last sync failed, with what Arcane said.
    pub failed: Option<String>,
}

/// A page of Arcane's lists.
#[derive(Deserialize)]
struct Page<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
struct Environment {
    id: String,
    name: String,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: String,
    name: String,
    #[serde(default)]
    service_count: usize,
    #[serde(default)]
    running_count: usize,
    #[serde(default)]
    git_ops_managed_by: Option<String>,
    #[serde(default)]
    update_info: Option<UpdateInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    #[serde(default)]
    image_refs: Vec<String>,
    #[serde(default)]
    update_info_by_ref: std::collections::BTreeMap<String, RefInfo>,
    #[serde(default)]
    last_checked_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefInfo {
    #[serde(default)]
    has_update: bool,
    #[serde(default)]
    update_type: String,
    #[serde(default)]
    current_version: String,
    #[serde(default)]
    latest_version: String,
    #[serde(default)]
    error: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitSync {
    id: String,
    #[serde(default)]
    last_sync_status: String,
    #[serde(default)]
    last_sync_error: Option<String>,
}

/// Each path's answer from Arcane, empty where it did not know the path.
fn get(url: &str, paths: &[String]) -> Result<Vec<String>, Failure> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec![url];
    args.extend(paths.iter().map(String::as_str));
    let out = query(op::HOMELAB_GET, &args)?;
    Ok(out.split('\0').map(str::to_string).collect())
}

fn list<T: for<'de> Deserialize<'de>>(json: &str) -> Vec<T> {
    serde_json::from_str::<Page<T>>(json)
        .map(|p| p.data)
        .unwrap_or_default()
}

impl Homelab {
    /// The homelab Arcane at `url` serves, in two rounds: the servers, then the stacks and syncs
    /// of each.
    pub fn load(url: &str) -> Result<Homelab, Failure> {
        let first = get(url, &["/environments?limit=100".into()])?;
        let envs: Vec<Environment> = list(first.first().map_or("", String::as_str));
        let paths: Vec<String> = envs
            .iter()
            .flat_map(|e| {
                [
                    format!("/environments/{}/projects?limit=500", e.id),
                    format!("/environments/{}/gitops-syncs?limit=500", e.id),
                ]
            })
            .collect();
        let second = get(url, &paths)?;
        let at = |i: usize| second.get(i).map_or("", String::as_str);
        let servers = envs
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let (projects, syncs) = (at(i * 2), at(i * 2 + 1));
                Server {
                    id: e.id.clone(),
                    name: e.name.clone(),
                    online: e.status == "online" && !projects.is_empty(),
                    stacks: stacks(&list(projects), &list(syncs)),
                }
            })
            .collect();
        Ok(Homelab { servers })
    }
}

fn stacks(projects: &[Project], syncs: &[GitSync]) -> Vec<Stack> {
    let mut stacks: Vec<Stack> = projects
        .iter()
        .map(|p| {
            let info = p.update_info.as_ref();
            let images = info
                .map(|i| {
                    i.image_refs
                        .iter()
                        .map(|reference| Image {
                            reference: reference.clone(),
                            state: i
                                .update_info_by_ref
                                .get(reference)
                                .map_or(Update::Unknown, update),
                        })
                        .collect()
                })
                .unwrap_or_default();
            Stack {
                id: p.id.clone(),
                name: p.name.clone(),
                running: p.running_count,
                services: p.service_count,
                images,
                checked: info
                    .and_then(|i| i.last_checked_at.as_deref())
                    .and_then(|at| at.parse().ok()),
                sync: p.git_ops_managed_by.as_ref().map(|id| Sync {
                    id: id.clone(),
                    failed: syncs.iter().find(|s| &s.id == id).and_then(|s| {
                        (s.last_sync_status == "failed")
                            .then(|| s.last_sync_error.clone().unwrap_or_default())
                    }),
                }),
            }
        })
        .collect();
    stacks.sort_by(|a, b| a.name.cmp(&b.name));
    stacks
}

fn update(info: &RefInfo) -> Update {
    if !info.error.is_empty() {
        return Update::Unknown;
    }
    if !info.has_update {
        return Update::Current;
    }
    match info.update_type.as_str() {
        "digest" => Update::Digest,
        _ if !info.latest_version.is_empty() && info.latest_version != info.current_version => {
            Update::Version(info.latest_version.clone())
        }
        _ => Update::Digest,
    }
}

/// A stack as its line tells it: running of its services, then what most needs saying.
struct Assessment {
    extra: Option<String>,
    tone: Tone,
    /// An update or a newer version waits, whatever else the tone says.
    update: bool,
}

impl Stack {
    /// Seconds since Arcane checked its images, at `now`.
    fn age(&self, now: jiff::Timestamp) -> Option<i64> {
        self.checked.map(|at| now.as_second() - at.as_second())
    }

    fn assess(&self, now: jiff::Timestamp) -> Assessment {
        let has = |f: fn(&Update) -> bool| self.images.iter().any(|i| f(&i.state));
        let digest = has(|u| *u == Update::Digest);
        let version = self.images.iter().find_map(|i| match &i.state {
            Update::Version(tag) => Some((i.reference.as_str(), tag.as_str())),
            _ => None,
        });
        let stale = self.age(now).is_none_or(|a| a > STALE) && !self.images.is_empty();
        let (extra, mut tone) = if self.sync.as_ref().is_some_and(|s| s.failed.is_some()) {
            (Some(t("sync failed").to_string()), Tone::Warn)
        } else if digest {
            (Some(t("update").to_string()), Tone::Accent)
        } else if let Some((image, tag)) = version {
            let repo = image.rsplit_once(':').map_or(image, |(r, _)| r);
            let name = repo.rsplit('/').next().unwrap_or(repo);
            (
                Some(tf(
                    "{image} available",
                    &[("image", &format!("{name}:{tag}"))],
                )),
                Tone::Accent,
            )
        } else if has(|u| *u == Update::Unknown) {
            (Some(t("unknown").to_string()), Tone::Muted)
        } else if stale {
            (Some(t("unchecked").to_string()), Tone::Warn)
        } else if self.sync.is_none() {
            (Some(t("not in git").to_string()), Tone::Muted)
        } else {
            (None, Tone::Ok)
        };
        if self.services > 0 && self.running == 0 {
            tone = Tone::Error;
        } else if self.running < self.services {
            tone = Tone::Warn;
        }
        Assessment {
            extra,
            tone,
            update: digest || version.is_some(),
        }
    }
}

/// A stack in a line: running of its services, then what most needs saying, apart by `sep`.
pub fn status(stack: &Stack, now: jiff::Timestamp, sep: &str) -> (String, Tone) {
    let a = stack.assess(now);
    let text = format!("{}/{}", stack.running, stack.services);
    match a.extra {
        Some(extra) => (format!("{text} {sep} {extra}"), a.tone),
        None => (text, a.tone),
    }
}

/// How many stacks of a server wait for an update, none where Arcane did not reach it.
pub fn waiting(server: &Server, now: jiff::Timestamp) -> Option<usize> {
    server.online.then(|| {
        server
            .stacks
            .iter()
            .filter(|s| s.assess(now).update)
            .count()
    })
}

/// A server in a line: running of the services of all its stacks, and how many wait for an
/// update, apart by `sep`.
pub fn summary(server: &Server, now: jiff::Timestamp, sep: &str) -> (String, Tone) {
    if !server.online {
        return (t("not reached").into(), Tone::Muted);
    }
    let all: Vec<Assessment> = server.stacks.iter().map(|s| s.assess(now)).collect();
    let running: usize = server.stacks.iter().map(|s| s.running).sum();
    let services: usize = server.stacks.iter().map(|s| s.services).sum();
    let updates = all.iter().filter(|a| a.update).count();
    let worst = [Tone::Error, Tone::Warn, Tone::Accent]
        .into_iter()
        .find(|t| all.iter().any(|a| a.tone == *t))
        .unwrap_or(Tone::Ok);
    let text = format!("{running}/{services}");
    match updates {
        0 => (text, worst),
        n => (
            format!("{text} {sep} {}", tn("{n} update", "{n} updates", n, &[])),
            worst,
        ),
    }
}

/// Each image of a stack, its state, and the tone of the state.
pub fn images(stack: &Stack, now: jiff::Timestamp) -> Vec<(String, String, Tone)> {
    let stale = stack.age(now).is_none_or(|a| a > STALE);
    stack
        .images
        .iter()
        .map(|image| {
            let (state, tone) = match &image.state {
                Update::Version(tag) => (tf("{image} available", &[("image", tag)]), Tone::Accent),
                Update::Digest => (t("update").into(), Tone::Accent),
                Update::Unknown => (t("unknown").into(), Tone::Muted),
                Update::Current if stale => (t("unchecked").into(), Tone::Warn),
                Update::Current => (t("up to date").into(), Tone::Ok),
            };
            (image.reference.clone(), state, tone)
        })
        .collect()
}

/// When Arcane last checked the images of a stack, in a sentence.
pub fn checked(stack: &Stack, now: jiff::Timestamp) -> String {
    let Some(age) = stack.age(now) else {
        return t("Arcane names no check.").into();
    };
    if age < 3600 {
        tf("Arcane checked {n} min ago.", &[("n", &(age / 60))])
    } else if age < STALE {
        tf("Arcane checked {n} h ago.", &[("n", &(age / 3600))])
    } else {
        tf("Arcane checked {n} days ago.", &[("n", &(age / 86400))])
    }
}

/// A homelab as the screenshots and the tests of the pages show it.
#[cfg(test)]
pub fn example() -> Homelab {
    // Checked an hour ago, whenever the example is shown.
    let at = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(1);
    let image = |reference: &str, state| Image {
        reference: reference.into(),
        state,
    };
    Homelab {
        servers: vec![Server {
            id: "0".into(),
            name: "nas.fritz.box".into(),
            online: true,
            stacks: vec![
                Stack {
                    id: "p1".into(),
                    name: "cup".into(),
                    running: 2,
                    services: 2,
                    images: vec![
                        image("ghcr.io/sergi0g/cup:v3.5.1", Update::Current),
                        image("ghcr.io/linuxserver/socket-proxy:3.4.7", Update::Current),
                    ],
                    checked: Some(at),
                    sync: Some(Sync {
                        id: "s1".into(),
                        failed: None,
                    }),
                },
                Stack {
                    id: "p2".into(),
                    name: "jellyfin".into(),
                    running: 1,
                    services: 1,
                    images: vec![image("jellyfin/jellyfin:10", Update::Digest)],
                    checked: Some(at),
                    sync: Some(Sync {
                        id: "s2".into(),
                        failed: None,
                    }),
                },
            ],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> jiff::Timestamp {
        "2026-10-18T12:00:00Z".parse().unwrap()
    }

    fn stack(images: Vec<Image>, running: usize, services: usize) -> Stack {
        Stack {
            id: "p".into(),
            name: "s".into(),
            running,
            services,
            images,
            checked: Some("2026-10-18T11:00:00Z".parse().unwrap()),
            sync: Some(Sync {
                id: "x".into(),
                failed: None,
            }),
        }
    }

    fn image(reference: &str, state: Update) -> Image {
        Image {
            reference: reference.into(),
            state,
        }
    }

    #[test]
    fn a_stack_counts_what_runs_and_names_what_waits() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let s = stack(
            vec![image("a:1", Update::Current), image("b:2", Update::Digest)],
            2,
            2,
        );
        assert_eq!(
            status(&s, now(), "·"),
            ("2/2 · update".into(), Tone::Accent)
        );
        let s = stack(vec![image("x/a:1", Update::Version("1.1".into()))], 1, 1);
        assert_eq!(
            status(&s, now(), "·"),
            ("1/1 · a:1.1 available".into(), Tone::Accent)
        );
        let s = stack(vec![image("a:1", Update::Current)], 0, 1);
        assert_eq!(status(&s, now(), "·"), ("0/1".into(), Tone::Error));
        let mut s = stack(vec![image("a:1", Update::Current)], 1, 2);
        assert_eq!(status(&s, now(), "·"), ("1/2".into(), Tone::Warn));
        s.checked = Some("2026-10-10T00:00:00Z".parse().unwrap());
        s.running = 2;
        assert_eq!(
            status(&s, now(), "·"),
            ("2/2 · unchecked".into(), Tone::Warn)
        );
        assert_eq!(checked(&s, now()), "Arcane checked 8 days ago.");
        let mut s = stack(vec![image("a:1", Update::Unknown)], 1, 1);
        assert_eq!(
            status(&s, now(), "·"),
            ("1/1 · unknown".into(), Tone::Muted)
        );
        s.images[0].state = Update::Current;
        s.sync = None;
        assert_eq!(
            status(&s, now(), "·"),
            ("1/1 · not in git".into(), Tone::Muted)
        );
        s.sync = Some(Sync {
            id: "x".into(),
            failed: Some("compose file not found".into()),
        });
        assert_eq!(
            status(&s, now(), "·"),
            ("1/1 · sync failed".into(), Tone::Warn)
        );
    }

    #[test]
    fn a_server_sums_its_stacks_and_says_where_arcane_did_not_reach_it() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let mut server = example().servers.remove(0);
        server.stacks[0].checked = Some("2026-10-18T11:00:00Z".parse().unwrap());
        server.stacks[1].checked = Some("2026-10-18T11:00:00Z".parse().unwrap());
        assert_eq!(
            summary(&server, now(), "·"),
            ("3/3 · 1 update".into(), Tone::Accent)
        );
        assert_eq!(waiting(&server, now()), Some(1));
        server.online = false;
        assert_eq!(
            summary(&server, now(), "·"),
            ("not reached".into(), Tone::Muted)
        );
        assert_eq!(waiting(&server, now()), None);
    }

    /// Answers as Arcane 2.17 gives them, cut to what the manager reads.
    const PROJECTS: &str = r#"{"success":true,"data":[
        {"id":"p2","name":"web","status":"running","serviceCount":1,"runningCount":1,
         "gitOpsManagedBy":"s1","updateInfo":{"imageRefs":["traefik/whoami:v1.10"],
         "updateInfoByRef":{"traefik/whoami:v1.10":{"hasUpdate":true,"updateType":"digest",
         "currentVersion":"v1.10","latestVersion":"v1.10","error":""}},
         "lastCheckedAt":"2026-10-25T10:47:50.273869909Z"}},
        {"id":"p1","name":"adguard","status":"stopped","serviceCount":1,"runningCount":0,
         "gitOpsManagedBy":null,"updateInfo":{"imageRefs":["adguard/adguardhome:v0.107"],
         "updateInfoByRef":{"adguard/adguardhome:v0.107":{"hasUpdate":true,"updateType":"tag",
         "currentVersion":"v0.107","latestVersion":"v0.108","error":""}}}}],
        "pagination":{"totalItems":2}}"#;

    #[test]
    fn stacks_are_read_from_arcane_with_their_images_and_sync() {
        let syncs = list::<GitSync>(
            r#"{"data":[{"id":"s1","lastSyncStatus":"failed","lastSyncError":"no such file"}]}"#,
        );
        let stacks = stacks(&list(PROJECTS), &syncs);
        assert_eq!(stacks[0].name, "adguard", "by name");
        assert_eq!(stacks[0].images[0].state, Update::Version("v0.108".into()));
        assert_eq!((stacks[0].running, stacks[0].services), (0, 1));
        assert!(stacks[0].sync.is_none() && stacks[0].checked.is_none());
        assert_eq!(stacks[1].images[0].state, Update::Digest);
        assert_eq!(
            stacks[1].sync.as_ref().unwrap().failed.as_deref(),
            Some("no such file")
        );
        assert!(stacks[1].checked.is_some());
        assert!(list::<Project>("").is_empty(), "an empty answer is no list");
    }
}
