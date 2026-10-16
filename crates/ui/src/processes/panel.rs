use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

use iced::widget::{button, checkbox, column, container, pick_list, row, space, text};
use iced::{Element, Fill};
use peepers_core::process::{Details, Process, Signal};

use super::Message;
use crate::format;
use crate::widgets::stats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    High,
    Normal,
    Low,
    Lowest,
}

impl Priority {
    pub const ALL: [Self; 4] = [Self::High, Self::Normal, Self::Low, Self::Lowest];

    pub fn nice(self) -> i32 {
        match self {
            Self::High => -5,
            Self::Normal => 0,
            Self::Low => 10,
            Self::Lowest => 19,
        }
    }

    fn of(nice: i32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|priority| priority.nice() == nice)
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::High => "High",
            Self::Normal => "Normal",
            Self::Low => "Low",
            Self::Lowest => "Lowest",
        };
        write!(formatter, "{name} ({})", self.nice())
    }
}

pub struct Panel<'a> {
    pub process: &'a Process,
    pub details: &'a Details,
    pub user: Cow<'a, str>,
    pub parent: Option<&'a str>,
    pub running_for: Duration,
    pub cpus: &'a [usize],
}

pub fn view<'a>(panel: &Panel<'a>) -> Element<'a, Message> {
    let Panel {
        process, details, ..
    } = panel;
    let unavailable = format::unavailable;
    let path = |path: &Option<std::path::PathBuf>| {
        path.as_ref()
            .map_or_else(unavailable, |path| path.display().to_string())
    };
    let facts = [
        ("User", panel.user.to_string()),
        ("State", process.state.label().to_owned()),
        (
            "Parent",
            panel.parent.map_or_else(
                || process.parent.to_string(),
                |name| format!("{name} ({})", process.parent),
            ),
        ),
        ("Threads", process.threads.to_string()),
        ("Running for", format::uptime(panel.running_for)),
        (
            "File descriptors",
            details
                .descriptors
                .map_or_else(unavailable, |count| count.to_string()),
        ),
        (
            "Read from disk",
            details.read_bytes.map_or_else(unavailable, format::bytes),
        ),
        (
            "Written to disk",
            details.write_bytes.map_or_else(unavailable, format::bytes),
        ),
    ];
    let signal = |label, signal| {
        button(label)
            .on_press(Message::Signal(signal))
            .style(button::secondary)
    };
    let tree = |label, signal| {
        button(label)
            .on_press(Message::SignalTree(signal))
            .style(button::secondary)
    };
    let actions = row![
        signal("End task", Signal::Terminate),
        signal("Force quit", Signal::Kill),
        tree("End process tree", Signal::Terminate),
        tree("Force quit tree", Signal::Kill),
        signal("Suspend", Signal::Stop),
        signal("Resume", Signal::Continue),
        button("Sample")
            .on_press(Message::Sample)
            .style(button::secondary),
        space::horizontal(),
        text("Priority").style(text::secondary),
        pick_list(Priority::ALL, Priority::of(process.nice), Message::Priority),
    ]
    .spacing(8)
    .align_y(iced::Center);

    column![
        text(format!("{} ({})", process.name, process.pid)).size(20),
        text(details.command.clone().unwrap_or_else(unavailable)).style(text::secondary),
        text(format!(
            "{}    {}",
            path(&details.executable),
            path(&details.directory)
        ))
        .size(12)
        .style(text::secondary),
        stats(facts),
        affinity(panel.cpus, details.affinity.as_deref()),
        actions,
    ]
    .spacing(10)
    .padding(12)
    .into()
}

/// One checkbox per logical processor; ticking one applies at once.
fn affinity<'a>(cpus: &[usize], allowed: Option<&[usize]>) -> Element<'a, Message> {
    let Some(allowed) = allowed else {
        return text("Processor affinity: not readable")
            .style(text::secondary)
            .into();
    };
    column![
        text("Processor affinity").style(text::secondary).size(12),
        container(
            row(cpus.iter().map(|&cpu| {
                checkbox(allowed.contains(&cpu))
                    .label(cpu.to_string())
                    .on_toggle(move |_| Message::Affinity(cpu))
                    .into()
            }))
            .spacing(12)
            .wrap(),
        )
        .width(Fill),
    ]
    .spacing(4)
    .into()
}
