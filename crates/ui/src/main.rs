mod apps;
mod benchmarks;
mod blocking;
mod connections;
mod disk_space;
mod drivers;
mod energy;
mod flight;
mod format;
mod gpu_bench;
mod graph;
mod history;
mod icons;
mod info;
mod ipc;
mod notice;
mod performance;
mod power;
mod pressure;
mod processes;
mod selectable;
mod services;
mod settings;
mod startup;
mod storage;
mod telemetry;
mod theme;
mod tray;
mod users;
mod widgets;
mod window;

#[cfg(test)]
mod tests;

use std::env;
use std::fmt;
use std::io::ErrorKind;
use std::iter;
use std::process;
use std::sync::Arc;

use iced::time;
use iced::widget::{button, column, container, row, rule, scrollable, text};
use iced::{Element, Fill, Subscription, Task, Theme};
use peepers_core::ipc::{Command, PAGES};
use peepers_core::telemetry::Sample;
use peepers_core::tray::{Event as Click, Gauges};
use serde::{Deserialize, Serialize};

use crate::apps::Apps;
use crate::benchmarks::Benchmarks;
use crate::connections::Connections;
use crate::disk_space::DiskSpace;
use crate::drivers::Drivers;
use crate::flight::Flight;
use crate::history::History;
use crate::info::Info;
use crate::ipc::Door;
use crate::performance::Performance;
use crate::processes::Processes;
use crate::services::Services;
use crate::settings::Preferences;
use crate::startup::StartupApps;
use crate::tray::Indicator;
use crate::users::Users;
use crate::window::{Window, Windows};

const USAGE: &str = "Usage: peepers [OPTION]

Without an option this starts the application. With one it asks the running application to act:
  --toggle-pop          open the popout, or close it
  --show-pop            open the popout unless it is open
  --hide-pop            close the popout if it is open
  --toggle-window       show the main window, or hide it
  --show-window         open the main window unless it is open
  --hide-window         close the main window if it is open
  --toggle-pause        hold the numbers still, or let them move again
  --pause               hold the numbers still
  --resume              let the numbers move again
  --page NAME           show a page, opening the window if it is closed
  --flight start|stop   start or stop the flight recorder
  --quit                end the application, asking about a recording that is running
  --quit save           end it, saving a recording that is running first
  --quit discard        end it, dropping a recording that is running";

/// The command a command line asks the running application for.
fn parse(arguments: &[String]) -> Result<Command, String> {
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match arguments.as_slice() {
        ["--toggle-pop"] => Ok(Command::TogglePopout),
        ["--show-pop"] => Ok(Command::ShowPopout),
        ["--hide-pop"] => Ok(Command::HidePopout),
        ["--toggle-window"] => Ok(Command::ToggleWindow),
        ["--show-window"] => Ok(Command::ShowWindow),
        ["--hide-window"] => Ok(Command::HideWindow),
        ["--toggle-pause"] => Ok(Command::TogglePause),
        ["--pause"] => Ok(Command::Pause),
        ["--resume"] => Ok(Command::Resume),
        ["--flight", "start"] => Ok(Command::StartRecording),
        ["--flight", "stop"] => Ok(Command::StopRecording),
        ["--quit"] => Ok(Command::Quit),
        ["--quit", "save"] => Ok(Command::QuitSaving),
        ["--quit", "discard"] => Ok(Command::QuitDiscarding),
        ["--page", name] => Command::open_page(name)
            .ok_or_else(|| format!("No page called {name}. Pages: {}", PAGES.join(", "))),
        _ => Err(format!("Unknown option {}", arguments.join(" "))),
    }
}

/// What the command line asks for in place of the application, as an exit status.
fn command_line(arguments: &[String]) -> i32 {
    match parse(arguments) {
        Ok(command) => match peepers_core::ipc::send(command) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        },
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            2
        }
    }
}

