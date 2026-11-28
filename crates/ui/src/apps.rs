use crate::theme::Element;
use crate::theme::styles;
use std::cmp::Ordering;

use iced::widget::{button, checkbox, column, container, row, space, table, text, text_input};
use iced::{Fill, Task};
use peepers_core::apps::{self, App, Scope};

use crate::blocking;
use crate::format;
use crate::selectable;
use crate::widgets::{Sortable, Sorting, scrolling, sort_header, stats};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Column {
    #[default]
    Name,
    InstalledFor,
    Categories,
}

impl Sortable for Column {
    fn descending_first(self) -> bool {
        false
    }
}

impl Column {
    fn compare(self, a: &App, b: &App) -> Ordering {
        match self {
            Self::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Self::InstalledFor => scope(a.scope).cmp(scope(b.scope)),
            Self::Categories => a.categories.cmp(&b.categories),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Vec<App>),
    Sort(Column),
    Filter(String),
    Hidden(bool),
    Select(String),
}

#[derive(Debug, Default)]
pub struct Apps {
    list: Option<Vec<App>>,
    sorting: Sorting<Column>,
    filter: String,
    hidden: bool,
    selected: Option<String>,
}

impl Apps {
    pub fn enter() -> Task<Message> {
        blocking::run(apps::read).map(Message::Loaded)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => return Self::enter(),
            Message::Loaded(apps) => self.list = Some(apps),
            Message::Sort(column) => self.sorting.press(column),
            Message::Filter(filter) => self.filter = filter,
            Message::Hidden(hidden) => self.hidden = hidden,
            Message::Select(id) => self.selected = Some(id),
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(apps) = &self.list else {
            return text("Reading the desktop entries")
                .style(styles::text_secondary)
                .into();
        };
        let needle = self.filter.to_lowercase();
        let mut rows: Vec<&App> = apps
            .iter()
            .filter(|app| self.hidden || !app.hidden)
            .filter(|app| {
                app.name.to_lowercase().contains(&needle)
                    || app
                        .categories
                        .iter()
                        .any(|c| c.to_lowercase().contains(&needle))
            })
            .collect();
        self.sorting
            .sort(&mut rows, |column, a, b| column.compare(a, b));
        let count = rows.len();
        let header = |label, column| {
            sort_header(label, self.sorting.direction(column), Message::Sort(column))
        };

        let selected = self.selected.as_deref();
        let columns = [
            table::column(header("Name", Column::Name), |app: &App| {
                container(text(app.name.clone())).padding([0, 6])
            })
            .width(Fill),
            table::column(
                header("Installed for", Column::InstalledFor),
                |app: &App| text(scope(app.scope)),
            ),
            table::column(header("Categories", Column::Categories), |app: &App| {
                text(app.categories.join(", "))
            })
            .width(Fill),
        ];

        let toolbar = row![
            text_input("Filter by name or category", &self.filter)
                .on_input(Message::Filter)
                .width(320),
            checkbox(self.hidden)
                .label("Show entries without a launcher")
                .on_toggle(Message::Hidden),
            button("Refresh")
                .on_press(Message::Refresh)
                .style(styles::button_secondary),
            space::horizontal(),
            text(format!("{count} applications")).style(styles::text_secondary),
        ]
        .spacing(16)
        .padding(8)
        .align_y(iced::Center);

        column![
            toolbar,
            scrolling(selectable::table(
                columns,
                rows,
                |app| Message::Select(app.id.clone()),
                |app| selected == Some(app.id.as_str()),
            ))
            .width(Fill)
            .height(Fill)
        ]
        .extend(
            selected
                .and_then(|id| apps.iter().find(|app| app.id == id))
                .map(details),
        )
        .into()
    }
}

fn details(app: &App) -> Element<'_, Message> {
    column![
        text(&app.name).size(20).style(styles::heading),
        text(app.comment.clone().unwrap_or_default()).style(styles::text_secondary),
        stats([
            (
                "Command",
                app.command.clone().unwrap_or_else(format::unavailable)
            ),
            ("Icon", app.icon.clone().unwrap_or_else(format::unavailable)),
            ("Entry", app.path.display().to_string()),
        ]),
    ]
    .spacing(8)
    .padding(12)
    .into()
}

fn scope(scope: Scope) -> &'static str {
    match scope {
        Scope::User => "This user",
        Scope::System => "Everyone",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::widgets::pressed;

    /// The rows in the order pressing `column` once puts them, or twice with `again`.
    fn ordered(column: Column, again: bool, rows: &[App]) -> Vec<&str> {
        let mut sorting = Sorting::new(column);
        if again {
            sorting.press(column);
        }
        let mut rows: Vec<&App> = rows.iter().collect();
        sorting.sort(&mut rows, |column, a, b| column.compare(a, b));
        rows.into_iter().map(|row| row.name.as_str()).collect()
    }

    fn app(name: &str, scope: Scope, categories: &[&str]) -> App {
        App {
            id: format!("{name}.desktop"),
            name: name.to_owned(),
            comment: None,
            command: None,
            icon: None,
            categories: categories.iter().map(|c| (*c).to_owned()).collect(),
            path: PathBuf::new(),
            scope,
            hidden: false,
        }
    }

    fn installed() -> Vec<App> {
        vec![
            app("Zed", Scope::User, &["Development"]),
            app("blender", Scope::System, &["Graphics", "3DGraphics"]),
            app("Mpv", Scope::System, &["AudioVideo"]),
        ]
    }

    #[test]
    fn every_column_orders_the_applications() {
        let installed = installed();
        assert_eq!(
            ordered(Column::Name, false, &installed),
            ["blender", "Mpv", "Zed"]
        );
        assert_eq!(
            ordered(Column::InstalledFor, false, &installed),
            ["blender", "Mpv", "Zed"]
        );
        assert_eq!(
            ordered(Column::Categories, false, &installed),
            ["Mpv", "Zed", "blender"]
        );
    }

    #[test]
    fn pressing_a_heading_orders_the_table_by_it() {
        let mut apps = Apps {
            list: Some(installed()),
            ..Apps::default()
        };
        for message in pressed(apps.view(), "Categories") {
            drop(apps.update(message));
        }
        assert_eq!(apps.sorting.direction(Column::Categories), Some(false));
    }
}
