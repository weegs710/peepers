use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "peepers-energy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn reading(watts: Option<f64>, active: bool) -> Observation {
    Observation {
        entity: Entity::new(CPU, "CPU", Kind::Hardware),
        watts,
        active,
    }
}

#[test]
fn integrates_energy_splits_hours_and_counts_a_burst_once() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0, 7200);
    store.record(
        3590.0,
        3610.0,
        &[reading(Some(180.0), true)],
        &BTreeSet::new(),
    );
    let report = store.report(24, 7200);
    let metrics = &report.rows[0].metrics;
    assert_eq!(metrics.watt_hours(), Some(1.0));
    assert_eq!(metrics.average_watts(), Some(180.0));
    assert_eq!(metrics.active_seconds, 20.0);
    assert_eq!(metrics.bursts, 1);
    assert_eq!(report.periods[0].rows[CPU].metrics.joules, 1800.0);
    assert_eq!(report.periods[1].rows[CPU].metrics.joules, 1800.0);
}

#[test]
fn unknown_readings_are_not_zero_and_missing_hours_stay_empty() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0, 7200);
    store.record(10.0, 20.0, &[reading(None, true)], &BTreeSet::new());
    let report = store.report(24, 7200);
    assert_eq!(report.rows[0].metrics.watt_hours(), None);
    assert_eq!(report.rows[0].metrics.active_seconds, 10.0);
    assert!(report.periods[1].rows.is_empty());
    store.record(20.0, 30.0, &[reading(Some(4.0), false)], &BTreeSet::new());
    let report = store.report(24, 7200);
    assert_eq!(report.rows[0].metrics.measured_seconds, 10.0);
    assert_eq!(report.rows[0].metrics.observed_seconds, 20.0);
    assert_eq!(report.rows[0].metrics.average_watts(), Some(4.0));
}

#[test]
fn restart_preserves_totals_and_duplicate_intervals_cannot_add_energy() {
    let directory = Directory::new();
    {
        let mut store = Store::open(&directory.0, 100);
        store.record(10.0, 20.0, &[reading(Some(10.0), true)], &BTreeSet::new());
        store.flush().unwrap();
    }
    let mut store = Store::open(&directory.0, 100);
    store.record(10.0, 20.0, &[reading(Some(10.0), true)], &BTreeSet::new());
    store.record(
        20.0,
        30.0,
        &[reading(Some(10.0), true)],
        &BTreeSet::from([CPU.into()]),
    );
    assert_eq!(store.report(24, 100).rows[0].metrics.joules, 200.0);
    assert_eq!(store.report(24, 100).rows[0].metrics.bursts, 1);
}

#[test]
fn another_collector_cannot_overwrite_history() {
    let directory = Directory::new();
    let mut first = Store::open(&directory.0, 100);
    first.record(10.0, 20.0, &[reading(Some(10.0), false)], &BTreeSet::new());
    first.flush().unwrap();
    let mut second = Store::open(&directory.0, 100);
    second.record(20.0, 30.0, &[reading(Some(100.0), false)], &BTreeSet::new());
    assert!(second.flush().is_err());
    assert!(second.report(24, 100).warning.unwrap().contains("lock"));
    drop(second);
    drop(first);
    assert_eq!(
        Store::open(&directory.0, 100).report(24, 100).rows[0]
            .metrics
            .joules,
        100.0
    );
}

#[test]
fn corrupt_history_is_reported_and_never_overwritten() {
    let directory = Directory::new();
    fs::write(directory.0.join("0.json"), "broken").unwrap();
    let mut store = Store::open(&directory.0, 100);
    store.record(10.0, 20.0, &[reading(Some(10.0), false)], &BTreeSet::new());
    assert!(store.flush().is_err());
    assert!(store.report(24, 100).warning.unwrap().contains("untouched"));
    assert_eq!(
        fs::read_to_string(directory.0.join("0.json")).unwrap(),
        "broken"
    );
}

#[test]
fn retention_removes_only_expired_hour_files() {
    let directory = Directory::new();
    fs::write(directory.0.join("0.json"), "expired").unwrap();
    fs::write(directory.0.join("notes.json"), "keep").unwrap();
    let _store = Store::open(&directory.0, RETENTION + HOUR);
    assert!(!directory.0.join("0.json").exists());
    assert!(directory.0.join("notes.json").exists());
}

