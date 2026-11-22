use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{Entity, Kind, Observation};
use crate::process::Process;
use crate::sensors::Chip;
use crate::sysfs;
use crate::telemetry::Sample;

#[derive(Default)]
pub(super) struct Attributor {
    identities: HashMap<u32, Identity>,
    sensors: HashMap<String, (Instant, String)>,
}

struct Identity {
    started: Duration,
    refreshed: Instant,
    name: String,
    entity: Entity,
}

impl Attributor {
    pub(super) fn attribute(&mut self, sample: &Sample, cpu: Option<f64>) -> Vec<Observation> {
        let mut groups: BTreeMap<String, (Entity, f64)> = BTreeMap::new();
        for process in &sample.processes {
            let identity = self
                .identities
                .entry(process.pid)
                .or_insert_with(|| identify(process));
            if identity.started != process.started
                || identity.name != process.name
                || identity.refreshed.elapsed() >= Duration::from_secs(30)
            {
                *identity = identify(process);
            }
            let group = groups
                .entry(identity.entity.id.clone())
                .or_insert_with(|| (identity.entity.clone(), 0.0));
            if process.cpu.is_finite() {
                group.1 += f64::from(process.cpu.max(0.0));
            }
        }
        let alive: HashSet<_> = sample.processes.iter().map(|p| p.pid).collect();
        self.identities.retain(|pid, _| alive.contains(pid));
        let used: f64 = groups.values().map(|(_, used)| used).sum();
        let cores = u32::try_from(sample.vitals.cpu.cores.len()).unwrap_or(u32::MAX);
        // Idle capacity remains unattributed. Normalize over-counts from staggered proc reads.
        let capacity = (f64::from(cores) * 100.0).max(used).max(1.0);
        let mut result: Vec<_> = groups
            .into_values()
            .map(|(entity, used)| Observation {
                entity,
                watts: cpu.map(|w| w * used / capacity),
                active: used > 0.0,
            })
            .collect();
        result.push(Observation {
            entity: Entity::new(
                "cpu-unattributed",
                "Idle / shared / unattributed CPU",
                Kind::Unattributed,
            ),
            watts: cpu.map(|w| w * (1.0 - used / capacity).max(0.0)),
            active: false,
        });
        result
    }

    pub(super) fn sensor_id(&mut self, chip: &Chip, label: &str) -> String {
        let entry = self
            .sensors
            .entry(chip.id.clone())
            .or_insert_with(|| (Instant::now(), device_id(chip)));
        if entry.0.elapsed() >= Duration::from_secs(30) {
            *entry = (Instant::now(), device_id(chip));
        }
        format!("sensor/{}/{}/{label}", entry.1, chip.name)
    }
}

fn device_id(chip: &Chip) -> String {
    let path = if let Some(card) = chip.id.strip_prefix("nvidia-") {
        format!("/sys/class/drm/{card}/device")
    } else {
        format!("/sys/class/hwmon/{}/device", chip.id)
    };
    fs::canonicalize(path).map_or_else(
        |_| chip.id.clone(),
        |path| path.to_string_lossy().into_owned(),
    )
}

fn identify(process: &Process) -> Identity {
    let cgroup = fs::read_to_string(format!("/proc/{}/cgroup", process.pid)).unwrap_or_default();
    let exe = fs::read_link(format!("/proc/{}/exe", process.pid)).ok();
    Identity {
        started: process.started,
        refreshed: Instant::now(),
        name: process.name.clone(),
        entity: group(process.uid, &process.name, exe.as_deref(), &cgroup),
    }
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "systemd unit suffixes are case-sensitive"
)]
pub(super) fn group(uid: u32, name: &str, exe: Option<&Path>, cgroup: &str) -> Entity {
    // Prefer cgroup v2; on v1 only the systemd hierarchy identifies service boundaries.
    let path = cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .or_else(|| {
            cgroup.lines().find_map(|line| {
                let mut fields = line.splitn(3, ':');
                fields.next()?;
                fields
                    .next()?
                    .split(',')
                    .any(|part| part == "name=systemd")
                    .then(|| fields.next())
                    .flatten()
            })
        });
    if let Some(path) = path {
        for unit in path.split('/').rev() {
            if unit.starts_with("app-") && (unit.ends_with(".scope") || unit.ends_with(".service"))
            {
                let app = app_name(unit);
                return Entity::new(format!("application/{app}"), app, Kind::Application);
            }
            if unit.ends_with(".service") && !unit.starts_with("user@") {
                // Service descendants belong to the same service, not to their individual PIDs.
                let boundary = path.rfind(unit).map_or(path, |i| &path[..i + unit.len()]);
                return Entity::new(
                    format!("service/{uid}/{boundary}"),
                    format!("{unit} (uid {uid})"),
                    Kind::Service,
                );
            }
        }
    }
    let label = exe
        .and_then(Path::file_name)
        .map_or_else(|| name.to_owned(), |s| s.to_string_lossy().into_owned());
    Entity::new(format!("application/{label}"), label, Kind::Application)
}

