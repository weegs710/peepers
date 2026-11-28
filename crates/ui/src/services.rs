use crate::theme::Element;
use crate::theme::styles;
use std::cmp::Ordering;
use std::time::Duration;

use iced::time;
use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, container, row, space, table, text, text_input};
use iced::{Fill, Subscription, Task};
use peepers_core::systemd::{self, Action, Active, Bus, Service};

use crate::blocking;
use crate::notice::{self, Notice};
use crate::selectable;
use crate::widgets::{Sortable, Sorting, scrolling, sort_header, stats};

const REFRESH: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Column {
    #[default]
    Service,
    Description,
    Status,
    Manager,
}

impl Sortable for Column {
    fn descending_first(self) -> bool {
        false
    }
}

impl Column {
    fn compare(self, a: &Service, b: &Service) -> Ordering {
        match self {
            Self::Service => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Self::Description => a
                .description
                .to_lowercase()
                .cmp(&b.description.to_lowercase()),
            Self::Status => status(a).cmp(&status(b)),
            Self::Manager => a.bus.cmp(&b.bus),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Result<Vec<Service>, String>),
    Sort(Column),
    Filter(String),
    RunningOnly(bool),
    Select(String, Bus),
    Control(Action),
    Done(Result<(), String>),
    Dismiss,
}

#[derive(Debug, Default)]
pub struct Services {
    list: Option<Vec<Service>>,
    sorting: Sorting<Column>,
    filter: String,
    running_only: bool,
    selected: Option<(String, Bus)>,
    notice: Option<Notice>,
}

impl Services {
    pub fn enter() -> Task<Message> {
        blocking::run(systemd::read).map(Message::Loaded)
    }

    pub fn subscription() -> Subscription<Message> {
        time::every(REFRESH).map(|_| Message::Refresh)
    }

