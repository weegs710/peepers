use iced::widget::{column, container, row, scrollable, space, text};
use iced::{Element, Fill};
use peepers_core::pressure::Pressure;
use peepers_core::process::Process;
use peepers_core::telemetry::Vitals;

use crate::format;
use crate::graph::{Graph, LABELLED_HEIGHT, Scale, Tone, level};
use crate::history::History;
use crate::processes::Processes;
use crate::widgets::flow;

/// How many of the heaviest processes each list names.
const CONTRIBUTORS: usize = 5;
/// The narrowest a resource's card is drawn.
const CARD_WIDTH: f32 = 340.0;
const CARD_SPACING: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resource {
    Cpu,
    Memory,
    Io,
}

impl Resource {
    const ALL: [Self; 3] = [Self::Cpu, Self::Memory, Self::Io];

    fn title(self) -> &'static str {
        match self {
            Self::Cpu => "Processor stalls",
            Self::Memory => "Memory stalls",
            Self::Io => "Storage stalls",
        }
    }

    fn of(self, vitals: &Vitals) -> Option<Pressure> {
        match self {
            Self::Cpu => vitals.pressure.cpu,
            Self::Memory => vitals.pressure.memory,
            Self::Io => vitals.pressure.io,
        }
    }
}

pub fn view<'a, Message: 'a>(history: &'a History, processes: &Processes) -> Element<'a, Message> {
    let Some(latest) = history.latest() else {
        return container(text("Waiting for the first sample").style(text::secondary))
            .center(Fill)
            .into();
    };
    scrollable(
        column![
            text("System pressure").size(24),
            summary(latest),
            flow(CARD_WIDTH, CARD_SPACING, move || {
                Resource::ALL
                    .map(|resource| card(history, latest, resource))
                    .into()
            }),
            row![
                contributors(
                    "Top processor users",
                    processes.all(),
                    |process| format!("{:.0}%", process.cpu),
                    |a, b| b.cpu.total_cmp(&a.cpu)
                ),
                contributors(
                    "Top memory users",
                    processes.all(),
                    |process| format::bytes(process.memory),
                    |a, b| b.memory.cmp(&a.memory)
                ),
            ]
            .spacing(24),
        ]
        .spacing(16)
        .padding(16),
    )
    .width(Fill)
    .height(Fill)
    .into()
}

/// One sentence on what the machine is waiting for, and how loaded the scheduler is.
fn summary<'a, Message: 'a>(latest: &Vitals) -> Element<'a, Message> {
    let driver = Resource::ALL
        .into_iter()
        .filter_map(|resource| Some((resource, resource.of(latest)?.some.avg10)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, stalled)| *stalled >= 1.0);
    let driver = driver.map_or_else(
        || "Nothing is waiting on a resource.".to_owned(),
        |(resource, stalled)| {
            format!(
                "Mostly waiting on {}: tasks stalled {stalled:.1}% of the last 10 seconds.",
                resource.title().to_lowercase()
            )
        },
    );
    let per_processor = latest.load.one / level(latest.cpu.cores.len() as u64).max(1.0);
    column![
        text(driver),
        text(format!(
            "{per_processor:.2} runnable tasks per logical processor over the last minute"
        ))
        .style(text::secondary)
    ]
    .spacing(2)
    .into()
}

fn card<'a, Message: 'a>(
    history: &History,
    latest: &Vitals,
    resource: Resource,
) -> Element<'a, Message> {
    let Some(now) = resource.of(latest) else {
        return container(text(format!(
            "{}: not reported by this kernel",
            resource.title()
        )))
        .padding(12)
        .into();
    };
    let some = history.series(|vitals| {
        resource
            .of(vitals)
            .map_or(0.0, |pressure| pressure.some.avg10)
    });
    let mut lines = vec![
        text(format!(
            "Over 10 s {:.1}%, 60 s {:.1}%, 5 min {:.1}%",
            now.some.avg10, now.some.avg60, now.some.avg300
        ))
        .style(text::secondary)
        .into(),
    ];
    lines.extend(now.full.map(|full| {
        Element::from(
            text(format!(
                "Everything stalled at once: {:.1}% over 10 s",
                full.avg10
            ))
            .style(text::secondary),
        )
    }));
    container(
        column![
            row![
                text(resource.title()).size(18),
                space::horizontal(),
                text(format!("{:.1}%", now.some.avg10))
            ],
            container(
                Graph::new(Scale::Fit { floor: 10.0 })
                    .line(some, Tone::Warning)
                    .named("Stalled (10 s)")
                    .readout(|value| format!("{value:.1}%"))
                    .view()
            )
            .height(LABELLED_HEIGHT),
            column(lines).spacing(2),
        ]
        .spacing(8),
    )
    .width(Fill)
    .padding(12)
    .style(container::rounded_box)
    .into()
}

fn contributors<'a, Message: 'a>(
    title: &'a str,
    processes: &[Process],
    value: impl Fn(&Process) -> String,
    order: impl Fn(&Process, &Process) -> std::cmp::Ordering,
) -> Element<'a, Message> {
    let mut heaviest: Vec<&Process> = processes.iter().collect();
    heaviest.sort_by(|a, b| order(a, b));
    let rows = heaviest.into_iter().take(CONTRIBUTORS).map(|process| {
        row![
            text(process.name.clone()).width(Fill),
            text(value(process)).style(text::secondary)
        ]
        .spacing(16)
        .into()
    });
    column![text(title).size(18), column(rows).spacing(4)]
        .spacing(8)
        .width(Fill)
        .into()
}
