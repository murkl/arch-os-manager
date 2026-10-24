//! What the clean-up would remove, part by part, as cleanup.sh lists it.

use serde::Deserialize;

use super::parse_size;

/// What the clean-up can remove, in the order it removes it. `cleanup run` takes each by its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    Orphans,
    Cache,
    Aur,
    Flatpak,
    Docker,
    Podman,
}

impl Part {
    pub const ALL: [Part; 6] = [
        Part::Orphans,
        Part::Cache,
        Part::Aur,
        Part::Flatpak,
        Part::Docker,
        Part::Podman,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Part::Orphans => "orphans",
            Part::Cache => "cache",
            Part::Aur => "aur",
            Part::Flatpak => "flatpak",
            Part::Docker => "docker",
            Part::Podman => "podman",
        }
    }

    pub fn from_id(id: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.id() == id)
    }

    /// Taken unless chosen otherwise: what the system no longer needs. Images are pulled or built
    /// on purpose, as packages are installed, so no engine is taken by default.
    pub fn default(self) -> bool {
        !matches!(self, Part::Docker | Part::Podman)
    }
}

/// A container engine as the plan found it.
#[derive(Clone, Debug, PartialEq)]
pub enum Engine {
    /// The images no container uses, and what pruning frees by the engine's own count.
    Ready {
        unused: Vec<String>,
        bytes: Option<u64>,
    },
    /// Docker's service is not running, and asking or cleaning it up would start it, with every
    /// container set to restart.
    Stopped,
    /// This user may not write to Docker's socket.
    Denied,
    /// It did not answer, or not in a way that could be read.
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub orphans: Vec<(String, u64)>,
    pub cache: Vec<(String, u64)>,
    pub aur: Vec<(String, u64)>,
    /// Only where it is installed.
    pub docker: Option<Engine>,
    pub podman: Option<Engine>,
}

/// A line of `docker system df --format json`.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerUsage {
    r#type: String,
    reclaimable: String,
}

/// `docker system df --verbose --format json`, of which only the images.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerImages {
    images: Vec<DockerImage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerImage {
    repository: String,
    tag: String,
    #[serde(rename = "ID")]
    id: String,
    /// How many containers use it, as text.
    containers: String,
}

/// A line of `podman system df --format '{{json .}}'`.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PodmanUsage {
    r#type: String,
    raw_reclaimable: u64,
}

impl Plan {
    pub fn parse(out: &str) -> Plan {
        let mut plan = Plan::default();
        let (mut docker, mut podman) = (Found::default(), Found::default());
        for line in out.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            match f.as_slice() {
                ["orphan", name, size] => plan.orphans.push((name.to_string(), parse_size(size))),
                ["cache", path, bytes] => plan
                    .cache
                    .push((path.to_string(), bytes.parse().unwrap_or(0))),
                ["aur", path, bytes] => plan
                    .aur
                    .push((path.to_string(), bytes.parse().unwrap_or(0))),
                ["docker", state] => docker.state = Some(state.to_string()),
                ["docker-df", json] => {
                    docker.read(serde_json::from_str::<DockerUsage>(json).map(|u| {
                        match u.r#type.as_str() {
                            // Pruning the images, then the build cache.
                            "Images" | "Build Cache" => {
                                let size = u.reclaimable.split(" (").next().unwrap_or_default();
                                decimal_size(size)
                            }
                            _ => Some(0),
                        }
                    }))
                }
                ["docker-du", json] => match serde_json::from_str::<DockerImages>(json) {
                    Ok(du) => docker.unused.extend(
                        du.images
                            .iter()
                            .filter(|i| i.containers == "0")
                            .map(|i| image(&i.repository, &i.tag, &i.id)),
                    ),
                    Err(_) => docker.broken = true,
                },
                ["podman", state] => podman.state = Some(state.to_string()),
                ["podman-df", json] => {
                    podman.read(serde_json::from_str::<PodmanUsage>(json).map(|u| {
                        Some(if u.r#type == "Images" {
                            u.raw_reclaimable
                        } else {
                            0
                        })
                    }))
                }
                ["podman-image", name, id] => {
                    let (repository, tag) = name.rsplit_once(':').unwrap_or((name, "<none>"));
                    podman.unused.push(image(repository, tag, id));
                }
                _ => {}
            }
        }
        plan.docker = docker.engine();
        plan.podman = podman.engine();
        plan
    }

    /// The space a part frees, `None` where only the removal itself tells.
    pub fn bytes(&self, part: Part) -> Option<u64> {
        let sum = |v: &[(String, u64)]| v.iter().map(|(_, b)| b).sum();
        match part {
            Part::Orphans => Some(sum(&self.orphans)),
            Part::Cache => Some(sum(&self.cache)),
            Part::Aur => Some(sum(&self.aur)),
            Part::Flatpak => None,
            Part::Docker | Part::Podman => match self.engine(part)? {
                Engine::Ready { bytes, .. } => *bytes,
                _ => None,
            },
        }
    }

    pub fn engine(&self, part: Part) -> Option<&Engine> {
        match part {
            Part::Docker => self.docker.as_ref(),
            Part::Podman => self.podman.as_ref(),
            _ => None,
        }
    }

    /// Whether a part can be cleaned up now: an engine that runs, asked or not.
    pub fn ready(&self, part: Part) -> bool {
        match part {
            Part::Docker | Part::Podman => {
                matches!(
                    self.engine(part),
                    Some(Engine::Ready { .. } | Engine::Denied)
                )
            }
            _ => true,
        }
    }

    /// What the parts taken free together, as far as it is known before.
    pub fn frees(&self, takes: impl Fn(Part) -> bool) -> u64 {
        Part::ALL
            .into_iter()
            .filter(|p| takes(*p) && self.ready(*p))
            .filter_map(|p| self.bytes(p))
            .sum()
    }
}

