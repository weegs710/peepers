use std::sync::Arc;
use std::thread;
use std::time::Duration;

use iced::{Size, Theme};
use iced_test::Simulator;
use peepers_core::telemetry::Sampler;

use super::*;

const WINDOW: Size = Size::new(1280.0, 800.0);

fn ui(app: &Peepers) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings::default(),
        WINDOW,
        app.view(iced::window::Id::unique()),
    )
}

/// Applies a message, dropping the task it asks for: tests have no runtime to run it.
fn update(app: &mut Peepers, message: Message) {
    drop(app.update(message));
}

fn sampled() -> Peepers {
    let mut app = Peepers::default();
    drop(app.window.open::<Message>());
    let mut sampler = Sampler::default();
    for _ in 0..30 {
        update(
            &mut app,
            Message::Sampled(Arc::new(sampler.sample().unwrap())),
        );
        thread::sleep(Duration::from_millis(30));
    }
    app
}

fn click(app: &mut Peepers, target: &str) {
    let mut simulator = ui(app);
    simulator.click(target).unwrap();
    for message in simulator.into_messages() {
        update(app, message);
    }
}

#[test]
fn the_rail_switches_pages() {
    let mut app = sampled();
    click(&mut app, "Performance");
    assert_eq!(app.page, Page::Performance);
    click(&mut app, "Processes");
    assert_eq!(app.page, Page::Processes);
}

#[test]
fn performance_lists_the_cpu_and_memory_first() {
    let mut app = sampled();
    click(&mut app, "Performance");
    let mut ui = ui(&app);
    assert!(ui.find("CPU").is_ok());
    assert!(ui.find("Memory").is_ok());
}

#[test]
fn a_failed_sample_is_reported() {
    let mut app = sampled();
    update(&mut app, Message::Failed(ErrorKind::PermissionDenied));
    let mut ui = ui(&app);
    assert!(ui.find("Sampling failed: permission denied").is_ok());
}

#[test]
fn selecting_a_process_opens_its_panel() {
    let mut app = sampled();
    let before = ui(&app).find("End task").is_ok();
    update(
        &mut app,
        Message::Processes(processes::Message::Select(std::process::id())),
    );
    assert!(!before && ui(&app).find("End task").is_ok());
}

#[test]
fn the_filter_narrows_the_table() {
    let mut app = sampled();
    update(
        &mut app,
        Message::Processes(processes::Message::Filter(
            "no process is named this".to_owned(),
        )),
    );
    assert!(ui(&app).find("0 processes").is_ok());
}

#[test]
fn the_panel_offers_a_sample_of_the_selected_process() {
    let mut app = sampled();
    update(
        &mut app,
        Message::Processes(processes::Message::Select(std::process::id())),
    );
    assert!(ui(&app).find("Sample").is_ok());
}

#[test]
fn what_a_command_came_to_is_shown_with_nothing_selected() {
    let mut app = sampled();
    update(
        &mut app,
        Message::Processes(processes::Message::Finished(Ok(
            "Started sleep 5 as 4242".to_owned()
        ))),
    );
    assert!(ui(&app).find("Started sleep 5 as 4242").is_ok());
}

#[test]
fn the_rail_shows_a_light_for_each_disk() {
    let app = sampled();
    let disk = app.history.latest().unwrap().disks[0].name.clone();
    assert!(ui(&app).find(disk).is_ok());
}

#[test]
fn settings_offers_the_tray_icon_and_says_why_it_is_missing() {
    let mut app = sampled();
    click(&mut app, "Settings");
    update(
        &mut app,
        Message::Tray(tray::Message::Started(Err(
            "No tray to show the icon in".to_owned()
        ))),
    );
    let mut ui = ui(&app);
    assert!(
        ui.find("Show a tray icon with the processor and memory load")
            .is_ok()
    );
    assert!(ui.find("No tray to show the icon in").is_ok());
}