fn main() -> iced::Result {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if !arguments.is_empty() {
        process::exit(command_line(&arguments));
    }
    iced::daemon(Peepers::new, Peepers::update, Peepers::view)
        .title(|_: &Peepers, _| "peepers".to_owned())
        .subscription(Peepers::subscription)
        .theme(Peepers::theme)
        .scale_factor(|app, _| app.preferences.settings.scale())
        .run()
}

#[derive(Default)]
struct Peepers {
    page: Page,
    history: History,
    energy: energy::Energy,
    collector: telemetry::Control,
    failure: Option<ErrorKind>,
    processes: Processes,
    performance: Performance,
    connections: Connections,
    drivers: Drivers,
    services: Services,
    benchmarks: Benchmarks,
    flight: Flight,
    disk_space: DiskSpace,
    apps: Apps,
    startup: StartupApps,
    users: Users,
    info: Info,
    preferences: Preferences,
    windows: Windows,
    tray: Indicator,
    door: Door,
    /// The newest reading, kept whether or not a window is open to show it.
    latest: Option<Arc<Sample>>,
    /// Whether new samples are kept from the pages, so what is on screen holds still.
    paused: bool,
    /// The application was asked to end while a recording was running, and is waiting to be told
    /// what becomes of it.
    asking_to_quit: bool,
    /// A recording is being saved, and the application ends once it is.
    quitting: bool,
}

/// What becomes of a recording that is running when the application is asked to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Save,
    Discard,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Page {
    #[default]
    Processes,
    Performance,
    Pressure,
    Power,
    Flight,
    Benchmarks,
    Connections,
    Drivers,
    Services,
    DiskSpace,
    Apps,
    Startup,
    Users,
    Info,
    Settings,
}

impl Page {
    /// The rail's sections, named for what the user does in them, in the order the rail shows them.
    const SECTIONS: [(&'static str, &'static [Self]); 5] = [
        (
            "WATCH",
            &[
                Self::Processes,
                Self::Performance,
                Self::Pressure,
                Self::Power,
            ],
        ),
        ("RECORD", &[Self::Flight, Self::Benchmarks]),
        (
            "INSPECT",
            &[
                Self::Connections,
                Self::Drivers,
                Self::DiskSpace,
                Self::Apps,
                Self::Info,
            ],
        ),
        ("MANAGE", &[Self::Services, Self::Startup, Self::Users]),
        ("PEEPERS", &[Self::Settings]),
    ];

    const ALL: [Self; 15] = [
        Self::Processes,
        Self::Performance,
        Self::Pressure,
        Self::Power,
        Self::Flight,
        Self::Benchmarks,
        Self::Connections,
        Self::Drivers,
        Self::DiskSpace,
        Self::Apps,
        Self::Info,
        Self::Services,
        Self::Startup,
        Self::Users,
        Self::Settings,
    ];

    /// The name the command line and the bus know the page by.
    fn slug(self) -> &'static str {
        match self {
            Self::Processes => "processes",
            Self::Performance => "performance",
            Self::Pressure => "pressure",
            Self::Power => "power",
            Self::Flight => "flight",
            Self::Benchmarks => "benchmarks",
            Self::Connections => "connections",
            Self::Drivers => "drivers",
            Self::DiskSpace => "disk-space",
            Self::Apps => "apps",
            Self::Info => "info",
            Self::Services => "services",
            Self::Startup => "startup",
            Self::Users => "users",
            Self::Settings => "settings",
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.slug() == slug)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Processes => "Processes",
            Self::Performance => "Performance",
            Self::Pressure => "Pressure",
            Self::Power => "Power",
            Self::Flight => "Flight recorder",
            Self::Benchmarks => "Benchmarks",
            Self::Connections => "Connections",
            Self::Drivers => "Drivers",
            Self::Services => "Services",
            Self::DiskSpace => "Disk space",
            Self::Apps => "Installed apps",
            Self::Startup => "Startup apps",
            Self::Users => "Users",
            Self::Info => "System information",
            Self::Settings => "Settings",
        }
    }
}

