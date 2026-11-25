use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{Bucket, HOUR, Metrics, Observation, RETENTION, Report};

const VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    bucket: Bucket,
}

/// Hourly files are atomically replaced. A lock prevents two Peepers instances from overwriting
/// each other's history. Bad files are reported and left untouched, including the current hour.
pub struct Store {
    directory: Option<PathBuf>,
    lock: Option<File>,
    buckets: BTreeMap<u64, Bucket>,
    dirty: BTreeSet<u64>,
    blocked: BTreeSet<u64>,
    warnings: Vec<String>,
    write_error: Option<String>,
}

impl Store {
    #[must_use]
    pub fn open_default() -> Self {
        match state_directory() {
            Some(directory) => Self::open(&directory, unix_now()),
            None => Self::memory("No absolute XDG_STATE_HOME or HOME; history is in memory only"),
        }
    }

    fn memory(warning: &str) -> Self {
        Self {
            directory: None,
            lock: None,
            buckets: BTreeMap::new(),
            dirty: BTreeSet::new(),
            blocked: BTreeSet::new(),
            warnings: vec![warning.into()],
            write_error: None,
        }
    }

    #[must_use]
    pub fn open(directory: &Path, now: u64) -> Self {
        let mut store = Self::memory("");
        store.warnings.clear();
        store.directory = Some(directory.to_owned());
        let setup = || -> Result<File, String> {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(directory)
                .map_err(|e| e.to_string())?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(directory.join("collector.lock"))
                .map_err(|e| e.to_string())?;
            lock.try_lock().map_err(|e| {
                format!("Cannot lock energy history (another collector may be running): {e}")
            })?;
            Ok(lock)
        };
        match setup() {
            Ok(lock) => store.lock = Some(lock),
            Err(error) => store
                .warnings
                .push(format!("History is in memory only: {error}")),
        }
        if let Err(error) = store.load(now) {
            store
                .warnings
                .push(format!("Some energy history could not be loaded: {error}"));
        }
        store
    }

