use std::collections::HashMap;
use std::path::PathBuf;
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
        let now = Instant::now();
        let elapsed = self.at.replace(now).map_or(Duration::ZERO, |at| now - at);
        let mut domains: Vec<Domain> =
            sysfs::numbered_dirs(POWERCAP_DIR, "intel-rapl:")
                .filter_map(|domain| {
                    let counter: Option<u64> = sysfs::read(domain.join("energy_uj"));
                    let before =
                        counter.and_then(|counter| self.before.insert(domain.clone(), counter));
                    let watts = counter.zip(before).filter(|_| !elapsed.is_zero()).map(
                        |(after, before)| {
                            let range =
                                sysfs::read(domain.join("max_energy_range_uj")).unwrap_or(u64::MAX);
                            ratio(
                                per_second(energy_delta(before, after, range), elapsed),
                                1_000_000,
                            )
                        },
                    );
                    Some(Domain {
                        name: sysfs::read(domain.join("name"))?,
                        watts,
                        readable: counter.is_some(),
                    })
                })
                .collect();
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
