use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::math::percent;
use crate::stat::{Stat, Times};
use crate::sysfs;

const CPU_DIR: &str = "/sys/devices/system/cpu";
const NODE_DIR: &str = "/sys/devices/system/node";

#[derive(Debug, Clone, PartialEq)]
pub struct Core {
    pub id: usize,
    pub usage: f32,
    pub kernel: f32,
    pub frequency_khz: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub usage: f32,
    pub kernel: f32,
    pub cores: Vec<Core>,
}

#[derive(Debug, Default)]
pub struct Sampler {
    before: Option<Stat>,
}

impl Sampler {
    /// # Errors
    ///
    /// Fails when `/proc/stat` cannot be read.
    pub fn sample(&mut self) -> io::Result<Usage> {
        let after = Stat::read()?;
        let before = self.before.take().unwrap_or_else(|| after.clone());
        let (usage, kernel) = shares(before.total, after.total);
        let cores = after
            .cores
            .iter()
            .map(|&(id, times)| {
                let earlier = before
                    .cores
                    .iter()
                    .find_map(|&(other, earlier)| (other == id).then_some(earlier))
                    .unwrap_or(times);
                let (usage, kernel) = shares(earlier, times);
                Core {
                    id,
                    usage,
                    kernel,
                    frequency_khz: sysfs::read(format!(
                        "{CPU_DIR}/cpu{id}/cpufreq/scaling_cur_freq"
                    )),
                }
            })
            .collect();
        self.before = Some(after);
        Ok(Usage {
            usage,
            kernel,
            cores,
        })
    }
}

fn shares(before: Times, after: Times) -> (f32, f32) {
    let delta = after.since(before);
    (
        percent(delta.busy(), delta.total()),
        percent(delta.kernel(), delta.total()),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheKind {
    Data,
    Instruction,
    Unified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    pub level: u8,
    pub kind: CacheKind,
    pub size: u64,
    pub instances: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub model: Option<String>,
    pub packages: usize,
    pub cores: usize,
    pub threads: usize,
    pub numa_nodes: usize,
    pub caches: Vec<Cache>,
    pub virtualization: Option<&'static str>,
    pub driver: Option<String>,
    pub governor: Option<String>,
    pub preference: Option<String>,
    pub min_khz: Option<u64>,
    pub max_khz: Option<u64>,
}

impl Info {
    #[must_use]
    pub fn read() -> Self {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let topology: Vec<(u32, u32)> = cpu_dirs()
            .filter_map(|dir| {
                Some((
                    sysfs::read(dir.join("topology/physical_package_id"))?,
                    sysfs::read(dir.join("topology/core_id"))?,
                ))
            })
            .collect();

        Self {
            model: cpuinfo_field(&cpuinfo, "model name").map(str::to_owned),
            packages: topology
                .iter()
                .map(|&(package, _)| package)
                .collect::<BTreeSet<_>>()
                .len(),
            cores: topology.iter().collect::<BTreeSet<_>>().len(),
            threads: topology.len(),
            numa_nodes: sysfs::numbered_dirs(NODE_DIR, "node").count(),
            caches: caches(),
            virtualization: virtualization(cpuinfo_field(&cpuinfo, "flags").unwrap_or_default()),
            driver: policy("scaling_driver"),
            governor: policy("scaling_governor"),
            preference: policy("energy_performance_preference"),
            min_khz: policy("cpuinfo_min_freq"),
            max_khz: policy("cpuinfo_max_freq"),
        }
    }
}

fn policy<T: FromStr>(name: &str) -> Option<T> {
    sysfs::read(format!("{CPU_DIR}/cpu0/cpufreq/{name}"))
}

fn cpu_dirs() -> impl Iterator<Item = PathBuf> {
    sysfs::numbered_dirs(CPU_DIR, "cpu")
}

fn cpuinfo_field<'a>(cpuinfo: &'a str, key: &str) -> Option<&'a str> {
    cpuinfo.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then_some(value.trim())
    })
}

fn virtualization(flags: &str) -> Option<&'static str> {
    [("svm", "AMD-V"), ("vmx", "VT-x")]
        .into_iter()
        .find(|(flag, _)| {
            flags
                .split_ascii_whitespace()
                .any(|present| present == *flag)
        })
        .map(|(_, name)| name)
}

/// Instances are counted as the distinct sets of processors sharing a cache.
fn caches() -> Vec<Cache> {
    let mut found: BTreeMap<_, BTreeSet<String>> = BTreeMap::new();
    let indexes = cpu_dirs().flat_map(|dir| sysfs::numbered_dirs(dir.join("cache"), "index"));
    for (key, shared) in indexes.filter_map(|index| cache_at(&index)) {
        found.entry(key).or_default().insert(shared);
    }
    found
        .into_iter()
        .map(|((level, kind, size), shared)| Cache {
            level,
            kind,
            size,
            instances: shared.len(),
        })
        .collect()
}

fn cache_at(index: &Path) -> Option<((u8, CacheKind, u64), String)> {
    Some((
        (
            sysfs::read(index.join("level"))?,
            parse_kind(&sysfs::read::<String>(index.join("type"))?)?,
            parse_size(&sysfs::read::<String>(index.join("size"))?)?,
        ),
        sysfs::read(index.join("shared_cpu_list"))?,
    ))
}

fn parse_kind(kind: &str) -> Option<CacheKind> {
    match kind {
        "Data" => Some(CacheKind::Data),
        "Instruction" => Some(CacheKind::Instruction),
        "Unified" => Some(CacheKind::Unified),
        _ => None,
    }
}

fn parse_size(size: &str) -> Option<u64> {
    let (digits, shift) = match size.as_bytes().last()? {
        b'K' => (size.strip_suffix('K')?, 10),
        b'M' => (size.strip_suffix('M')?, 20),
        _ => (size, 0),
    };
    Some(digits.parse::<u64>().ok()? << shift)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_sizes_carry_their_unit() {
        assert_eq!(parse_size("32K"), Some(32 << 10));
        assert_eq!(parse_size("16M"), Some(16 << 20));
        assert_eq!(parse_size("512"), Some(512));
        assert_eq!(parse_size("junk"), None);
    }

    #[test]
    fn cpuinfo_fields_match_on_the_whole_key() {
        let text = "model name\t: Some CPU\nmodel\t\t: 68\n";
        assert_eq!(cpuinfo_field(text, "model name"), Some("Some CPU"));
        assert_eq!(cpuinfo_field(text, "model"), Some("68"));
    }

    #[test]
    fn virtualization_names_the_vendor_extension() {
        assert_eq!(virtualization("fpu svm sse"), Some("AMD-V"));
        assert_eq!(virtualization("fpu vmx"), Some("VT-x"));
        assert_eq!(virtualization("fpu"), None);
    }

    #[test]
    fn this_machine_reports_a_topology() {
        let info = Info::read();
        assert!(info.threads >= info.cores && info.cores >= 1 && info.packages >= 1);
    }

    #[test]
    fn sampling_yields_a_core_per_online_processor() {
        let mut sampler = Sampler::default();
        sampler.sample().unwrap();
        let usage = sampler.sample().unwrap();
        assert_ne!(usage.cores.len(), 0);
        assert!((0.0..=100.0).contains(&usage.usage));
    }
}
