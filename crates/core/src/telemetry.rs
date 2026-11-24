use std::io;
use std::time::Duration;

use crate::cpu::{self, Usage};
use crate::disk::{self, Disk};
use crate::gpu::{self, Adapter};
use crate::memory::Memory;
use crate::network::{self, Interface};
use crate::power::{self, Domain};
use crate::pressure::{Pressure, Resource};
use crate::process::{self, Process};
use crate::sensors::{self, Cached, Chip};
use crate::system::{self, Load};
use crate::users::Users;

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
    /// The names of the users that own the processes.
    pub users: Users,
}

#[derive(Debug, Default)]
pub struct Sampler {
    cpu: cpu::Sampler,
    disk: disk::Sampler,
    network: network::Sampler,
    power: power::Sampler,
    process: process::Sampler,
    gpu: gpu::Sampler,
    chips: Cached<Vec<Chip>>,
    users: Users,
}

impl Sampler {
    /// # Errors
    ///
    /// Fails when a kernel interface that every Linux system has cannot be read.
    pub fn sample(&mut self) -> io::Result<Sample> {
        let adapters = self.gpu.sample();
        let mut chips = self.chips.get(sensors::chips);
        chips.extend(gpu::tool_chips(&adapters));
        let vitals = Vitals {
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
            adapters,
            chips,
            power: self.power.sample(),
        };
        let processes = self.process.sample()?;
        self.users
            .learn(processes.iter().map(|process| process.uid));
        Ok(Sample {
            vitals,
            processes,
            users: self.users.clone(),
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
        assert_ne!(sample.vitals.cpu.cores.len(), 0);
        assert!(
            sample
                .processes
                .iter()
                .any(|process| process.pid == std::process::id())
        );
    }
}
