use std::sync::Arc;
use std::thread;
use std::time::Duration;

use iced::{Size, Theme};
use iced_test::Simulator;
use peepers_core::health::{Ata, Health, Nvme};
use peepers_core::telemetry::Sampler;

use super::*;

const WINDOW: Size = Size::new(1280.0, 800.0);
const POPOUT: Size = Size::new(300.0, 560.0);

fn ui(app: &Peepers) -> Simulator<'_, Message> {
    ui_sized(app, WINDOW)
}

fn ui_sized(app: &Peepers, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings::default(),
        size,
        app.view(iced::window::Id::unique()),
    )
}

/// The popout as it looks once it is open.
fn popout(app: &mut Peepers) -> Simulator<'_, Message> {
    update(app, Message::TogglePopout);
    let id = app.windows.popout.id().unwrap();
    Simulator::with_size(iced::Settings::default(), POPOUT, app.view(id))
}

/// Applies a message, dropping the task it asks for: tests have no runtime to run it.
fn update(app: &mut Peepers, message: Message) {
    drop(app.update(message));
}

fn sampled() -> Peepers {
    let mut app = Peepers::default();
    drop(app.windows.main.open::<Message>());
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
fn the_sections_hold_every_page_once_in_the_order_of_the_full_list() {
    let shown: Vec<Page> = Page::SECTIONS
        .iter()
        .flat_map(|(_, pages)| pages.iter().copied())
        .collect();
    assert_eq!(shown, Page::ALL);
}

#[test]
fn the_rail_names_what_each_group_of_pages_is_for() {
    let app = sampled();
    let mut ui = ui(&app);
    for (title, _) in Page::SECTIONS {
        assert!(ui.find(title).is_ok(), "{title}");
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
fn choosing_a_process_on_the_pressure_page_shows_its_panel_below_the_lists() {
    let mut app = sampled();
    update(&mut app, Message::Navigate(Page::Pressure));
    assert!(ui(&app).find("Top storage users").is_ok());
    assert!(ui(&app).find("End task").is_err());
    update(
        &mut app,
        Message::Processes(processes::Message::Select(std::process::id())),
    );
    assert!(ui(&app).find("End task").is_ok());
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

#[test]
fn the_popout_lists_every_device_and_nothing_else() {
    let mut app = sampled();
    let mut ui = popout(&mut app);
    assert!(ui.find("CPU").is_ok() && ui.find("Memory").is_ok());
    assert!(ui.find("Processes").is_err());
}

#[test]
fn the_rail_opens_and_closes_the_popout() {
    let mut app = sampled();
    click(&mut app, "Open popout");
    assert!(app.windows.popout.is_open());
    assert!(ui(&app).find("Close popout").is_ok());
}

#[test]
fn a_command_from_another_process_opens_and_closes_the_popout() {
    let mut app = sampled();
    let toggle = || Message::Ipc(ipc::Message::Received(Command::TogglePopout));
    update(&mut app, toggle());
    assert!(app.windows.popout.is_open());
    let id = app.windows.popout.id().unwrap();
    update(&mut app, toggle());
    update(&mut app, Message::Window(window::Event::Closed(id)));
    assert!(!app.windows.popout.is_open());
}

fn sent(app: &mut Peepers, command: Command) {
    update(app, Message::Ipc(ipc::Message::Received(command)));
}

#[test]
fn another_process_can_show_or_hide_the_popout_without_toggling_it_back() {
    let mut app = sampled();
    sent(&mut app, Command::HidePopout);
    assert!(!app.windows.popout.is_open());
    sent(&mut app, Command::ShowPopout);
    let id = app.windows.popout.id().unwrap();
    sent(&mut app, Command::ShowPopout);
    assert_eq!(app.windows.popout.id(), Some(id));
    sent(&mut app, Command::HidePopout);
    update(&mut app, Message::Window(window::Event::Closed(id)));
    assert!(!app.windows.popout.is_open());
}

#[test]
fn showing_the_popout_while_it_is_closing_opens_it_again() {
    let mut app = sampled();
    sent(&mut app, Command::ShowPopout);
    let first = app.windows.popout.id().unwrap();
    sent(&mut app, Command::HidePopout);
    sent(&mut app, Command::ShowPopout);
    assert_ne!(app.windows.popout.id(), Some(first));
    update(&mut app, Message::Window(window::Event::Closed(first)));
    assert!(app.windows.popout.is_open());
}

#[test]
fn another_process_can_hide_and_show_the_main_window() {
    let mut app = sampled();
    let id = app.windows.main.id().unwrap();
    sent(&mut app, Command::ToggleWindow);
    update(&mut app, Message::Window(window::Event::Closed(id)));
    assert!(!app.windows.main.is_open());
    sent(&mut app, Command::ToggleWindow);
    assert!(app.windows.main.is_open());
}

#[test]
fn another_process_can_show_or_hide_the_main_window_without_toggling_it_back() {
    let mut app = sampled();
    let id = app.windows.main.id().unwrap();
    sent(&mut app, Command::ShowWindow);
    assert_eq!(app.windows.main.id(), Some(id));
    sent(&mut app, Command::HideWindow);
    sent(&mut app, Command::HideWindow);
    update(&mut app, Message::Window(window::Event::Closed(id)));
    assert!(!app.windows.main.is_open());
    sent(&mut app, Command::HideWindow);
    assert!(!app.windows.main.is_open());
    sent(&mut app, Command::ShowWindow);
    assert!(app.windows.main.is_open());
}

#[test]
fn another_process_can_toggle_the_pause() {
    let mut app = sampled();
    sent(&mut app, Command::TogglePause);
    assert!(app.paused);
    sent(&mut app, Command::TogglePause);
    assert!(!app.paused);
}

#[test]
fn pausing_and_resuming_from_another_process_do_not_toggle() {
    let mut app = sampled();
    sent(&mut app, Command::Pause);
    sent(&mut app, Command::Pause);
    assert!(app.paused);
    sent(&mut app, Command::Resume);
    sent(&mut app, Command::Resume);
    assert!(!app.paused);
}

#[test]
fn another_process_can_record_with_no_window_open() {
    let mut app = Peepers::default();
    update(
        &mut app,
        Message::Sampled(Arc::new(Sampler::default().sample().unwrap())),
    );
    sent(&mut app, Command::StartRecording);
    assert!(app.flight.status().is_some());
    sent(&mut app, Command::StopRecording);
    assert!(app.flight.status().is_none());
}

#[test]
fn another_process_can_open_a_page_in_a_closed_window() {
    let mut app = Peepers::default();
    sent(&mut app, Command::OpenPage("services"));
    assert_eq!(app.page, Page::Services);
    assert!(app.windows.main.is_open());
}

#[test]
fn the_bus_names_the_pages_as_the_application_does() {
    let slugs: Vec<_> = Page::ALL.iter().map(|page| page.slug()).collect();
    assert_eq!(slugs, PAGES);
    for page in Page::ALL {
        assert_eq!(Page::from_slug(page.slug()), Some(page));
    }
    assert_eq!(Page::from_slug("nonsense"), None);
}

#[test]
fn quitting_with_nothing_recording_ends_the_application() {
    let mut app = sampled();
    assert_eq!(app.update(Message::Quit(None)).units(), 1);
    assert!(!app.asking_to_quit);
}

#[test]
fn quitting_while_recording_asks_first_and_keeps_the_recording() {
    let mut app = sampled();
    update(&mut app, Message::Flight(flight::Message::Start));
    drop(app.update(Message::Quit(None)));
    assert!(app.asking_to_quit && app.flight.is_recording());
    assert!(ui(&app).find("Save and quit").is_ok());
    assert!(ui(&app).find("Discard and quit").is_ok());
    click(&mut app, "Keep running");
    assert!(!app.asking_to_quit && app.flight.is_recording());
    assert!(ui(&app).find("Save and quit").is_err());
}

#[test]
fn discarding_ends_the_application_with_nothing_saved() {
    let mut app = sampled();
    update(&mut app, Message::Flight(flight::Message::Start));
    assert_eq!(app.update(Message::Quit(Some(Fate::Discard))).units(), 1);
    assert!(!app.flight.is_recording());
}

#[test]
fn saving_waits_for_the_file_before_ending() {
    let mut app = sampled();
    update(&mut app, Message::Flight(flight::Message::Start));
    drop(app.update(Message::Quit(Some(Fate::Save))));
    assert!(app.quitting && !app.flight.is_recording());
    let saved = Message::Flight(flight::Message::Saved(Ok("/recordings/a.json".into())));
    assert_eq!(app.update(saved).units(), 1);
}

#[test]
fn a_recording_that_cannot_be_saved_keeps_the_application_open() {
    let mut app = sampled();
    update(&mut app, Message::Flight(flight::Message::Start));
    drop(app.update(Message::Quit(Some(Fate::Save))));
    let failed = Message::Flight(flight::Message::Saved(Err("the disk is full".to_owned())));
    drop(app.update(failed));
    assert!(!app.quitting);
}

#[test]
fn closing_the_last_window_with_a_recording_running_asks_first() {
    let mut app = sampled();
    update(&mut app, Message::Flight(flight::Message::Start));
    let id = app.windows.main.id().unwrap();
    update(&mut app, Message::Window(window::Event::CloseRequested(id)));
    assert!(app.asking_to_quit);
}

#[test]
fn another_process_can_answer_the_question_a_running_recording_raises() {
    let mut app = sampled();
    update(&mut app, Message::Flight(flight::Message::Start));
    sent(&mut app, Command::Quit);
    assert!(app.asking_to_quit);
    sent(&mut app, Command::QuitSaving);
    assert!(app.quitting && !app.asking_to_quit && !app.flight.is_recording());
}

#[test]
fn another_process_can_discard_a_running_recording_and_end() {
    let mut app = sampled();
    update(&mut app, Message::Flight(flight::Message::Start));
    let discard = Message::Ipc(ipc::Message::Received(Command::QuitDiscarding));
    assert_eq!(app.update(discard).units(), 1);
    assert!(!app.flight.is_recording());
}

#[test]
fn what_other_processes_read_follows_the_application() {
    let mut app = sampled();
    update(&mut app, Message::Navigate(Page::Power));
    update(&mut app, Message::TogglePause);
    let state = app.ipc_state();
    assert!(state.paused && state.window_open);
    assert!(!state.popout_open && !state.recording);
    assert_eq!(state.page, "power");
    sent(&mut app, Command::StartRecording);
    assert!(app.ipc_state().recording);
}

#[test]
fn the_command_line_names_what_it_asks_for() {
    let asks = |arguments: &[&str]| {
        parse(
            &arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(asks(&["--toggle-pop"]), Ok(Command::TogglePopout));
    assert_eq!(asks(&["--show-pop"]), Ok(Command::ShowPopout));
    assert_eq!(asks(&["--hide-pop"]), Ok(Command::HidePopout));
    assert_eq!(asks(&["--toggle-window"]), Ok(Command::ToggleWindow));
    assert_eq!(asks(&["--show-window"]), Ok(Command::ShowWindow));
    assert_eq!(asks(&["--hide-window"]), Ok(Command::HideWindow));
    assert_eq!(asks(&["--toggle-pause"]), Ok(Command::TogglePause));
    assert_eq!(asks(&["--pause"]), Ok(Command::Pause));
    assert_eq!(asks(&["--resume"]), Ok(Command::Resume));
    assert_eq!(asks(&["--flight", "start"]), Ok(Command::StartRecording));
    assert_eq!(asks(&["--flight", "stop"]), Ok(Command::StopRecording));
    assert_eq!(asks(&["--quit"]), Ok(Command::Quit));
    assert_eq!(asks(&["--quit", "save"]), Ok(Command::QuitSaving));
    assert_eq!(asks(&["--quit", "discard"]), Ok(Command::QuitDiscarding));
    assert_eq!(
        asks(&["--page", "disk-space"]),
        Ok(Command::OpenPage("disk-space"))
    );
}

#[test]
fn a_command_line_that_asks_for_nothing_real_is_refused() {
    let refused = |arguments: &[&str]| {
        parse(
            &arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap_err()
    };
    assert!(refused(&["--page", "nonsense"]).contains("disk-space"));
    assert!(refused(&["--page"]).starts_with("Unknown option"));
    assert!(refused(&["--flight", "pause"]).starts_with("Unknown option"));
    assert!(refused(&["--quit", "now"]).starts_with("Unknown option"));
    assert!(refused(&["--quit", "save", "discard"]).starts_with("Unknown option"));
    assert!(refused(&["--nonsense"]).starts_with("Unknown option"));
}

#[test]
fn the_popout_keeps_the_history_going_with_the_main_window_closed() {
    let mut app = sampled();
    update(&mut app, Message::TogglePopout);
    let main = app.windows.main.id().unwrap();
    update(&mut app, Message::Window(window::Event::Closed(main)));
    let before = app.history.series(|_| 0.0).len();
    update(
        &mut app,
        Message::Sampled(Arc::new(Sampler::default().sample().unwrap())),
    );
    assert_eq!(app.history.series(|_| 0.0).len(), before + 1);
}

#[test]
fn a_recording_starts_from_the_newest_sample_with_no_window_open() {
    let mut app = Peepers::default();
    update(
        &mut app,
        Message::Sampled(Arc::new(Sampler::default().sample().unwrap())),
    );
    assert!(app.history.latest().is_none());
    update(&mut app, Message::Flight(flight::Message::Start));
    assert!(app.flight.status().is_some());
}

#[test]
fn recordings_are_listed_and_deleting_asks_before_it_deletes() {
    let mut app = sampled();
    update(&mut app, Message::Navigate(Page::Flight));
    let files = [
        "peepers-2026-10-16-1148-12min-gaming",
        "peepers-2026-10-15-0930-3min",
    ];
    update(
        &mut app,
        Message::Flight(flight::Message::Files(
            files
                .map(|name| format!("/recordings/{name}.json").into())
                .to_vec(),
        )),
    );
    assert!(ui(&app).find("2 recordings").is_ok());
    assert!(ui(&app).find(format!("{}.json", files[0])).is_ok());
    click(&mut app, "Delete all");
    assert!(ui(&app).find("Delete all 2 recordings?").is_ok());
    click(&mut app, "Keep");
    assert!(ui(&app).find("Delete all").is_ok());
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

/// The first `NVMe` disk of this machine opened on the Performance page, if it has one.
fn nvme_disk() -> Option<(Peepers, String)> {
    let mut app = sampled();
    let name = app
        .history
        .latest()?
        .disks
        .iter()
        .find(|disk| disk.kind == peepers_core::disk::Kind::Nvme)?
        .name
        .clone();
    update(&mut app, Message::Navigate(Page::Performance));
    update(
        &mut app,
        Message::Performance(performance::Message::Select(performance::Device::Disk(
            name.clone(),
        ))),
    );
    Some((app, name))
}

fn health(percent_used: u8, media_errors: u64) -> Health {
    Health::Nvme(Nvme {
        percent_used,
        spare: 100,
        spare_threshold: 10,
        bytes_read: Some(13_935_985_664_000),
        bytes_written: Some(27_993_332_736_000),
        power_on_hours: 425,
        power_cycles: 623,
        unsafe_shutdowns: 71,
        media_errors,
        warnings: Vec::new(),
    })
}

fn sata_health(bad_sectors: u64) -> Health {
    Health::Ata(Ata {
        failing: false,
        attributes_failing: Some(0),
        attributes_failed_before: Some(1),
        bad_sectors: Some(bad_sectors),
        power_on_hours: Some(12_000),
        self_test: "success".to_owned(),
    })
}

fn report(app: &mut Peepers, name: &str, result: Result<Health, String>) {
    update(
        app,
        Message::Performance(performance::Message::Health(name.to_owned(), result)),
    );
}

#[test]
fn an_nvme_disk_shows_its_health() {
    let Some((mut app, name)) = nvme_disk() else {
        return;
    };
    assert!(ui(&app).find("Reading the drive's health").is_ok());
    report(&mut app, &name, Ok(health(2, 0)));
    let mut ui = ui(&app);
    assert!(ui.find("No problems reported").is_ok());
    assert!(ui.find("2% used").is_ok());
    assert!(ui.find("Unsafe shutdowns").is_ok());
}

#[test]
fn a_worn_drive_says_what_is_wrong() {
    let Some((mut app, name)) = nvme_disk() else {
        return;
    };
    report(&mut app, &name, Ok(health(96, 4)));
    let mut ui = ui(&app);
    assert!(
        ui.find("4 media errors, 96% of its rated endurance is used")
            .is_ok()
    );
    assert!(ui.find("No problems reported").is_err());
}

#[test]
fn a_sata_drive_shows_its_smart_summary() {
    let Some((mut app, name)) = nvme_disk() else {
        return;
    };
    report(&mut app, &name, Ok(sata_health(0)));
    {
        let mut view = ui(&app);
        assert!(view.find("No problems reported").is_ok());
        assert!(view.find("12000 hours").is_ok());
        assert!(view.find("Bad sectors").is_ok());
        assert!(view.find("Last self-test").is_ok());
    }

    report(&mut app, &name, Ok(sata_health(8)));
    assert!(ui(&app).find("8 bad sectors").is_ok());
}

#[test]
fn a_drive_that_cannot_be_read_says_why() {
    let Some((mut app, name)) = nvme_disk() else {
        return;
    };
    report(&mut app, &name, Err("udisks is not running".to_owned()));
    assert!(
        ui(&app)
            .find("Health unavailable: udisks is not running")
            .is_ok()
    );
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
    render_sizes(&mut app, &directory);
    render_devices(&mut app, &directory);
    render_recordings(&mut app, &directory);
    render_health(&directory);
    let snapshot = popout(&mut app).snapshot(&Theme::Dark).unwrap();
    assert!(
        snapshot
            .matches_image(format!("{directory}/Popout"))
            .unwrap()
    );
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

/// The pages that lay cells out across the window, tiled wide and floated narrow.
fn render_sizes(app: &mut Peepers, directory: &str) {
    for size in [Size::new(1354.0, 1026.0), Size::new(992.0, 822.0)] {
        for page in [Page::Performance, Page::Pressure, Page::Power, Page::Flight] {
            update(app, Message::Navigate(page));
            let snapshot = ui_sized(app, size).snapshot(&Theme::Dark).unwrap();
            assert!(
                snapshot
                    .matches_image(format!("{directory}/{page:?}-{}", size.width))
                    .unwrap()
            );
        }
    }
}

/// The detail pages with the fewest facts, which have the most room to waste.
fn render_devices(app: &mut Peepers, directory: &str) {
    let gpu = app
        .history
        .latest()
        .and_then(|vitals| vitals.adapters.first())
        .map(|adapter| performance::Device::Gpu(adapter.card.clone()));
    update(app, Message::Navigate(Page::Performance));
    for (file, device) in [
        Some(("Memory", performance::Device::Memory)),
        gpu.map(|gpu| ("Gpu", gpu)),
    ]
    .into_iter()
    .flatten()
    {
        update(
            app,
            Message::Performance(performance::Message::Select(device)),
        );
        let snapshot = ui_sized(app, Size::new(1354.0, 1026.0))
            .snapshot(&Theme::Dark)
            .unwrap();
        assert!(
            snapshot
                .matches_image(format!("{directory}/Performance-{file}"))
                .unwrap()
        );
    }
    update(
        app,
        Message::Performance(performance::Message::Select(performance::Device::Cpu)),
    );
}

/// The disk page of an `NVMe` drive in good health, in trouble, and with no reading.
fn render_health(directory: &str) {
    let readings = [
        ("Health-Good", Ok(health(2, 0))),
        ("Health-Worn", Ok(health(96, 4))),
        ("Health-Sata", Ok(sata_health(8))),
        (
            "Health-Unavailable",
            Err("udisks is not running".to_owned()),
        ),
    ];
    for (file, result) in readings {
        let Some((mut app, name)) = nvme_disk() else {
            return;
        };
        report(&mut app, &name, result);
        let snapshot = ui(&app).snapshot(&Theme::Dark).unwrap();
        assert!(
            snapshot
                .matches_image(format!("{directory}/{file}"))
                .unwrap()
        );
    }
}

/// The recordings list with one deletion waiting for its answer.
fn render_recordings(app: &mut Peepers, directory: &str) {
    update(app, Message::Flight(flight::Message::Close));
    update(
        app,
        Message::Flight(flight::Message::Files(
            [
                "peepers-2026-10-16-1148-12min-gaming",
                "peepers-2026-10-15-0930-3min",
                "peepers-1792122032",
            ]
            .map(|name| format!("/recordings/{name}.json").into())
            .to_vec(),
        )),
    );
    update(
        app,
        Message::Flight(flight::Message::Ask(flight::Doomed::One(
            "/recordings/peepers-2026-10-15-0930-3min.json".into(),
        ))),
    );
    update(app, Message::Navigate(Page::Flight));
    let snapshot = ui(app).snapshot(&Theme::Dark).unwrap();
    assert!(
        snapshot
            .matches_image(format!("{directory}/Flight-List"))
            .unwrap()
    );
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
