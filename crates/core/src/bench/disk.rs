use std::fs::{self, File};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use rustix::fs::{Advice, OFlags, fadvise, statvfs};
use serde::{Deserialize, Serialize};

use super::{Pair, check};
use crate::math::per_second;
use crate::volumes;

const BLOCK: usize = 1 << 20;
const SMALL_BLOCK: usize = 4096;
/// Direct I/O needs buffers that start on a page boundary.
const ALIGN: usize = 4096;
/// Distinct random blocks the write phase cycles through, made before the clock starts so that
/// producing data is never part of what is timed.
const POOL_BLOCKS: usize = 16;
const RANDOM_PHASE: Duration = Duration::from_secs(2);
const PARALLEL: usize = 16;
/// Free space left alone: a benchmark must not be what fills a disk.
const SPARE: u64 = 1 << 30;
const FILE_NAME: &str = "peepers-benchmark.tmp";

/// What the tested folder is stored on, which decides what the numbers can mean.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    Storage,
    /// A filesystem in RAM, so the speed is the memory's and not any disk's.
    Memory,
    /// Another machine's disk, so the speed includes the network between them.
    Network,
}

/// How long one 4 KiB read takes, in microseconds.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Access {
    pub median_us: f32,
    pub p99_us: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Disk {
    pub filesystem: String,
    pub source: String,
    pub medium: Medium,
    /// Bytes written to the temporary test file, which is deleted afterwards.
    pub size: u64,
    /// Whether the test went around the system's caches. When it could not, reads may have come
    /// from memory and say little about the disk.
    pub direct: bool,
    /// Bytes per second written and flushed to the disk.
    pub write: u64,
    /// Bytes per second read back in 1 MiB steps.
    pub read: u64,
    pub access: Access,
    /// Random 4 KiB reads per second: one request at a time, and `parallel` at once.
    pub iops: Pair<u64>,
    pub parallel: usize,
}

/// Removes the benchmark file however the run ends.
struct TestFile(PathBuf);

impl Drop for TestFile {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.0));
    }
}

/// A buffer whose usable part starts on a page boundary.
struct Aligned {
    bytes: Vec<u8>,
    start: usize,
    len: usize,
}

impl Aligned {
    fn new(len: usize) -> Self {
        let bytes = vec![0; len + ALIGN];
        let start = bytes.as_ptr().align_offset(ALIGN);
        Self { bytes, start, len }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[self.start..self.start + self.len]
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes[self.start..self.start + self.len]
    }
}

