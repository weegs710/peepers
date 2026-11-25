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
    assert_eq!(fallback.id, "application/application");
    assert_eq!(
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

#[test]
fn launch_ids_and_executable_versions_share_one_application() {
    use attribution::group;
    use std::path::Path;
    let expected = group(1000, "umbriel", None, "");
    for scope in [
        "app-umbriel-1306-2.scope",
        "app-umbriel-4788-abcdef123456.scope",
        "app-umbriel-1297.scope",
        "app-umbriel@123.service",
    ] {
        assert_eq!(
            group(1000, "worker", None, &format!("0::/app.slice/{scope}")),
            expected
        );
    }
    for executable in ["/nix/store/old/bin/umbriel", "/nix/store/new/bin/umbriel"] {
        assert_eq!(
            group(1000, "umbriel", Some(Path::new(executable)), ""),
            expected
        );
    }
    assert_eq!(group(0, "umbriel", None, "").id, expected.id);
    assert_ne!(group(1000, "other", None, "").id, expected.id);
}

#[test]
fn simultaneous_processes_and_restarts_record_one_group_interval() {
    let mut sample = sample();
    sample.processes[0].name = "umbriel".into();
    let mut second = sample.processes[0].clone();
    second.pid -= 1;
    second.uid = 30001;
    sample.processes.push(second);
    let mut attributor = attribution::Attributor::default();
    let directory = Directory::new();
    let mut store = Store::open(&directory.0, 100);
    let readings = attributor.attribute(&sample, Some(100.0));
    store.record(10.0, 20.0, &readings, &BTreeSet::new());
    let active = readings
        .iter()
        .filter(|r| r.active)
        .map(|r| r.entity.id.clone())
        .collect();
    sample.processes.remove(0);
    sample.processes[0].pid -= 1;
    let readings = attributor.attribute(&sample, Some(100.0));
    store.record(20.0, 30.0, &readings, &active);
    store.flush().unwrap();
    drop(store);
    let report = Store::open(&directory.0, 100).report(24, 100);
    let apps: Vec<_> = report
        .rows
        .iter()
        .filter(|r| r.entity.kind == Kind::Application)
        .collect();
    assert_eq!(apps.len(), 1);
    let metrics = &apps[0].metrics;
    assert_eq!(metrics.joules, 750.0);
    assert_eq!(metrics.measured_seconds, 20.0);
    assert_eq!(metrics.observed_seconds, 20.0);
    assert_eq!(metrics.active_seconds, 20.0);
    assert_eq!(metrics.average_watts(), Some(37.5));
    assert_eq!(metrics.peak_watts, Some(50.0));
    assert_eq!(metrics.bursts, 1);
    assert!(report.warning.is_none());
}

#[test]
fn saved_launch_entries_combine_in_rankings_and_timelines_without_rewriting_history() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0, 7200);
    let observations: Vec<_> = [
        "app/1000/umbriel-1306",
        "app/30001/umbriel-4788",
        "exe/1000//nix/store/old/bin/umbriel",
        "application/umbriel",
    ]
    .into_iter()
    .map(|id| Observation {
        entity: Entity::new(id, "old label", Kind::Application),
        watts: Some(10.0),
        active: true,
    })
    .collect();
    store.record(3590.0, 3610.0, &observations, &BTreeSet::new());
    store.flush().unwrap();
    drop(store);
    let original = fs::read(directory.0.join("0.json")).unwrap();
    let store = Store::open(&directory.0, 7200);
    for hours in [24, 7 * 24, 30 * 24, 90 * 24] {
        let report = store.report(hours, 7200);
        assert_eq!(report.rows.len(), 1);
        let row = &report.rows[0];
        assert_eq!(row.entity.id, "application/umbriel");
        assert_eq!(row.entity.name, "umbriel");
        assert_eq!(row.metrics.joules, 800.0);
        assert_eq!(row.metrics.observed_seconds, 20.0);
        assert_eq!(row.metrics.measured_seconds, 20.0);
        assert_eq!(row.metrics.active_seconds, 20.0);
        assert_eq!(
            report
                .periods
                .iter()
                .filter_map(|p| p.rows.get(&row.entity.id))
                .map(|r| r.metrics.joules)
                .sum::<f64>(),
            800.0
        );
        assert!(report.warning.unwrap().contains("overlap timing"));
    }
    assert_eq!(fs::read(directory.0.join("0.json")).unwrap(), original);
}

fn many(count: usize, name_bytes: usize) -> Vec<Observation> {
    (0..count)
        .map(|index| Observation {
            entity: Entity::new(
                format!("application/{index}-{}", "x".repeat(name_bytes)),
                format!("{index}"),
                Kind::Application,
            ),
            watts: Some(1.0),
            active: true,
        })
        .collect()
}

#[test]
fn an_hour_holds_a_bounded_number_of_rows_and_still_loads() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0, 7200);
    store.record(3600.0, 3610.0, &many(5000, 0), &BTreeSet::new());
    store.record(3610.0, 3620.0, &many(5000, 0), &BTreeSet::new());
    store.flush().unwrap();
    let rows = Store::open(&directory.0, 7200).report(1, 3700).rows;
    assert!(rows.len() <= 4097, "{} rows in one hour", rows.len());
    let joules: f64 = rows.iter().map(|row| row.metrics.joules).sum();
    assert_eq!(joules, 2.0 * 5000.0 * 10.0);
}

#[test]
fn an_hour_too_big_to_save_does_not_stop_later_hours_saving() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0, 10_800);
    store.record(3600.0, 3610.0, &many(5000, 8 << 10), &BTreeSet::new());
    store.record(
        7200.0,
        7210.0,
        &[reading(Some(5.0), true)],
        &BTreeSet::new(),
    );
    assert!(store.flush().is_err());
    assert!(
        directory.0.join("7200.json").exists(),
        "the later hour was not saved"
    );
    assert!(
        !directory.0.join("3600.partial").exists(),
        "the rejected hour left a partial file"
    );
}
