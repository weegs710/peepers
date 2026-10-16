use std::fs;
use std::io;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Load {
    pub one: f32,
    pub five: f32,
    pub fifteen: f32,
    /// Tasks currently runnable or waiting uninterruptibly, per the scheduler.
    pub runnable: u32,
    pub tasks: u32,
}

impl Load {
    /// # Errors
    ///
    /// Fails when `/proc/loadavg` cannot be read or is malformed.
    pub fn read() -> io::Result<Self> {
        Self::parse(&fs::read_to_string("/proc/loadavg")?)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))
    }

    /// `0.52 0.58 0.59 2/1234 56789`: three averages, then runnable over total tasks.
    fn parse(text: &str) -> Option<Self> {
        let mut fields = text.split_ascii_whitespace();
        let (runnable, tasks) = fields.clone().nth(3)?.split_once('/')?;
        Some(Self {
            one: fields.next()?.parse().ok()?,
            five: fields.next()?.parse().ok()?,
            fifteen: fields.next()?.parse().ok()?,
            runnable: runnable.parse().ok()?,
            tasks: tasks.parse().ok()?,
        })
    }
}

/// # Errors
///
/// Fails when `/proc/uptime` cannot be read or is malformed.
pub fn uptime() -> io::Result<Duration> {
    fs::read_to_string("/proc/uptime")?
        .split_ascii_whitespace()
        .next()
        .and_then(|seconds| seconds.parse().ok())
        .map(Duration::from_secs_f64)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loadavg_splits_averages_from_task_counts() {
        let load = Load::parse("0.50 1.25 2.00 3/1234 56789\n").unwrap();
        assert!((load.five - 1.25).abs() < f32::EPSILON);
        assert_eq!((load.runnable, load.tasks), (3, 1234));
    }

    #[test]
    fn malformed_loadavg_is_rejected() {
        assert!(Load::parse("0.50 1.25").is_none());
        assert!(Load::parse("0.5 1.0 2.0 31234 1").is_none());
    }

    #[test]
    fn this_machine_has_been_up_for_a_while() {
        assert!(uptime().unwrap() > Duration::ZERO);
    }
}
