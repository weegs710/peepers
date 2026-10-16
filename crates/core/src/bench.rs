mod cpu;
mod disk;
mod history;
mod internet;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use serde::{Deserialize, Serialize};

pub use cpu::{Cpu, Latency, Level};
pub use disk::{Access, Disk, Medium};
pub use history::{History, Loaded, Record};
pub use internet::{Internet, Server, servers};

/// One measurement made with a single thread or connection, and the same with many at once.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pair<T> {
    pub single: T,
    pub multi: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Cpu,
    /// Writes and reads back a file of `size` bytes in `directory`, then deletes it.
    Disk {
        directory: PathBuf,
        size: u64,
    },
    /// Measures latency, jitter, download and upload against a public `LibreSpeed` server: the
    /// one given, or the nearest.
    Internet {
        server: Option<Server>,
    },
}

/// How a graphics adapter is attached to the machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuKind {
    /// Part of the processor, sharing the system's memory.
    Integrated,
    /// A card or chip of its own with memory of its own.
    Discrete,
    #[default]
    Other,
}

/// One graphics adapter's result: single-precision compute in billions of operations per second,
/// and copy throughput in bytes per second.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    pub adapter: String,
    #[serde(default)]
    pub kind: GpuKind,
    pub gigaflops: f32,
    pub bandwidth: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    Cpu(Cpu),
    Disk(Disk),
    Gpu(Gpu),
    Internet(Internet),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// What the benchmark is doing now.
    Stage(&'static str),
    Finished(Result<Outcome, String>),
}

/// A benchmark running on a thread of its own. Dropping it stops the benchmark.
#[derive(Debug)]
pub struct Run {
    events: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
}

impl Run {
    #[must_use]
    pub fn start(job: Job) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, events) = mpsc::channel();
        let worker = Arc::clone(&stop);
        thread::spawn(move || {
            // The receiver is gone once the run was dropped; nobody is waiting for the events then.
            let stage = |stage| drop(sender.send(Event::Stage(stage)));
            let result = match job {
                Job::Cpu => cpu::run(&stage, &worker, cpu::PHASE).map(Outcome::Cpu),
                Job::Disk { directory, size } => {
                    disk::run(&directory, size, &stage, &worker).map(Outcome::Disk)
                }
                Job::Internet { server } => {
                    internet::run(server.as_ref(), &stage, &worker).map(Outcome::Internet)
                }
            };
            drop(sender.send(Event::Finished(result)));
        });
        Self { events, stop }
    }

    /// The events that have arrived since the last call.
    #[must_use]
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) const STOPPED: &str = "The benchmark was stopped";

/// The error to report when `stop` is set, so a cancelled run never posts a result.
pub(crate) fn check(stop: &AtomicBool) -> Result<(), String> {
    if stop.load(Ordering::Relaxed) {
        Err(STOPPED.to_owned())
    } else {
        Ok(())
    }
}
