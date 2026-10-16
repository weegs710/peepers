use std::fs;

use crate::cpu::{CacheKind, Info};
use crate::disk;
use crate::format::{bytes, frequency, rate, uptime};
use crate::network;
use crate::telemetry::Vitals;
use crate::volumes::Volume;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    pub rows: Vec<(String, String)>,
}

/// What this machine is, gathered for display and for saving as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub sections: Vec<Section>,
}

impl Report {
    #[must_use]
    pub fn build(vitals: &Vitals, cpu: &Info, volumes: &[Volume]) -> Self {
        Self {
            sections: vec![
                system(vitals),
                processor(cpu, vitals),
                memory(vitals),
                storage(vitals, volumes),
                networking(vitals),
                graphics(vitals),
            ],
        }
    }

    /// The report as plain text, one titled block per section.
    #[must_use]
    pub fn text(&self) -> String {
        let blocks: Vec<String> = self
            .sections
            .iter()
            .map(|section| {
                let width = section
                    .rows
                    .iter()
                    .map(|(key, _)| key.len())
                    .max()
                    .unwrap_or(0);
                let rows: Vec<String> = section
                    .rows
                    .iter()
                    .map(|(key, value)| format!("{key:<width$}  {value}"))
                    .collect();
                format!("[{}]\n{}\n", section.title, rows.join("\n"))
            })
            .collect();
        format!("System information\n\n{}", blocks.join("\n"))
    }
}

fn row(key: &str, value: impl Into<String>) -> (String, String) {
    (key.to_owned(), value.into())
}

fn file(path: &str) -> String {
    fs::read_to_string(path)
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

/// The `PRETTY_NAME` of the distribution, from `os-release`.
fn distribution() -> Option<String> {
    fs::read_to_string("/etc/os-release")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("PRETTY_NAME=")
                .map(|name| name.trim_matches('"').to_owned())
        })
}

fn system(vitals: &Vitals) -> Section {
    Section {
        title: "System",
        rows: vec![
            row("Host name", file("/proc/sys/kernel/hostname")),
            row("Operating system", distribution().unwrap_or_default()),
            row("Kernel", file("/proc/sys/kernel/osrelease")),
            row("Up time", uptime(vitals.uptime)),
            row(
                "Load average",
                format!(
                    "{:.2} {:.2} {:.2}",
                    vitals.load.one, vitals.load.five, vitals.load.fifteen
                ),
            ),
        ],
    }
}

fn processor(cpu: &Info, vitals: &Vitals) -> Section {
    let mut rows = vec![
        row("Model", cpu.model.clone().unwrap_or_default()),
        row("Sockets", cpu.packages.to_string()),
        row("Cores", cpu.cores.to_string()),
        row("Logical processors", vitals.cpu.cores.len().to_string()),
        row("NUMA nodes", cpu.numa_nodes.to_string()),
        row(
            "Virtualization",
            cpu.virtualization.unwrap_or("not supported"),
        ),
    ];
    rows.extend(cpu.max_khz.map(|khz| row("Maximum speed", frequency(khz))));
    for cache in &cpu.caches {
        let kind = match cache.kind {
            CacheKind::Data => " data",
            CacheKind::Instruction => " instruction",
            CacheKind::Unified => "",
        };
        rows.push(row(
            &format!("L{}{kind} cache", cache.level),
            format!("{} x {}", cache.instances, bytes(cache.size)),
        ));
    }
    rows.extend(
        cpu.governor
            .as_ref()
            .map(|governor| row("Governor", governor.clone())),
    );
    Section {
        title: "Processor",
        rows,
    }
}

fn memory(vitals: &Vitals) -> Section {
    let memory = &vitals.memory;
    Section {
        title: "Memory",
        rows: vec![
            row("Installed", bytes(memory.total)),
            row("Available", bytes(memory.available)),
            row("Swap", bytes(memory.swap_total)),
        ],
    }
}

fn storage(vitals: &Vitals, volumes: &[Volume]) -> Section {
    let mut rows: Vec<_> = vitals
        .disks
        .iter()
        .map(|disk| {
            let kind = match disk.kind {
                disk::Kind::Nvme => "NVMe SSD",
                disk::Kind::Ssd => "SSD",
                disk::Kind::Hdd => "hard disk",
            };
            row(
                &disk.name,
                format!(
                    "{} {kind}, {}",
                    disk.model.clone().unwrap_or_default(),
                    bytes(disk.size)
                ),
            )
        })
        .collect();
    rows.extend(volumes.iter().map(|volume| {
        row(
            &volume.mount.display().to_string(),
            format!(
                "{} {}, {} of {} used",
                volume.filesystem,
                volume.source,
                bytes(volume.used()),
                bytes(volume.total)
            ),
        )
    }));
    Section {
        title: "Storage",
        rows,
    }
}

fn networking(vitals: &Vitals) -> Section {
    let rows = vitals
        .interfaces
        .iter()
        .filter(|interface| {
            matches!(
                interface.kind,
                network::Kind::Ethernet | network::Kind::Wireless
            )
        })
        .map(|interface| {
            let addresses: Vec<String> = interface
                .ipv4
                .iter()
                .map(ToString::to_string)
                .chain(interface.ipv6.iter().map(ToString::to_string))
                .collect();
            row(
                &interface.name,
                format!(
                    "{} {}, {}, up {} down {}",
                    interface.model.clone().unwrap_or_default(),
                    interface.mac.clone().unwrap_or_default(),
                    addresses.join(" "),
                    rate(interface.send_per_second),
                    rate(interface.receive_per_second),
                ),
            )
        })
        .collect();
    Section {
        title: "Network",
        rows,
    }
}

fn graphics(vitals: &Vitals) -> Section {
    let rows = vitals
        .adapters
        .iter()
        .map(|adapter| {
            let memory = adapter.vram.map_or_else(String::new, |pool| {
                format!(", {} video memory", bytes(pool.total))
            });
            row(
                &adapter.card,
                format!(
                    "{} ({}){memory}",
                    adapter
                        .model
                        .clone()
                        .unwrap_or_else(|| format!("{:04x}", adapter.device)),
                    adapter.driver.clone().unwrap_or_default(),
                ),
            )
        })
        .collect();
    Section {
        title: "Graphics",
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::Sampler;
    use crate::volumes;

    fn report() -> Report {
        let vitals = Sampler::default().sample().unwrap().vitals;
        Report::build(&vitals, &Info::read(), &volumes::read())
    }

    #[test]
    fn every_section_is_present_and_titled() {
        let titles: Vec<_> = report()
            .sections
            .iter()
            .map(|section| section.title)
            .collect();
        assert_eq!(
            titles,
            [
                "System",
                "Processor",
                "Memory",
                "Storage",
                "Network",
                "Graphics"
            ]
        );
    }

    #[test]
    fn the_text_aligns_each_section_and_names_the_machine() {
        let text = report().text();
        assert!(text.starts_with("System information\n\n[System]\n"));
        assert!(text.contains("\n[Processor]\n"));
        assert!(text.contains(&file("/proc/sys/kernel/hostname")));
    }

    #[test]
    fn this_machine_reports_its_memory_and_a_disk() {
        let report = report();
        assert!(
            report.sections[2]
                .rows
                .iter()
                .any(|(key, value)| key == "Installed" && !value.is_empty())
        );
        assert!(!report.sections[3].rows.is_empty());
    }
}