#[test]
fn pausing_holds_the_pages_while_samples_keep_arriving() {
    let mut app = sampled();
    click(&mut app, "Pause updates");
    let held = app.history.latest().cloned();
    let mut sampler = Sampler::default();
    thread::sleep(Duration::from_millis(30));
    update(
        &mut app,
        Message::Sampled(Arc::new(sampler.sample().unwrap())),
    );
    assert_eq!(app.history.latest().cloned(), held);
    click(&mut app, "Resume updates");
    update(
        &mut app,
        Message::Sampled(Arc::new(sampler.sample().unwrap())),
    );
    assert_ne!(app.history.latest().cloned(), held);
}

fn recording(frames: usize) -> peepers_core::trace::Trace {
    let vitals = Sampler::default().sample().unwrap().vitals;
    let mut recorder = peepers_core::trace::Recorder::start(&vitals);
    for _ in 1..frames {
        thread::sleep(Duration::from_millis(5));
        recorder.record(&vitals);
    }
    recorder.finish()
}

#[test]
fn a_recording_runs_while_other_pages_are_open() {
    let mut app = sampled();
    update(&mut app, Message::Flight(flight::Message::Start));
    update(&mut app, Message::Navigate(Page::Processes));
    let mut sampler = Sampler::default();
    for _ in 0..2 {
        update(
            &mut app,
            Message::Sampled(Arc::new(sampler.sample().unwrap())),
        );
    }
    assert!(ui(&app).find("Recording, 3 samples").is_ok());
}

#[test]
fn a_loaded_recording_can_be_played_and_closed() {
    let mut app = sampled();
    update(&mut app, Message::Navigate(Page::Flight));
    update(
        &mut app,
        Message::Flight(flight::Message::Loaded(Ok(Box::new(recording(4))))),
    );
    assert!(ui(&app).find("Play").is_ok());
    click(&mut app, "Play");
    assert!(ui(&app).find("Pause").is_ok());
    click(&mut app, "Close");
    assert!(ui(&app).find("Play").is_err());
}

#[test]
fn a_failed_load_says_why() {
    let mut app = sampled();
    update(&mut app, Message::Navigate(Page::Flight));
    update(
        &mut app,
        Message::Flight(flight::Message::Loaded(Err(
            "The recording is not valid data.".to_owned(),
        ))),
    );
    assert!(ui(&app).find("The recording is not valid data.").is_ok());
}

#[test]
#[ignore = "writes PNGs for visual review into the directory named by PEEPERS_SHOTS"]
fn render_pages() {
    let directory =
        std::env::var("PEEPERS_SHOTS").expect("PEEPERS_SHOTS names the output directory");
    let mut app = sampled();
    update(
        &mut app,
        Message::Processes(processes::Message::Select(std::process::id())),
    );
    update(
        &mut app,
        Message::Processes(processes::Message::TreeView(true)),
    );
    update(
        &mut app,
        Message::Connections(connections::Message::Loaded(connections::load())),
    );
    update(
        &mut app,
        Message::Drivers(drivers::Message::Loaded(peepers_core::drivers::read())),
    );
    update(
        &mut app,
        Message::DiskSpace(disk_space::Message::Volumes(peepers_core::volumes::read())),
    );
    update(
        &mut app,
        Message::Apps(apps::Message::Loaded(peepers_core::apps::read())),
    );
    update(
        &mut app,
        Message::Startup(startup::Message::Loaded(peepers_core::startup::read())),
    );
    update(
        &mut app,
        Message::Services(services::Message::Loaded(peepers_core::systemd::read())),
    );
    update(
        &mut app,
        Message::Users(users::Message::Loaded(peepers_core::sessions::read())),
    );
    let vitals = Sampler::default().sample().unwrap().vitals;
    let (cpu, volumes) = (
        peepers_core::cpu::Info::read(),
        peepers_core::volumes::read(),
    );
    let report = peepers_core::report::Report::build(&vitals, &cpu, &volumes);
    update(
        &mut app,
        Message::Info(info::Message::Built(Box::new(report))),
    );
    update(
        &mut app,
        Message::Flight(flight::Message::Loaded(Ok(Box::new(recording(12))))),
    );
    update(
        &mut app,
        Message::Benchmarks(benchmarks::Message::Loaded(Ok(measured()))),
    );
    for page in Page::ALL {
        update(&mut app, Message::Navigate(page));
        let snapshot = ui(&app).snapshot(&Theme::Dark).unwrap();
        assert!(
            snapshot
                .matches_image(format!("{directory}/{page:?}"))
                .unwrap()
        );
    }
    update(&mut app, Message::Navigate(Page::Benchmarks));
    for section in [
        benchmarks::Section::Gpu,
        benchmarks::Section::Disk,
        benchmarks::Section::Internet,
    ] {
        update(
            &mut app,
            Message::Benchmarks(benchmarks::Message::Select(section)),
        );
        let snapshot = ui(&app).snapshot(&Theme::Dark).unwrap();
        assert!(
            snapshot
                .matches_image(format!("{directory}/Benchmarks-{section:?}"))
                .unwrap()
        );
    }
}

