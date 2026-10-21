//! What pacman, makepkg and their helpers are doing, read from their lines.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Sync,
    Download,
    Verify,
    Install,
    Remove,
    Hooks,
    Build,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub phase: Option<Phase>,
    /// How many of `total` are through. pacman ends the line of a package at 100 %, but names
    /// a hook as it starts.
    pub done: u32,
    pub total: u32,
    /// The package being worked on, where the line names one.
    pub item: String,
}

impl Progress {
    pub fn feed(&mut self, line: &str) {
        let line = line.trim();
        let section = |p: Phase| Progress {
            phase: Some(p),
            ..Progress::default()
        };
        if line.starts_with(":: Synchronizing package databases") {
            *self = section(Phase::Sync);
        } else if line.starts_with(":: Retrieving packages") {
            *self = section(Phase::Download);
        } else if line.starts_with(":: Running pre-transaction hooks")
            || line.starts_with(":: Running post-transaction hooks")
        {
            *self = section(Phase::Hooks);
        } else if let Some(rest) = line.strip_prefix("==> Making package: ") {
            *self = Progress {
                item: rest.split_whitespace().next().unwrap_or_default().into(),
                ..section(Phase::Build)
            };
        } else if let Some(i) = line.find("Total (") {
            if let Some((done, total, _)) = counter(&line[i + 6..]) {
                *self = Progress {
                    done,
                    total,
                    ..section(Phase::Download)
                };
            }
        } else if let Some((done, total, rest)) = counter(line) {
            let mut words = rest.split_whitespace();
            let verb = words.next().unwrap_or_default();
            let phase = match verb {
                "checking" | "loading" => Phase::Verify,
                "installing" | "upgrading" | "reinstalling" | "downgrading" => Phase::Install,
                "removing" => Phase::Remove,
                _ if self.phase == Some(Phase::Hooks) => Phase::Hooks,
                _ => return,
            };
            let item = match phase {
                Phase::Install | Phase::Remove => words
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches("...")
                    .to_string(),
                _ => String::new(),
            };
            *self = Progress {
                phase: Some(phase),
                done: match phase {
                    Phase::Hooks => done.saturating_sub(1),
                    _ => done,
                },
                total,
                item,
            };
        }
    }

    pub fn fraction(&self) -> Option<f64> {
        (self.total > 0).then(|| f64::from(self.done.min(self.total)) / f64::from(self.total))
    }
}

/// `( 3/45) rest` as 3, 45 and the rest.
fn counter(s: &str) -> Option<(u32, u32, &str)> {
    let inner = s.strip_prefix('(')?;
    let (count, rest) = inner.split_once(')')?;
    let (done, total) = count.split_once('/')?;
    Some((
        done.trim().parse().ok()?,
        total.trim().parse().ok()?,
        rest.trim(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fed(lines: &[&str]) -> Progress {
        let mut p = Progress::default();
        lines.iter().for_each(|l| p.feed(l));
        p
    }

    #[test]
    fn follows_an_upgrade() {
        assert_eq!(
            fed(&[":: Synchronizing package databases..."]).phase,
            Some(Phase::Sync)
        );
        let download = fed(&[
            ":: Retrieving packages...",
            " Total (12/45)   100.5 MiB  12.3 MiB/s 00:08 [####--]  40%",
        ]);
        assert_eq!(
            (download.phase, download.done, download.total),
            (Some(Phase::Download), 12, 45)
        );
        let verify = fed(&["( 3/45) checking package integrity  [###---]  50%"]);
        assert_eq!(verify.phase, Some(Phase::Verify));
        let install = fed(&["(12/45) upgrading firefox                    [####] 100%"]);
        assert_eq!(
            install,
            Progress {
                phase: Some(Phase::Install),
                done: 12,
                total: 45,
                item: "firefox".into()
            }
        );
        let hooks = fed(&[
            ":: Running post-transaction hooks...",
            "(2/9) Arming ConditionNeedsUpdate...",
        ]);
        assert_eq!(
            (hooks.phase, hooks.done),
            (Some(Phase::Hooks), 1),
            "the second runs, the first is done"
        );
        assert_eq!(
            fed(&["==> Making package: paru 2.1.0-1 (Mon)"]).item,
            "paru"
        );
    }

    #[test]
    fn ignores_what_it_does_not_know() {
        let p = fed(&["(1/2) something else", "warning: foo"]);
        assert_eq!(p, Progress::default());
        assert_eq!(p.fraction(), None);
    }
}
