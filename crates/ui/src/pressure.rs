use crate::theme::Element;
use crate::theme::styles;
use std::cmp::Ordering;

use iced::Fill;
use iced::widget::{button, column, container, row, scrollable, space, text};
use peepers_core::pressure::Pressure;
use peepers_core::process::{Process, State};
use peepers_core::telemetry::Vitals;

use crate::format;
use crate::graph::{Graph, LABELLED_HEIGHT, Scale, Tone, as_rate, level};
use crate::history::History;
use crate::processes::{Message, Processes};
use crate::widgets::flow;

/// How many of the heaviest processes each list names.
const CONTRIBUTORS: usize = 5;
/// The narrowest a list of processes is drawn.
const LIST_WIDTH: f32 = 300.0;
const LIST_SPACING: f32 = 24.0;
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

/// Choosing a process in a list shows what it is below the lists, so the messages are the
/// process table's own.
pub fn view<'a>(history: &'a History, processes: &'a Processes) -> Element<'a, Message> {
    let Some(latest) = history.latest() else {
        return container(text("Waiting for the first sample").style(styles::text_secondary))
            .center(Fill)
            .into();
    };
    let selected = processes.selected();
    scrollable(
        column![
            text("System pressure").size(24).style(styles::heading),
            summary(latest),
            flow(CARD_WIDTH, CARD_SPACING, move || {
                Resource::ALL
                    .map(|resource| card(history, latest, resource))
                    .into()
            }),
            flow(LIST_WIDTH, LIST_SPACING, move || {
                vec![
                    contributors(
                        "Top processor users",
                        processes.all().iter(),
                        selected,
                        |process| format!("{:.0}%", process.cpu),
                        |a, b| b.cpu.total_cmp(&a.cpu),
                    ),
                    contributors(
                        "Top memory users",
                        processes.all().iter(),
                        selected,
                        |process| format::bytes(process.memory),
                        |a, b| b.memory.cmp(&a.memory),
                    ),
                    column![
                        contributors(
                            "Top storage users",
                            processes
                                .all()
                                .iter()
                                .filter(|process| touches_storage(process)),
                            selected,
                            storage_value,
                            storage_order,
                        ),
                        text("Only your own processes report what they read and write.")
                            .size(12)
                            .style(styles::text_secondary),
                    ]
                    .spacing(8)
                    .into(),
                ]
            }),
        ]
        .extend(processes.panel())
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
        .style(styles::text_secondary)
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
        .style(styles::text_secondary)
        .into(),
    ];
    lines.extend(now.full.map(|full| {
        Element::from(
            text(format!(
                "Everything stalled at once: {:.1}% over 10 s",
                full.avg10
            ))
            .style(styles::text_secondary),
        )
    }));
    container(
        column![
            row![
                text(resource.title())
                    .size(18)
                    .style(styles::section_heading),
                space::horizontal(),
                text(format!("{:.1}%", now.some.avg10))
            ],
            container(
                Graph::new(Scale::Fit { floor: 10.0 })
                    .key("pressure")
                    .instance(match resource {
                        Resource::Cpu => "cpu",
                        Resource::Memory => "memory",
                        Resource::Io => "io",
                    })
                    .line(some, Tone::Warning)
                    .named("Stalled (10 s)")
                    .series("stalled")
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
    .style(styles::container_rounded_box)
    .into()
}

/// The heaviest few of `processes`, each a button that selects it.
fn contributors<'a>(
    title: &'a str,
    processes: impl Iterator<Item = &'a Process>,
    selected: Option<u32>,
    value: impl Fn(&Process) -> String,
    order: impl Fn(&Process, &Process) -> Ordering,
) -> Element<'a, Message> {
    let mut heaviest: Vec<&Process> = processes.collect();
    heaviest.sort_by(|a, b| order(a, b));
    let rows = heaviest.into_iter().take(CONTRIBUTORS).map(|process| {
        button(
            row![
                text(process.name.clone()).width(Fill),
                text(value(process)).style(styles::text_secondary)
            ]
            .spacing(16),
        )
        .width(Fill)
        .padding([2, 6])
        .on_press(Message::Select(process.pid))
        .style(if selected == Some(process.pid) {
            styles::button_secondary
        } else {
            styles::button_text
        })
        .into()
    });
    column![
        text(title).size(18).style(styles::section_heading),
        column(rows).spacing(4)
    ]
    .spacing(8)
    .width(Fill)
    .into()
}

fn waiting_on_disk(process: &Process) -> bool {
    process.state == State::DiskSleep
}

/// A process is a suspect for storage stalls while it waits on the disk or while it moves data.
fn touches_storage(process: &Process) -> bool {
    waiting_on_disk(process) || process.io.is_some_and(|io| io > 0)
}

/// Processes stuck on the disk first, since those are what a stall is made of, then the busiest.
fn storage_order(a: &Process, b: &Process) -> Ordering {
    waiting_on_disk(b)
        .cmp(&waiting_on_disk(a))
        .then(b.io.cmp(&a.io))
}

fn storage_value(process: &Process) -> String {
    let rate = process.io.map(|io| as_rate(level(io)));
    match (waiting_on_disk(process), rate) {
        (true, Some(rate)) => format!("waiting, {rate}"),
        (true, None) => "waiting".to_owned(),
        (false, Some(rate)) => rate,
        (false, None) => format::unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn process(state: State, io: Option<u64>) -> Process {
        Process {
            pid: 1,
            parent: 0,
            name: String::new(),
            state,
            uid: 0,
            threads: 1,
            nice: 0,
            cpu: 0.0,
            memory: 0,
            io,
            started: Duration::ZERO,
        }
    }

    #[test]
    fn a_process_waiting_on_the_disk_ranks_above_a_busier_one_that_is_not() {
        let stuck = process(State::DiskSleep, Some(10));
        let busy = process(State::Running, Some(5_000_000));
        assert_eq!(storage_order(&stuck, &busy), Ordering::Less);
        assert_eq!(storage_order(&busy, &stuck), Ordering::Greater);
    }

    #[test]
    fn among_processes_not_waiting_the_one_moving_more_data_ranks_first() {
        let heavy = process(State::Running, Some(2_000));
        let light = process(State::Running, Some(1_000));
        assert_eq!(storage_order(&heavy, &light), Ordering::Less);
    }

    #[test]
    fn only_a_process_that_waits_or_moves_data_is_a_suspect() {
        assert!(touches_storage(&process(State::DiskSleep, None)));
        assert!(touches_storage(&process(State::Running, Some(1))));
        assert!(!touches_storage(&process(State::Running, Some(0))));
        assert!(!touches_storage(&process(State::Sleeping, None)));
    }

    #[test]
    fn a_waiting_process_says_so_beside_what_it_moves() {
        assert_eq!(storage_value(&process(State::DiskSleep, None)), "waiting");
        assert_eq!(
            storage_value(&process(State::DiskSleep, Some(1_048_576))),
            "waiting, 1.0 MiB/s"
        );
        assert_eq!(
            storage_value(&process(State::Running, Some(1_048_576))),
            "1.0 MiB/s"
        );
    }
}
