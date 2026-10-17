mod panel;
mod rows;

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::time::Duration;

use iced::widget::{button, checkbox, column, row, space, table, text, text_input};
use iced::{Element, Fill, Right, Task};
use peepers_core::process::{self, Details, Observation, Process, Signal};
use peepers_core::telemetry::Sample;
use peepers_core::users::Users;

use self::panel::{Panel, Priority};
use self::rows::Row;
use crate::widgets::scrolling;
use crate::{blocking, format, storage};

/// Long enough for a thread that works in bursts to show up.
const SAMPLE_WINDOW: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Cpu,
    Memory,
    Name,
    Pid,
    User,
    Threads,
}

impl Sort {
    /// Numbers read best largest first, names smallest first.
    fn descending_first(self) -> bool {
        matches!(self, Self::Cpu | Self::Memory | Self::Threads)
    }

    fn compare(self, a: &Process, b: &Process, users: &Users) -> Ordering {
        match self {
            Self::Cpu => a.cpu.total_cmp(&b.cpu),
            Self::Memory => a.memory.cmp(&b.memory),
            Self::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Self::Pid => a.pid.cmp(&b.pid),
            Self::User => users.name(a.uid).cmp(&users.name(b.uid)),
            Self::Threads => a.threads.cmp(&b.threads),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Notice {
    text: String,
    failed: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Sort(Sort),
    Filter(String),
    TreeView(bool),
    Select(u32),
    Collapse(u32),
    Signal(Signal),
    SignalTree(Signal),
    Priority(Priority),
    Affinity(usize),
    Command(String),
    Run,
    Sample,
    Finished(Result<String, String>),
    Dismiss,
}

#[derive(Debug)]
pub struct Processes {
    sort: Sort,
    descending: bool,
    tree: bool,
    filter: String,
    command: String,
    list: Vec<Process>,
    users: Users,
    cpus: Vec<usize>,
    uptime: std::time::Duration,
    collapsed: HashSet<u32>,
    selected: Option<u32>,
    details: Details,
    notice: Option<Notice>,
}

impl Default for Processes {
    fn default() -> Self {
        Self {
            sort: Sort::default(),
            descending: Sort::default().descending_first(),
            tree: false,
            filter: String::new(),
            command: String::new(),
            list: Vec::new(),
            users: Users::read(),
            cpus: Vec::new(),
            uptime: std::time::Duration::ZERO,
            collapsed: HashSet::new(),
            selected: None,
            details: Details::default(),
            notice: None,
        }
    }
}

impl Processes {
    /// Every process in the latest sample, in the order the table shows them.
    pub fn all(&self) -> &[Process] {
        &self.list
    }

    /// The process the panel below the table describes.
    pub fn selected(&self) -> Option<u32> {
        self.selected
    }

    /// The name of a process in the latest sample.
    pub fn name(&self, pid: u32) -> Option<&str> {
        self.list
            .iter()
            .find(|process| process.pid == pid)
            .map(|process| process.name.as_str())
    }

    pub fn replace(&mut self, sample: &Sample) {
        self.list.clone_from(&sample.processes);
        self.cpus = sample.vitals.cpu.cores.iter().map(|core| core.id).collect();
        self.uptime = sample.vitals.uptime;
        if self
            .list
            .iter()
            .any(|process| self.users.name(process.uid).is_none())
        {
            self.users = Users::read();
        }
        self.order();
        self.refresh_details();
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Sort(sort) => {
                self.descending = if sort == self.sort {
                    !self.descending
                } else {
                    sort.descending_first()
                };
                self.sort = sort;
                self.order();
            }
            Message::Filter(filter) => self.filter = filter,
            Message::TreeView(tree) => self.tree = tree,
            Message::Select(pid) => {
                self.selected = Some(pid);
                self.notice = None;
                self.refresh_details();
            }
            Message::Collapse(pid) => {
                if !self.collapsed.remove(&pid) {
                    self.collapsed.insert(pid);
                }
            }
            Message::Signal(signal) => {
                if let Some(pid) = self.selected {
                    self.report(
                        process::send(pid, signal),
                        &format!("Sent {signal:?} to {pid}"),
                    );
                }
            }
            Message::SignalTree(signal) => {
                if let Some(pid) = self.selected {
                    let targets = process::descendants(&self.list, pid)
                        .into_iter()
                        .chain([pid]);
                    let result = targets
                        .clone()
                        .try_for_each(|target| process::send(target, signal));
                    self.report(
                        result,
                        &format!("Sent {signal:?} to {} processes", targets.count()),
                    );
                }
            }
            Message::Priority(priority) => {
                if let Some(pid) = self.selected {
                    self.report(
                        process::set_nice(pid, priority.nice()),
                        &format!("Priority of {pid} set to {priority}"),
                    );
                }
            }
            Message::Affinity(cpu) => {
                if let (Some(pid), Some(mut allowed)) =
                    (self.selected, self.details.affinity.clone())
                {
                    match allowed.iter().position(|&other| other == cpu) {
                        Some(index) => {
                            allowed.remove(index);
                        }
                        None => allowed.push(cpu),
                    }
                    self.report(
                        process::set_affinity(pid, &allowed),
                        &format!("Affinity of {pid} updated"),
                    );
                }
            }
            Message::Command(command) => {
                self.command = command;
                return Task::none();
            }
            Message::Run => return self.run(),
            Message::Sample => return self.sample(),
            Message::Finished(result) => {
                let (text, failed) =
                    result.map_or_else(|error| (error, true), |text| (text, false));
                self.notice = Some(Notice { text, failed });
                return Task::none();
            }
            Message::Dismiss => self.notice = None,
        }
        self.refresh_details();
        Task::none()
    }

    fn run(&mut self) -> Task<Message> {
        let command = std::mem::take(&mut self.command).trim().to_owned();
        if command.is_empty() {
            return Task::none();
        }
        blocking::run(move || {
            process::spawn(&command)
                .map(|pid| format!("Started {command} as {pid}"))
                .map_err(|error| error.to_string())
        })
        .map(Message::Finished)
    }

    fn sample(&mut self) -> Task<Message> {
        let Some(pid) = self.selected else {
            return Task::none();
        };
        self.notice = Some(Notice {
            text: format!("Sampling {pid} for {} seconds", SAMPLE_WINDOW.as_secs()),
            failed: false,
        });
        blocking::run(move || {
            let observation =
                Observation::capture(pid, SAMPLE_WINDOW).map_err(|error| error.to_string())?;
            storage::export(
                Path::new(&storage::default_folder()),
                &format!("sample-{pid}"),
                &observation.text(),
            )
            .map(|path| format!("Saved {}", path.display()))
        })
        .map(Message::Finished)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let needle = self.filter.to_lowercase();
        let rows = if self.tree && needle.is_empty() {
            rows::tree(&self.list, &self.collapsed)
        } else {
            rows::flat(&self.list, |process| self.matches(process, &needle))
        };
        let count = rows.len();

        let header = |label: &str, sort: Sort| {
            let marker = match (sort == self.sort, self.descending) {
                (false, _) => "",
                (true, true) => " v",
                (true, false) => " ^",
            };
            button(text(format!("{label}{marker}")))
                .on_press(Message::Sort(sort))
                .style(button::text)
        };
        let selected = self.selected;
        let users = &self.users;
        let columns = [
            table::column(header("Name", Sort::Name), move |row: Row| {
                name(row, selected)
            })
            .width(Fill),
            table::column(header("PID", Sort::Pid), |row: Row| text(row.process.pid))
                .align_x(Right),
            table::column(header("User", Sort::User), move |row: Row| {
                user(users, row.process)
            }),
            table::column(header("CPU", Sort::Cpu), |row: Row| {
                text!("{:.1}%", row.process.cpu)
            })
            .align_x(Right),
            table::column(header("Memory", Sort::Memory), |row: Row| {
                text(format::bytes(row.process.memory))
            })
            .align_x(Right),
            table::column(header("Threads", Sort::Threads), |row: Row| {
                text(row.process.threads)
            })
            .align_x(Right),
        ];

        let toolbar = row![
            text_input("Filter by name, user or PID", &self.filter)
                .on_input(Message::Filter)
                .width(320),
            checkbox(self.tree)
                .label("Process tree")
                .on_toggle(Message::TreeView),
            space::horizontal(),
            text_input("Run a command", &self.command)
                .on_input(Message::Command)
                .on_submit(Message::Run)
                .width(240),
            button("Run").on_press(Message::Run),
            text(format!("{count} processes")).style(text::secondary),
        ]
        .spacing(16)
        .padding(8)
        .align_y(iced::Center);

        column![toolbar]
            .extend(self.notice.as_ref().map(notice))
            .push(
                scrolling(table(columns, rows).padding_x(10).padding_y(2))
                    .width(Fill)
                    .height(Fill),
            )
            .extend(self.panel())
            .into()
    }

    /// What the selected process is, with the actions that apply to it.
    pub fn panel(&self) -> Option<Element<'_, Message>> {
        let process = self
            .list
            .iter()
            .find(|process| Some(process.pid) == self.selected)?;
        Some(panel::view(&Panel {
            process,
            details: &self.details,
            user: self.user_name(process.uid),
            parent: self
                .list
                .iter()
                .find(|other| other.pid == process.parent)
                .map(|other| other.name.as_str()),
            running_for: self.uptime.saturating_sub(process.started),
            cpus: &self.cpus,
        }))
    }

    fn matches(&self, process: &Process, needle: &str) -> bool {
        needle.is_empty()
            || process.name.to_lowercase().contains(needle)
            || process.pid.to_string().contains(needle)
            || self.user_name(process.uid).to_lowercase().contains(needle)
    }

    /// The user's name, or the number when the system has no name for it.
    pub fn user_name(&self, uid: u32) -> Cow<'_, str> {
        self.users
            .name(uid)
            .map_or_else(|| Cow::Owned(uid.to_string()), Cow::Borrowed)
    }

