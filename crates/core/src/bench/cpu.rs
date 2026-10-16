use std::hint::black_box;
use std::sync::Barrier;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{Pair, check};
use crate::cpu::{Cache, CacheKind, Info};

/// How long each throughput phase measures by default.
pub(super) const PHASE: Duration = Duration::from_millis(1500);
/// How long one latency trial measures; the best of a few is kept, because noise only slows it.
const TRIAL: Duration = Duration::from_millis(150);
const TRIALS: usize = 3;
/// Independent values in flight at once: enough that the wait for each result is covered and a
/// core's arithmetic units, not their latency, set the pace.
const LANES: usize = 16;
/// Passes between looks at the clock.
const BATCH: u32 = 4_096;
const CHASE_STEPS: u64 = 8_192;
/// Memory is probed with at least this much, and with more on a processor whose caches are bigger.
const MEMORY_FLOOR: u64 = 64 << 20;
const SINGLE_BUFFER: usize = 64 << 20;
/// What every thread streams through together, so the machine's memory is stressed the same
/// whether it has four threads or sixty-four.
const TOTAL_BUFFER: usize = 512 << 20;
const MIN_BUFFER: usize = 16 << 20;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    L1,
    L2,
    L3,
    Memory,
}

/// How long one load takes when each depends on the one before, over a working set of `bytes`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Latency {
    pub level: Level,
    pub bytes: u64,
    pub nanoseconds: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Cpu {
    pub threads: usize,
    /// Billions of integer operations per second.
    pub integer: Pair<f32>,
    /// Billions of double-precision floating-point operations per second.
    pub float: Pair<f32>,
    pub latencies: Vec<Latency>,
    /// Bytes per second read from memory.
    pub bandwidth: Pair<u64>,
}

pub(super) fn run(
    stage: &dyn Fn(&'static str),
    stop: &AtomicBool,
    phase: Duration,
) -> Result<Cpu, String> {
    let threads = thread::available_parallelism().map_or(1, usize::from);
    let both = |name: &'static str, run: &dyn Fn(usize) -> Result<f64, String>| {
        stage(name);
        Ok::<_, String>(Pair {
            single: run(1)?,
            multi: run(threads)?,
        })
    };

    let integer = both("Integer math", &|threads| {
        throughput(
            threads,
            phase,
            stop,
            |seed| lanes(seed as u64),
            integer_pass,
        )
    })?;
    let float = both("Floating-point math", &|threads| {
        throughput(threads, phase, stop, |_| [1.0_f64; LANES], float_pass)
    })?;

    stage("Cache and memory latency");
    let caches = Info::read().caches;
    let latencies = working_sets(&caches)
        .into_iter()
        .map(|(level, bytes)| {
            Ok(Latency {
                level,
                bytes,
                nanoseconds: latency(bytes, stop)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    stage("Memory bandwidth");
    let bandwidth = Pair {
        single: stream(1, SINGLE_BUFFER, phase, stop)?,
        multi: stream(
            threads,
            (TOTAL_BUFFER / threads).max(MIN_BUFFER),
            phase,
            stop,
        )?,
    };

    Ok(Cpu {
        threads,
        integer: giga(integer),
        float: giga(float),
        latencies,
        bandwidth,
    })
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "rates of billions fit an f32 to the precision shown"
)]
fn giga(rates: Pair<f64>) -> Pair<f32> {
    Pair {
        single: (rates.single / 1e9) as f32,
        multi: (rates.multi / 1e9) as f32,
    }
}

/// Runs `pass` on `threads` threads at once for `phase` and returns the units of work per second
/// they finish together. Each `pass` returns how many units it did.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts of operations in a few seconds stay far below 2^52"
)]
fn throughput<S: Send>(
    threads: usize,
    phase: Duration,
    stop: &AtomicBool,
    setup: impl Fn(usize) -> S + Sync,
    pass: impl Fn(&mut S) -> u64 + Sync,
) -> Result<f64, String> {
    let start = Barrier::new(threads);
    let rate: f64 = thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|seed| {
                let (start, setup, pass) = (&start, &setup, &pass);
                scope.spawn(move || {
                    let mut state = setup(seed);
                    start.wait();
                    let (began, mut done) = (Instant::now(), 0_u64);
                    while began.elapsed() < phase {
                        done += pass(&mut state);
                    }
                    black_box(&state);
                    done as f64 / began.elapsed().as_secs_f64()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap_or(0.0))
            .sum()
    });
    check(stop)?;
    Ok(rate)
}

fn lanes(seed: u64) -> [u64; LANES] {
    [0x9e37_79b9_7f4a_7c15 ^ seed; LANES]
}

/// A rotate, an add and an xor on each of the independent values.
fn integer_pass(lanes: &mut [u64; LANES]) -> u64 {
    for _ in 0..BATCH {
        for lane in lanes.iter_mut() {
            *lane = lane.rotate_left(5) ^ lane.wrapping_add(0x9e37_79b9_7f4a_7c15);
        }
    }
    black_box(&mut *lanes);
    u64::from(BATCH) * 3 * LANES as u64
}

/// A multiply and an add on each of the independent values, which settle near a fixed point so
/// they never overflow.
fn float_pass(lanes: &mut [f64; LANES]) -> u64 {
    for _ in 0..BATCH {
        for lane in lanes.iter_mut() {
            *lane = *lane * 0.999_999 + 0.5;
        }
    }
    black_box(&mut *lanes);
    u64::from(BATCH) * 2 * LANES as u64
}

/// The bytes per second `threads` threads read from buffers of `bytes` each, none of which fits
/// in cache.
fn stream(threads: usize, bytes: usize, phase: Duration, stop: &AtomicBool) -> Result<u64, String> {
    let rate = throughput(
        threads,
        phase,
        stop,
        |_| vec![1_u64; bytes / 8],
        |buffer| {
            let mut sums = [0_u64; 4];
            for chunk in buffer.as_chunks::<4>().0 {
                for (sum, &word) in sums.iter_mut().zip(chunk) {
                    *sum = sum.wrapping_add(word);
                }
            }
            black_box(sums);
            buffer.len() as u64 * 8
        },
    )?;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive rate of bytes per second fits a u64"
    )]
    Ok(rate as u64)
}

