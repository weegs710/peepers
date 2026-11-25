//! Persistent energy accounting. Hardware channels are independent, never additive totals.
//! CPU attribution is a utilization model, not a measurement of a process's electrical draw.
mod attribution;
mod store;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::sensors::Kind as SensorKind;
use crate::telemetry::Sample;
use attribution::Attributor;
pub use store::Store;

pub const HOUR: u64 = 3600;
pub const RETENTION: u64 = 90 * 24 * HOUR;
pub const CPU: &str = "cpu-packages";
/// Rows one hour keeps. Further entities share one row, so processes that keep renaming
/// themselves cannot grow the history without bound.
const MAX_ROWS: usize = 4096;
const OVERFLOW: &str = "application/row-limit";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Hardware,
    DeviceActivity,
    Application,
    Service,
    Unattributed,
}

impl Kind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Hardware => "Hardware reported",
            Self::DeviceActivity => "Device · activity only",
            Self::Application => "App · CPU estimate",
            Self::Service => "Service · CPU estimate",
            Self::Unattributed => "CPU remainder",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub kind: Kind,
}

impl Entity {
    fn new(id: impl Into<String>, name: impl Into<String>, kind: Kind) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub joules: f64,
    /// Only intervals for which energy was available; unavailable is not zero.
    pub measured_seconds: f64,
    pub observed_seconds: f64,
    /// CPU-active sampled intervals, or intervals with positive hardware power.
    pub active_seconds: f64,
    /// Transitions into CPU activity, not scheduler wakeups.
    pub bursts: u64,
    pub peak_watts: Option<f64>,
}

impl Metrics {
    pub fn merge(&mut self, other: &Self) {
        self.joules += other.joules;
        self.measured_seconds += other.measured_seconds;
        self.observed_seconds += other.observed_seconds;
        self.active_seconds += other.active_seconds;
        self.bursts += other.bursts;
        self.peak_watts = match (self.peak_watts, other.peak_watts) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }

    #[must_use]
    pub fn watt_hours(&self) -> Option<f64> {
        (self.measured_seconds > 0.0).then_some(self.joules / 3600.0)
    }

    #[must_use]
    pub fn average_watts(&self) -> Option<f64> {
        (self.measured_seconds > 0.0).then(|| self.joules / self.measured_seconds)
    }

