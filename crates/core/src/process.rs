mod action;
mod details;
mod sample;

use std::collections::HashMap;
use std::fs::{self, DirEntry};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::str::FromStr;
use std::time::Duration;

use rustix::param::{clock_ticks_per_second, page_size};

use crate::math::percent;
use crate::stat::Stat;

pub use action::{Signal, descendants, send, set_affinity, set_nice, spawn};
pub use details::Details;
pub use sample::{Observation, Thread};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Sleeping,
    /// Waiting on I/O and not interruptible, which is what inflates the load average.
    DiskSleep,
    Stopped,
    Zombie,
    Idle,
    Other(char),
}

impl State {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Sleeping => "Sleeping",
            Self::DiskSleep => "Waiting on disk",
            Self::Stopped => "Suspended",
            Self::Zombie => "Defunct",
            Self::Idle => "Idle",
            Self::Other(_) => "Other",
        }
    }
}

impl From<char> for State {
    fn from(state: char) -> Self {
        match state {
            'R' => Self::Running,
            'S' => Self::Sleeping,
            'D' => Self::DiskSleep,
            'T' | 't' => Self::Stopped,
            'Z' => Self::Zombie,
            'I' => Self::Idle,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
    pub state: State,
    pub uid: u32,
    pub threads: u32,
    pub nice: i32,
    /// Share of one core, so a process saturating four cores reads 400.
    pub cpu: f32,
    pub memory: u64,
    /// How long after boot the process started.
    pub started: Duration,
}

#[derive(Debug, Default)]
pub struct Sampler {
    ticks: HashMap<u32, u64>,
    total: u64,
}

/// A process as one read of its `stat` file describes it.
struct Reading {
    pid: u32,
    parent: u32,
    name: String,
    state: State,
    ticks: u64,
    nice: i32,
    threads: u32,
    started: u64,
    pages: u64,
}

impl Sampler {
    /// # Errors
    ///
    /// Fails when `/proc` or `/proc/stat` cannot be read.
    pub fn sample(&mut self) -> io::Result<Vec<Process>> {
        let stat = Stat::read()?;
        let total = stat.total.total();
        let elapsed = total.saturating_sub(self.total) / stat.cores.len() as u64;
        let page = page_size() as u64;
        let hertz = clock_ticks_per_second();
        let mut ticks = HashMap::new();

        let processes = fs::read_dir("/proc")?
            .filter_map(|entry| read(&entry.ok()?))
            .map(|(reading, uid)| {
                ticks.insert(reading.pid, reading.ticks);
                Process {
                    cpu: self.ticks.get(&reading.pid).map_or(0.0, |&before| {
                        percent(reading.ticks.saturating_sub(before), elapsed)
                    }),
                    memory: reading.pages * page,
                    started: Duration::from_secs(reading.started / hertz),
                    pid: reading.pid,
                    parent: reading.parent,
                    name: reading.name,
                    state: reading.state,
                    uid,
                    threads: reading.threads,
                    nice: reading.nice,
                }
            })
            .collect();

        self.ticks = ticks;
        self.total = total;
        Ok(processes)
    }
}

fn read(entry: &DirEntry) -> Option<(Reading, u32)> {
    let pid = entry.file_name().to_str()?.parse().ok()?;
    let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
    Some((parse_stat(pid, &stat)?, entry.metadata().ok()?.uid()))
}

/// Positions of the fields after the closing parenthesis of the name.
mod field {
    pub const STATE: usize = 0;
    pub const PARENT: usize = 1;
    pub const USER_TICKS: usize = 11;
    pub const SYSTEM_TICKS: usize = 12;
    pub const NICE: usize = 16;
    pub const THREADS: usize = 17;
    pub const START: usize = 19;
    pub const PAGES: usize = 21;
}

/// The name may itself contain parentheses, so the fields start after the last one.
fn parse_stat(pid: u32, stat: &str) -> Option<Reading> {
    let (_, rest) = stat.split_once('(')?;
    let (name, fields) = rest.rsplit_once(") ")?;
    let fields: Vec<&str> = fields.split_ascii_whitespace().collect();
    Some(Reading {
        pid,
        parent: at(&fields, field::PARENT)?,
        name: name.to_owned(),
        state: fields.get(field::STATE)?.chars().next()?.into(),
        ticks: at::<u64>(&fields, field::USER_TICKS)? + at::<u64>(&fields, field::SYSTEM_TICKS)?,
        nice: at(&fields, field::NICE)?,
        threads: at(&fields, field::THREADS)?,
        started: at(&fields, field::START)?,
        pages: at(&fields, field::PAGES)?,
    })
}

fn at<T: FromStr>(fields: &[&str], index: usize) -> Option<T> {
    fields.get(index)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "42 (a ) (b) S 7 42 42 0 -1 4194560 100 0 0 0 7 3 0 0 20 -5 4 0 1000 1000000 25 18446744073709551615 1 1 0";

    #[test]
    fn stat_name_may_contain_parentheses() {
        let reading = parse_stat(42, STAT).unwrap();
        assert_eq!(reading.name, "a ) (b");
        assert_eq!(reading.ticks, 10);
        assert_eq!(reading.pages, 25);
    }

    #[test]
    fn stat_fields_land_in_the_right_places() {
        let reading = parse_stat(42, STAT).unwrap();
        assert_eq!(
            (
                reading.parent,
                reading.state,
                reading.nice,
                reading.threads,
                reading.started
            ),
            (7, State::Sleeping, -5, 4, 1000)
        );
    }

    #[test]
    fn stat_without_enough_fields_is_rejected() {
        assert!(parse_stat(42, "42 (short) S 1").is_none());
    }

    #[test]
    fn unknown_state_letters_are_kept() {
        assert_eq!(State::from('X'), State::Other('X'));
        assert_eq!(State::from('t'), State::Stopped);
    }

    #[test]
    fn sampling_finds_this_process_with_its_owner() {
        let mut sampler = Sampler::default();
        let pid = std::process::id();
        sampler.sample().unwrap();
        let found = sampler
            .sample()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == pid)
            .unwrap();
        assert!(found.memory > 0 && found.threads >= 1);
        assert_eq!(found.parent, std::os::unix::process::parent_id());
    }
}