impl fmt::Display for Page {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone)]
enum Message {
    Energy(energy::Message),
    EnergyFlushed(Result<(), String>),
    QuitWithoutEnergySave,
    Sampled(Arc<Sample>),
    Failed(ErrorKind),
    Navigate(Page),
    Processes(processes::Message),
    Performance(performance::Message),
    Connections(connections::Message),
    Drivers(drivers::Message),
    Services(services::Message),
    Flight(flight::Message),
    Benchmarks(benchmarks::Message),
    DiskSpace(disk_space::Message),
    Apps(apps::Message),
    Startup(startup::Message),
    Users(users::Message),
    Info(info::Message),
    Settings(settings::Message),
    Tray(tray::Message),
    Ipc(ipc::Message),
    Window(window::Event),
    /// Draws the page again so a success notice leaves on time.
    Tick,
    TogglePause,
    TogglePopout,
    /// End the application, with `None` asking what to do if a recording is running.
    Quit(Option<Fate>),
    KeepRunning,
}

impl Peepers {
    fn new() -> (Self, Task<Message>) {
        let preferences = Preferences::load();
        let mut app = Self {
            page: preferences.settings.startup.page,
            preferences,
            ..Self::default()
        };
        let window = app.windows.main.open();
        let tray = app.sync_tray();
        let door = Door::open().map(Message::Ipc);
        (app, Task::batch([window, tray, door]))
    }

    /// Starts or removes the tray icon so it matches the setting, and opens the window when
    /// removing the icon would leave no way back to it.
    fn sync_tray(&mut self) -> Task<Message> {
        let tray = self
            .tray
            .sync(self.preferences.settings.startup.tray)
            .map(Message::Tray);
        if self.tray.is_active() || self.windows.main.is_open() {
            tray
        } else {
            Task::batch([tray, self.windows.main.open()])
        }
    }

    fn theme(&self, _window: iced::window::Id) -> Theme {
        self.preferences.theme()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.apply(message);
        self.door.publish(self.ipc_state());
        task
    }