    fn valid(&self) -> bool {
        [
            self.joules,
            self.measured_seconds,
            self.observed_seconds,
            self.active_seconds,
        ]
        .into_iter()
        .all(|v| v.is_finite() && v >= 0.0)
            && self.measured_seconds <= self.observed_seconds + 0.001
            && self.active_seconds <= self.observed_seconds + 0.001
            && self.peak_watts.is_none_or(|v| v.is_finite() && v >= 0.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub entity: Entity,
    pub metrics: Metrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bucket {
    pub start: u64,
    pub through: f64,
    pub observed_seconds: f64,
    pub rows: BTreeMap<String, Row>,
}

impl Bucket {
    fn new(start: u64) -> Self {
        Self {
            start,
            through: 0.0,
            observed_seconds: 0.0,
            rows: BTreeMap::new(),
        }
    }

    fn add(&mut self, entity: &Entity, metrics: &Metrics) {
        let row = self.rows.entry(entity.id.clone()).or_insert_with(|| Row {
            entity: entity.clone(),
            metrics: Metrics::default(),
        });
        row.metrics.merge(metrics);
    }

    /// Adds a reading as `add` does until the hour has `MAX_ROWS` rows; after that, new entities
    /// share one row whose times are capped by the hour's, as loading a saved hour requires.
    fn record(&mut self, entity: &Entity, metrics: &Metrics) {
        if self.rows.len() < MAX_ROWS || self.rows.contains_key(&entity.id) {
            self.add(entity, metrics);
            return;
        }
        let overflow = Entity::new(
            OVERFLOW,
            "Other processes (row limit reached)",
            Kind::Application,
        );
        self.add(&overflow, metrics);
        if let Some(row) = self.rows.get_mut(OVERFLOW) {
            let shared = &mut row.metrics;
            shared.observed_seconds = shared.observed_seconds.min(self.observed_seconds);
            shared.measured_seconds = shared.measured_seconds.min(shared.observed_seconds);
            shared.active_seconds = shared.active_seconds.min(shared.observed_seconds);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub hours: u64,
    pub start: u64,
    pub end: u64,
    pub observed_seconds: f64,
    pub rows: Vec<Row>,
    /// UTC hours for a day view, UTC days for longer views. Empty bins are gaps.
    pub periods: Vec<Bucket>,
    pub directory: Option<PathBuf>,
    pub warning: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone)]
struct Observation {
    entity: Entity,
    watts: Option<f64>,
    active: bool,
}

/// Own this on the sampling thread; neither filesystem writes nor aggregation run in the UI.
pub struct Collector {
    store: Store,
    attributor: Attributor,
    before: Option<(Instant, Duration, f64)>,
    active: BTreeSet<String>,
    previous_sensors: BTreeMap<String, f64>,
    last_save: Instant,
    status: String,
}

impl Collector {
    #[must_use]
    pub fn open() -> Self {
        Self::with_store(Store::open_default())
    }

    #[must_use]
    pub fn with_store(store: Store) -> Self {
        Self {
            store,
            attributor: Attributor::default(),
            before: None,
            active: BTreeSet::new(),
            previous_sensors: BTreeMap::new(),
            last_save: Instant::now(),
            status: "Waiting for consecutive readings".into(),
        }
    }

    /// Invalidates the interval after a failed sample. Never bridges missing readings.
    pub fn gap(&mut self) {
        self.before = None;
        self.active.clear();
        self.previous_sensors.clear();
        self.status = "Sampling interrupted; the missing interval is left blank".into();
    }

    pub fn record(&mut self, sample: &Sample, now: Instant, wall: SystemTime) {
        let Ok(wall) = wall.duration_since(UNIX_EPOCH) else {
            self.gap();
            return;
        };
        let wall = wall.as_secs_f64();
        let previous = self.before.replace((now, sample.vitals.uptime, wall));
        let Some((before, uptime, previous_wall)) = previous else {
            // Prime point sensors immediately; the next sample can integrate their first interval.
            // Counter and process baselines are also primed, but no startup energy is recorded.
            self.observations(sample, 1.0);
            return;
        };
        let seconds = now.duration_since(before).as_secs_f64();
        let awake = sample.vitals.uptime.saturating_sub(uptime).as_secs_f64();
        // /proc/uptime includes suspend. Instant does not on Linux. Wall jumps also leave gaps.
        if !(0.0..=30.0).contains(&seconds)
            || seconds == 0.0
            || (awake - seconds).abs() > 2.0
            || (wall - previous_wall - seconds).abs() > 2.0
        {
            self.active.clear();
            self.previous_sensors.clear();
            self.status =
                "Sleep or clock change detected; the missing interval is left blank".into();
            return;
        }
        let observations = self.observations(sample, seconds);
        self.store
            .record(wall - seconds, wall, &observations, &self.active);
        self.active = observations
            .iter()
            .filter(|o| o.active)
            .map(|o| o.entity.id.clone())
            .collect();
        if self.last_save.elapsed() >= Duration::from_secs(30) {
            let _ = self.flush();
        }
    }

    fn observations(&mut self, sample: &Sample, seconds: f64) -> Vec<Observation> {
        let packages: Vec<_> = sample
            .vitals
            .power
            .iter()
            .filter(|d| d.name.starts_with("package-"))
            .collect();
        let cpu = (!packages.is_empty())
            .then(|| {
                packages
                    .iter()
                    .map(|d| d.joules.filter(|v| v.is_finite() && *v >= 0.0))
                    .sum::<Option<f64>>()
            })
            .flatten()
            .map(|j| j / seconds);
        self.status = if cpu.is_some() {
            "Collecting · CPU estimates use each group's share of total CPU capacity".into()
        } else if packages.iter().any(|d| !d.readable) {
            "CPU energy unavailable: RAPL counters need read permission. Activity is still recorded.".into()
        } else {
            "CPU energy unavailable: waiting for supported, consecutive RAPL readings. Activity is still recorded.".into()
        };
        let mut observations = vec![Observation {
            entity: Entity::new(CPU, "CPU packages (combined)", Kind::Hardware),
            watts: cpu,
            active: cpu.is_some_and(|v| v > 0.0),
        }];
        observations.extend(
            sample
                .vitals
                .power
                .iter()
                .filter(|d| !d.name.starts_with("package-"))
                .map(|d| Observation {
                    entity: Entity::new(
                        format!("rapl/{}", d.name),
                        format!("{} (separate domain)", d.name),
                        Kind::Hardware,
                    ),
                    watts: d.joules.map(|j| j / seconds),
                    active: d.watts.is_some_and(|v| v > 0.0),
                }),
        );
        let mut current_sensors = BTreeMap::new();
        for chip in &sample.vitals.chips {
            for reading in chip.readings.iter().filter(|r| r.kind == SensorKind::Power) {
                let id = self.attributor.sensor_id(chip, &reading.label);
                let watts = f64::from(reading.value());
                if !watts.is_finite() || watts < 0.0 {
                    continue;
                }
                current_sensors.insert(id.clone(), watts);
                observations.push(Observation {
                    entity: Entity::new(
                        id.clone(),
                        format!(
                            "{} · {}",
                            chip.model.as_deref().unwrap_or(&chip.name),
                            reading.label
                        ),
                        Kind::Hardware,
                    ),
                    watts: self
                        .previous_sensors
                        .get(&id)
                        .map(|old| (old + watts) / 2.0),
                    active: watts > 0.0,
                });
            }
        }
        // Battery flow is useful only during discharge; charging current is not machine draw.
        for (id, watts) in attribution::batteries() {
            if let Some(watts) = watts {
                current_sensors.insert(id.clone(), watts);
            }
            observations.push(Observation {
                entity: Entity::new(
                    format!("battery/{id}"),
                    format!("{id} discharge (whole laptop)"),
                    Kind::Hardware,
                ),
                watts: watts
                    .zip(self.previous_sensors.get(&id))
                    .map(|(new, old)| (new + old) / 2.0),
                active: watts.is_some_and(|v| v > 0.0),
            });
        }
        self.previous_sensors = current_sensors;
        observations.extend(attribution::device_activity(sample));
        observations.extend(self.attributor.attribute(sample, cpu));
        observations
    }

    /// # Errors
    /// Fails if the history cannot be written. The in-memory readings remain available.
    pub fn flush(&mut self) -> Result<(), String> {
        self.last_save = Instant::now();
        self.store.flush()
    }

    #[must_use]
    pub fn report(&self, hours: u64, now: u64) -> Report {
        let mut report = self.store.report(hours, now);
        report.status.clone_from(&self.status);
        report
    }
}

#[cfg(test)]
mod tests;
