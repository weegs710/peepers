use std::path::PathBuf;

use iced::widget::{button, column, row, scrollable, space, text, text_input};
use iced::{Element, Fill, Task};
use peepers_core::cpu;
use peepers_core::report::Report;
use peepers_core::telemetry::Vitals;
use peepers_core::volumes;

use crate::{blocking, storage};

#[derive(Debug, Clone)]
pub enum Message {
    Built(Box<Report>),
    Directory(String),
    Export,
    Exported(Result<PathBuf, String>),
}

#[derive(Debug)]
pub struct Info {
    report: Option<Report>,
    directory: String,
    notice: Option<(String, bool)>,
}

impl Default for Info {
    fn default() -> Self {
        Self {
            report: None,
            directory: storage::default_folder(),
            notice: None,
        }
    }
}

impl Info {
    /// Gathers the report from what the machine reports right now.
    pub fn enter(latest: Option<&Vitals>) -> Task<Message> {
        latest.cloned().map_or_else(Task::none, |vitals| {
            blocking::run(move || {
                Box::new(Report::build(&vitals, &cpu::Info::read(), &volumes::read()))
            })
            .map(Message::Built)
        })
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Built(report) => self.report = Some(*report),
            Message::Directory(directory) => self.directory = directory,
            Message::Export => {
                if let Some(report) = &self.report {
                    let (text, directory) = (report.text(), PathBuf::from(&self.directory));
                    return blocking::run(move || {
                        storage::export(&directory, "system-information", &text)
                    })
                    .map(Message::Exported);
                }
            }
            Message::Exported(result) => {
                self.notice = Some(match result {
                    Ok(path) => (format!("Saved {}", path.display()), false),
                    Err(error) => (format!("The report could not be saved: {error}"), true),
                });
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(report) = &self.report else {
            return text("Collecting system information")
                .style(text::secondary)
                .into();
        };
        let sections = report.sections.iter().map(|section| {
            let rows = section.rows.iter().map(|(key, value)| {
                row![text(key).style(text::secondary).width(220), text(value)]
                    .spacing(16)
                    .into()
            });
            column![text(section.title).size(20), column(rows).spacing(4)]
                .spacing(8)
                .into()
        });
        let notice = self.notice.as_ref().map(|(message, failed)| {
            Element::from(text(message).style(if *failed { text::danger } else { text::success }))
        });
        column![
            row![
                text("System information").size(24),
                space::horizontal(),
                text_input("Folder for the exported report", &self.directory)
                    .on_input(Message::Directory)
                    .width(360),
                button("Export").on_press(Message::Export),
            ]
            .spacing(12)
            .align_y(iced::Center),
        ]
        .extend(notice)
        .push(
            scrollable(column(sections).spacing(20))
                .width(Fill)
                .height(Fill),
        )
        .spacing(16)
        .padding(16)
        .into()
    }
}
