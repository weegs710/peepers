use std::path::PathBuf;
use std::time::Duration;

use iced::time;
use iced::widget::{
    button, column, container, progress_bar, row, scrollable, space, table, text, text_input,
};
use iced::{Element, Fill, Length, Right, Subscription, Task};
use peepers_core::scan::{Progress, Report, Scan};
use peepers_core::volumes::{self, Volume};

use crate::blocking;
use crate::format;
use crate::graph::level;

const POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Folders,
    Files,
    Types,
}

#[derive(Debug, Clone)]
pub enum Message {
    Volumes(Vec<Volume>),
    Path(String),
    Start(PathBuf),
    Stop,
    Poll,
    Tab(Tab),
}

#[derive(Debug)]
pub struct DiskSpace {
    volumes: Vec<Volume>,
    path: String,
    scan: Option<Scan>,
    progress: Progress,
    report: Option<Report>,
    tab: Tab,
}

impl Default for DiskSpace {
    fn default() -> Self {
        Self {
            volumes: Vec::new(),
            path: String::new(),
            scan: None,
            progress: Progress { files: 0, bytes: 0 },
            report: None,
            tab: Tab::Folders,
        }
    }
}

impl DiskSpace {
    pub fn enter() -> Task<Message> {
        blocking::run(volumes::read).map(Message::Volumes)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.scan.is_some() {
            time::every(POLL).map(|_| Message::Poll)
        } else {
            Subscription::none()
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Volumes(volumes) => self.volumes = volumes,
            Message::Path(path) => self.path = path,
            Message::Start(root) => {
                self.path = root.display().to_string();
                self.scan = Some(Scan::start(root));
                self.report = None;
                self.progress = Progress { files: 0, bytes: 0 };
            }
            Message::Stop => {
                if let Some(scan) = &self.scan {
                    scan.stop();
                }
            }
            Message::Poll => {
                if let Some(scan) = &self.scan {
                    if let Some(report) = scan.finished() {
                        self.report = Some(report);
                        self.scan = None;
                    } else {
                        self.progress = scan.progress();
                    }
                }
            }
            Message::Tab(tab) => self.tab = tab,
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let volumes = column(self.volumes.iter().map(|volume| {
            button(
                column![
                    row![
                        text(volume.mount.display().to_string()),
                        space::horizontal(),
                        text(format!(
                            "{} of {} used",
                            format::bytes(volume.used()),
                            format::bytes(volume.total)
                        ))
                        .style(text::secondary),
                    ],
                    progress_bar(0.0..=100.0, volume.used_percent()).girth(6),
                    text(format!("{} on {}", volume.filesystem, volume.source))
                        .size(12)
                        .style(text::secondary),
                ]
                .spacing(4),
            )
            .width(Fill)
            .on_press(Message::Start(volume.mount.clone()))
            .style(button::text)
            .into()
        }))
        .spacing(2);

        let control = if self.scan.is_some() {
            button("Stop scanning").on_press(Message::Stop)
        } else {
            button("Scan").on_press_maybe(
                (!self.path.is_empty()).then(|| Message::Start(PathBuf::from(&self.path))),
            )
        };
        let toolbar = row![
            text_input("Folder to measure, or pick a disk above", &self.path)
                .on_input(Message::Path)
                .on_submit_maybe(
                    (!self.path.is_empty()).then(|| Message::Start(PathBuf::from(&self.path)))
                ),
            control,
        ]
        .spacing(8)
        .align_y(iced::Center);

        column![
            scrollable(volumes).height(Length::FillPortion(2)),
            toolbar,
            self.results()
        ]
        .spacing(12)
        .padding(12)
        .into()
    }

    fn results(&self) -> Element<'_, Message> {
        if self.scan.is_some() {
            return text(format!(
                "Scanning: {} files, {} so far. Stopping keeps what has been measured.",
                self.progress.files,
                format::bytes(self.progress.bytes)
            ))
            .into();
        }
        let Some(report) = &self.report else {
            return text("Press a disk or enter a folder to measure it.")
                .style(text::secondary)
                .into();
        };
        let tab = |label, tab: Tab| {
            button(label)
                .on_press(Message::Tab(tab))
                .style(if self.tab == tab {
                    button::primary
                } else {
                    button::text
                })
        };
        let summary = format!(
            "{}: {} in {} files{}",
            report.root.display(),
            format::bytes(report.bytes),
            report.files,
            if report.complete {
                ""
            } else {
                " (stopped early)"
            }
        );
        column![
            row![
                tab("Largest folders", Tab::Folders),
                tab("Largest files", Tab::Files),
                tab("File types", Tab::Types)
            ]
            .spacing(8),
            text(summary).style(text::secondary),
            container(scrollable(self.list(report)).height(Fill)).height(Length::FillPortion(3)),
        ]
        .spacing(8)
        .into()
    }

    fn list<'a>(&self, report: &'a Report) -> Element<'a, Message> {
        let share = |bytes: u64| {
            progress_bar(0.0..=1.0, level(bytes) / level(report.bytes.max(1))).girth(6)
        };
        match self.tab {
            Tab::Folders | Tab::Files => {
                let entries = if self.tab == Tab::Folders {
                    &report.folders
                } else {
                    &report.largest_files
                };
                let columns = [
                    table::column("Path", |entry: &'a peepers_core::scan::Entry| {
                        text(entry.path.display().to_string())
                    })
                    .width(Fill),
                    table::column("Size", |entry: &'a peepers_core::scan::Entry| {
                        text(format::bytes(entry.bytes))
                    })
                    .align_x(Right),
                    table::column("Share", move |entry: &'a peepers_core::scan::Entry| {
                        share(entry.bytes).length(140)
                    }),
                ];
                table(columns, entries).padding_x(10).padding_y(2).into()
            }
            Tab::Types => {
                let columns = [
                    table::column("Type", |kind: &'a peepers_core::scan::FileType| {
                        text(if kind.extension.is_empty() {
                            "(no extension)".to_owned()
                        } else {
                            format!(".{}", kind.extension)
                        })
                    })
                    .width(Fill),
                    table::column("Files", |kind: &'a peepers_core::scan::FileType| {
                        text(kind.files)
                    })
                    .align_x(Right),
                    table::column("Size", |kind: &'a peepers_core::scan::FileType| {
                        text(format::bytes(kind.bytes))
                    })
                    .align_x(Right),
                    table::column("Share", move |kind: &'a peepers_core::scan::FileType| {
                        share(kind.bytes).length(140)
                    }),
                ];
                table(columns, &report.types)
                    .padding_x(10)
                    .padding_y(2)
                    .into()
            }
        }
    }
}