#[test]
fn grouping_handles_services_app_instances_and_executable_fallbacks() {
    use attribution::group;
    use std::path::Path;
    let a = group(
        1000,
        "child",
        None,
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-firefox-123.scope/child\n",
    );
    let b = group(
        1000,
        "firefox",
        None,
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-firefox-456.scope\n",
    );
    assert_eq!(a.id, b.id);
    assert_eq!(a.kind, Kind::Application);
    let service = group(
        0,
        "worker",
        None,
        "0::/system.slice/backup.service/workers\n",
    );
    assert_eq!(service.kind, Kind::Service);
    assert!(service.id.ends_with("backup.service"));
    let fallback = group(
        1000,
        "short",
        Some(Path::new("/opt/bin/application")),
        "0::/user.slice/user@1000.service/session.scope",
    );
    assert_eq!(fallback.kind, Kind::Application);
    assert!(fallback.id.ends_with("/opt/bin/application"));
    assert_ne!(
        fallback.id,
        group(1001, "short", Some(Path::new("/opt/bin/application")), "").id
    );
}

fn sample() -> Sample {
    let mut sample = crate::telemetry::Sampler::default().sample().unwrap();
    sample.vitals.chips.clear();
    sample.vitals.power = vec![crate::power::Domain {
        name: "package-0".into(),
        watts: Some(100.0),
        joules: Some(100.0),
        readable: true,
    }];
    sample.vitals.cpu.cores.truncate(2);
    let mut process = sample.processes[0].clone();
    process.pid = u32::MAX;
    process.cpu = 50.0;
    process.name = "test-app".into();
    sample.processes = vec![process];
    sample
}

#[test]
fn cpu_attribution_balances_with_remainder_and_ignores_platform_overlap() {
    let mut sample = sample();
    sample.vitals.power.push(crate::power::Domain {
        name: "psys".into(),
        watts: Some(200.0),
        joules: Some(200.0),
        readable: true,
    });
    let directory = Directory::new();
    let mut collector = Collector::with_store(Store::open(&directory.0, 100));
    let observations = collector.observations(&sample, 1.0);
    assert_eq!(
        observations
            .iter()
            .find(|o| o.entity.id == CPU)
            .unwrap()
            .watts,
        Some(100.0)
    );
    let attributed: f64 = observations
        .iter()
        .filter(|o| o.entity.kind == Kind::Application)
        .filter_map(|o| o.watts)
        .sum();
    let remainder = observations
        .iter()
        .find(|o| o.entity.kind == Kind::Unattributed)
        .unwrap()
        .watts
        .unwrap();
    assert_eq!(attributed, 25.0);
    assert_eq!(remainder, 75.0);
    assert_eq!(attributed + remainder, 100.0);
    sample.vitals.power[0].joules = None;
    assert!(
        collector
            .observations(&sample, 1.0)
            .iter()
            .filter(|o| o.entity.kind == Kind::Application)
            .all(|o| o.watts.is_none())
    );
}

#[test]
fn suspend_and_failed_samples_do_not_bridge_energy_across_gaps() {
    let directory = Directory::new();
    let mut collector = Collector::with_store(Store::open(&directory.0, 100));
    let mut sample = sample();
    sample.vitals.uptime = Duration::from_secs(100);
    let start = Instant::now();
    collector.record(&sample, start, UNIX_EPOCH + Duration::from_secs(100));
    sample.vitals.uptime += Duration::from_secs(1);
    collector.record(
        &sample,
        start + Duration::from_secs(1),
        UNIX_EPOCH + Duration::from_secs(101),
    );
    assert_eq!(
        collector
            .report(24, 101)
            .rows
            .iter()
            .find(|r| r.entity.id == CPU)
            .unwrap()
            .metrics
            .joules,
        100.0
    );
    sample.vitals.uptime += Duration::from_secs(300);
    collector.record(
        &sample,
        start + Duration::from_secs(2),
        UNIX_EPOCH + Duration::from_secs(401),
    );
    collector.gap();
    collector.record(
        &sample,
        start + Duration::from_secs(3),
        UNIX_EPOCH + Duration::from_secs(402),
    );
    assert_eq!(
        collector
            .report(24, 402)
            .rows
            .iter()
            .find(|r| r.entity.id == CPU)
            .unwrap()
            .metrics
            .joules,
        100.0
    );
}

#[test]
fn small_wall_clock_adjustments_do_not_scale_measured_energy() {
    let directory = Directory::new();
    let mut collector = Collector::with_store(Store::open(&directory.0, 100));
    let mut sample = sample();
    sample.vitals.uptime = Duration::from_secs(100);
    let start = Instant::now();
    collector.record(&sample, start, UNIX_EPOCH + Duration::from_secs(100));
    sample.vitals.uptime += Duration::from_secs(1);
    collector.record(
        &sample,
        start + Duration::from_secs(1),
        UNIX_EPOCH + Duration::from_secs(102),
    );
    let report = collector.report(24, 102);
    let cpu = &report
        .rows
        .iter()
        .find(|r| r.entity.id == CPU)
        .unwrap()
        .metrics;
    assert_eq!(cpu.joules, 100.0);
    assert_eq!(cpu.measured_seconds, 1.0);
}
