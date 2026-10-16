use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::math::{millis, per_second, percent, ratio};
use crate::sensors::{self, Cached, Reading};
use crate::sysfs;

const SECTOR: u64 = 512;
const BLOCK_DIR: &str = "/sys/block";

/// Cumulative counters of one block device, as `/proc/diskstats` reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Counters {
    reads: u64,
    sectors_read: u64,
    read_ms: u64,
    writes: u64,
    sectors_written: u64,
    write_ms: u64,
    io_ms: u64,
}

impl Counters {
    fn since(self, before: Self) -> Self {
        Self {
            reads: self.reads.saturating_sub(before.reads),
            sectors_read: self.sectors_read.saturating_sub(before.sectors_read),
            read_ms: self.read_ms.saturating_sub(before.read_ms),
            writes: self.writes.saturating_sub(before.writes),
            sectors_written: self.sectors_written.saturating_sub(before.sectors_written),
            write_ms: self.write_ms.saturating_sub(before.write_ms),
            io_ms: self.io_ms.saturating_sub(before.io_ms),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Nvme,
    Ssd,
    Hdd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    pub name: String,
    pub model: Option<String>,
    pub kind: Kind,
    pub size: u64,
    pub scheduler: Option<String>,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_per_second: u64,
    pub write_per_second: u64,
    /// Share of the interval with at least one request in flight.
    pub active: f32,
    /// Mean time per completed request, in milliseconds.
    pub response_ms: f32,
    pub sensors: Vec<Reading>,
}

#[derive(Debug, Default)]
pub struct Sampler {
    before: HashMap<String, Counters>,
    sensors: HashMap<String, Cached<Vec<Reading>>>,
    at: Option<Instant>,
}

impl Sampler {
    /// # Errors
    ///
    /// Fails when `/proc/diskstats` cannot be read.
    pub fn sample(&mut self) -> io::Result<Vec<Disk>> {
        let now = Instant::now();
        let elapsed = self.at.replace(now).map_or(Duration::ZERO, |at| now - at);
        let rate = |sectors: u64| per_second(sectors * SECTOR, elapsed);
        let stats = parse(&fs::read_to_string("/proc/diskstats")?);

        let mut disks: Vec<_> = stats
            .iter()
            .filter(|(name, _)| is_physical(name))
            .map(|(name, &counters)| {
                let delta = counters.since(self.before.get(name).copied().unwrap_or(counters));
                let device = Path::new(BLOCK_DIR).join(name);
                Disk {
                    name: name.clone(),
                    model: sysfs::read(device.join("device/model")),
                    kind: kind(name, &device),
                    size: sysfs::read::<u64>(device.join("size")).unwrap_or(0) * SECTOR,
                    scheduler: sysfs::read::<String>(device.join("queue/scheduler"))
                        .as_deref()
                        .and_then(active_scheduler),
                    read_bytes: counters.sectors_read * SECTOR,
                    write_bytes: counters.sectors_written * SECTOR,
                    read_per_second: rate(delta.sectors_read),
                    write_per_second: rate(delta.sectors_written),
                    active: percent(delta.io_ms, millis(elapsed)).min(100.0),
                    response_ms: ratio(delta.read_ms + delta.write_ms, delta.reads + delta.writes),
                    sensors: self
                        .sensors
                        .entry(name.clone())
                        .or_default()
                        .get(|| sensors::read_device(&device.join("device"))),
                }
            })
            .collect();
        disks.sort_by(|a, b| a.name.cmp(&b.name));
        self.sensors.retain(|name, _| stats.contains_key(name));

        self.before = stats;
        Ok(disks)
    }
}

/// Whole devices backed by hardware: they have a `device` link, unlike loop, zram, md and dm.
fn is_physical(name: &str) -> bool {
    Path::new(BLOCK_DIR).join(name).join("device").exists()
}

fn kind(name: &str, device: &Path) -> Kind {
    if name.starts_with("nvme") {
        Kind::Nvme
    } else if sysfs::read::<u8>(device.join("queue/rotational")) == Some(1) {
        Kind::Hdd
    } else {
        Kind::Ssd
    }
}

/// The scheduler file lists every choice and brackets the active one: `none [mq-deadline]`.
fn active_scheduler(choices: &str) -> Option<String> {
    let (_, rest) = choices.split_once('[')?;
    Some(rest.split_once(']')?.0.to_owned())
}

/// Columns after the device name: reads, merged, sectors read, read ms, writes, merged,
/// sectors written, write ms, in flight, io ms, and later kernels' discard and flush columns.
fn parse(text: &str) -> HashMap<String, Counters> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_ascii_whitespace().skip(2);
            let name = fields.next()?.to_owned();
            let values = fields
                .map(|field| field.parse().ok())
                .collect::<Option<Vec<u64>>>()?;
            let [
                reads,
                _,
                sectors_read,
                read_ms,
                writes,
                _,
                sectors_written,
                write_ms,
                _,
                io_ms,
                ..,
            ] = values[..]
            else {
                return None;
            };
            Some((
                name,
                Counters {
                    reads,
                    sectors_read,
                    read_ms,
                    writes,
                    sectors_written,
                    write_ms,
                    io_ms,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISKSTATS: &str = " 259  0 nvme0n1 100 0 2000 50 40 0 800 20 0 70 70 0 0 0 0 0 0\n \
                             259  1 nvme0n1p1 90 0 1900 45 40 0 800 20 0 60 65 0 0 0 0 0 0\n";

    #[test]
    fn diskstats_columns_are_picked_by_position() {
        assert_eq!(
            parse(DISKSTATS)["nvme0n1"],
            Counters {
                reads: 100,
                sectors_read: 2000,
                read_ms: 50,
                writes: 40,
                sectors_written: 800,
                write_ms: 20,
                io_ms: 70,
            }
        );
    }

    #[test]
    fn short_lines_are_dropped() {
        assert!(parse(" 7 0 loop0 1 2 3\n").is_empty());
    }

    #[test]
    fn partitions_are_parsed_but_not_physical() {
        assert!(parse(DISKSTATS).contains_key("nvme0n1p1"));
        assert!(!is_physical("nvme0n1p1"));
    }

    #[test]
    fn the_active_scheduler_is_the_bracketed_one() {
        assert_eq!(
            active_scheduler("none [mq-deadline] kyber").as_deref(),
            Some("mq-deadline")
        );
        assert_eq!(active_scheduler("none"), None);
    }

    #[test]
    fn the_first_sample_reports_no_rate() {
        let disks = Sampler::default().sample().unwrap();
        assert!(
            disks
                .iter()
                .all(|disk| disk.read_per_second == 0 && disk.write_per_second == 0)
        );
    }
}