fn app_name(unit: &str) -> String {
    let name = unit
        .strip_prefix("app-")
        .unwrap_or(unit)
        .trim_end_matches(".scope")
        .trim_end_matches(".service");
    launch_name(name)
}

fn launch_name(name: &str) -> String {
    let name = name.split_once('@').map_or(name, |(name, _)| name);
    let mut name = name;
    while let Some((base, _)) = name.rsplit_once('-').filter(|(_, suffix)| {
        !suffix.is_empty()
            && (suffix.chars().all(|c| c.is_ascii_digit())
                || (suffix.len() >= 6 && suffix.chars().all(|c| c.is_ascii_hexdigit())))
    }) {
        name = base;
    }
    name.to_owned()
}

/// Resolve saved launch identities and executable paths to the current application key.
pub(super) fn report_entity(entity: &Entity) -> Entity {
    if entity.kind != Kind::Application {
        return entity.clone();
    }
    let Some((source, rest)) = entity.id.split_once('/') else {
        return entity.clone();
    };
    let Some((_, identity)) = rest.split_once('/') else {
        return entity.clone();
    };
    let label = match source {
        "app" => launch_name(identity),
        "exe" => Path::new(identity)
            .file_name()
            .map_or_else(|| identity.to_owned(), |s| s.to_string_lossy().into_owned()),
        _ => return entity.clone(),
    };
    Entity::new(format!("application/{label}"), label, Kind::Application)
}

pub(super) fn device_activity(sample: &Sample) -> Vec<Observation> {
    let mut readings = Vec::new();
    let mut add = |class: &str, name: &str, model: Option<&str>, active| {
        readings.push(Observation {
            entity: Entity::new(
                format!("activity/{class}/{name}"),
                format!("{} ({name}) · activity", model.unwrap_or(class)),
                Kind::DeviceActivity,
            ),
            watts: None,
            active,
        });
    };
    for disk in &sample.vitals.disks {
        add("disk", &disk.name, disk.model.as_deref(), disk.active > 0.0);
    }
    for network in &sample.vitals.interfaces {
        if matches!(
            network.kind,
            crate::network::Kind::Ethernet | crate::network::Kind::Wireless
        ) {
            add(
                "network",
                &network.name,
                network.model.as_deref(),
                network.receive_per_second > 0 || network.send_per_second > 0,
            );
        }
    }
    for gpu in &sample.vitals.adapters {
        if let Some(busy) = gpu.busy {
            add("gpu", &gpu.card, gpu.model.as_deref(), busy > 0.0);
        }
    }
    readings
}

pub(super) fn batteries() -> Vec<(String, Option<f64>)> {
    fs::read_dir("/sys/class/power_supply")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| sysfs::read::<String>(e.path().join("type")).as_deref() == Some("Battery"))
        .map(|entry| {
            let path = entry.path();
            let watts =
                if sysfs::read::<String>(path.join("status")).as_deref() == Some("Discharging") {
                    sysfs::read::<f64>(path.join("power_now"))
                        .map(|p| p / 1e6)
                        .or_else(|| {
                            Some(
                                sysfs::read::<f64>(path.join("current_now"))?.abs()
                                    * sysfs::read::<f64>(path.join("voltage_now"))?
                                    / 1e12,
                            )
                        })
                        .filter(|w| w.is_finite() && *w >= 0.0)
                } else {
                    None
                };
            (entry.file_name().to_string_lossy().into_owned(), watts)
        })
        .collect()
}
