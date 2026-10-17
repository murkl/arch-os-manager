//! What the machine uses right now, from the kernel's counters.

/// The share in use, from 0 to 1, of the processors, the memory and the root file system, each
/// unknown where it was not read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub cpu: Option<f64>,
    pub memory: Option<f64>,
    pub disk: Option<f64>,
}

/// The counters of `/proc/stat`: busy and all ticks since boot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ticks {
    busy: u64,
    all: u64,
}

impl Ticks {
    pub fn parse(stat: &str) -> Option<Ticks> {
        let fields: Vec<u64> = stat
            .lines()
            .next()?
            .strip_prefix("cpu ")?
            .split_whitespace()
            .map(|f| f.parse().ok())
            .collect::<Option<_>>()?;
        // user to steal. guest and guest_nice are already in user and nice.
        let all: u64 = fields.iter().take(8).sum();
        let idle = fields.get(3)? + fields.get(4).unwrap_or(&0);
        Some(Ticks {
            busy: all.saturating_sub(idle),
            all,
        })
    }

    /// The share of the processors busy between `before` and these counters.
    pub fn since(self, before: Ticks) -> Option<f64> {
        let all = self.all.checked_sub(before.all).filter(|&d| d > 0)?;
        let busy = self.busy.checked_sub(before.busy)?.min(all);
        Some(busy as f64 / all as f64)
    }
}

/// `/proc/meminfo`, in use as what `MemAvailable` leaves, as free counts it.
pub fn memory(meminfo: &str) -> Option<f64> {
    let kib = |key: &str| {
        meminfo.lines().find_map(|l| {
            l.strip_prefix(key)?
                .strip_prefix(':')?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
    };
    let (total, available) = (kib("MemTotal")?, kib("MemAvailable")?);
    (total > 0).then(|| total.saturating_sub(available) as f64 / total as f64)
}

/// A file system's blocks as df counts them: what is reserved for root is not available.
pub fn disk(blocks: u64, free: u64, available: u64) -> Option<f64> {
    let used = blocks.saturating_sub(free);
    let usable = used + available;
    (usable > 0).then(|| (used as f64 / usable as f64).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  56165 23 17604 11483977 4742 2517 1238 0 0 0\n\
        cpu0 6203 2 1952 713710 861 243 223 0 0 0\n\
        intr 1234 0 0\nctxt 99\n";

    #[test]
    fn counts_the_share_of_busy_ticks_between_two_readings() {
        let before = Ticks::parse(STAT).unwrap();
        let later = STAT.replacen(
            "cpu  56165 23 17604 11483977 4742",
            "cpu  56215 23 17654 11484077 4742",
            1,
        );
        assert_eq!(Ticks::parse(&later).unwrap().since(before), Some(0.5));
        assert_eq!(before.since(before), None, "no time passed");
        assert_eq!(Ticks::parse("intr 1\n"), None);
    }

    #[test]
    fn memory_in_use_is_what_is_not_available() {
        let share = memory(
            "MemTotal:       32772784 kB\nMemFree:        18411232 kB\n\
             MemAvailable:   28394852 kB\n",
        );
        assert_eq!(share, Some((32772784.0 - 28394852.0) / 32772784.0));
        assert_eq!(memory("MemTotal:       32772784 kB\n"), None);
    }

    #[test]
    fn a_disk_is_as_full_as_df_says() {
        let share = disk(488116241, 138199235, 136810394).unwrap();
        assert_eq!((share * 100.0).round(), 72.0);
        assert_eq!(disk(0, 0, 0), None);
    }
}