    /// What other processes can read about the application.
    fn ipc_state(&self) -> peepers_core::ipc::State {
        peepers_core::ipc::State {
            paused: self.paused,
            popout_open: self.windows.popout.is_open(),
            window_open: self.windows.main.is_open(),
            page: self.page.slug(),
            recording: self.flight.is_recording(),
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.tray.set_paused(paused);
    }

    /// Changes the popout the way `change` says, and tells the tray menu how it ended up.
    fn popout(&mut self, change: fn(&mut Window) -> Task<Message>) -> Task<Message> {
        let task = change(&mut self.windows.popout);
        self.tray.set_popout_open(self.windows.popout.is_open());
        task
    }

    /// Ends the application. A recording that is running is saved or discarded first as `fate`
    /// says, and when it says nothing the window asks, because either choice could lose something.
    fn quit(&mut self, fate: Option<Fate>) -> Task<Message> {
        self.asking_to_quit = false;
        match (fate, self.flight.is_recording()) {
            (None, true) => {
                self.asking_to_quit = true;
                self.windows.main.reveal()
            }
            (Some(Fate::Save), true) => {
                self.quitting = true;
                self.update(Message::Flight(flight::Message::Stop))
            }
            (Some(Fate::Discard), true) => {
                self.flight.discard();
                self.finish()
            }
            (_, false) => self.finish(),
        }
    }

    fn energy_flushed(&mut self, result: Result<(), String>) -> Task<Message> {
        self.energy.saving = false;
        match result {
            Ok(()) => iced::exit(),
            Err(error) => {
                self.energy.save_error = Some(error);
                self.page = Page::Power;
                self.windows.main.reveal()
            }
        }
    }

    fn finish(&mut self) -> Task<Message> {
        if self.energy.saving {
            return Task::none();
        }
        self.energy.saving = true;
        let collector = self.collector.clone();
        blocking::run(move || collector.flush()).map(Message::EnergyFlushed)
    }

    /// Carries out what another process asked for.
    fn run(&mut self, command: Command) -> Task<Message> {
        match command {
            Command::TogglePopout => self.popout(Window::toggle),
            Command::ShowPopout => self.popout(Window::show),
            Command::HidePopout => self.popout(Window::hide),
            Command::ToggleWindow => self.windows.main.toggle(),
            Command::ShowWindow => self.windows.main.show(),
            Command::HideWindow => self.windows.main.hide(),
            Command::TogglePause => self.update(Message::TogglePause),
            Command::Pause => {
                self.set_paused(true);
                Task::none()
            }
            Command::Resume => {
                self.set_paused(false);
                Task::none()
            }
            Command::StartRecording => self.update(Message::Flight(flight::Message::Start)),
            Command::StopRecording => self.update(Message::Flight(flight::Message::Stop)),
            Command::Quit => self.quit(None),
            Command::QuitSaving => self.quit(Some(Fate::Save)),
            Command::QuitDiscarding => self.quit(Some(Fate::Discard)),
            Command::OpenPage(name) => match Page::from_slug(name) {
                Some(page) => {
                    let window = self.windows.main.reveal();
                    Task::batch([window, self.update(Message::Navigate(page))])
                }
                None => Task::none(),
            },
        }
    }

    fn on_sample(&mut self, sample: Arc<Sample>) {
        self.failure = None;
        self.flight.record(&sample.vitals);
        self.tray.show(Gauges::from(&sample.vitals));
        let main = self.windows.main.is_open();
        if !self.paused && (main || self.windows.popout.is_open()) {
            if main {
                self.processes.replace(&sample);
            }
            self.history.push(sample.vitals.clone());
        }
        self.latest = Some(sample);
    }

    fn apply(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Energy(message) => {
                if let energy::Message::Range(range) = &message {
                    self.collector.range(range.hours());
                }
                self.energy.update(message);
            }
            Message::EnergyFlushed(result) => return self.energy_flushed(result),
            Message::QuitWithoutEnergySave => return iced::exit(),
            Message::Sampled(sample) => self.on_sample(sample),
            Message::Failed(kind) => self.failure = Some(kind),
            Message::TogglePause => self.set_paused(!self.paused),
            Message::TogglePopout => return self.popout(Window::toggle),
            Message::Navigate(page) => {
                self.page = page;
                return self.enter();
            }
            Message::Processes(message) => {
                return self.processes.update(message).map(Message::Processes);
            }
            Message::Performance(message) => {
                return self
                    .performance
                    .update(message, &self.history)
                    .map(Message::Performance);
            }
            Message::Connections(message) => {
                return self.connections.update(message).map(Message::Connections);
            }
            Message::Drivers(message) => return self.drivers.update(message).map(Message::Drivers),
            Message::Services(message) => {
                return self.services.update(message).map(Message::Services);
            }
            Message::DiskSpace(message) => {
                return self.disk_space.update(message).map(Message::DiskSpace);
            }
            Message::Quit(fate) => return self.quit(fate),
            Message::KeepRunning => self.asking_to_quit = false,
            Message::Flight(message) => {
                if self.quitting
                    && let flight::Message::Saved(result) = &message
                {
                    if result.is_ok() {
                        return self.finish();
                    }
                    self.quitting = false;
                }
                return self
                    .flight
                    .update(message, self.latest.as_deref().map(|sample| &sample.vitals))
                    .map(Message::Flight);
            }
            Message::Benchmarks(message) => {
                return self.benchmarks.update(message).map(Message::Benchmarks);
            }
            Message::Apps(message) => return self.apps.update(message).map(Message::Apps),
            Message::Startup(message) => return self.startup.update(message).map(Message::Startup),
            Message::Users(message) => return self.users.update(message).map(Message::Users),
            Message::Info(message) => return self.info.update(message).map(Message::Info),
            Message::Tick => {}
            Message::Settings(message) => {
                let saved = self.preferences.update(message).map(Message::Settings);
                return Task::batch([saved, self.sync_tray()]);
            }
            Message::Tray(tray::Message::Started(result)) => {
                self.tray
                    .started(result, self.preferences.settings.startup.tray);
                self.tray.set_window_open(self.windows.main.is_open());
                self.tray.set_popout_open(self.windows.popout.is_open());
                self.tray.set_paused(self.paused);
            }
            Message::Tray(tray::Message::Clicked(click)) => match click {
                Click::Activate => return self.windows.main.activate(),
                Click::Window => return self.windows.main.toggle(),
                Click::Popout => return self.update(Message::TogglePopout),
                Click::Pause => return self.update(Message::TogglePause),
                Click::Quit => return self.quit(None),
            },
            Message::Ipc(ipc::Message::Started(result)) => self.door.started(result),
            Message::Ipc(ipc::Message::Received(command)) => return self.run(command),
            Message::Window(event) => {
                if let window::Event::CloseRequested(id) = event
                    && self.windows.main.owns(id)
                    && !self.tray.is_active()
                {
                    return self.quit(None);
                }
                let main = self.windows.main.update(event, self.tray.is_active());
                let popout = self.windows.popout.update(event, true);
                self.tray.set_window_open(self.windows.main.is_open());
                self.tray.set_popout_open(self.windows.popout.is_open());
                return Task::batch([main, popout]);
            }
        }
        Task::none()
    }

