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
    recorder.record(&vitals);
    recorder.finish()
}

#[test]
fn a_saved_trace_loads_back_identical() {
    let directory = directory("round-trip");
    let trace = recording();
    let path = trace.save(&directory).unwrap();
    assert_eq!(Trace::load(&path).unwrap(), trace);
}

#[test]
fn saving_never_replaces_an_existing_recording() {
    let directory = directory("no-overwrite");
    let trace = recording();
    trace.save(&directory).unwrap();
    let error = trace.save(&directory).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
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
