use std::fs;
use std::path::PathBuf;

use rustix::process::Pid;
use rustix::thread::sched_getaffinity;

/// What a process exposes beyond its `stat` line. Every field is `None` when the kernel denies
/// access, which for other users' processes is the normal case without privilege.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Details {
    pub command: Option<String>,
    pub executable: Option<PathBuf>,
    pub directory: Option<PathBuf>,
    pub descriptors: Option<usize>,
    pub read_bytes: Option<u64>,
    pub write_bytes: Option<u64>,
    /// The logical processors the scheduler may use for the process.
    pub affinity: Option<Vec<usize>>,
}

impl Details {
    #[must_use]
    pub fn read(pid: u32) -> Self {
        let proc = PathBuf::from(format!("/proc/{pid}"));
        let io = fs::read_to_string(proc.join("io")).ok();
        let counter = |name: &str| io.as_deref().and_then(|io| super::counter(io, name));
        Self {
            command: fs::read(proc.join("cmdline"))
                .ok()
                .and_then(|bytes| command(&bytes)),
            executable: fs::read_link(proc.join("exe")).ok(),
            directory: fs::read_link(proc.join("cwd")).ok(),
            descriptors: fs::read_dir(proc.join("fd")).ok().map(Iterator::count),
            read_bytes: counter("read_bytes:"),
            write_bytes: counter("write_bytes:"),
            affinity: affinity(pid),
        }
    }
}

/// Arguments are NUL-separated; kernel threads have none.
fn command(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let joined = text.trim_end_matches('\0').replace('\0', " ");
    (!joined.is_empty()).then_some(joined)
}

fn affinity(pid: u32) -> Option<Vec<usize>> {
    let set = sched_getaffinity(Pid::from_raw(i32::try_from(pid).ok()?)).ok()?;
    Some(
        (0..rustix::thread::CpuSet::MAX_CPU)
            .filter(|&cpu| set.is_set(cpu))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_joined_with_spaces() {
        assert_eq!(command(b"sleep\x0090\x00").as_deref(), Some("sleep 90"));
        assert_eq!(command(b""), None);
    }

    #[test]
    fn this_process_describes_itself() {
        let details = Details::read(std::process::id());
        assert!(details.command.is_some_and(|command| !command.is_empty()));
        assert!(details.executable.is_some() && details.directory.is_some());
        assert!(details.descriptors.is_some_and(|count| count >= 3));
        assert!(details.affinity.is_some_and(|cpus| !cpus.is_empty()));
    }

    #[test]
    fn a_missing_process_has_no_details() {
        assert_eq!(Details::read(u32::MAX), Details::default());
    }
}
