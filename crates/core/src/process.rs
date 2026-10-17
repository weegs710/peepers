mod action;
mod details;
mod sample;

use std::collections::HashMap;
use std::fs::{self, DirEntry};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::str::FromStr;
use std::time::{Duration, Instant};

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
    /// Bytes a second moved to or from storage. Unknown for the first sample and for processes
    /// whose `io` file only its owner or root may read.
    pub io: Option<u64>,
    /// How long after boot the process started.
    pub started: Duration,
}

#[derive(Debug, Default)]
pub struct Sampler {
    ticks: HashMap<u32, u64>,
    moved: HashMap<u32, u64>,
    total: u64,
    at: Option<Instant>,
}

/// A process as one read of its `stat` and `io` files describes it.
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
    /// Bytes moved to or from storage since the process began.
    moved: Option<u64>,
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
        let now = Instant::now();
        let millis = self.at.map(|at| {
            u64::try_from(now.duration_since(at).as_millis())
                .unwrap_or(u64::MAX)
                .max(1)
        });
        let (mut ticks, mut moved) = (HashMap::new(), HashMap::new());

        let processes = fs::read_dir("/proc")?
            .filter_map(|entry| read(&entry.ok()?))
            .map(|(reading, uid)| {
                ticks.insert(reading.pid, reading.ticks);
                if let Some(bytes) = reading.moved {
                    moved.insert(reading.pid, bytes);
                }
                Process {
                    cpu: self.ticks.get(&reading.pid).map_or(0.0, |&before| {
                        percent(reading.ticks.saturating_sub(before), elapsed)
                    }),
                    io: match (millis, reading.moved, self.moved.get(&reading.pid)) {
                        (Some(millis), Some(now), Some(&before)) => {
                            Some(now.saturating_sub(before).saturating_mul(1000) / millis)
                        }
                        _ => None,
                    },
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
        self.moved = moved;
        self.total = total;
        self.at = Some(now);
        Ok(processes)
    }
}

fn read(entry: &DirEntry) -> Option<(Reading, u32)> {
    let pid = entry.file_name().to_str()?.parse().ok()?;
    let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
    let mut reading = parse_stat(pid, &stat)?;
    reading.moved = fs::read_to_string(entry.path().join("io"))
        .ok()
        .and_then(|io| parse_io(&io));
    Some((reading, entry.metadata().ok()?.uid()))
}

/// One counter, such as `read_bytes:`, from the contents of an `io` file.
fn counter(io: &str, name: &str) -> Option<u64> {
    io.lines()
        .find_map(|line| line.strip_prefix(name)?.trim().parse().ok())
}

/// Bytes read from and written to storage, from the contents of an `io` file.
fn parse_io(io: &str) -> Option<u64> {
    Some(counter(io, "read_bytes:")? + counter(io, "write_bytes:")?)
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
        moved: None,
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
    fn io_counts_what_reached_storage_and_ignores_the_rest() {
        let io = "rchar: 900\nwchar: 800\nsyscr: 1\nsyscw: 2\nread_bytes: 4096\nwrite_bytes: 512\ncancelled_write_bytes: 0\n";
        assert_eq!(parse_io(io), Some(4608));
        assert_eq!(parse_io("rchar: 900\n"), None);
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
        assert!(found.io.is_some());
        assert_eq!(found.parent, std::os::unix::process::parent_id());
    }
}