    /// What a page needs to load when it comes into view.
    fn enter(&self) -> Task<Message> {
        match self.page {
            Page::Processes | Page::Performance | Page::Pressure | Page::Power | Page::Settings => {
                Task::none()
            }
            Page::Connections => Connections::enter().map(Message::Connections),
            Page::Drivers => Drivers::enter().map(Message::Drivers),
            Page::Services => Services::enter().map(Message::Services),
            Page::Flight => self.flight.enter().map(Message::Flight),
            Page::Benchmarks => self.benchmarks.enter().map(Message::Benchmarks),
            Page::DiskSpace => DiskSpace::enter().map(Message::DiskSpace),
            Page::Apps => Apps::enter().map(Message::Apps),
            Page::Startup => StartupApps::enter().map(Message::Startup),
            Page::Users => Users::enter().map(Message::Users),
            Page::Info => Info::enter(self.history.latest()).map(Message::Info),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let page = match self.page {
            Page::Connections => Connections::subscription().map(Message::Connections),
            Page::Drivers => Drivers::subscription().map(Message::Drivers),
            Page::Services => Services::subscription().map(Message::Services),
            Page::DiskSpace => self.disk_space.subscription().map(Message::DiskSpace),
            Page::Flight => self.flight.subscription().map(Message::Flight),
            Page::Benchmarks => self.benchmarks.subscription().map(Message::Benchmarks),
            Page::Users => Users::subscription().map(Message::Users),
            Page::Processes
            | Page::Performance
            | Page::Pressure
            | Page::Power
            | Page::Apps
            | Page::Startup
            | Page::Info
            | Page::Settings => Subscription::none(),
        };
        let fading = match self.page {
            Page::Processes | Page::Pressure => self.processes.fading(),
            Page::Services => self.services.fading(),
            Page::Users => self.users.fading(),
            Page::Flight => self.flight.fading(),
            Page::Info => self.info.fading(),
            _ => false,
        };
        let fade = if fading {
            time::every(notice::REFRESH).map(|_| Message::Tick)
        } else {
            Subscription::none()
        };
        let interval = self.preferences.settings.sampling.interval.duration();
        Subscription::batch([
            fade,
            telemetry::subscription(&self.collector, interval),
            window::events().map(Message::Window),
            settings::keys().map(Message::Settings),
            self.preferences.subscription().map(Message::Settings),
            self.door.subscription().map(Message::Ipc),
            self.tray.subscription().map(Message::Tray),
            page,
        ])
    }

    fn power_view(&self) -> Element<'_, Message> {
        let notice = self.energy.save_error.as_ref().map(|error| {
            column![
                text(format!(
                    "Could not save energy history before quitting: {error}"
                ))
                .style(text::danger),
                row![
                    button("Retry saving and quit").on_press(Message::Quit(None)),
                    button("Quit without saving energy").on_press(Message::QuitWithoutEnergySave)
                ]
                .spacing(12),
            ]
            .spacing(12)
            .into()
        });
        power::view(
            &self.history,
            self.preferences.settings.units.temperature,
            notice,
            self.energy.view().map(Message::Energy),
        )
    }

