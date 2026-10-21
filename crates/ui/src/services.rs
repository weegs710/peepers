use std::time::Duration;

use iced::time;
use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, row, space, table, text, text_input};
use iced::{Element, Fill, Subscription, Task};
use peepers_core::systemd::{self, Action, Active, Bus, Service};

use crate::blocking;
use crate::widgets::{scrolling, stats};

const REFRESH: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Result<Vec<Service>, String>),
    Filter(String),
    RunningOnly(bool),
    Select(String, Bus),
    Control(Action),
    Done(Result<(), String>),
}

#[derive(Debug, Default)]
pub struct Services {
    list: Option<Vec<Service>>,
    filter: String,
    running_only: bool,
    selected: Option<(String, Bus)>,
    notice: Option<(String, bool)>,
}

impl Services {
    pub fn enter() -> Task<Message> {
        blocking::run(systemd::read).map(Message::Loaded)
    }

    pub fn subscription() -> Subscription<Message> {
        time::every(REFRESH).map(|_| Message::Refresh)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => return Self::enter(),
            Message::Loaded(Ok(list)) => self.list = Some(list),
            Message::Loaded(Err(error)) => {
                self.notice = Some((
                    format!("The service manager could not be read: {error}"),
                    true,
                ));
            }
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
                    Ok(()) => ("Requested".to_owned(), false),
                    Err(error) => (error, true),
                });
                return Self::enter();
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(list) = &self.list else {
            return match &self.notice {
                Some((error, _)) => text(error).style(text::danger),
                None => text("Asking the service manager").style(text::secondary),
            }
            .into();
        };
        let needle = self.filter.to_lowercase();
        let rows: Vec<&Service> = list
            .iter()
            .filter(|service| !self.running_only || service.active == Active::Active)
            .filter(|service| {
                service.name.to_lowercase().contains(&needle)
                    || service.description.to_lowercase().contains(&needle)
            })
            .collect();
        let count = rows.len();

        let selected = self.selected.as_ref();
        let columns = [
            table::column("Service", move |service: &Service| {
                button(text(service.name.clone()).wrapping(Wrapping::WordOrGlyph))
                    .on_press(Message::Select(service.name.clone(), service.bus))
                    .padding([0, 6])
                    .style(if selected == Some(&(service.name.clone(), service.bus)) {
                        button::primary
                    } else {
                        button::text
                    })
            })
            .width(Fill),
            table::column("Description", |service: &Service| {
                text(service.description.clone())
            })
            .width(Fill),
            table::column("Status", |service: &Service| {
                text(status(service)).style(match service.active {
                    Active::Active => text::success,
                    Active::Failed => text::danger,
                    _ => text::default,
                })
            }),
            table::column("Manager", |service: &Service| {
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
            text(format!("{count} services")).style(text::secondary),
        ]
        .spacing(16)
        .padding(8)
        .align_y(iced::Center);

        column![
            toolbar,
            scrolling(table(columns, rows).padding_x(10).padding_y(2))
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
                .style(button::secondary)
        };
        Some(
            column![
                text(&service.name).size(20),
                text(&service.description).style(text::secondary),
                stats([
                    ("Load state", service.load.clone()),
                    ("Active state", status(service)),
                    ("Detail", service.sub.clone())
                ]),
                row![
                    action("Start", Action::Start),
                    action("Stop", Action::Stop),
                    action("Restart", Action::Restart),
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
