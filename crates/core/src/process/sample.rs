use std::collections::HashMap;
use std::fmt::Write;
use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use rustix::param::clock_ticks_per_second;

use super::{State, parse_stat};
use crate::format::bytes;
use crate::math::{millis, percent};

/// One thread of the observed process.
#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    pub id: u32,
    pub name: String,
    pub state: State,
    /// Share of one core over the observation.
    pub cpu: f32,
    /// The kernel function the thread is blocked in, when it is blocked and the kernel says.
    pub waiting_in: Option<String>,
}

/// What one process was doing for a short while: where its threads spent the time, how much
/// memory it held, and what it kept open.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub pid: u32,
    pub name: String,
    pub command: Option<String>,
    pub window: Duration,
    /// Busiest first.
    pub threads: Vec<Thread>,
    pub memory: Vec<(String, u64)>,
    /// File descriptor number and what it points at.
    pub files: Vec<(u32, String)>,
}

impl Observation {
    /// Watches the process for `window`.
    ///
    /// # Errors
    ///
    /// Fails when the process is gone or its threads cannot be listed.
    pub fn capture(pid: u32, window: Duration) -> io::Result<Self> {
        let proc = format!("/proc/{pid}");
        let started = Instant::now();
        let before = thread_ticks(&proc)?;
        thread::sleep(window);
        let elapsed = started.elapsed();
        let hertz = clock_ticks_per_second();
        let available = millis(elapsed) * hertz / 1000;

        let mut threads: Vec<Thread> = thread_ticks(&proc)?
            .into_iter()
            .map(|(id, (name, state, ticks))| Thread {
                cpu: percent(
                    ticks.saturating_sub(before.get(&id).map_or(0, |&(_, _, earlier)| earlier)),
                    available,
                ),
                waiting_in: waiting_in(&proc, id),
                id,
                name,
                state,
            })
            .collect();
        threads.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.id.cmp(&b.id)));

        Ok(Self {
            pid,
            name: threads
                .iter()
                .find(|thread| thread.id == pid)
                .map(|thread| thread.name.clone())
                .unwrap_or_default(),
            command: super::Details::read(pid).command,
            window: elapsed,
            threads,
            memory: fs::read_to_string(format!("{proc}/status"))
                .map(|status| memory(&status))
                .unwrap_or_default(),
            files: files(&proc),
        })
    }

    /// The observation as plain text, to read or to attach to a bug report.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = format!(
            "Sample of {} ({})\n{}\nObserved for {:.1} seconds\n\n[Threads]\n",
            self.name,
            self.pid,
            self.command.as_deref().unwrap_or(""),
            self.window.as_secs_f32()
        );
        let _ = writeln!(
            text,
            "{:>8}  {:>7}  {:<16}  {:<16}  Waiting in",
            "Thread", "CPU", "State", "Name"
        );
        for thread in &self.threads {
            let _ = writeln!(
                text,
                "{:>8}  {:>6.1}%  {:<16}  {:<16}  {}",
                thread.id,
                thread.cpu,
                thread.state.label(),
                thread.name,
                thread.waiting_in.as_deref().unwrap_or("")
            );
        }
        text.push_str("\n[Memory]\n");
        for (name, size) in &self.memory {
            let _ = writeln!(text, "{name:<10}  {}", bytes(*size));
        }
        let _ = write!(text, "\n[Open files: {}]\n", self.files.len());
        for (descriptor, target) in &self.files {
            let _ = writeln!(text, "{descriptor:>5}  {target}");
        }
        text
    }
}

/// Every thread's name, state and cumulative processor ticks, by thread id.
fn thread_ticks(proc: &str) -> io::Result<HashMap<u32, (String, State, u64)>> {
    Ok(fs::read_dir(format!("{proc}/task"))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let id = entry.file_name().to_str()?.parse().ok()?;
            let reading = parse_stat(id, &fs::read_to_string(entry.path().join("stat")).ok()?)?;
            Some((id, (reading.name, reading.state, reading.ticks)))
        })
        .collect())
}

/// The kernel prints `0` for a thread that is not blocked.
fn waiting_in(proc: &str, thread: u32) -> Option<String> {
    let function = fs::read_to_string(format!("{proc}/task/{thread}/wchan")).ok()?;
    let function = function.trim();
    (!function.is_empty() && function != "0").then(|| function.to_owned())
}

/// The `Vm*` and `Rss*` lines of a `status` file, which state their sizes in kibibytes.
fn memory(status: &str) -> Vec<(String, u64)> {
    status
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            if !(name.starts_with("Vm") || name.starts_with("Rss")) {
                return None;
            }
            let kibibytes: u64 = value.trim().strip_suffix("kB")?.trim().parse().ok()?;
            Some((name.to_owned(), kibibytes * 1024))
        })
        .collect()
}

fn files(proc: &str) -> Vec<(u32, String)> {
    let Ok(entries) = fs::read_dir(Path::new(proc).join("fd")) else {
        return Vec::new();
    };
    let mut files: Vec<(u32, String)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let descriptor = entry.file_name().to_str()?.parse().ok()?;
            let target = fs::read_link(entry.path()).ok()?;
            Some((descriptor, target.display().to_string()))
        })
        .collect();
    files.sort_unstable();
    files
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    const STATUS: &str = "Name:\tfirefox\nVmPeak:\t  4096 kB\nVmRSS:\t   512 kB\nRssAnon:\t 256 kB\nThreads:\t4\nVmFlags:\tnone\n";

    #[test]
    fn memory_lines_become_bytes() {
        assert_eq!(
            memory(STATUS),
            [
                ("VmPeak".to_owned(), 4096 * 1024),
                ("VmRSS".to_owned(), 512 * 1024),
                ("RssAnon".to_owned(), 256 * 1024),
            ]
        );
    }

    #[test]
    fn a_missing_process_cannot_be_sampled() {
        assert!(Observation::capture(u32::MAX, Duration::ZERO).is_err());
    }

    #[test]
    fn this_process_is_described_in_text() {
        let observation =
            Observation::capture(std::process::id(), Duration::from_millis(50)).unwrap();
        assert!(
            observation
                .threads
                .iter()
                .any(|thread| thread.id == observation.pid)
        );
        assert!(observation.memory.iter().any(|(name, _)| name == "VmRSS"));
        assert!(observation.files.len() >= 3);
        let text = observation.text();
        assert!(text.starts_with(&format!(
            "Sample of {} ({})",
            observation.name, observation.pid
        )));
        assert!(text.contains("[Threads]") && text.contains("[Memory]"));
        assert!(text.contains("Thread      CPU  State"));
    }

    #[test]
    fn a_busy_thread_shows_its_share_of_a_core() {
        let stop = AtomicBool::new(false);
        let observation = thread::scope(|scope| {
            scope.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    std::hint::spin_loop();
                }
            });
            let observation = Observation::capture(std::process::id(), Duration::from_millis(400));
            stop.store(true, Ordering::Relaxed);
            observation.unwrap()
        });
        assert!(
            observation.threads[0].cpu > 50.0,
            "{:?}",
            observation.threads[0]
        );
    }
}
