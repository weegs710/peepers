use crate::theme::Element;
use crate::theme::styles;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::Duration;

use iced::time;
use iced::widget::{button, column, container, row, space, table, text};
use iced::{Fill, Right, Subscription, Task};
use peepers_core::process::Process;
use peepers_core::sessions::{self, Action, Session};

use crate::blocking;
use crate::format;
use crate::notice::{self, Notice};
use crate::processes::Processes;
use crate::selectable;
use crate::widgets::{Sortable, Sorting, scrolling, sort_header};

const REFRESH: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Column {
    User,
    Sessions,
    Processes,
    Cpu,
    #[default]
    Memory,
}

impl Sortable for Column {
    /// Numbers read best largest first, names smallest first.
    fn descending_first(self) -> bool {
        self != Self::User
    }
}

impl Column {
    fn compare(self, a: &Row, b: &Row) -> Ordering {
        match self {
            Self::User => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Self::Sessions => a.sessions.cmp(&b.sessions),
            Self::Processes => a.usage.processes.cmp(&b.usage.processes),
            Self::Cpu => a.usage.cpu.total_cmp(&b.usage.cpu),
            Self::Memory => a.usage.memory.cmp(&b.usage.memory),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Result<Vec<Session>, String>),
    Sort(Column),
    Select(u32),
    Control(Action),
    Done(Result<(), String>),
    Dismiss,
}

#[derive(Debug, Default)]
pub struct Users {
    sessions: Vec<Session>,
    sorting: Sorting<Column>,
    selected: Option<u32>,
    notice: Option<Notice>,
}

/// One line of the users table.
#[derive(Debug, Clone)]
struct Row {
    uid: u32,
    name: String,
    sessions: usize,
    usage: Usage,
}

/// What one user's processes add up to.
#[derive(Debug, Clone, Default, PartialEq)]
struct Usage {
    processes: usize,
    cpu: f32,
    memory: u64,
}

impl Users {
    pub fn enter() -> Task<Message> {
        blocking::run(sessions::read).map(Message::Loaded)
    }

    pub fn subscription() -> Subscription<Message> {
        time::every(REFRESH).map(|_| Message::Refresh)
    }

    pub fn fading(&self) -> bool {
        notice::fading(self.notice.as_ref())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Sort(column) => self.sorting.press(column),
            Message::Refresh => return Self::enter(),
            Message::Loaded(Ok(sessions)) => self.sessions = sessions,
            Message::Loaded(Err(error)) => {
                self.notice = Some(Notice::failure(format!(
                    "The login manager could not be read: {error}"
                )));
            }
            Message::Select(uid) => {
                self.selected = Some(uid);
                self.notice = None;
            }
            Message::Control(action) => {
                let ids: Vec<String> = self
                    .sessions
                    .iter()
                    .filter(|session| Some(session.uid) == self.selected)
                    .map(|session| session.id.clone())
                    .collect();
                return blocking::run(move || {
                    ids.iter().try_for_each(|id| sessions::control(id, action))
                })
                .map(Message::Done);
            }
            Message::Done(result) => {
                self.notice = Some(match result {
                    Ok(()) => Notice::success("Requested"),
                    Err(error) => Notice::failure(error),
                });
                return Self::enter();
            }
            Message::Dismiss => self.notice = None,
        }
        Task::none()
    }

    pub fn view<'a>(&'a self, processes: &'a Processes) -> Element<'a, Message> {
        let sessions = |uid: u32| {
            self.sessions
                .iter()
                .filter(|session| session.uid == uid)
                .count()
        };
        let mut rows: Vec<Row> = usage(processes.all())
            .into_iter()
            .map(|(uid, usage)| Row {
                uid,
                name: processes.user_name(uid).into_owned(),
                sessions: sessions(uid),
                usage,
            })
            .collect();
        self.sorting.sort(&mut rows, Column::compare);
        let header = |label, column| {
            sort_header(label, self.sorting.direction(column), Message::Sort(column))
        };

        let selected = self.selected;
        let columns = [
            table::column(header("User", Column::User), |row: Row| {
                container(text(row.name)).padding([0, 6])
            })
            .width(Fill),
            table::column(header("Sessions", Column::Sessions), |row: Row| {
                text(row.sessions)
            })
            .align_x(Right),
            table::column(header("Processes", Column::Processes), |row: Row| {
                text(row.usage.processes)
            })
            .align_x(Right),
            table::column(header("CPU", Column::Cpu), |row: Row| {
                text(format!("{:.1}%", row.usage.cpu))
            })
            .align_x(Right),
            table::column(header("Memory", Column::Memory), |row: Row| {
                text(format::bytes(row.usage.memory))
            })
            .align_x(Right),
        ];

        let actions = selected.map(|uid| {
            row![
                text(format!("{}", processes.user_name(uid)))
                    .size(20)
                    .style(styles::heading),
                notice::shown(self.notice.as_ref()).map_or_else(
                    || space::horizontal().into(),
                    |notice| notice.view(Message::Dismiss)
                ),
                button("Lock")
                    .on_press_maybe((sessions(uid) > 0).then_some(Message::Control(Action::Lock)))
                    .style(styles::button_secondary),
                button("Sign off")
                    .on_press_maybe(
                        (sessions(uid) > 0).then_some(Message::Control(Action::SignOff))
                    )
                    .style(styles::button_danger),
            ]
            .spacing(12)
            .padding(12)
            .align_y(iced::Center)
            .into()
        });

        column![
            scrolling(selectable::table(
                columns,
                rows,
                |row| Message::Select(row.uid),
                |row| selected == Some(row.uid),
            ))
            .width(Fill)
            .height(Fill)
        ]
        .extend(actions)
        .into()
    }
}