    fn load(&mut self, now: u64) -> io::Result<()> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        let oldest = oldest(now);
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let Some(hour) = file_hour(&entry.path()) else {
                continue;
            };
            if hour < oldest {
                if self.lock.is_some() {
                    fs::remove_file(entry.path())?;
                }
                continue;
            }
            if hour > now / HOUR * HOUR {
                continue;
            }
            match read_bucket(&entry.path(), hour) {
                Ok(bucket) => {
                    self.buckets.insert(hour, bucket);
                }
                Err(error) => {
                    self.blocked.insert(hour);
                    self.warnings.push(format!(
                        "Left unreadable history {} untouched: {error}",
                        entry.path().display()
                    ));
                }
            }
        }
        Ok(())
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "validated Unix timestamps and intervals are within u64 and f64 reporting precision"
    )]
    pub(super) fn record(
        &mut self,
        start: f64,
        end: f64,
        readings: &[Observation],
        active: &BTreeSet<String>,
    ) {
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || end <= start
            || end - start > 30.0
        {
            return;
        }
        let mut cursor = start;
        let mut first = true;
        while cursor < end {
            let hour = cursor as u64 / HOUR * HOUR;
            let until = end.min((hour + HOUR) as f64);
            let bucket = self
                .buckets
                .entry(hour)
                .or_insert_with(|| Bucket::new(hour));
            // Repeated timestamps after restart or a backwards clock jump cannot count twice.
            cursor = cursor.max(bucket.through);
            let seconds = (until - cursor).max(0.0);
            if seconds > 0.0 {
                bucket.observed_seconds += seconds;
                bucket.through = until;
                for observation in readings {
                    let watts = observation.watts.filter(|v| v.is_finite() && *v >= 0.0);
                    let metrics = Metrics {
                        joules: watts.unwrap_or(0.0) * seconds,
                        measured_seconds: if watts.is_some() { seconds } else { 0.0 },
                        observed_seconds: seconds,
                        active_seconds: if observation.active { seconds } else { 0.0 },
                        bursts: u64::from(
                            first && observation.active && !active.contains(&observation.entity.id),
                        ),
                        peak_watts: watts,
                    };
                    bucket.record(&observation.entity, &metrics);
                }
                self.dirty.insert(hour);
            }
            cursor = until;
            first = false;
        }
        let oldest = oldest(end as u64);
        self.buckets.retain(|hour, _| *hour >= oldest);
        self.dirty.retain(|hour| *hour >= oldest);
    }

    /// # Errors
    /// Fails if storage is unavailable or any updated hour cannot be saved.
    pub fn flush(&mut self) -> Result<(), String> {
        let result = self.save();
        self.write_error = result.as_ref().err().cloned();
        result
    }

    fn save(&mut self) -> Result<(), String> {
        let directory = self
            .directory
            .as_ref()
            .filter(|_| self.lock.is_some())
            .ok_or_else(|| {
                "Energy history is in memory only; storage is unavailable or locked".to_owned()
            })?;
        let mut oversized = false;
        for hour in self.dirty.clone() {
            if self.blocked.contains(&hour) {
                continue;
            }
            let bucket = &self.buckets[&hour];
            let partial = directory.join(format!("{hour}.partial"));
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&partial)
                .map_err(|e| e.to_string())?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer(
                &mut writer,
                &Saved {
                    version: VERSION,
                    bucket: bucket.clone(),
                },
            )
            .map_err(|e| e.to_string())?;
            writer.flush().map_err(|e| e.to_string())?;
            if writer
                .get_ref()
                .metadata()
                .map_err(|e| e.to_string())?
                .len()
                > MAX_FILE_BYTES
            {
                drop(fs::remove_file(&partial));
                oversized = true;
                continue;
            }
            writer.get_ref().sync_all().map_err(|e| e.to_string())?;
            fs::rename(&partial, directory.join(format!("{hour}.json")))
                .map_err(|e| e.to_string())?;
            self.dirty.remove(&hour);
        }
        // Only remove this format's expired hourly files, never arbitrary files in the folder.
        let cutoff = self
            .buckets
            .last_key_value()
            .map_or(0, |(&hour, _)| oldest(hour));
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if file_hour(&entry.path()).is_some_and(|hour| hour < cutoff) {
                fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
            }
        }
        if oversized {
            return Err("An hourly energy record exceeded the 16 MiB file limit; its previous checkpoint was preserved".into());
        }
        if !self.dirty.is_disjoint(&self.blocked) {
            return Err("An unreadable hourly file was preserved; new readings for that hour could not be saved".into());
        }
        Ok(())
    }

    #[must_use]
    pub fn report(&self, hours: u64, now: u64) -> Report {
        let hours = hours.clamp(1, RETENTION / HOUR);
        let start = (now / HOUR * HOUR).saturating_sub((hours - 1) * HOUR);
        let step = if hours <= 24 { HOUR } else { 24 * HOUR };
        let mut total = Bucket::new(start);
        let mut periods = BTreeMap::new();
        for time in (start / step * step..=now).step_by(usize::try_from(step).unwrap_or(3600)) {
            periods.insert(time, Bucket::new(time));
        }
        let mut combined_legacy = false;
        for (_, bucket) in self.buckets.range(start..=now) {
            total.observed_seconds += bucket.observed_seconds;
            let time = bucket.start / step * step;
            let period = periods.entry(time).or_insert_with(|| Bucket::new(time));
            period.observed_seconds += bucket.observed_seconds;
            let mut grouped = Bucket::new(bucket.start);
            for row in bucket.rows.values() {
                let entity = super::attribution::report_entity(&row.entity);
                combined_legacy |= grouped.rows.contains_key(&entity.id);
                grouped.add(&entity, &row.metrics);
            }
            for row in grouped.rows.values_mut() {
                // Old hourly aggregates do not retain overlap timing. Preserve all energy,
                // but bound summed coverage/activity by the actual recorded interval.
                row.metrics.observed_seconds =
                    row.metrics.observed_seconds.min(bucket.observed_seconds);
                row.metrics.measured_seconds = row
                    .metrics
                    .measured_seconds
                    .min(row.metrics.observed_seconds);
                row.metrics.active_seconds =
                    row.metrics.active_seconds.min(row.metrics.observed_seconds);
                total.add(&row.entity, &row.metrics);
                period.add(&row.entity, &row.metrics);
            }
        }
        let mut rows: Vec<_> = total.rows.into_values().collect();
        rows.sort_by(|a, b| {
            b.metrics
                .joules
                .total_cmp(&a.metrics.joules)
                .then(
                    b.metrics
                        .active_seconds
                        .total_cmp(&a.metrics.active_seconds),
                )
                .then(a.entity.name.cmp(&b.entity.name))
        });
        let mut warnings = self.warnings.clone();
        warnings.extend(self.write_error.clone());
        if combined_legacy {
            warnings.push("Older application entries were combined. Energy totals are preserved; coverage, activity, averages, peaks and bursts may be approximate because hourly history does not retain overlap timing.".into());
        }
        Report {
            hours,
            start,
            end: now,
            observed_seconds: total.observed_seconds,
            rows,
            periods: periods.into_values().collect(),
            directory: self.directory.clone(),
            warning: (!warnings.is_empty()).then(|| warnings.join("\n")),
            status: String::new(),
        }
    }
}

fn read_bucket(path: &Path, hour: u64) -> io::Result<Bucket> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let saved: Saved = serde_json::from_reader(BufReader::new(file))?;
    let bucket = saved.bucket;
    #[expect(
        clippy::cast_precision_loss,
        reason = "hour timestamps fit reporting precision"
    )]
    let valid_time = bucket.through.is_finite()
        && bucket.through >= hour as f64
        && bucket.through <= (hour + HOUR) as f64;
    if saved.version != VERSION
        || bucket.start != hour
        || !valid_time
        || !bucket.observed_seconds.is_finite()
        || !(0.0..=3600.001).contains(&bucket.observed_seconds)
        || bucket.rows.iter().any(|(id, row)| {
            id != &row.entity.id
                || !row.metrics.valid()
                || row.metrics.observed_seconds > bucket.observed_seconds + 0.001
        })
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(bucket)
}

fn file_hour(path: &Path) -> Option<u64> {
    if path.extension()?.to_str()? != "json" {
        return None;
    }
    let hour = path.file_stem()?.to_str()?.parse::<u64>().ok()?;
    (hour % HOUR == 0).then_some(hour)
}

fn oldest(now: u64) -> u64 {
    (now / HOUR * HOUR).saturating_sub(RETENTION - HOUR)
}

fn state_directory() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(".local/state"))
        })
        .map(|p| p.join("peepers/energy"))
}

pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
