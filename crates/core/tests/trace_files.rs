use std::fs;
use std::path::PathBuf;

use peepers_core::telemetry::Sampler;
use peepers_core::trace::{Invalid, LoadError, Recorder, Trace};

fn directory(name: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    drop(fs::remove_dir_all(&directory));
    directory
}

fn recording() -> Trace {
    let vitals = Sampler::default().sample().unwrap().vitals;
    let mut recorder = Recorder::start(&vitals);
    // a clock as coarse as some arm64 boards' reads no time passing without it, and loading refuses that
    std::thread::sleep(std::time::Duration::from_millis(2));
    recorder.record(&vitals);
    recorder.finish()
}

#[test]
fn a_saved_trace_loads_back_identical() {
    let directory = directory("round-trip");
    let trace = recording();
    let path = trace.save(&directory, "run").unwrap();
    assert_eq!(Trace::load(&path).unwrap(), trace);
}

#[test]
fn a_saved_file_measures_what_the_recorder_said_it_would() {
    let directory = directory("size");
    let vitals = Sampler::default().sample().unwrap().vitals;
    let mut recorder = Recorder::start(&vitals);
    recorder.record(&vitals);
    recorder.record(&vitals);
    let size = recorder.size();
    let path = recorder.finish().save(&directory, "run").unwrap();
    assert_eq!(fs::metadata(path).unwrap().len(), size);
}

#[test]
fn saving_never_replaces_an_existing_recording() {
    let directory = directory("no-overwrite");
    let trace = recording();
    let first = trace.save(&directory, "run").unwrap();
    let second = trace.save(&directory, "run").unwrap();
    let third = trace.save(&directory, "run").unwrap();
    assert_eq!(
        [first, second, third].map(|path| path.file_name().unwrap().to_string_lossy().into_owned()),
        ["run.json", "run-2.json", "run-3.json"]
    );
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 3);
}

#[test]
fn files_that_are_not_recordings_are_refused() {
    let directory = directory("refused");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("junk.json");
    fs::write(&path, "{\"hello\": 1}").unwrap();
    assert!(matches!(Trace::load(&path), Err(LoadError::Json(_))));
    fs::write(
        &path,
        serde_json::to_string(&Trace {
            frames: Vec::new(),
            ..recording()
        })
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        Trace::load(&path),
        Err(LoadError::Invalid(Invalid::NoFrames))
    ));
    assert!(matches!(
        Trace::load(&directory.join("missing.json")),
        Err(LoadError::Io(_))
    ));
}

#[test]
fn files_planted_at_the_names_a_save_would_use_are_stepped_over_and_left_alone() {
    let directory = directory("planted");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("run.json.partial"), "planted").unwrap();
    std::os::unix::fs::symlink(directory.join("absent"), directory.join("run.json")).unwrap();
    let trace = recording();
    let path = trace.save(&directory, "run").unwrap();
    assert_eq!(path.file_name().unwrap(), "run-2.json");
    assert_eq!(Trace::load(&path).unwrap(), trace);
    assert_eq!(
        fs::read_to_string(directory.join("run.json.partial")).unwrap(),
        "planted"
    );
    assert!(
        fs::symlink_metadata(directory.join("run.json"))
            .unwrap()
            .is_symlink()
    );
}