fn usage(processes: &[Process]) -> BTreeMap<u32, Usage> {
    let mut usage: BTreeMap<u32, Usage> = BTreeMap::new();
    for process in processes {
        let entry = usage.entry(process.uid).or_default();
        entry.processes += 1;
        entry.cpu += process.cpu;
        entry.memory += process.memory;
    }
    usage
}

#[cfg(test)]
mod tests {
    use peepers_core::process::State;

    use super::*;
    use crate::widgets::pressed;

    /// The rows in the order pressing `column` once puts them, or twice with `again`.
    fn ordered(column: Column, again: bool, rows: &[Row]) -> Vec<&str> {
        let mut sorting = Sorting::new(column);
        if again {
            sorting.press(column);
        }
        let mut rows: Vec<&Row> = rows.iter().collect();
        sorting.sort(&mut rows, |column, a, b| column.compare(a, b));
        rows.into_iter().map(|row| row.name.as_str()).collect()
    }

    fn row(name: &str, sessions: usize, processes: usize, cpu: f32, memory: u64) -> Row {
        Row {
            uid: 0,
            name: name.to_owned(),
            sessions,
            usage: Usage {
                processes,
                cpu,
                memory,
            },
        }
    }

    fn people() -> Vec<Row> {
        vec![
            row("weegs", 2, 300, 5.5, 4_000),
            row("Root", 0, 80, 1.0, 9_000),
            row("jellyfin", 0, 20, 12.0, 1_000),
        ]
    }

    #[test]
    fn every_column_orders_the_users_and_memory_leads_by_default() {
        let people = people();
        assert_eq!(
            ordered(Column::User, false, &people),
            ["jellyfin", "Root", "weegs"]
        );
        assert_eq!(ordered(Column::Sessions, false, &people)[0], "weegs");
        assert_eq!(
            ordered(Column::Processes, false, &people),
            ["weegs", "Root", "jellyfin"]
        );
        assert_eq!(
            ordered(Column::Cpu, false, &people),
            ["jellyfin", "weegs", "Root"]
        );
        assert_eq!(
            ordered(Column::Memory, false, &people),
            ["Root", "weegs", "jellyfin"]
        );
        assert_eq!(
            Sorting::<Column>::default().direction(Column::Memory),
            Some(true)
        );
    }

    #[test]
    fn pressing_a_heading_orders_the_table_by_it() {
        let mut users = Users::default();
        let processes = Processes::default();
        for message in pressed(users.view(&processes), "CPU") {
            drop(users.update(message));
        }
        assert_eq!(users.sorting.direction(Column::Cpu), Some(true));
    }

    fn process(uid: u32, cpu: f32, memory: u64) -> Process {
        Process {
            pid: 1,
            parent: 0,
            name: String::new(),
            state: State::Sleeping,
            uid,
            threads: 1,
            nice: 0,
            cpu,
            memory,
            io: None,
            started: Duration::ZERO,
        }
    }

    #[test]
    fn processes_add_up_per_user() {
        let usage = usage(&[
            process(0, 1.5, 100),
            process(1000, 2.0, 50),
            process(0, 0.5, 25),
        ]);
        assert_eq!(
            usage[&0],
            Usage {
                processes: 2,
                cpu: 2.0,
                memory: 125
            }
        );
        assert_eq!(usage[&1000].processes, 1);
    }
}
