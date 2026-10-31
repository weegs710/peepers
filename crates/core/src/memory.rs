use std::collections::HashMap;
use std::fs;
use std::io;

use crate::math::percent;

/// All sizes are in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    pub free: u64,
    /// The kernel's estimate plus what the ZFS cache can evict.
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
    pub zfs_cache: Option<ZfsCache>,
}

/// The ZFS cache, which the kernel's estimate of available memory leaves out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZfsCache {
    pub size: u64,
    pub evictable: u64,
}

impl Memory {
    /// # Errors
    ///
    /// Fails when `/proc/meminfo` or `/proc/vmstat` cannot be read or is malformed.
    pub fn read() -> io::Result<Self> {
        Self::parse(
            &fs::read_to_string("/proc/meminfo")?,
            &fs::read_to_string("/proc/vmstat")?,
            fs::read_to_string("/proc/spl/kstat/zfs/arcstats")
                .ok()
                .as_deref(),
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

    fn parse(meminfo: &str, vmstat: &str, arcstats: Option<&str>) -> Option<Self> {
        let kilobytes = counters(meminfo, ':', 0);
        let bytes = |key: &str| kilobytes.get(key).map(|kb| kb << 10);
        let pages = counters(vmstat, ' ', 0);
        let zfs_cache = arcstats.and_then(ZfsCache::parse);
        Some(Self {
            total: bytes("MemTotal")?,
            free: bytes("MemFree")?,
            available: bytes("MemAvailable")? + zfs_cache.map_or(0, |cache| cache.evictable),
            cached: bytes("Cached")?,
            shared: bytes("Shmem")?,
            slab: bytes("Slab")?,
            swap_total: bytes("SwapTotal")?,
            swap_free: bytes("SwapFree")?,
            committed: bytes("Committed_AS")?,
            commit_limit: bytes("CommitLimit")?,
            pages_in: *pages.get("pgpgin")?,
            pages_out: *pages.get("pgpgout")?,
            zfs_cache,
        })
    }
}

impl ZfsCache {
    fn parse(arcstats: &str) -> Option<Self> {
        let stats = counters(arcstats, ' ', 1);
        let stat = |name: &str| stats.get(name).copied();
        let size = stat("size")?;
        let clean = [
            "mru_evictable_data",
            "mru_evictable_metadata",
            "mfu_evictable_data",
            "mfu_evictable_metadata",
        ]
        .into_iter()
        .map(stat)
        .sum::<Option<u64>>()?;
        // ZFS offers the kernel no more than this, so that the cache keeps its floor.
        // See: https://github.com/openzfs/zfs/blob/zfs-2.4.4/module/os/linux/zfs/arc_os.c#L149
        Some(Self {
            size,
            evictable: clean.min(size.saturating_sub(stat("c_min")?)),
        })
    }
}

/// The number in the given column after each line's key.
fn counters(text: &str, separator: char, column: usize) -> HashMap<&str, u64> {
    text.lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(separator)?;
            Some((
                key,
                rest.split_ascii_whitespace().nth(column)?.parse().ok()?,
            ))
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

    fn arcstats(floor: u64) -> String {
        format!(
            "22 1 0x01 148 40256 1567514895 82111878837244\n\
             name                            type data\n\
             c_min                           4    {floor}\n\
             size                            4    10000\n\
             mru_evictable_data              4    3000\n\
             mru_evictable_metadata          4    500\n\
             mru_ghost_evictable_data        4    9000\n\
             mfu_evictable_data              4    2000\n\
             mfu_evictable_metadata          4    500\n"
        )
    }

    #[test]
    fn meminfo_kilobytes_become_bytes() {
        let memory = Memory::parse(MEMINFO, VMSTAT, None).unwrap();
        assert_eq!(memory.total, 16_000 << 10);
        assert_eq!(memory.used(), 7_000 << 10);
        assert_eq!(memory.swap_used(), 500 << 10);
        assert_eq!(memory.zfs_cache, None);
    }

    #[test]
    fn vmstat_page_counters_are_read_verbatim() {
        let memory = Memory::parse(MEMINFO, VMSTAT, None).unwrap();
        assert_eq!((memory.pages_in, memory.pages_out), (42, 17));
    }

    #[test]
    fn a_missing_field_rejects_the_whole_reading() {
        assert!(Memory::parse("MemTotal: 1 kB\n", VMSTAT, None).is_none());
    }

    #[test]
    fn the_zfs_cache_can_evict_what_its_resident_lists_hold() {
        let cache = ZfsCache::parse(&arcstats(2_000)).unwrap();
        assert_eq!((cache.size, cache.evictable), (10_000, 6_000));
    }

    #[test]
    fn the_zfs_cache_keeps_its_floor() {
        assert_eq!(ZfsCache::parse(&arcstats(7_000)).unwrap().evictable, 3_000);
        assert_eq!(ZfsCache::parse(&arcstats(12_000)).unwrap().evictable, 0);
    }

    #[test]
    fn a_missing_counter_gives_no_zfs_cache() {
        let partial = arcstats(2_000).replace("mfu_evictable_metadata", "mfu_metadata");
        assert_eq!(ZfsCache::parse(&partial), None);
        assert_eq!(
            Memory::parse(MEMINFO, VMSTAT, Some(&partial))
                .unwrap()
                .used(),
            7_000 << 10
        );
    }

    #[test]
    fn what_the_zfs_cache_can_evict_is_available_and_not_in_use() {
        let memory = Memory::parse(MEMINFO, VMSTAT, Some(&arcstats(2_000))).unwrap();
        assert_eq!(memory.available, (9_000 << 10) + 6_000);
        assert_eq!(memory.used(), (7_000 << 10) - 6_000);
        assert_eq!(memory.zfs_cache.map(|cache| cache.size), Some(10_000));
    }

    #[test]
    fn this_machine_has_memory() {
        assert!(Memory::read().unwrap().total > 0);
    }
}
