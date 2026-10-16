use std::io;
use std::time::Duration;

use crate::cpu::{self, Usage};
use crate::disk::{self, Disk};
use crate::gpu::Adapter;
use crate::memory::Memory;
use crate::network::{self, Interface};
use crate::power::{self, Domain};
use crate::pressure::{Pressure, Resource};
use crate::process::{self, Process};
use crate::sensors::{self, Cached, Chip};
use crate::system::{self, Load};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pressures {
    pub cpu: Option<Pressure>,
    pub memory: Option<Pressure>,
    pub io: Option<Pressure>,
}

/// Everything except the process list, which is large and has no use in a history.
#[derive(Debug, Clone, PartialEq)]
pub struct Vitals {
    pub uptime: Duration,
    pub load: Load,
    pub cpu: Usage,
    pub memory: Memory,
    pub pressure: Pressures,
    pub disks: Vec<Disk>,
    pub interfaces: Vec<Interface>,
    pub adapters: Vec<Adapter>,
    pub chips: Vec<Chip>,
    pub power: Vec<Domain>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub vitals: Vitals,
    pub processes: Vec<Process>,
}

#[derive(Debug, Default)]
pub struct Sampler {
    cpu: cpu::Sampler,
    disk: disk::Sampler,
    network: network::Sampler,
    power: power::Sampler,
    process: process::Sampler,
    chips: Cached<Vec<Chip>>,
}

impl Sampler {
    /// # Errors
    ///
    /// Fails when a kernel interface that every Linux system has cannot be read.
    pub fn sample(&mut self) -> io::Result<Sample> {
        Ok(Sample {
            vitals: Vitals {
                uptime: system::uptime()?,
                load: Load::read()?,
                cpu: self.cpu.sample()?,
                memory: Memory::read()?,
                pressure: Pressures {
                    cpu: Pressure::read(Resource::Cpu),
                    memory: Pressure::read(Resource::Memory),
                    io: Pressure::read(Resource::Io),
                },
                disks: self.disk.sample()?,
                interfaces: self.network.sample()?,
                adapters: Adapter::read_all(),
                chips: self.chips.get(sensors::chips),
                power: self.power.sample(),
            },
            processes: self.process.sample()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_covers_the_whole_machine() {
        let sample = Sampler::default().sample().unwrap();
        assert!(sample.vitals.memory.total > 0);
        assert!(!sample.vitals.cpu.cores.is_empty());
        assert!(
            sample
                .processes
                .iter()
                .any(|process| process.pid == std::process::id())
        );
    }
}
