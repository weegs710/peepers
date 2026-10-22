use std::cmp::Ordering;

use iced::widget::{button, column, container, row, space, table, text};
use iced::{Element, Fill, Task};
use peepers_core::startup::{self, Scope, Startup};

use crate::blocking;
use crate::format;
use crate::notice::Notice;
use crate::widgets::{Sortable, Sorting, scrolling, sort_header};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Column {
    #[default]
    Name,
    Status,
    DefinedFor,
    Command,
}

impl Sortable for Column {
    fn descending_first(self) -> bool {
        false
    }
}

impl Column {
    fn compare(self, a: &Startup, b: &Startup) -> Ordering {
        match self {
            Self::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Self::Status => a.enabled.cmp(&b.enabled),
            Self::DefinedFor => defined_for(a.scope).cmp(defined_for(b.scope)),
            Self::Command => a.command.cmp(&b.command),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Vec<Startup>),
    Sort(Column),
    Toggle(Startup),
    Toggled(Result<(), String>),
    Dismiss,
}

#[derive(Debug, Default)]
pub struct StartupApps {
    entries: Option<Vec<Startup>>,
    sorting: Sorting<Column>,
    failure: Option<Notice>,
}

impl StartupApps {
    pub fn enter() -> Task<Message> {
        blocking::run(startup::read).map(Message::Loaded)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(entries) => self.entries = Some(entries),
            Message::Sort(column) => self.sorting.press(column),
            Message::Toggle(entry) => {
                return blocking::run(move || {
                    startup::set_enabled(&entry, !entry.enabled).map_err(|error| error.to_string())
                })
                .map(Message::Toggled);
            }
            Message::Toggled(result) => {
                self.failure = result
                    .err()
                    .map(|error| Notice::failure(format!("Could not change the entry: {error}")));
                return Self::enter();
            }
            Message::Dismiss => self.failure = None,
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(entries) = &self.entries else {
            return text("Reading the autostart entries")
                .style(text::secondary)
                .into();
        };
        let mut rows: Vec<&Startup> = entries.iter().collect();
        self.sorting
            .sort(&mut rows, |column, a, b| column.compare(a, b));
        let header = |label, column| {
            sort_header(label, self.sorting.direction(column), Message::Sort(column))
        };
        let columns = [
            table::column(header("Name", Column::Name), |entry: &Startup| {
                text(entry.name.clone())
            })
            .width(Fill),
            table::column(header("Status", Column::Status), |entry: &Startup| {
                button(if entry.enabled { "Enabled" } else { "Disabled" })
                    .on_press(Message::Toggle(entry.clone()))
                    .style(if entry.enabled {
                        button::success
                    } else {
                        button::secondary
                    })
            }),
            table::column(
                header("Defined for", Column::DefinedFor),
                |entry: &Startup| text(defined_for(entry.scope)),
            ),
            table::column(header("Command", Column::Command), |entry: &Startup| {
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
            scrolling(table(columns, rows).padding_x(10).padding_y(2))
                .width(Fill)
                .height(Fill),
        ]
        .extend(
            self.failure
                .as_ref()
                .map(|failure| container(failure.view(Message::Dismiss)).padding(8).into()),
        )
        .into()
    }
}

fn defined_for(scope: Scope) -> &'static str {
    if scope == Scope::User {
        "This user"
    } else {
        "Everyone"
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::widgets::pressed;

    /// The rows in the order pressing `column` once puts them, or twice with `again`.
    fn ordered(column: Column, again: bool, rows: &[Startup]) -> Vec<&str> {
        let mut sorting = Sorting::new(column);
        if again {
            sorting.press(column);
        }
        let mut rows: Vec<&Startup> = rows.iter().collect();
        sorting.sort(&mut rows, |column, a, b| column.compare(a, b));
        rows.into_iter().map(|row| row.name.as_str()).collect()
    }

    fn entry(name: &str, enabled: bool, scope: Scope, command: Option<&str>) -> Startup {
        Startup {
            id: format!("{name}.desktop"),
            name: name.to_owned(),
            command: command.map(str::to_owned),
            comment: None,
            enabled,
            scope,
            path: PathBuf::new(),
        }
    }

    fn entries() -> Vec<Startup> {
        vec![
            entry("Tray", true, Scope::User, Some("tray")),
            entry("agent", false, Scope::System, None),
            entry("Sync", true, Scope::System, Some("sync")),
        ]
    }

    #[test]
    fn every_column_orders_the_entries() {
        let entries = entries();
        assert_eq!(
            ordered(Column::Name, false, &entries),
            ["agent", "Sync", "Tray"]
        );
        assert_eq!(
            ordered(Column::Status, false, &entries),
            ["agent", "Tray", "Sync"]
        );
        assert_eq!(
            ordered(Column::DefinedFor, false, &entries),
            ["agent", "Sync", "Tray"]
        );
        assert_eq!(
            ordered(Column::Command, false, &entries),
            ["agent", "Sync", "Tray"]
        );
    }

    #[test]
    fn pressing_a_heading_orders_the_table_by_it() {
        let mut startup = StartupApps {
            entries: Some(entries()),
            ..StartupApps::default()
        };
        for message in pressed(startup.view(), "Command") {
            drop(startup.update(message));
        }
        assert_eq!(startup.sorting.direction(Column::Command), Some(false));
    }
}