/// What the plan said of one engine, until all of it is read.
#[derive(Default)]
struct Found {
    state: Option<String>,
    unused: Vec<String>,
    bytes: u64,
    /// A size could not be read, so the sum is not known.
    unknown: bool,
    /// A line that should have been JSON was not.
    broken: bool,
}

impl Found {
    /// Adds a usage line: what it frees, `None` for a size that could not be read.
    fn read(&mut self, usage: Result<Option<u64>, serde_json::Error>) {
        match usage {
            Ok(Some(b)) => self.bytes += b,
            Ok(None) => self.unknown = true,
            Err(_) => self.broken = true,
        }
    }

    fn engine(self) -> Option<Engine> {
        Some(match self.state?.as_str() {
            "ready" if !self.broken => Engine::Ready {
                unused: self.unused,
                bytes: (!self.unknown).then_some(self.bytes),
            },
            "stopped" => Engine::Stopped,
            "denied" => Engine::Denied,
            _ => Engine::Failed,
        })
    }
}

/// An image by its name and tag, one without either by the start of its id, as engines list it.
fn image(repository: &str, tag: &str, id: &str) -> String {
    match (repository, tag) {
        ("<none>", _) => id.trim_start_matches("sha256:").chars().take(12).collect(),
        (_, "<none>") => repository.to_string(),
        _ => format!("{repository}:{tag}"),
    }
}

