mod apps;
mod benchmarks;
mod blocking;
mod connections;
mod disk_space;
mod drivers;
mod flight;
mod format;
mod gpu_bench;
mod graph;
mod history;
mod info;
mod performance;
mod power;
mod pressure;
mod processes;
mod services;
mod settings;
mod startup;
mod storage;
mod telemetry;
mod tray;
mod users;
mod widgets;
mod window;

#[cfg(test)]
mod tests;

use std::fmt;
use std::io::ErrorKind;
use std::iter;
use std::sync::Arc;

use iced::widget::{button, column, container, row, rule, scrollable, text};
use iced::{Element, Fill, Subscription, Task, Theme};
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
use crate::performance::Performance;
use crate::processes::Processes;
use crate::services::Services;
use crate::settings::Preferences;
use crate::startup::StartupApps;
use crate::tray::Indicator;
use crate::users::Users;
use crate::window::Windows;

fn main() -> iced::Result {
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
    /// Whether new samples are kept from the pages, so what is on screen holds still.
    paused: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    Window(window::Event),
    TogglePause,
    TogglePopout,
}

impl Peepers {
    fn new() -> (Self, Task<Message>) {
        let preferences = Preferences::load();
        let mut app = Self {
            page: preferences.settings.start_page,
            preferences,
            ..Self::default()
        };
        let window = app.windows.main.open();
        let tray = app.sync_tray();
        (app, Task::batch([window, tray]))
    }

    /// Starts or removes the tray icon so it matches the setting, and opens the window when
    /// removing the icon would leave no way back to it.
    fn sync_tray(&mut self) -> Task<Message> {
        let tray = self
            .tray
            .sync(self.preferences.settings.tray)
            .map(Message::Tray);
        if self.tray.is_active() || self.windows.main.is_open() {
            tray
        } else {
            Task::batch([tray, self.windows.main.open()])
        }
    }

    fn theme(&self, _window: iced::window::Id) -> Theme {
        self.preferences.settings.theme()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Sampled(sample) => {
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
            }
            Message::Failed(kind) => self.failure = Some(kind),
            Message::TogglePause => {
                self.paused = !self.paused;
                self.tray.set_paused(self.paused);
            }
            Message::TogglePopout => {
                let task = self.windows.popout.toggle();
                self.tray.set_popout_open(self.windows.popout.is_open());
                return task;
            }
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
            Message::Flight(message) => {
                return self
                    .flight
                    .update(message, self.history.latest())
                    .map(Message::Flight);
            }
            Message::Benchmarks(message) => {
                return self.benchmarks.update(message).map(Message::Benchmarks);
            }
            Message::Apps(message) => return self.apps.update(message).map(Message::Apps),
            Message::Startup(message) => return self.startup.update(message).map(Message::Startup),
            Message::Users(message) => return self.users.update(message).map(Message::Users),
            Message::Info(message) => return self.info.update(message).map(Message::Info),
            Message::Settings(message) => {
                let saved = self.preferences.update(message).map(Message::Settings);
                return Task::batch([saved, self.sync_tray()]);
            }
            Message::Tray(tray::Message::Started(result)) => {
                self.tray.started(result, self.preferences.settings.tray);
                self.tray.set_window_open(self.windows.main.is_open());
                self.tray.set_popout_open(self.windows.popout.is_open());
                self.tray.set_paused(self.paused);
            }
            Message::Tray(tray::Message::Clicked(click)) => match click {
                Click::Activate => return self.windows.main.activate(),
                Click::Window => return self.windows.main.toggle(),
                Click::Popout => return self.update(Message::TogglePopout),
                Click::Pause => return self.update(Message::TogglePause),
                Click::Quit => return iced::exit(),
            },
            Message::Window(event) => {
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
        let interval = self.preferences.settings.interval.duration();
        Subscription::batch([
            telemetry::subscription(interval),
            window::events().map(Message::Window),
            settings::keys().map(Message::Settings),
            self.tray.subscription().map(Message::Tray),
            page,
        ])
    }

    fn view(&self, window: iced::window::Id) -> Element<'_, Message> {
        if self.windows.popout.owns(window) {
            return performance::glance(&self.history);
        }
        let pages = column(Page::SECTIONS.iter().flat_map(|&(title, pages)| {
            iter::once(heading(title)).chain(pages.iter().map(|&page| {
                button(page.label())
                    .width(Fill)
                    .on_press(Message::Navigate(page))
                    .style(if page == self.page {
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
                .view(&self.history, self.preferences.settings.temperature)
                .map(Message::Performance),
            Page::Pressure => pressure::view(&self.history, &self.processes),
            Page::Power => power::view(&self.history, self.preferences.settings.temperature),
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

        column![row![rail, content].height(Fill)]
            .extend(
                self.failure
                    .map(|kind| text!("Sampling failed: {kind}").style(text::danger).into()),
            )
            .into()
    }
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
