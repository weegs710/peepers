use iced::widget::{button, column, row, scrollable, space, table, text};
use iced::{Element, Fill, Task};
use peepers_core::startup::{self, Scope, Startup};

use crate::blocking;
use crate::format;

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Vec<Startup>),
    Toggle(Startup),
    Toggled(Result<(), String>),
}

#[derive(Debug, Default)]
pub struct StartupApps {
    entries: Option<Vec<Startup>>,
    failure: Option<String>,
}

impl StartupApps {
    pub fn enter() -> Task<Message> {
        blocking::run(startup::read).map(Message::Loaded)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(entries) => self.entries = Some(entries),
            Message::Toggle(entry) => {
                return blocking::run(move || {
                    startup::set_enabled(&entry, !entry.enabled).map_err(|error| error.to_string())
                })
                .map(Message::Toggled);
            }
            Message::Toggled(result) => {
                self.failure = result.err();
                return Self::enter();
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(entries) = &self.entries else {
            return text("Reading the autostart entries")
                .style(text::secondary)
                .into();
        };
        let columns = [
            table::column("Name", |entry: &Startup| text(entry.name.clone())).width(Fill),
            table::column("Status", |entry: &Startup| {
                button(if entry.enabled { "Enabled" } else { "Disabled" })
                    .on_press(Message::Toggle(entry.clone()))
                    .style(if entry.enabled {
                        button::success
                    } else {
                        button::secondary
                    })
            }),
            table::column("Defined for", |entry: &Startup| {
                text(if entry.scope == Scope::User {
                    "This user"
                } else {
                    "Everyone"
                })
            }),
            table::column("Command", |entry: &Startup| {
                text(entry.command.clone().unwrap_or_else(format::unavailable))
            })
            .width(Fill),
        ];
        column![
            row![
                space::horizontal(),
                text(format!("{} startup apps", entries.len())).style(text::secondary)
            ]
            .padding(8),
            scrollable(table(columns, entries).padding_x(10).padding_y(2))
                .width(Fill)
                .height(Fill),
        ]
        .extend(self.failure.as_ref().map(|failure| {
            text(format!("Could not change the entry: {failure}"))
                .style(text::danger)
                .into()
        }))
        .into()
    }
}
