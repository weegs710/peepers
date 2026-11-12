use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::math::{per_second, ratio};
use crate::sysfs;

const POWERCAP_DIR: &str = "/sys/class/powercap";

/// A power-capping domain such as the CPU package, from the kernel's RAPL interface.
#[derive(Debug, Clone, PartialEq)]
pub struct Domain {
    pub name: String,
    /// Average draw over the last interval. `None` until a second reading exists, and when
    /// reading the counter is not allowed, which without privilege is the usual case.
    pub watts: Option<f32>,
    /// Energy since the preceding successful, consecutive reading, in joules.
    pub joules: Option<f64>,
    pub readable: bool,
}

#[derive(Debug, Default)]
pub struct Sampler {
    before: HashMap<PathBuf, u64>,
    at: Option<Instant>,
}

impl Sampler {
    /// The top-level domains, each with its draw since the previous call.
    pub fn sample(&mut self) -> Vec<Domain> {
        self.sample_in(Path::new(POWERCAP_DIR), Instant::now())
    }

    fn sample_in(&mut self, root: &Path, now: Instant) -> Vec<Domain> {
        let elapsed = self.at.replace(now).map_or(Duration::ZERO, |at| now - at);
        let mut next = HashMap::new();
        let mut domains: Vec<Domain> = sysfs::numbered_dirs(root, "intel-rapl:")
            .filter_map(|domain| {
                let counter: Option<u64> = sysfs::read(domain.join("energy_uj"));
                let before = self.before.get(&domain).copied();
                if let Some(counter) = counter {
                    next.insert(domain.clone(), counter);
                }
                let joules = counter.zip(before).map(|(after, before)| {
                    let range = sysfs::read(domain.join("max_energy_range_uj")).unwrap_or(u64::MAX);
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "microjoule counter precision exceeds reporting precision"
                    )]
                    {
                        energy_delta(before, after, range) as f64 / 1_000_000.0
                    }
                });
                let watts =
                    counter
                        .zip(before)
                        .filter(|_| !elapsed.is_zero())
                        .map(|(after, before)| {
                            let range =
                                sysfs::read(domain.join("max_energy_range_uj")).unwrap_or(u64::MAX);
                            ratio(
                                per_second(energy_delta(before, after, range), elapsed),
                                1_000_000,
                            )
                        });
                Some(Domain {
                    name: sysfs::read(domain.join("name"))?,
                    watts,
                    joules,
                    readable: counter.is_some(),
                })
            })
            .collect();
        self.before = next;
        domains.sort_by(|a, b| a.name.cmp(&b.name));
        domains
    }
}

/// The energy used between two counter readings, allowing for the counter wrapping at `range`.
fn energy_delta(before: u64, after: u64, range: u64) -> u64 {
    if after >= before {
        after - before
    } else {
        range.saturating_sub(before).saturating_add(after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_are_plain_differences() {
        assert_eq!(energy_delta(100, 350, 1000), 250);
    }

    #[test]
    fn a_wrapped_counter_continues_from_its_range() {
        assert_eq!(energy_delta(900, 50, 1000), 150);
    }

    #[test]
    fn missing_counters_require_a_fresh_baseline_and_wrapped_energy_is_preserved() {
        use std::fs;
        let root = std::env::temp_dir().join(format!("peepers-rapl-test-{}", std::process::id()));
        let package = root.join("intel-rapl:0");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("name"), "package-0").unwrap();
        fs::write(package.join("max_energy_range_uj"), "10000000").unwrap();
        fs::write(package.join("energy_uj"), "9000000").unwrap();
        let mut sampler = Sampler::default();
        let now = Instant::now();
        assert_eq!(sampler.sample_in(&root, now)[0].joules, None);
        fs::write(package.join("energy_uj"), "1000000").unwrap();
        assert_eq!(
            sampler.sample_in(&root, now + Duration::from_secs(1))[0].joules,
            Some(2.0)
        );
        fs::remove_file(package.join("energy_uj")).unwrap();
        assert!(!sampler.sample_in(&root, now + Duration::from_secs(2))[0].readable);
        fs::write(package.join("energy_uj"), "7000000").unwrap();
        assert_eq!(
            sampler.sample_in(&root, now + Duration::from_secs(3))[0].joules,
            None
        );
        fs::write(package.join("energy_uj"), "8000000").unwrap();
        let sample = sampler.sample_in(&root, now + Duration::from_secs(4));
        assert_eq!(sample[0].joules, Some(1.0));
        assert_eq!(sample[0].watts, Some(1.0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_first_sample_has_no_rate() {
        assert!(
            Sampler::default()
                .sample()
                .iter()
                .all(|domain| domain.watts.is_none())
        );
    }

    #[test]
    fn unreadable_domains_are_listed_as_such() {
        let domains = Sampler::default().sample();
        assert!(
            domains
                .iter()
                .all(|domain| domain.readable || domain.watts.is_none())
        );
    }
}
