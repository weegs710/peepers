use crate::theme::Element;
use crate::theme::styles;
use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::Duration;

use iced::time;
use iced::widget::{button, column, container, progress_bar, row, space, table, text, text_input};
use iced::{Fill, Length, Right, Subscription, Task};
use peepers_core::scan::{Entry, FileType, Progress, Report, Scan};
use peepers_core::volumes::{self, Volume};

use crate::blocking;
use crate::format;
use crate::graph::level;
use crate::widgets::{Sortable, Sorting, scrolling, sort_header};

const POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Folders,
    Files,
    Types,
}

/// The columns of the folders and files tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntryColumn {
    Path,
    #[default]
    Size,
    Share,
}

impl Sortable for EntryColumn {
    fn descending_first(self) -> bool {
        self != Self::Path
    }
}

impl EntryColumn {
    fn compare(self, a: &Entry, b: &Entry) -> Ordering {
        match self {
            Self::Path => a.path.cmp(&b.path),
            Self::Size | Self::Share => a.bytes.cmp(&b.bytes),
        }
    }
}

/// The columns of the file types table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeColumn {
    Type,
    Files,
    #[default]
    Size,
    Share,
}

impl Sortable for TypeColumn {
    fn descending_first(self) -> bool {
        self != Self::Type
    }
}

