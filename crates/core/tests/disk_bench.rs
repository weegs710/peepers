use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use peepers_core::bench::{Event, Job, Outcome, Run};

fn directory(name: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    drop(fs::remove_dir_all(&directory));
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn finish(run: &Run) -> Result<Outcome, String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        for event in run.poll() {
            if let Event::Finished(result) = event {
                return result;
            }
        }
        assert!(Instant::now() < deadline, "benchmark did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_disk_run_measures_everything_and_leaves_nothing_behind() {
    let directory = directory("disk-bench");
    let run = Run::start(Job::Disk {
        directory: directory.clone(),
        size: 8 << 20,
    });
    let Outcome::Disk(disk) = finish(&run).unwrap() else {
        panic!("expected a disk result")
    };
    assert!(disk.write > 0 && disk.read > 0);
    assert!(disk.iops.single > 0 && disk.iops.multi > 0);
    assert!(disk.access.median_us > 0.0 && disk.access.p99_us >= disk.access.median_us);
    assert_eq!((disk.size, disk.parallel), (8 << 20, 16));
    assert!(!disk.filesystem.is_empty());
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
}

#[test]
fn a_run_without_the_space_fails_before_writing() {
    let directory = directory("disk-bench-space");
    let run = Run::start(Job::Disk {
        directory: directory.clone(),
        size: 1 << 60,
    });
    assert!(finish(&run).unwrap_err().contains("free"));
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
}

#[test]
fn a_stopped_run_removes_its_file_and_reports_the_stop() {
    let directory = directory("disk-bench-stop");
    let run = Run::start(Job::Disk {
        directory: directory.clone(),
        size: 512 << 20,
    });
    run.stop();
    assert!(finish(&run).is_err());
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
}
