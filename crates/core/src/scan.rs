use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

/// How many of the largest files and folders a report keeps.
const KEPT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileType {
    /// The extension without its dot, empty for files without one.
    pub extension: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub root: PathBuf,
    pub bytes: u64,
    pub files: u64,
    pub folders: Vec<Entry>,
    pub largest_files: Vec<Entry>,
    pub types: Vec<FileType>,
    /// `false` when the scan was stopped before it reached every folder.
    pub complete: bool,
}

#[derive(Debug, Default)]
struct Counters {
    files: AtomicU64,
    bytes: AtomicU64,
}

/// What a running scan has covered so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub files: u64,
    pub bytes: u64,
}

/// A scan running on a thread of its own. Dropping it stops the scan.
#[derive(Debug)]
pub struct Scan {
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    report: mpsc::Receiver<Report>,
}

impl Scan {
    /// Measures disk usage the way `du` does, by allocated blocks, without crossing into other
    /// filesystems or following symbolic links.
    #[must_use]
    pub fn start(root: PathBuf) -> Self {
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, report) = mpsc::channel();
        let (worker_counters, worker_stop) = (Arc::clone(&counters), Arc::clone(&stop));
        thread::spawn(move || {
            let mut walk = Walk::new(&worker_counters, &worker_stop);
            let total = walk.directory(
                &root,
                fs::symlink_metadata(&root).map_or(0, |meta| meta.dev()),
            );
            // The receiver is gone when the scan was dropped; the result is not wanted then.
            let _ = sender.send(walk.finish(root, total));
        });
        Self {
            counters,
            stop,
            report,
        }
    }

    #[must_use]
    pub fn progress(&self) -> Progress {
        Progress {
            files: self.counters.files.load(Ordering::Relaxed),
            bytes: self.counters.bytes.load(Ordering::Relaxed),
        }
    }

    /// Ends the scan early; its report then keeps what was measured so far.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// The report, once the scan has ended.
    #[must_use]
    pub fn finished(&self) -> Option<Report> {
        self.report.try_recv().ok()
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Walk<'a> {
    counters: &'a Counters,
    stop: &'a AtomicBool,
    folders: BinaryHeap<Reverse<(u64, PathBuf)>>,
    files: BinaryHeap<Reverse<(u64, PathBuf)>>,
    types: BTreeMap<String, (u64, u64)>,
}

impl<'a> Walk<'a> {
    fn new(counters: &'a Counters, stop: &'a AtomicBool) -> Self {
        Self {
            counters,
            stop,
            folders: BinaryHeap::new(),
            files: BinaryHeap::new(),
            types: BTreeMap::new(),
        }
    }

    /// The bytes under `path`, which is recorded as a folder once all of its entries are counted.
    fn directory(&mut self, path: &Path, device: u64) -> u64 {
        let mut total = 0;
        for entry in fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            if self.stop.load(Ordering::Relaxed) {
                break;
            }
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            total += if meta.is_dir() {
                if meta.dev() == device {
                    self.directory(&entry.path(), device)
                } else {
                    0
                }
            } else {
                self.file(&entry.path(), &meta)
            };
        }
        keep(&mut self.folders, total, path);
        total
    }

    fn file(&mut self, path: &Path, meta: &fs::Metadata) -> u64 {
        let bytes = meta.blocks() * 512;
        let extension = path
            .extension()
            .map_or_else(String::new, |ext| ext.to_string_lossy().to_lowercase());
        let slot = self.types.entry(extension).or_default();
        slot.0 += 1;
        slot.1 += bytes;
        keep(&mut self.files, bytes, path);
        self.counters.files.fetch_add(1, Ordering::Relaxed);
        self.counters.bytes.fetch_add(bytes, Ordering::Relaxed);
        bytes
    }

    fn finish(self, root: PathBuf, bytes: u64) -> Report {
        let largest = |heap: BinaryHeap<Reverse<(u64, PathBuf)>>| {
            heap.into_sorted_vec()
                .into_iter()
                .map(|Reverse((bytes, path))| Entry { path, bytes })
                .collect()
        };
        let mut types: Vec<FileType> = self
            .types
            .into_iter()
            .map(|(extension, (files, bytes))| FileType {
                extension,
                files,
                bytes,
            })
            .collect();
        types.sort_by_key(|kind| Reverse(kind.bytes));
        Report {
            root,
            bytes,
            files: self.counters.files.load(Ordering::Relaxed),
            folders: largest(self.folders),
            largest_files: largest(self.files),
            types,
            complete: !self.stop.load(Ordering::Relaxed),
        }
    }
}

/// Keeps the [`KEPT`] largest entries: the heap's smallest is evicted when a larger one arrives.
fn keep(heap: &mut BinaryHeap<Reverse<(u64, PathBuf)>>, bytes: u64, path: &Path) {
    if heap.len() < KEPT {
        heap.push(Reverse((bytes, path.to_path_buf())));
    } else if heap
        .peek()
        .is_some_and(|Reverse((smallest, _))| bytes > *smallest)
    {
        heap.pop();
        heap.push(Reverse((bytes, path.to_path_buf())));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn scan(root: &str) -> Report {
        let scan = Scan::start(PathBuf::from(root));
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(report) = scan.finished() {
                return report;
            }
            assert!(Instant::now() < deadline, "scan did not finish");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_scan_of_the_crate_finds_its_sources() {
        let report = scan(env!("CARGO_MANIFEST_DIR"));
        assert!(report.complete && report.bytes > 0 && report.files > 0);
        assert!(
            report
                .largest_files
                .iter()
                .any(|entry| entry.path.ends_with("src/scan.rs"))
        );
        assert!(
            report
                .types
                .iter()
                .any(|kind| kind.extension == "rs" && kind.files > 5)
        );
        assert_eq!(
            report.folders.first().map(|entry| entry.bytes),
            Some(report.bytes)
        );
    }

    #[test]
    fn largest_entries_come_first_in_descending_order() {
        let report = scan(env!("CARGO_MANIFEST_DIR"));
        assert!(
            report
                .largest_files
                .windows(2)
                .all(|pair| pair[0].bytes >= pair[1].bytes)
        );
        assert!(
            report
                .folders
                .windows(2)
                .all(|pair| pair[0].bytes >= pair[1].bytes)
        );
    }

    #[test]
    fn a_stopped_scan_reports_what_it_had_and_says_so() {
        let scan = Scan::start(PathBuf::from("/nix/store"));
        scan.stop();
        let deadline = Instant::now() + Duration::from_secs(20);
        let report = loop {
            if let Some(report) = scan.finished() {
                break report;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        };
        assert!(!report.complete);
    }

    #[test]
    fn keeping_evicts_the_smallest_once_full() {
        let mut heap = BinaryHeap::new();
        for bytes in 0..=u64::try_from(KEPT).unwrap() {
            keep(&mut heap, bytes, Path::new("x"));
        }
        assert_eq!(heap.len(), KEPT);
        assert_eq!(heap.peek().map(|Reverse((smallest, _))| *smallest), Some(1));
    }
}
