use std::fs;
use std::io;
use std::str::FromStr;

/// One `cpu` line of `/proc/stat`, in clock ticks. Guest time is already part of user time.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Times {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl Times {
    #[must_use]
    pub fn total(self) -> u64 {
        self.busy() + self.idle + self.iowait
    }

    #[must_use]
    pub fn busy(self) -> u64 {
        self.user + self.nice + self.system + self.irq + self.softirq + self.steal
    }

    #[must_use]
    pub fn kernel(self) -> u64 {
        self.system + self.irq + self.softirq
    }

    #[must_use]
    pub fn since(self, before: Self) -> Self {
        Self {
            user: self.user.saturating_sub(before.user),
            nice: self.nice.saturating_sub(before.nice),
            system: self.system.saturating_sub(before.system),
            idle: self.idle.saturating_sub(before.idle),
            iowait: self.iowait.saturating_sub(before.iowait),
            irq: self.irq.saturating_sub(before.irq),
            softirq: self.softirq.saturating_sub(before.softirq),
            steal: self.steal.saturating_sub(before.steal),
        }
    }
}

impl FromStr for Times {
    type Err = ();

    fn from_str(columns: &str) -> Result<Self, ()> {
        let mut columns = columns.split_ascii_whitespace().map(|c| c.parse().ok());
        let mut next = || columns.next().flatten().ok_or(());
        Ok(Self {
            user: next()?,
            nice: next()?,
            system: next()?,
            idle: next()?,
            iowait: next()?,
            irq: next()?,
            softirq: next()?,
            steal: next()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub total: Times,
    /// Online logical processors only, as `(id, times)`.
    pub cores: Vec<(usize, Times)>,
}

impl Stat {
    /// # Errors
    ///
    /// Fails when `/proc/stat` cannot be read or is malformed.
    pub fn read() -> io::Result<Self> {
        Self::parse(&fs::read_to_string("/proc/stat")?)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))
    }

    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let total = lines.next()?.strip_prefix("cpu ")?.parse().ok()?;
        let cores = lines
            .map_while(|line| line.strip_prefix("cpu")?.split_once(' '))
            .map(|(id, columns)| Some((id.parse().ok()?, columns.parse().ok()?)))
            .collect::<Option<Vec<_>>>()?;
        (!cores.is_empty()).then_some(Self { total, cores })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "cpu  10 20 30 40 50 60 70 80 900 1000\n\
                        cpu0 1 2 3 4 5 6 7 8 0 0\n\
                        cpu2 8 7 6 5 4 3 2 1 0 0\n\
                        intr 5\n";

    #[test]
    fn parses_total_and_online_cores_with_their_ids() {
        let stat = Stat::parse(TEXT).unwrap();
        assert_eq!(stat.total.user, 10);
        assert_eq!(
            stat.cores.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [0, 2]
        );
    }

    #[test]
    fn guest_columns_stay_out_of_the_total() {
        assert_eq!(Stat::parse(TEXT).unwrap().total.total(), 360);
    }

    #[test]
    fn busy_excludes_idle_and_iowait() {
        let times = Stat::parse(TEXT).unwrap().total;
        assert_eq!(times.busy(), 10 + 20 + 30 + 60 + 70 + 80);
    }

    #[test]
    fn deltas_saturate_when_a_counter_resets() {
        let after = Times {
            user: 1,
            ..Times::default()
        };
        let before = Times {
            user: 5,
            ..Times::default()
        };
        assert_eq!(after.since(before).user, 0);
    }

    #[test]
    fn text_without_cores_is_rejected() {
        assert!(Stat::parse("cpu  1 2 3 4 5 6 7 8\nintr 5\n").is_none());
    }
}