    fn order(&mut self) {
        let (sort, descending, users) = (self.sort, self.descending, &self.users);
        self.list.sort_by(|a, b| {
            let ordering = sort.compare(a, b, users);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }

    fn refresh_details(&mut self) {
        match self.selected {
            Some(pid) if self.list.iter().any(|process| process.pid == pid) => {
                self.details = Details::read(pid);
            }
            Some(pid) => {
                self.selected = None;
                self.details = Details::default();
                self.notice = Some(Notice {
                    text: format!("Process {pid} has exited"),
                    failed: false,
                });
            }
            None => {}
        }
    }

    fn report(&mut self, result: io::Result<()>, success: &str) {
        self.notice = Some(match result {
            Ok(()) => Notice {
                text: success.to_owned(),
                failed: false,
            },
            Err(error) => Notice {
                text: error.to_string(),
                failed: true,
            },
        });
    }
}

fn notice(notice: &Notice) -> Element<'_, Message> {
    row![
        text(&notice.text).style(if notice.failed {
            text::danger
        } else {
            text::success
        }),
        button("Dismiss")
            .on_press(Message::Dismiss)
            .style(button::text),
    ]
    .spacing(8)
    .padding([0, 8])
    .align_y(iced::Center)
    .into()
}

fn name(row: Row<'_>, selected: Option<u32>) -> Element<'_, Message> {
    let toggle: Element<_> = if row.children > 0 {
        button(text(if row.collapsed { "+" } else { "-" }))
            .on_press(Message::Collapse(row.process.pid))
            .padding([0, 6])
            .style(button::text)
            .into()
    } else {
        space().width(24).into()
    };
    iced::widget::row![
        space().width(f32::from(u16::try_from(row.depth).unwrap_or(u16::MAX)) * 16.0),
        toggle,
        button(text(&row.process.name))
            .on_press(Message::Select(row.process.pid))
            .padding([0, 6])
            .style(if selected == Some(row.process.pid) {
                button::primary
            } else {
                button::text
            }),
    ]
    .align_y(iced::Center)
    .into()
}

fn user<'a>(users: &'a Users, process: &Process) -> Element<'a, Message> {
    text(
        users
            .name(process.uid)
            .map_or_else(|| process.uid.to_string(), str::to_owned),
    )
    .into()
}
