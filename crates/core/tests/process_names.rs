use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use peepers_core::process::{Observation, Sampler};

#[test]
fn a_process_whose_name_is_not_utf8_is_still_listed() {
    let mut child = Command::new("sh")
        .args(["-c", r"printf '\377peepers' > /proc/self/comm; read -r _"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let comm = format!("/proc/{}/comm", child.id());
    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::read(&comm).unwrap() != b"\xffpeepers\n" {
        assert!(Instant::now() < deadline, "the child never renamed itself");
        thread::sleep(Duration::from_millis(10));
    }
    let listed = Sampler::default()
        .sample()
        .unwrap()
        .into_iter()
        .find(|process| process.pid == child.id());
    let observed = Observation::capture(child.id(), Duration::from_millis(20)).unwrap();
    drop(child.stdin.take());
    child.wait().unwrap();
    assert_eq!(
        listed.map(|process| process.name),
        Some("\u{fffd}peepers".to_owned())
    );
    assert_eq!(observed.name, "\u{fffd}peepers");
}
