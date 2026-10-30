use std::cmp::Ordering;
use std::time::Duration;

use iced::time;
use iced::widget::{checkbox, column, container, row, space, table, text, text_input};
use iced::{Element, Fill, Right, Subscription, Task};
use peepers_core::drivers::{self, Driver, State};

use crate::blocking;
use crate::format;
use crate::selectable;
use crate::widgets::{Sortable, Sorting, scrolling, sort_header, stats};

const REFRESH: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Column {
    #[default]
    Module,
    Size,
    State,
    UsedBy,
    SourceVersion,
}

impl Sortable for Column {
    fn descending_first(self) -> bool {
        matches!(self, Self::Size | Self::UsedBy)
    }
}

impl Column {
    fn compare(self, a: &Driver, b: &Driver) -> Ordering {
        match self {
            Self::Module => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Self::Size => a.size.cmp(&b.size),
            Self::State => state(a.state).cmp(state(b.state)),
            Self::UsedBy => a.used_by.len().cmp(&b.used_by.len()),
            Self::SourceVersion => a.source_version.cmp(&b.source_version),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Vec<Driver>),
    Filter(String),
    BuiltIn(bool),
    Select(String),
    Sort(Column),
}

#[derive(Debug, Default)]
pub struct Drivers {
    list: Option<Vec<Driver>>,
    sorting: Sorting<Column>,
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
            Message::Sort(column) => self.sorting.press(column),
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
        let mut rows: Vec<&Driver> = drivers
            .iter()
            .filter(|driver| self.built_in || !driver.built_in())
            .filter(|driver| driver.name.to_lowercase().contains(&needle))
            .collect();
        self.sorting
            .sort(&mut rows, |column, a, b| column.compare(a, b));
        let count = rows.len();
        let header = |label, column| {
            sort_header(label, self.sorting.direction(column), Message::Sort(column))
        };

        let selected = self.selected.as_deref();
        let columns = [
            table::column(header("Module", Column::Module), |driver: &Driver| {
                container(text(driver.name.clone())).padding([0, 6])
            })
            .width(Fill),
            table::column(header("Size", Column::Size), |driver: &Driver| {
                text(
                    driver
                        .size
                        .map_or_else(|| "built in".to_owned(), format::bytes),
                )
            })
            .align_x(Right),
            table::column(header("State", Column::State), |driver: &Driver| {
                text(state(driver.state))
            }),
            table::column(header("Used by", Column::UsedBy), |driver: &Driver| {
                text(driver.used_by.len())
            })
            .align_x(Right),
            table::column(
                header("Source version", Column::SourceVersion),
                |driver: &Driver| {
                    text(
                        driver
                            .source_version
                            .clone()
                            .unwrap_or_else(format::unavailable),
                    )
                },
            ),
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
            scrolling(selectable::table(
                columns,
                rows,
                |driver| Message::Select(driver.name.clone()),
                |driver| selected == Some(driver.name.as_str()),
            ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::pressed;

    /// The rows in the order pressing `column` once puts them, or twice with `again`.
    fn ordered(column: Column, again: bool, rows: &[Driver]) -> Vec<&str> {
        let mut sorting = Sorting::new(column);
        if again {
            sorting.press(column);
        }
        let mut rows: Vec<&Driver> = rows.iter().collect();
        sorting.sort(&mut rows, |column, a, b| column.compare(a, b));
        rows.into_iter().map(|row| row.name.as_str()).collect()
    }

    fn driver(name: &str, size: Option<u64>, used_by: usize, version: Option<&str>) -> Driver {
        Driver {
            name: name.to_owned(),
            size,
            state: size.map(|_| State::Live),
            used_by: vec![String::new(); used_by],
            source_version: version.map(str::to_owned),
            taint: None,
            parameters: 0,
            path: None,
            depends: Vec::new(),
        }
    }

    fn modules() -> Vec<Driver> {
        vec![
            driver("nvme", Some(100), 2, Some("1.0")),
            driver("amdgpu", Some(900), 5, None),
            driver("ext4", None, 0, Some("0.5")),
        ]
    }

    #[test]
    fn every_column_orders_the_modules() {
        let modules = modules();
        assert_eq!(
            ordered(Column::Module, false, &modules),
            ["amdgpu", "ext4", "nvme"]
        );
        assert_eq!(
            ordered(Column::Size, false, &modules),
            ["amdgpu", "nvme", "ext4"]
        );
        assert_eq!(
            ordered(Column::State, false, &modules),
            ["ext4", "nvme", "amdgpu"]
        );
        assert_eq!(
            ordered(Column::UsedBy, false, &modules),
            ["amdgpu", "nvme", "ext4"]
        );
        assert_eq!(
            ordered(Column::SourceVersion, false, &modules),
            ["amdgpu", "ext4", "nvme"]
        );
    }

    #[test]
    fn pressing_a_column_again_turns_the_order_round() {
        let modules = modules();
        assert_eq!(
            ordered(Column::Module, true, &modules),
            ["nvme", "ext4", "amdgpu"]
        );
        assert_eq!(
            ordered(Column::Size, true, &modules),
            ["ext4", "nvme", "amdgpu"]
        );
    }

    #[test]
    fn pressing_a_heading_orders_the_table_by_it() {
        let mut drivers = Drivers {
            list: Some(modules()),
            ..Drivers::default()
        };
        for message in pressed(drivers.view(), "Used by") {
            drop(drivers.update(message));
        }
        assert_eq!(drivers.sorting.direction(Column::UsedBy), Some(true));
    }
}
