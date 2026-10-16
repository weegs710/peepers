use std::collections::HashMap;
use std::fs;
use std::io;

use crate::math::percent;

/// All sizes are in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub cached: u64,
    pub shared: u64,
    pub slab: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub committed: u64,
    pub commit_limit: u64,
    pub pages_in: u64,
    pub pages_out: u64,
}

impl Memory {
    /// # Errors
    ///
    /// Fails when `/proc/meminfo` or `/proc/vmstat` cannot be read or is malformed.
    pub fn read() -> io::Result<Self> {
        Self::parse(
            &fs::read_to_string("/proc/meminfo")?,
            &fs::read_to_string("/proc/vmstat")?,
        )
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))
    }

    #[must_use]
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    #[must_use]
    pub fn used_percent(&self) -> f32 {
        percent(self.used(), self.total)
    }

    #[must_use]
    pub fn swap_used(&self) -> u64 {
        self.swap_total.saturating_sub(self.swap_free)
    }

    fn parse(meminfo: &str, vmstat: &str) -> Option<Self> {
        let kilobytes = counters(meminfo, ':');
        let bytes = |key: &str| kilobytes.get(key).map(|kb| kb << 10);
        let pages = counters(vmstat, ' ');
        Some(Self {
            total: bytes("MemTotal")?,
            free: bytes("MemFree")?,
            available: bytes("MemAvailable")?,
            cached: bytes("Cached")?,
            shared: bytes("Shmem")?,
            slab: bytes("Slab")?,
            swap_total: bytes("SwapTotal")?,
            swap_free: bytes("SwapFree")?,
            committed: bytes("Committed_AS")?,
            commit_limit: bytes("CommitLimit")?,
            pages_in: *pages.get("pgpgin")?,
            pages_out: *pages.get("pgpgout")?,
        })
    }
}

fn counters(text: &str, separator: char) -> HashMap<&str, u64> {
    text.lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(separator)?;
            Some((key, rest.split_ascii_whitespace().next()?.parse().ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &str = "MemTotal:       16000 kB\nMemFree:         4000 kB\nMemAvailable:    9000 kB\n\
                           Cached:          3000 kB\nShmem:            500 kB\nSlab:             700 kB\n\
                           SwapTotal:       2000 kB\nSwapFree:        1500 kB\nCommitted_AS:    8000 kB\n\
                           CommitLimit:    10000 kB\n";
    const VMSTAT: &str = "nr_free_pages 1000\npgpgin 42\npgpgout 17\n";

    #[test]
    fn meminfo_kilobytes_become_bytes() {
        let memory = Memory::parse(MEMINFO, VMSTAT).unwrap();
        assert_eq!(memory.total, 16_000 << 10);
        assert_eq!(memory.used(), 7_000 << 10);
        assert_eq!(memory.swap_used(), 500 << 10);
    }

    #[test]
    fn vmstat_page_counters_are_read_verbatim() {
        let memory = Memory::parse(MEMINFO, VMSTAT).unwrap();
        assert_eq!((memory.pages_in, memory.pages_out), (42, 17));
    }

    #[test]
    fn a_missing_field_rejects_the_whole_reading() {
        assert!(Memory::parse("MemTotal: 1 kB\n", VMSTAT).is_none());
    }

    #[test]
    fn this_machine_has_memory() {
        assert!(Memory::read().unwrap().total > 0);
    }
}
