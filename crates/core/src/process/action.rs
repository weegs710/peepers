use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use rustix::process::{Pid, kill_process, setpriority_process};
use rustix::thread::{CpuSet, sched_setaffinity};

use super::Process;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Terminate,
    Kill,
    Stop,
    Continue,
    Interrupt,
    Hangup,
}

impl From<Signal> for rustix::process::Signal {
    fn from(signal: Signal) -> Self {
        match signal {
            Signal::Terminate => Self::TERM,
            Signal::Kill => Self::KILL,
            Signal::Stop => Self::STOP,
            Signal::Continue => Self::CONT,
            Signal::Interrupt => Self::INT,
            Signal::Hangup => Self::HUP,
        }
    }
}

/// Long enough for a command the shell cannot find or run to have failed.
const SETTLE: Duration = Duration::from_millis(250);

/// Starts `command` through the shell, out of this process's group and without its terminal.
///
/// # Errors
///
/// Fails when the shell cannot start, or when the command has already ended unsuccessfully.
pub fn spawn(command: &str) -> io::Result<u32> {
    let mut child = Command::new("sh")
        .args(["-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let pid = child.id();
    thread::sleep(SETTLE);
    match child.try_wait()? {
        Some(status) if !status.success() => {
            return Err(io::Error::other(format!(
                "The command ended at once with {status}"
            )));
        }
        Some(_) => {}
        None => {
            thread::spawn(move || child.wait());
        }
    }
    Ok(pid)
}

/// # Errors
///
/// Fails when the process is gone or belongs to another user.
pub fn send(pid: u32, signal: Signal) -> io::Result<()> {
    Ok(kill_process(process_id(pid)?, signal.into())?)
}

/// # Errors
///
/// Fails when the process is gone, or when raising its priority without privilege.
pub fn set_nice(pid: u32, nice: i32) -> io::Result<()> {
    Ok(setpriority_process(Some(process_id(pid)?), nice)?)
}

/// Pins every thread of the process to the given logical processors.
///
/// # Errors
///
/// Fails when the set is empty or names a processor that does not exist, when the process is
/// gone, or when it belongs to another user. Threads that exit mid-way are skipped.
pub fn set_affinity(pid: u32, cpus: &[usize]) -> io::Result<()> {
    if cpus.is_empty() || cpus.iter().any(|&cpu| cpu >= CpuSet::MAX_CPU) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut set = CpuSet::new();
    for &cpu in cpus {
        set.set(cpu);
    }
    for entry in fs::read_dir(format!("/proc/{pid}/task"))?.filter_map(Result::ok) {
        if let Some(thread) = entry.file_name().to_str().and_then(|id| id.parse().ok()) {
            match sched_setaffinity(Some(process_id(thread)?), &set) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

/// Every transitive child of `root`, children before their parents so a signal reaches the leaves first.
#[must_use]
pub fn descendants(processes: &[Process], root: u32) -> Vec<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for process in processes {
        children
            .entry(process.parent)
            .or_default()
            .push(process.pid);
    }
    let mut order = Vec::new();
    let mut seen = HashSet::from([root]);
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        for &child in children.get(&pid).into_iter().flatten() {
            if seen.insert(child) {
                order.push(child);
                stack.push(child);
            }
        }
    }
    order.reverse();
    order
}

fn process_id(pid: u32) -> io::Result<Pid> {
    i32::try_from(pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::ErrorKind::InvalidInput.into())
}

#[cfg(test)]
mod tests {
    use super::super::State;
    use super::*;

    fn process(pid: u32, parent: u32) -> Process {
        Process {
            pid,
            parent,
            name: String::new(),
            state: State::Sleeping,
            uid: 0,
            threads: 1,
            nice: 0,
            cpu: 0.0,
            memory: 0,
            started: Duration::ZERO,
        }
    }

    #[test]
    fn descendants_list_leaves_before_their_parents() {
        let processes = [
            process(2, 1),
            process(3, 2),
            process(4, 2),
            process(5, 1),
            process(9, 8),
        ];
        let order = descendants(&processes, 1);
        assert_eq!(order.len(), 4);
        let position = |pid| order.iter().position(|&other| other == pid).unwrap();
        assert!(position(3) < position(2) && position(4) < position(2));
    }

    #[test]
    fn a_cycle_in_the_parent_links_terminates() {
        let processes = [process(2, 3), process(3, 2)];
        assert_eq!(descendants(&processes, 2), [3]);
    }

    #[test]
    fn continuing_this_process_is_harmless() {
        assert!(send(std::process::id(), Signal::Continue).is_ok());
    }

    #[test]
    fn a_nonexistent_process_is_an_error() {
        assert!(send(u32::MAX, Signal::Terminate).is_err());
        assert!(set_nice(u32::MAX, 0).is_err());
    }

    #[test]
    fn pinning_to_no_processor_is_rejected() {
        assert_eq!(
            set_affinity(std::process::id(), &[]).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_command_that_runs_is_started() {
        let pid = spawn("sleep 30").unwrap();
        assert!(send(pid, Signal::Terminate).is_ok());
    }

    #[test]
    fn a_command_that_cannot_be_found_is_reported() {
        let error = spawn("peepers-no-such-command").unwrap_err();
        assert!(error.to_string().contains("ended at once"), "{error}");
    }

    #[test]
    fn a_command_that_finishes_at_once_is_not_an_error() {
        assert!(spawn("true").is_ok());
    }
}