pub(super) fn run(
    directory: &Path,
    size: u64,
    stage: &dyn Fn(&'static str),
    stop: &AtomicBool,
) -> Result<Disk, String> {
    let size = size - size % BLOCK as u64;
    stage("Checking free space");
    let free = statvfs(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let needed = size.saturating_add(SPARE);
    if size == 0 || free.f_bavail * free.f_frsize < needed {
        return Err(format!(
            "{} needs {} MiB free: the test file, and a spare GiB left alone",
            directory.display(),
            needed >> 20
        ));
    }
    let backing = volumes::backing(directory);
    let filesystem = backing
        .as_ref()
        .map_or("unknown", |b| b.filesystem.as_str());

    let path = directory.join(FILE_NAME);
    let buffered = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let _cleanup = TestFile(path.clone());
    let mut pool = Aligned::new(POOL_BLOCKS * BLOCK);
    fill(pool.as_mut_slice());
    let direct = open_direct(&path, &pool.as_slice()[..BLOCK]);
    let file = direct.as_ref().unwrap_or(&buffered);

    stage("Writing test file");
    let write = timed(size, || write_file(file, size, &pool, stop))?;
    if direct.is_none() {
        // Pages the kernel still holds would make the read-back measure memory, not the disk.
        fadvise(&buffered, 0, None, Advice::DontNeed).map_err(|error| error.to_string())?;
    }
    stage("Reading test file");
    let read = timed(size, || read_file(file, size, stop))?;
    stage("Reading random blocks, one at a time");
    let single = random_reads(file, size, 1, stop)?;
    stage("Reading random blocks, many at once");
    let multi = random_reads(file, size, PARALLEL, stop)?;

    Ok(Disk {
        filesystem: filesystem.to_owned(),
        source: backing
            .as_ref()
            .map_or_else(String::new, |b| b.source.clone()),
        medium: medium(filesystem),
        size,
        direct: direct.is_some(),
        write,
        read,
        access: access(single.latencies),
        iops: Pair {
            single: single.per_second,
            multi: multi.per_second,
        },
        parallel: PARALLEL,
    })
}

fn medium(filesystem: &str) -> Medium {
    match filesystem {
        "tmpfs" | "ramfs" | "devtmpfs" => Medium::Memory,
        "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "9p" | "ceph" | "glusterfs" => Medium::Network,
        _ => Medium::Storage,
    }
}

/// Reopens the file around the system's caches, if the filesystem allows that for a real write.
fn open_direct(path: &Path, probe: &[u8]) -> Option<File> {
    let flag = i32::try_from(OFlags::DIRECT.bits()).ok()?;
    let file = File::options()
        .read(true)
        .write(true)
        .custom_flags(flag)
        .open(path)
        .ok()?;
    file.write_all_at(probe, 0).ok()?;
    Some(file)
}

/// Bytes per second moved by `work`.
fn timed(bytes: u64, work: impl FnOnce() -> Result<(), String>) -> Result<u64, String> {
    let began = Instant::now();
    work()?;
    Ok(per_second(bytes, began.elapsed()))
}

fn write_file(file: &File, size: u64, pool: &Aligned, stop: &AtomicBool) -> Result<(), String> {
    for (index, offset) in (0..size).step_by(BLOCK).enumerate() {
        check(stop)?;
        let block = &pool.as_slice()[index % POOL_BLOCKS * BLOCK..][..BLOCK];
        file.write_all_at(block, offset)
            .map_err(|error| error.to_string())?;
    }
    file.sync_data().map_err(|error| error.to_string())
}

fn read_file(file: &File, size: u64, stop: &AtomicBool) -> Result<(), String> {
    let mut block = Aligned::new(BLOCK);
    for offset in (0..size).step_by(BLOCK) {
        check(stop)?;
        file.read_exact_at(block.as_mut_slice(), offset)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

struct RandomReads {
    per_second: u64,
    /// Every read's duration, kept only when one read ran at a time.
    latencies: Vec<Duration>,
}

/// Reads of 4 KiB at random aligned offsets from `threads` threads for a couple of seconds.
fn random_reads(
    file: &File,
    size: u64,
    threads: usize,
    stop: &AtomicBool,
) -> Result<RandomReads, String> {
    let slots = (size / SMALL_BLOCK as u64).max(1);
    let began = Instant::now();
    let results: Vec<Result<(u64, Vec<Duration>), String>> = thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|seed| {
                scope.spawn(move || {
                    let mut block = Aligned::new(SMALL_BLOCK);
                    let mut state = 0x2545_f491_4f6c_dd1d_u64 ^ (seed as u64 + 1) << 32;
                    let (mut reads, mut latencies) = (0_u64, Vec::new());
                    while began.elapsed() < RANDOM_PHASE {
                        check(stop)?;
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        let at = Instant::now();
                        file.read_exact_at(
                            block.as_mut_slice(),
                            state % slots * SMALL_BLOCK as u64,
                        )
                        .map_err(|error| error.to_string())?;
                        if threads == 1 {
                            latencies.push(at.elapsed());
                        }
                        reads += 1;
                    }
                    Ok((reads, latencies))
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err("A read thread panicked".to_owned()))
            })
            .collect()
    });
    let (mut reads, mut latencies) = (0, Vec::new());
    for result in results {
        let (count, durations) = result?;
        reads += count;
        latencies.extend(durations);
    }
    Ok(RandomReads {
        per_second: per_second(reads, began.elapsed()),
        latencies,
    })
}

fn access(mut latencies: Vec<Duration>) -> Access {
    latencies.sort_unstable();
    Access {
        median_us: percentile(&latencies, 0.5),
        p99_us: percentile(&latencies, 0.99),
    }
}

/// The duration below which `fraction` of the sorted `durations` fall, in microseconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "the index lies within the slice and counts of reads stay far below 2^23"
)]
fn percentile(durations: &[Duration], fraction: f32) -> f32 {
    let Some(last) = durations.len().checked_sub(1) else {
        return 0.0;
    };
    let at = ((last as f32 * fraction).round() as usize).min(last);
    durations[at].as_secs_f32() * 1e6
}

/// Incompressible bytes, so a filesystem that compresses cannot report speeds that are not real.
fn fill(bytes: &mut [u8]) {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    for chunk in bytes.as_chunks_mut::<8>().0 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *chunk = state.to_le_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_bytes_do_not_repeat() {
        let mut block = vec![0_u8; 64];
        fill(&mut block);
        assert!(
            block
                .as_chunks::<8>()
                .0
                .windows(2)
                .all(|pair| pair[0] != pair[1])
        );
        assert!(block.iter().any(|&byte| byte != 0));
    }

    #[test]
    fn an_aligned_buffer_starts_on_a_page_and_is_the_size_asked_for() {
        let buffer = Aligned::new(BLOCK);
        assert_eq!(buffer.as_slice().as_ptr() as usize % ALIGN, 0);
        assert_eq!(buffer.as_slice().len(), BLOCK);
    }

    #[test]
    fn percentiles_pick_from_the_sorted_durations() {
        let micros = |values: &[u64]| -> Vec<Duration> {
            values.iter().map(|&us| Duration::from_micros(us)).collect()
        };
        let sorted = micros(&(1..=100).collect::<Vec<_>>());
        assert!((percentile(&sorted, 0.5) - 51.0).abs() < 1.0);
        assert!((percentile(&sorted, 0.99) - 99.0).abs() < 1.5);
        assert!(percentile(&[], 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn filesystems_are_told_apart_by_what_holds_the_data() {
        assert_eq!(medium("tmpfs"), Medium::Memory);
        assert_eq!(medium("nfs4"), Medium::Network);
        assert_eq!(medium("zfs"), Medium::Storage);
        assert_eq!(medium("ext4"), Medium::Storage);
    }
}