/// The working sets to time loads over: half of each data cache level, so the data fits with
/// room to spare, and a stretch of memory too big for any of them.
fn working_sets(caches: &[Cache]) -> Vec<(Level, u64)> {
    let level = |wanted: u8| {
        caches
            .iter()
            .filter(|cache| cache.level == wanted && cache.kind != CacheKind::Instruction)
            .map(|cache| cache.size)
            .max()
    };
    let sets: Vec<_> = [(1, Level::L1), (2, Level::L2), (3, Level::L3)]
        .into_iter()
        .filter_map(|(number, name)| Some((name, level(number)? / 2)))
        .collect();
    let largest = caches.iter().map(|cache| cache.size).max().unwrap_or(0);
    [
        sets,
        vec![(Level::Memory, MEMORY_FLOOR.max(largest.saturating_mul(8)))],
    ]
    .concat()
}

/// The nanoseconds one dependent load takes over `bytes` of memory visited in random order.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts of loads in a fraction of a second stay far below 2^23"
)]
fn latency(bytes: u64, stop: &AtomicBool) -> Result<f32, String> {
    let entries = usize::try_from(bytes / 4).map_err(|error| error.to_string())?;
    let next = cycle(entries)?;
    let mut best = f32::MAX;
    for _ in 0..TRIALS {
        check(stop)?;
        let (began, mut at, mut steps) = (Instant::now(), 0_u32, 0_u64);
        while began.elapsed() < TRIAL {
            for _ in 0..CHASE_STEPS {
                at = next[at as usize];
            }
            steps += CHASE_STEPS;
        }
        black_box(at);
        best = best.min(began.elapsed().as_nanos() as f32 / steps as f32);
    }
    Ok(best)
}

/// A random order that visits every one of `entries` slots once before returning to the start,
/// so each load depends on the last and no stride can be predicted.
fn cycle(entries: usize) -> Result<Vec<u32>, String> {
    let last = u32::try_from(entries).map_err(|error| error.to_string())?;
    let mut next: Vec<u32> = (0..last).collect();
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    for i in (1..entries).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        next.swap(i, usize::try_from(state % i as u64).unwrap_or(0));
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::atomic::Ordering;

    use super::*;

    fn cache(level: u8, kind: CacheKind, size: u64) -> Cache {
        Cache {
            level,
            kind,
            size,
            instances: 1,
        }
    }

    #[test]
    fn a_pass_changes_its_values_so_it_cannot_be_skipped() {
        let mut integers = lanes(0);
        let before = integers;
        assert_eq!(
            integer_pass(&mut integers),
            u64::from(BATCH) * 3 * LANES as u64
        );
        assert_ne!(before, integers);
        let mut floats = [1.0_f64; LANES];
        assert_eq!(float_pass(&mut floats), u64::from(BATCH) * 2 * LANES as u64);
        assert!(
            floats
                .iter()
                .all(|value| value.is_finite() && (*value - 1.0).abs() > f64::EPSILON)
        );
    }

    #[test]
    fn threads_start_from_different_seeds() {
        assert_ne!(lanes(0), lanes(1));
    }

    #[test]
    fn the_chase_visits_every_slot_before_it_repeats() {
        let next = cycle(1_000).unwrap();
        let (mut at, mut seen) = (0_usize, HashSet::new());
        for _ in 0..next.len() {
            assert!(seen.insert(at));
            at = next[at] as usize;
        }
        assert_eq!((seen.len(), at), (1_000, 0));
    }

    #[test]
    fn working_sets_are_half_of_each_data_cache_and_a_stretch_of_memory() {
        let caches = [
            cache(1, CacheKind::Data, 32 << 10),
            cache(1, CacheKind::Instruction, 32 << 10),
            cache(2, CacheKind::Unified, 512 << 10),
            cache(3, CacheKind::Unified, 16 << 20),
        ];
        assert_eq!(
            working_sets(&caches),
            [
                (Level::L1, 16 << 10),
                (Level::L2, 256 << 10),
                (Level::L3, 8 << 20),
                (Level::Memory, 128 << 20),
            ]
        );
    }

    #[test]
    fn a_machine_that_reports_no_caches_is_still_timed_on_memory() {
        assert_eq!(working_sets(&[]), [(Level::Memory, MEMORY_FLOOR)]);
    }

    #[test]
    fn a_short_run_measures_everything() {
        let stop = AtomicBool::new(false);
        let cpu = run(&|_| {}, &stop, Duration::from_millis(50)).unwrap();
        assert!(cpu.integer.single > 0.0 && cpu.integer.multi > 0.0);
        assert!(cpu.float.single > 0.0 && cpu.float.multi > 0.0);
        assert!(cpu.bandwidth.single > 0 && cpu.bandwidth.multi > 0);
        assert!(
            cpu.latencies
                .iter()
                .all(|latency| latency.nanoseconds > 0.0)
        );
        assert_eq!(
            cpu.latencies.last().map(|latency| latency.level),
            Some(Level::Memory)
        );
    }

    #[test]
    fn a_stopped_run_posts_no_result() {
        let stop = AtomicBool::new(false);
        stop.store(true, Ordering::Relaxed);
        assert!(run(&|_| {}, &stop, Duration::from_millis(20)).is_err());
    }
}