/// Results of every kind, with the numbers this machine produced, for the benchmarks page.
fn measured() -> peepers_core::bench::Loaded {
    use peepers_core::bench::{
        Access, Cpu, Disk, Gpu, GpuKind, History, Internet, Latency, Level, Loaded, Medium,
        Outcome, Pair,
    };
    let latency = |level, bytes, nanoseconds| Latency {
        level,
        bytes,
        nanoseconds,
    };
    let mut history = History::default();
    history.push(Outcome::Cpu(Cpu {
        threads: 16,
        integer: Pair {
            single: 15.1,
            multi: 83.8,
        },
        float: Pair {
            single: 21.7,
            multi: 204.6,
        },
        latencies: vec![
            latency(Level::L1, 16 << 10, 1.1),
            latency(Level::L2, 256 << 10, 2.7),
            latency(Level::L3, 8 << 20, 14.3),
            latency(Level::Memory, 128 << 20, 92.8),
        ],
        bandwidth: Pair {
            single: 32_436_971_064,
            multi: 44_896_804_640,
        },
    }));
    history.push(Outcome::Disk(Disk {
        filesystem: "zfs".to_owned(),
        source: "zroot/cache".to_owned(),
        medium: Medium::Storage,
        size: 1 << 30,
        direct: true,
        write: 1_068_780_544,
        read: 2_650_618_809,
        access: Access {
            median_us: 179.5,
            p99_us: 306.1,
        },
        iops: Pair {
            single: 5493,
            multi: 34_718,
        },
        parallel: 16,
    }));
    history.push(Outcome::Gpu(Gpu {
        adapter: "AMD Radeon 680M (RADV REMBRANDT)".to_owned(),
        kind: GpuKind::Integrated,
        gigaflops: 1606.9,
        bandwidth: 62_028_910_151,
    }));
    history.push(Outcome::Gpu(Gpu {
        adapter: "AMD Radeon RX 6600M (RADV NAVI23)".to_owned(),
        kind: GpuKind::Discrete,
        gigaflops: 3721.3,
        bandwidth: 160_131_936_468,
    }));
    history.push(Outcome::Internet(Internet {
        server: "New York, United States (2) (Clouvider)".to_owned(),
        latency_ms: 34.6,
        jitter_ms: 1.5,
        loss_percent: 0.0,
        download: Pair {
            single: 42_755_039,
            multi: 84_261_664,
        },
        upload: Pair {
            single: 10_341_587,
            multi: 32_466_534,
        },
        download_streams: 6,
        upload_streams: 3,
        loaded_download_ms: Some(34.2),
        loaded_upload_ms: Some(33.0),
        isp: None,
        address: None,
    }));
    Loaded {
        history,
        skipped: 0,
    }
}