/// A size as Docker prints it, decimal: `5.278GB`, `76.64MB`, `0B`.
fn decimal_size(s: &str) -> Option<u64> {
    let at = s.find(|c: char| c.is_ascii_alphabetic())?;
    let value: f64 = s[..at].parse().ok()?;
    let power = ["B", "kB", "MB", "GB", "TB", "PB"]
        .iter()
        .position(|u| *u == &s[at..])?;
    Some((value * 1000f64.powi(power as i32)).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Docker 29 and Podman 6 as they answered, the names neutral.
    const ENGINES: &str = concat!(
        "docker\tready\n",
        r#"docker-df	{"Active":"1","Reclaimable":"5.278GB (73%)","Size":"7.175GB","TotalCount":"6","Type":"Images"}"#,
        "\n",
        r#"docker-df	{"Active":"2","Reclaimable":"0B (0%)","Size":"523.4MB","TotalCount":"2","Type":"Containers"}"#,
        "\n",
        r#"docker-df	{"Active":"0","Reclaimable":"880.6MB (100%)","Size":"880.6MB","TotalCount":"4","Type":"Local Volumes"}"#,
        "\n",
        r#"docker-df	{"Active":"0","Reclaimable":"76.64MB","Size":"4.55GB","TotalCount":"14","Type":"Build Cache"}"#,
        "\n",
        r#"docker-du	{"Images":[{"Containers":"0","CreatedAt":"2026-10-15 19:39:55 +0200 CEST","CreatedSince":"3 days ago","Digest":"","ID":"sha256:b0f6405e55821bc64cdc189940a738d7b2e127041abe7a229745a6424e16829a","Repository":"example/dev","SharedSize":"567.6MB","Size":"3.08GB","Tag":"latest","UniqueSize":"2.514GB"},{"Containers":"2","CreatedAt":"2026-10-12 19:57:38 +0200 CEST","CreatedSince":"6 days ago","Digest":"","ID":"sha256:061679925e5d5028f2df9395d441ce6c4b9accf0157ff3486c06b6962e014ef1","Repository":"archlinux","SharedSize":"0B","Size":"1.33GB","Tag":"base-devel","UniqueSize":"1.329GB"},{"Containers":"0","CreatedAt":"2026-10-12 19:57:16 +0200 CEST","CreatedSince":"6 days ago","Digest":"","ID":"sha256:3c64b538588416593f69a1aebd2f2ec0f2541b29159f32556fa7def2167147a0","Repository":"<none>","SharedSize":"567.6MB","Size":"576MB","Tag":"<none>","UniqueSize":"8.261MB"}],"Containers":[],"Volumes":[],"BuildCache":[{"CacheType":"regular","CreatedAt":"2026-10-13 01:10:57.417870846 +0000 UTC","CreatedSince":"6 days ago","Description":"","ID":"zphuk1u7folzxf2bbq34wzx90","InUse":"false","LastUsedAt":"2026-10-13 01:19:42.044963726 +0000 UTC","LastUsedSince":"6 days ago","Parent":"","Shared":"true","Size":"568MB","UsageCount":"2"}]}"#,
        "\n",
        "podman\tready\n",
        r#"podman-df	{"Type":"Images","Total":2,"Active":1,"RawSize":13406767,"RawReclaimable":8715873,"TotalCount":2,"Size":"13.41MB","Reclaimable":"8.716MB (65%)"}"#,
        "\n",
        r#"podman-df	{"Type":"Containers","Total":1,"Active":0,"RawSize":0,"RawReclaimable":0,"TotalCount":1,"Size":"0B","Reclaimable":"0B (0%)"}"#,
        "\n",
        "podman-image\tdocker.io/library/alpine:latest\t320994c3b997\n",
    );

    #[test]
    fn reads_every_part_and_sums_what_is_taken() {
        let p = Plan::parse(
            "orphan\tfoo\t1.00 MiB\ncache\t/var/cache/pacman/pkg/a.pkg.tar.zst\t1024\naur\t/home/u/.cache/paru/clone/x\t2048\n",
        );
        assert_eq!(p.bytes(Part::Orphans), Some(1024 * 1024));
        assert_eq!(p.bytes(Part::Flatpak), None);
        assert_eq!(p.frees(|_| true), 1024 * 1024 + 1024 + 2048);
        assert_eq!(p.frees(|part| part == Part::Aur), 2048);
        assert_eq!((p.docker, p.podman), (None, None));
    }

    #[test]
    fn an_engine_frees_its_own_count_of_images_and_build_cache() {
        let p = Plan::parse(ENGINES);
        assert_eq!(
            p.docker,
            Some(Engine::Ready {
                unused: vec!["example/dev:latest".into(), "3c64b5385884".into()],
                bytes: Some(5_278_000_000 + 76_640_000),
            })
        );
        assert_eq!(
            p.podman,
            Some(Engine::Ready {
                unused: vec!["docker.io/library/alpine:latest".into()],
                bytes: Some(8_715_873),
            })
        );
        assert_eq!(p.frees(|part| part == Part::Podman), 8_715_873);
    }

    #[test]
    fn a_running_engine_not_asked_is_cleaned_up_without_a_known_size() {
        for (state, engine, ready) in [
            ("stopped", Engine::Stopped, false),
            ("denied", Engine::Denied, true),
            ("failed", Engine::Failed, false),
        ] {
            let p = Plan::parse(&format!("docker\t{state}\n"));
            assert_eq!(p.docker, Some(engine));
            assert_eq!(p.ready(Part::Docker), ready, "{state}");
            assert_eq!(p.bytes(Part::Docker), None);
            assert_eq!(p.frees(|_| true), 0);
        }
        let garbled = Plan::parse("docker\tready\ndocker-du\t{\"Images\":\n");
        assert_eq!(garbled.docker, Some(Engine::Failed));
        let odd = Plan::parse(
            "docker\tready\ndocker-df\t{\"Type\":\"Images\",\"Reclaimable\":\"1.2XB\"}\ndocker-df\t{\"Type\":\"Build Cache\",\"Reclaimable\":\"1MB\"}\n",
        );
        assert_eq!(odd.bytes(Part::Docker), None);
        assert!(odd.ready(Part::Docker));
    }

    #[test]
    fn ids_and_defaults() {
        for part in Part::ALL {
            assert_eq!(Part::from_id(part.id()), Some(part));
        }
        assert_eq!(Part::from_id("trash"), None);
        let taken: Vec<&str> = Part::ALL
            .into_iter()
            .filter(|p| p.default())
            .map(Part::id)
            .collect();
        assert_eq!(taken, ["orphans", "cache", "aur", "flatpak"]);
    }

    #[test]
    fn every_part_is_a_step_of_the_run_and_every_step_a_part() {
        let steps: Vec<&str> = crate::script::op::CLEANUP_RUN
            .meta()
            .steps
            .keys()
            .map(String::as_str)
            .collect();
        let mut ids: Vec<&str> = Part::ALL.into_iter().map(Part::id).collect();
        ids.sort();
        assert_eq!(steps, ids);
    }

    #[test]
    fn docker_sizes_are_decimal() {
        assert_eq!(decimal_size("5.278GB"), Some(5_278_000_000));
        assert_eq!(decimal_size("4.096kB"), Some(4096));
        assert_eq!(decimal_size("0B"), Some(0));
        assert_eq!(decimal_size("12"), None);
        assert_eq!(decimal_size("1.2XB"), None);
    }
}
