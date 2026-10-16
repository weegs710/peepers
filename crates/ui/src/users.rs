use std::collections::BTreeMap;
use std::time::Duration;

use iced::time;
use iced::widget::{button, column, row, scrollable, space, table, text};
use iced::{Element, Fill, Right, Subscription, Task};
use peepers_core::process::Process;
use peepers_core::sessions::{self, Action, Session};

use crate::blocking;
use crate::format;
use crate::processes::Processes;

const REFRESH: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Result<Vec<Session>, String>),
    Select(u32),
    Control(Action),
    Done(Result<(), String>),
}

#[derive(Debug, Default)]
pub struct Users {
    sessions: Vec<Session>,
    selected: Option<u32>,
    notice: Option<(String, bool)>,
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => return Self::enter(),
            Message::Loaded(Ok(sessions)) => self.sessions = sessions,
            Message::Loaded(Err(error)) => {
                self.notice = Some((
                    format!("The login manager could not be read: {error}"),
                    true,
                ));
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
                    Ok(()) => ("Requested".to_owned(), false),
                    Err(error) => (error, true),
                });
                return Self::enter();
            }
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
        rows.sort_by_key(|row| std::cmp::Reverse(row.usage.memory));

        let selected = self.selected;
        let columns = [
            table::column("User", move |row: Row| {
                button(text(row.name))
                    .on_press(Message::Select(row.uid))
                    .padding([0, 6])
                    .style(if selected == Some(row.uid) {
                        button::primary
                    } else {
                        button::text
                    })
            })
            .width(Fill),
            table::column("Sessions", |row: Row| text(row.sessions)).align_x(Right),
            table::column("Processes", |row: Row| text(row.usage.processes)).align_x(Right),
            table::column("CPU", |row: Row| text(format!("{:.1}%", row.usage.cpu))).align_x(Right),
            table::column("Memory", |row: Row| text(format::bytes(row.usage.memory)))
                .align_x(Right),
        ];

        let actions = selected.map(|uid| {
            row![
                text(format!("{}", processes.user_name(uid))).size(20),
                space::horizontal(),
                text(
                    self.notice
                        .as_ref()
                        .map_or("", |(message, _)| message.as_str())
                )
                .style(
                    if self.notice.as_ref().is_some_and(|(_, failed)| *failed) {
                        text::danger
                    } else {
                        text::success
                    }
                ),
                button("Lock")
                    .on_press_maybe((sessions(uid) > 0).then_some(Message::Control(Action::Lock)))
                    .style(button::secondary),
                button("Sign off")
                    .on_press_maybe(
                        (sessions(uid) > 0).then_some(Message::Control(Action::SignOff))
                    )
                    .style(button::danger),
            ]
            .spacing(12)
            .padding(12)
            .align_y(iced::Center)
            .into()
        });

        column![
            scrollable(table(columns, rows).padding_x(10).padding_y(2))
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
