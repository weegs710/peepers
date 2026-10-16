use std::time::Duration;

use iced::time;
use iced::widget::{button, checkbox, column, row, space, table, text, text_input};
use iced::{Element, Fill, Right, Subscription, Task};
use peepers_core::drivers::{self, Driver, State};

use crate::blocking;
use crate::format;
use crate::widgets::{scrolling, stats};

const REFRESH: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Vec<Driver>),
    Filter(String),
    BuiltIn(bool),
    Select(String),
}

#[derive(Debug, Default)]
pub struct Drivers {
    list: Option<Vec<Driver>>,
    filter: String,
    built_in: bool,
    selected: Option<String>,
}

impl Drivers {
    pub fn enter() -> Task<Message> {
        blocking::run(drivers::read).map(Message::Loaded)
    }

    pub fn subscription() -> Subscription<Message> {
        time::every(REFRESH).map(|_| Message::Refresh)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => return Self::enter(),
            Message::Loaded(drivers) => self.list = Some(drivers),
            Message::Filter(filter) => self.filter = filter,
            Message::BuiltIn(built_in) => self.built_in = built_in,
            Message::Select(name) => self.selected = Some(name),
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(drivers) = &self.list else {
            return text("Reading the kernel modules")
                .style(text::secondary)
                .into();
        };
        let needle = self.filter.to_lowercase();
        let rows: Vec<&Driver> = drivers
            .iter()
            .filter(|driver| self.built_in || !driver.built_in())
            .filter(|driver| driver.name.to_lowercase().contains(&needle))
            .collect();
        let count = rows.len();

        let selected = self.selected.as_deref();
        let columns = [
            table::column("Module", move |driver: &Driver| {
                button(text(driver.name.clone()))
                    .on_press(Message::Select(driver.name.clone()))
                    .padding([0, 6])
                    .style(if selected == Some(driver.name.as_str()) {
                        button::primary
                    } else {
                        button::text
                    })
            })
            .width(Fill),
            table::column("Size", |driver: &Driver| {
                text(
                    driver
                        .size
                        .map_or_else(|| "built in".to_owned(), format::bytes),
                )
            })
            .align_x(Right),
            table::column("State", |driver: &Driver| text(state(driver.state))),
            table::column("Used by", |driver: &Driver| text(driver.used_by.len())).align_x(Right),
            table::column("Source version", |driver: &Driver| {
                text(
                    driver
                        .source_version
                        .clone()
                        .unwrap_or_else(format::unavailable),
                )
            }),
        ];

        let toolbar = row![
            text_input("Filter by module name", &self.filter)
                .on_input(Message::Filter)
                .width(320),
            checkbox(self.built_in)
                .label("Built-in modules")
                .on_toggle(Message::BuiltIn),
            space::horizontal(),
            text(format!("{count} modules")).style(text::secondary),
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
        .extend(
            selected
                .and_then(|name| drivers.iter().find(|driver| driver.name == name))
                .map(details),
        )
        .into()
    }
}

fn details(driver: &Driver) -> Element<'_, Message> {
    let list = |names: &[String]| {
        if names.is_empty() {
            format::unavailable()
        } else {
            names.join(", ")
        }
    };
    column![
        text(&driver.name).size(20),
        text(driver.path.as_ref().map_or_else(
            || "Built into the kernel image, so it cannot be unloaded".to_owned(),
            |path| path.display().to_string()
        ))
        .style(text::secondary),
        stats([
            ("Used by", list(&driver.used_by)),
            ("Depends on", list(&driver.depends)),
            ("Parameters", driver.parameters.to_string()),
            (
                "Taint",
                driver.taint.clone().unwrap_or_else(|| "none".to_owned())
            ),
        ]),
    ]
    .spacing(8)
    .padding(12)
    .into()
}

fn state(state: Option<State>) -> &'static str {
    match state {
        Some(State::Live) => "Live",
        Some(State::Loading) => "Loading",
        Some(State::Unloading) => "Unloading",
        None => "Built in",
    }
}