    fn view(&self, window: iced::window::Id) -> Element<'_, Message> {
        if self.windows.popout.owns(window) {
            return performance::glance(&self.history);
        }
        let pages = column(Page::SECTIONS.iter().flat_map(|&(title, pages)| {
            iter::once(heading(title)).chain(pages.iter().map(|&page| {
                let current = page == self.page;
                button(
                    row![icons::page(page, current), text(page.label())]
                        .spacing(8)
                        .align_y(iced::Center),
                )
                .width(Fill)
                .on_press(Message::Navigate(page))
                .style(if current {
                    button::primary
                } else {
                    button::text
                })
                .into()
            }))
        }))
        .spacing(4)
        .padding(8);
        let pause = button(if self.paused {
            "Resume updates"
        } else {
            "Pause updates"
        })
        .width(Fill)
        .on_press(Message::TogglePause)
        .style(if self.paused {
            button::primary
        } else {
            button::secondary
        });
        let popout = button(if self.windows.popout.is_open() {
            "Close popout"
        } else {
            "Open popout"
        })
        .width(Fill)
        .on_press(Message::TogglePopout)
        .style(button::secondary);
        let rail = column![
            scrollable(pages).height(Fill),
            container(column![pause, popout].spacing(4)).padding(8)
        ]
        .extend(self.history.latest().map(widgets::disk_lights))
        .extend(
            self.flight
                .status()
                .map(|status| text(status).size(12).style(text::danger).into()),
        )
        .width(180);
        let content = match self.page {
            Page::Processes => self.processes.view().map(Message::Processes),
            Page::Performance => self
                .performance
                .view(&self.history, self.preferences.settings.units.temperature)
                .map(Message::Performance),
            Page::Pressure => {
                pressure::view(&self.history, &self.processes).map(Message::Processes)
            }
            Page::Power => self.power_view(),
            Page::Flight => self.flight.view().map(Message::Flight),
            Page::Benchmarks => self
                .benchmarks
                .view(
                    self.history
                        .latest()
                        .map_or(&[][..], |vitals| vitals.adapters.as_slice()),
                )
                .map(Message::Benchmarks),
            Page::Connections => self
                .connections
                .view(&self.processes)
                .map(Message::Connections),
            Page::Drivers => self.drivers.view().map(Message::Drivers),
            Page::Services => self.services.view().map(Message::Services),
            Page::DiskSpace => self.disk_space.view().map(Message::DiskSpace),
            Page::Apps => self.apps.view().map(Message::Apps),
            Page::Startup => self.startup.view().map(Message::Startup),
            Page::Users => self.users.view(&self.processes).map(Message::Users),
            Page::Info => self.info.view().map(Message::Info),
            Page::Settings => self
                .preferences
                .view(self.tray.error())
                .map(Message::Settings),
        };

        column![]
            .extend(self.asking_to_quit.then(quit_prompt))
            .push(row![rail, content].height(Fill))
            .extend(
                self.failure
                    .map(|kind| text!("Sampling failed: {kind}").style(text::danger).into()),
            )
            .into()
    }
}

/// What the window asks when the application is to end with a recording still running.
fn quit_prompt() -> Element<'static, Message> {
    container(
        row![
            text("A recording is running. Save it before quitting?").style(text::danger),
            button("Save and quit")
                .on_press(Message::Quit(Some(Fate::Save)))
                .style(button::success),
            button("Discard and quit")
                .on_press(Message::Quit(Some(Fate::Discard)))
                .style(button::danger),
            button("Keep running")
                .on_press(Message::KeepRunning)
                .style(button::secondary),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding(8)
    .width(Fill)
    .style(container::bordered_box)
    .into()
}

/// A rail section's name with a rule running on from it to the edge.
fn heading(title: &str) -> Element<'_, Message> {
    container(
        row![
            text(title).size(11).style(text::secondary),
            rule::horizontal(1)
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .padding(iced::Padding::ZERO.top(8))
    .into()
}
