use std::fs;

/// Percentage of the window during which tasks were stalled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stall {
    pub avg10: f32,
    pub avg60: f32,
    pub avg300: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pressure {
    pub some: Stall,
    pub full: Option<Stall>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resource {
    Cpu,
    Memory,
    Io,
}

impl Pressure {
    /// `None` when the kernel has no pressure accounting or the file is unreadable.
    #[must_use]
    pub fn read(resource: Resource) -> Option<Self> {
        let name = match resource {
            Resource::Cpu => "cpu",
            Resource::Memory => "memory",
            Resource::Io => "io",
        };
        Self::parse(&fs::read_to_string(format!("/proc/pressure/{name}")).ok()?)
    }

    fn parse(text: &str) -> Option<Self> {
        let stall = |kind: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(kind)?.strip_prefix(' '))
                .and_then(Stall::parse)
        };
        Some(Self {
            some: stall("some")?,
            full: stall("full"),
        })
    }
}

impl Stall {
    fn parse(fields: &str) -> Option<Self> {
        let average = |name: &str| {
            fields
                .split_ascii_whitespace()
                .find_map(|field| field.strip_prefix(name)?.strip_prefix('=')?.parse().ok())
        };
        Some(Self {
            avg10: average("avg10")?,
            avg60: average("avg60")?,
            avg300: average("avg300")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "some avg10=1.50 avg60=0.25 avg300=0.00 total=100\n\
                        full avg10=0.50 avg60=0.10 avg300=0.00 total=40\n";

    #[test]
    fn parses_both_stall_lines() {
        let pressure = Pressure::parse(TEXT).unwrap();
        assert!((pressure.some.avg10 - 1.5).abs() < f32::EPSILON);
        assert!(
            pressure
                .full
                .is_some_and(|full| (full.avg60 - 0.1).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn full_is_optional() {
        assert!(
            Pressure::parse("some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n")
                .unwrap()
                .full
                .is_none()
        );
    }
}
