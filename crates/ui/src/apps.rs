use iced::widget::{button, checkbox, column, row, space, table, text, text_input};
use iced::{Element, Fill, Task};
use peepers_core::apps::{self, App, Scope};

use crate::blocking;
use crate::format;
use crate::widgets::{scrolling, stats};

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Vec<App>),
    Filter(String),
    Hidden(bool),
    Select(String),
}

#[derive(Debug, Default)]
pub struct Apps {
    list: Option<Vec<App>>,
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
            Message::Filter(filter) => self.filter = filter,
            Message::Hidden(hidden) => self.hidden = hidden,
            Message::Select(id) => self.selected = Some(id),
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(apps) = &self.list else {
            return text("Reading the desktop entries")
                .style(text::secondary)
                .into();
        };
        let needle = self.filter.to_lowercase();
        let rows: Vec<&App> = apps
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
        let count = rows.len();

        let selected = self.selected.as_deref();
        let columns = [
            table::column("Name", move |app: &App| {
                button(text(app.name.clone()))
                    .on_press(Message::Select(app.id.clone()))
                    .padding([0, 6])
                    .style(if selected == Some(app.id.as_str()) {
                        button::primary
                    } else {
                        button::text
                    })
            })
            .width(Fill),
            table::column("Installed for", |app: &App| text(scope(app.scope))),
            table::column("Categories", |app: &App| text(app.categories.join(", "))).width(Fill),
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
                .style(button::secondary),
            space::horizontal(),
            text(format!("{count} applications")).style(text::secondary),
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
                .and_then(|id| apps.iter().find(|app| app.id == id))
                .map(details),
        )
        .into()
    }
}

fn details(app: &App) -> Element<'_, Message> {
    column![
        text(&app.name).size(20),
        text(app.comment.clone().unwrap_or_default()).style(text::secondary),
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