impl TypeColumn {
    fn compare(self, a: &FileType, b: &FileType) -> Ordering {
        match self {
            Self::Type => a.extension.to_lowercase().cmp(&b.extension.to_lowercase()),
            Self::Files => a.files.cmp(&b.files),
            Self::Size | Self::Share => a.bytes.cmp(&b.bytes),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    SortEntries(EntryColumn),
    SortTypes(TypeColumn),
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
    entries: Sorting<EntryColumn>,
    types: Sorting<TypeColumn>,
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
            entries: Sorting::default(),
            types: Sorting::default(),
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
            Message::SortEntries(column) => self.entries.press(column),
            Message::SortTypes(column) => self.types.press(column),
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
                        .style(styles::text_secondary),
                    ],
                    progress_bar(0.0..=100.0, volume.used_percent()).girth(6),
                    text(format!("{} on {}", volume.filesystem, volume.source))
                        .size(12)
                        .style(styles::text_secondary),
                ]
                .spacing(4),
            )
            .width(Fill)
            .on_press(Message::Start(volume.mount.clone()))
            .style(styles::button_text)
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
            scrolling(volumes).height(Length::FillPortion(2)),
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
                .style(styles::text_secondary)
                .into();
        };
        let tab = |label, tab: Tab| {
            button(label)
                .on_press(Message::Tab(tab))
                .style(if self.tab == tab {
                    styles::button_primary
                } else {
                    styles::button_text
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
            text(summary).style(styles::text_secondary),
            container(scrolling(self.list(report)).height(Fill)).height(Length::FillPortion(3)),
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
                let mut entries: Vec<&Entry> = if self.tab == Tab::Folders {
                    &report.folders
                } else {
                    &report.largest_files
                }
                .iter()
                .collect();
                self.entries
                    .sort(&mut entries, |column, a, b| column.compare(a, b));
                let header = |label, column| {
                    sort_header(
                        label,
                        self.entries.direction(column),
                        Message::SortEntries(column),
                    )
                };
                let columns = [
                    table::column(header("Path", EntryColumn::Path), |entry: &'a Entry| {
                        text(entry.path.display().to_string())
                    })
                    .width(Fill),
                    table::column(header("Size", EntryColumn::Size), |entry: &'a Entry| {
                        text(format::bytes(entry.bytes))
                    })
                    .align_x(Right),
                    table::column(
                        header("Share", EntryColumn::Share),
                        move |entry: &'a Entry| share(entry.bytes).length(140),
                    ),
                ];
                table(columns, entries).padding_x(10).padding_y(2).into()
            }
            Tab::Types => {
                let mut kinds: Vec<&FileType> = report.types.iter().collect();
                self.types
                    .sort(&mut kinds, |column, a, b| column.compare(a, b));
                let header = |label, column| {
                    sort_header(
                        label,
                        self.types.direction(column),
                        Message::SortTypes(column),
                    )
                };
                let columns = [
                    table::column(header("Type", TypeColumn::Type), |kind: &'a FileType| {
                        text(if kind.extension.is_empty() {
                            "(no extension)".to_owned()
                        } else {
                            format!(".{}", kind.extension)
                        })
                    })
                    .width(Fill),
                    table::column(header("Files", TypeColumn::Files), |kind: &'a FileType| {
                        text(kind.files)
                    })
                    .align_x(Right),
                    table::column(header("Size", TypeColumn::Size), |kind: &'a FileType| {
                        text(format::bytes(kind.bytes))
                    })
                    .align_x(Right),
                    table::column(
                        header("Share", TypeColumn::Share),
                        move |kind: &'a FileType| share(kind.bytes).length(140),
                    ),
                ];
                table(columns, kinds).padding_x(10).padding_y(2).into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use peepers_core::scan::Report;

    use super::*;
    use crate::widgets::pressed;

    fn entry(path: &str, bytes: u64) -> Entry {
        Entry {
            path: PathBuf::from(path),
            bytes,
        }
    }

    fn kind(extension: &str, files: u64, bytes: u64) -> FileType {
        FileType {
            extension: extension.to_owned(),
            files,
            bytes,
        }
    }

    fn ordered_entries(column: EntryColumn, again: bool, entries: &[Entry]) -> Vec<&str> {
        let mut sorting = Sorting::new(column);
        if again {
            sorting.press(column);
        }
        let mut rows: Vec<&Entry> = entries.iter().collect();
        sorting.sort(&mut rows, |column, a, b| column.compare(a, b));
        rows.into_iter()
            .map(|row| row.path.to_str().unwrap())
            .collect()
    }

    fn ordered_types(column: TypeColumn, kinds: &[FileType]) -> Vec<&str> {
        let mut rows: Vec<&FileType> = kinds.iter().collect();
        Sorting::new(column).sort(&mut rows, |column, a, b| column.compare(a, b));
        rows.into_iter().map(|row| row.extension.as_str()).collect()
    }

    #[test]
    fn every_column_orders_the_folders_and_files() {
        let entries = [entry("/b", 10), entry("/c", 30), entry("/a", 20)];
        assert_eq!(
            ordered_entries(EntryColumn::Path, false, &entries),
            ["/a", "/b", "/c"]
        );
        assert_eq!(
            ordered_entries(EntryColumn::Size, false, &entries),
            ["/c", "/a", "/b"]
        );
        assert_eq!(
            ordered_entries(EntryColumn::Share, false, &entries),
            ["/c", "/a", "/b"]
        );
        assert_eq!(
            ordered_entries(EntryColumn::Size, true, &entries),
            ["/b", "/a", "/c"]
        );
    }

    #[test]
    fn every_column_orders_the_file_types() {
        let kinds = [kind("rs", 5, 100), kind("", 9, 50), kind("PNG", 2, 400)];
        assert_eq!(ordered_types(TypeColumn::Type, &kinds), ["", "PNG", "rs"]);
        assert_eq!(ordered_types(TypeColumn::Files, &kinds), ["", "rs", "PNG"]);
        assert_eq!(ordered_types(TypeColumn::Size, &kinds), ["PNG", "rs", ""]);
        assert_eq!(ordered_types(TypeColumn::Share, &kinds), ["PNG", "rs", ""]);
    }

    #[test]
    fn pressing_a_heading_orders_each_table_by_it() {
        let report = Report {
            root: PathBuf::from("/"),
            bytes: 100,
            files: 3,
            folders: vec![entry("/a", 10)],
            largest_files: vec![entry("/a/f", 10)],
            types: vec![kind("rs", 1, 10)],
            complete: true,
        };
        let mut disks = DiskSpace {
            report: Some(report),
            ..DiskSpace::default()
        };
        for message in pressed(disks.view(), "Path") {
            drop(disks.update(message));
        }
        assert_eq!(disks.entries.direction(EntryColumn::Path), Some(false));
        drop(disks.update(Message::Tab(Tab::Types)));
        for message in pressed(disks.view(), "Files") {
            drop(disks.update(message));
        }
        assert_eq!(disks.types.direction(TypeColumn::Files), Some(true));
    }
}