    pub fn fading(&self) -> bool {
        notice::fading(self.notice.as_ref())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => return Self::enter(),
            Message::Loaded(Ok(list)) => self.list = Some(list),
            Message::Loaded(Err(error)) => {
                self.notice = Some(Notice::failure(format!(
                    "The service manager could not be read: {error}"
                )));
            }
            Message::Sort(column) => self.sorting.press(column),
            Message::Filter(filter) => self.filter = filter,
            Message::RunningOnly(running_only) => self.running_only = running_only,
            Message::Select(name, bus) => {
                self.selected = Some((name, bus));
                self.notice = None;
            }
            Message::Control(action) => {
                if let Some((name, bus)) = self.selected.clone() {
                    return blocking::run(move || systemd::control(bus, &name, action))
                        .map(Message::Done);
                }
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

    pub fn view(&self) -> Element<'_, Message> {
        let Some(list) = &self.list else {
            return notice::shown(self.notice.as_ref()).map_or_else(
                || {
                    text("Asking the service manager")
                        .style(styles::text_secondary)
                        .into()
                },
                |notice| notice.view(Message::Dismiss),
            );
        };
        let needle = self.filter.to_lowercase();
        let mut rows: Vec<&Service> = list
            .iter()
            .filter(|service| !self.running_only || service.active == Active::Active)
            .filter(|service| {
                service.name.to_lowercase().contains(&needle)
                    || service.description.to_lowercase().contains(&needle)
            })
            .collect();
        self.sorting
            .sort(&mut rows, |column, a, b| column.compare(a, b));
        let count = rows.len();
        let header = |label, column| {
            sort_header(label, self.sorting.direction(column), Message::Sort(column))
        };

        let selected = self.selected.as_ref();
        let columns = [
            table::column(header("Service", Column::Service), |service: &Service| {
                container(text(service.name.clone()).wrapping(Wrapping::WordOrGlyph))
                    .padding([0, 6])
            })
            .width(Fill),
            table::column(
                header("Description", Column::Description),
                |service: &Service| text(service.description.clone()),
            )
            .width(Fill),
            table::column(header("Status", Column::Status), |service: &Service| {
                text(status(service)).style(match service.active {
                    Active::Active => styles::text_success,
                    Active::Failed => styles::text_danger,
                    _ => styles::text_default,
                })
            }),
            table::column(header("Manager", Column::Manager), |service: &Service| {
                text(if service.bus == Bus::System {
                    "System"
                } else {
                    "User"
                })
            }),
        ];

        let toolbar = row![
            text_input("Filter by name or description", &self.filter)
                .on_input(Message::Filter)
                .width(320),
            checkbox(self.running_only)
                .label("Only active services")
                .on_toggle(Message::RunningOnly),
            space::horizontal(),
            text(format!("{count} services")).style(styles::text_secondary),
        ]
        .spacing(16)
        .padding(8)
        .align_y(iced::Center);

        column![
            toolbar,
            scrolling(selectable::table(
                columns,
                rows,
                |service| Message::Select(service.name.clone(), service.bus),
                |service| {
                    selected.is_some_and(|(name, bus)| *name == service.name && *bus == service.bus)
                },
            ))
            .width(Fill)
            .height(Fill)
        ]
        .extend(self.panel(list))
        .into()
    }

    fn panel<'a>(&'a self, list: &'a [Service]) -> Option<Element<'a, Message>> {
        let (name, bus) = self.selected.as_ref()?;
        let service = list
            .iter()
            .find(|service| &service.name == name && service.bus == *bus)?;
        let action = |label, action| {
            button(label)
                .on_press(Message::Control(action))
                .style(styles::button_secondary)
        };
        Some(
            column![
                text(&service.name).size(20).style(styles::heading),
                text(&service.description).style(styles::text_secondary),
                stats([
                    ("Load state", service.load.clone()),
                    ("Active state", status(service)),
                    ("Detail", service.sub.clone())
                ]),
                row![
                    action("Start", Action::Start),
                    action("Stop", Action::Stop),
                    action("Restart", Action::Restart),
                    notice::shown(self.notice.as_ref())
                        .map_or_else(|| text("").into(), |notice| notice.view(Message::Dismiss)),
                ]
                .spacing(12)
                .align_y(iced::Center),
            ]
            .spacing(8)
            .padding(12)
            .into(),
        )
    }
}

/// Plain words for the manager's state and sub-state pair.
fn status(service: &Service) -> String {
    match (&service.active, service.sub.as_str()) {
        (Active::Active, "running") => "Running".to_owned(),
        (Active::Active, "exited") => "Finished".to_owned(),
        (Active::Active, other) => capitalize(other),
        (Active::Inactive, _) => "Stopped".to_owned(),
        (Active::Failed, _) => "Failed".to_owned(),
        (Active::Activating, _) => "Starting".to_owned(),
        (Active::Deactivating, _) => "Stopping".to_owned(),
        (Active::Reloading, _) => "Reloading".to_owned(),
        (Active::Other(state), _) => capitalize(state),
    }
}

fn capitalize(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::pressed;

    /// The rows in the order pressing `column` once puts them, or twice with `again`.
    fn ordered(column: Column, again: bool, rows: &[Service]) -> Vec<&str> {
        let mut sorting = Sorting::new(column);
        if again {
            sorting.press(column);
        }
        let mut rows: Vec<&Service> = rows.iter().collect();
        sorting.sort(&mut rows, |column, a, b| column.compare(a, b));
        rows.into_iter().map(|row| row.name.as_str()).collect()
    }

    fn named(name: &str, description: &str, bus: Bus, active: Active) -> Service {
        Service {
            name: name.to_owned(),
            description: description.to_owned(),
            bus,
            load: "loaded".to_owned(),
            sub: if active == Active::Active {
                "running"
            } else {
                "dead"
            }
            .to_owned(),
            active,
        }
    }

    fn units() -> Vec<Service> {
        vec![
            named("sshd.service", "Secure shell", Bus::System, Active::Active),
            named("Avahi.service", "mDNS", Bus::User, Active::Failed),
            named("cron.service", "Jobs", Bus::System, Active::Inactive),
        ]
    }

    #[test]
    fn every_column_orders_the_services() {
        let units = units();
        assert_eq!(
            ordered(Column::Service, false, &units),
            ["Avahi.service", "cron.service", "sshd.service"]
        );
        assert_eq!(
            ordered(Column::Description, false, &units),
            ["cron.service", "Avahi.service", "sshd.service"]
        );
        assert_eq!(
            ordered(Column::Status, false, &units),
            ["Avahi.service", "sshd.service", "cron.service"]
        );
        assert_eq!(
            ordered(Column::Manager, false, &units),
            ["sshd.service", "cron.service", "Avahi.service"]
        );
    }

    #[test]
    fn pressing_a_heading_orders_the_table_by_it() {
        let mut services = Services {
            list: Some(units()),
            ..Services::default()
        };
        for message in pressed(services.view(), "Status") {
            drop(services.update(message));
        }
        assert_eq!(services.sorting.direction(Column::Status), Some(false));
    }

    fn service(active: Active, sub: &str) -> Service {
        Service {
            name: "x.service".to_owned(),
            description: String::new(),
            bus: Bus::System,
            load: "loaded".to_owned(),
            active,
            sub: sub.to_owned(),
        }
    }

    #[test]
    fn states_read_in_plain_words() {
        assert_eq!(status(&service(Active::Active, "running")), "Running");
        assert_eq!(status(&service(Active::Active, "exited")), "Finished");
        assert_eq!(status(&service(Active::Active, "listening")), "Listening");
        assert_eq!(status(&service(Active::Inactive, "dead")), "Stopped");
        assert_eq!(status(&service(Active::Failed, "failed")), "Failed");
    }

    #[test]
    fn capitalizing_handles_empty_and_unicode() {
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("élan"), "Élan");
    }
}
