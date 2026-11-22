use std::sync::Arc;

use iced::widget::{button, column, container, pick_list, row, rule, scrollable, text, text_input};
use iced::{Element, Fill};
use peepers_core::energy::{CPU, Kind, Report, Row};

use crate::graph::{Graph, Scale, Tone};
use crate::widgets::fold_header;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Range {
    #[default]
    Day,
    Week,
    Month,
    Quarter,
}

impl Range {
    pub fn hours(self) -> u64 {
        match self {
            Self::Day => 24,
            Self::Week => 7 * 24,
            Self::Month => 30 * 24,
            Self::Quarter => 90 * 24,
        }
    }
}
impl std::fmt::Display for Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Day => "24 hours",
            Self::Week => "7 days",
            Self::Month => "30 days",
            Self::Quarter => "90 days",
        })
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Reported(Arc<Report>),
    Range(Range),
    Select(String),
    Filter(String),
    Activity(bool),
    Fold,
}

#[derive(Default)]
pub struct Energy {
    report: Option<Arc<Report>>,
    range: Range,
    selected: Option<String>,
    filter: String,
    activity: bool,
    /// Folded at every start, because the open section is far taller than the window.
    open: bool,
    pub save_error: Option<String>,
    pub saving: bool,
}

impl Energy {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Reported(report) => self.report = Some(report),
            Message::Range(range) => self.range = range,
            Message::Select(id) => {
                self.activity = self
                    .report
                    .as_ref()
                    .and_then(|r| r.rows.iter().find(|r| r.entity.id == id))
                    .is_some_and(|r| {
                        r.entity.kind == Kind::DeviceActivity
                            || (matches!(r.entity.kind, Kind::Application | Kind::Service)
                                && r.metrics.measured_seconds == 0.0)
                    });
                self.selected = Some(id);
            }
            Message::Filter(filter) => self.filter = filter,
            Message::Activity(activity) => self.activity = activity,
            Message::Fold => self.open = !self.open,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let header = fold_header("Energy history", self.open, Message::Fold);
        if !self.open {
            let warning = self
                .report
                .as_ref()
                .and_then(|report| report.warning.as_ref());
            return column![header]
                .extend(warning.map(|warning| text(warning).style(text::danger).into()))
                .spacing(12)
                .into();
        }
        let mut content = column![
            row![header, pick_list([Range::Day, Range::Week, Range::Month, Range::Quarter], Some(self.range), Message::Range)].spacing(24).align_y(iced::Center),
            text("Recorded while Peepers is running, including in the tray. Display pause does not pause collection.").style(text::secondary),
            text("CPU shares are estimates from CPU activity, not whole-app power or predicted savings. Hardware channels overlap: do not add them together.").style(text::secondary),
        ].spacing(12);
        let Some(report) = &self.report else {
            return content.push(text("Loading saved energy history…")).into();
        };
        if report.hours != self.range.hours() {
            return content.push(text("Loading selected range…")).into();
        }
        content = content.push(text(&report.status));
        if let Some(warning) = &report.warning {
            content = content.push(text(warning).style(text::danger));
        }
        content = content.push(
            text(format!(
                "{} — {} UTC · {} recorded · hourly records retained for 90 days",
                timestamp(report.start),
                timestamp(report.end),
                duration(report.observed_seconds)
            ))
            .size(13)
            .style(text::secondary),
        );
        if let Some(directory) = &report.directory {
            content = content.push(
                text(format!(
                    "Saved in {} · checkpoint every 30 seconds and on Quit",
                    directory.display()
                ))
                .size(12)
                .style(text::secondary),
            );
        }
        let selected = self.selected.as_deref().unwrap_or(CPU);
        if let Some(row) = report.rows.iter().find(|r| r.entity.id == selected) {
            content = content.push(Self::detail(report, row, self.activity));
        }
        content = content.push(
            text_input("Filter apps, services and hardware…", &self.filter)
                .on_input(Message::Filter),
        );
        for (title, kinds) in [
            (
                "Applications and services — estimated CPU energy",
                &[Kind::Application, Kind::Service, Kind::Unattributed][..],
            ),
            (
                "Hardware readings — separate, overlapping channels",
                &[Kind::Hardware][..],
            ),
            (
                "Device activity — watts unavailable here; see sensors above",
                &[Kind::DeviceActivity][..],
            ),
        ] {
            content = content.push(Self::table(report, title, kinds, &self.filter));
        }
        content.push(text("Energy and averages cover available readings only. “—” means unavailable. CPU-active time and bursts describe sampled activity; short-lived work between samples can be missed. Peaks are interval averages.").size(13).style(text::secondary)).into()
    }

    fn table<'a>(
        report: &'a Report,
        title: &'static str,
        kinds: &[Kind],
        filter: &str,
    ) -> Element<'a, Message> {
        let needle = filter.to_lowercase();
        let mut table = column![
            text(title).size(20),
            row![
                text("Name / source").width(Fill),
                text("Energy").width(95),
                text("Avg / peak").width(135),
                text("Active").width(95),
                text("Bursts").width(65)
            ]
            .spacing(12),
            rule::horizontal(1)
        ]
        .spacing(6);
        let mut count = 0;
        for row in report
            .rows
            .iter()
            .filter(|r| kinds.contains(&r.entity.kind))
            .filter(|r| r.entity.name.to_lowercase().contains(&needle))
        {
            count += 1;
            let m = &row.metrics;
            let active = matches!(
                row.entity.kind,
                Kind::Application | Kind::Service | Kind::DeviceActivity
            );
            table = table.push(
                row![
                    button(column![
                        text(&row.entity.name),
                        text(row.entity.kind.label())
                            .size(12)
                            .style(text::secondary)
                    ])
                    .style(button::text)
                    .on_press(Message::Select(row.entity.id.clone()))
                    .width(Fill),
                    text(energy(m.watt_hours())).width(95),
                    text(format!(
                        "{} / {}",
                        watts(m.average_watts()),
                        watts(m.peak_watts)
                    ))
                    .width(135),
                    text(if active {
                        duration(m.active_seconds)
                    } else {
                        "—".into()
                    })
                    .width(95),
                    text(if active {
                        m.bursts.to_string()
                    } else {
                        "—".into()
                    })
                    .width(65),
                ]
                .spacing(12)
                .align_y(iced::Center),
            );
        }
        if count == 0 {
            table = table.push(text("No matching readings in this range").style(text::secondary));
        }
        table.into()
    }

    fn detail<'a>(report: &'a Report, row: &'a Row, activity: bool) -> Element<'a, Message> {
        let daily = report.hours > 24;
        let metrics = &row.metrics;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "graph pixels need only f32 precision"
        )]
        let values = report
            .periods
            .iter()
            .map(|p| {
                p.rows
                    .get(&row.entity.id)
                    .and_then(|r| {
                        if activity {
                            Some(r.metrics.active_seconds / 60.0)
                        } else {
                            r.metrics.watt_hours()
                        }
                    })
                    .map(|v| v as f32)
            })
            .collect();
        let graph = Graph::new(Scale::Fit { floor: 0.01 })
            .span(report.periods.len())
            .optional_line(values, Tone::Primary)
            .named(&row.entity.name)
            .readout(move |v| {
                if activity {
                    format!("{v:.1} min")
                } else {
                    format!("{v:.3} Wh")
                }
            });
        let controls: Element<'a, Message> = if matches!(
            row.entity.kind,
            Kind::Application | Kind::Service | Kind::DeviceActivity
        ) {
            row![
                button("Energy timeline").on_press(Message::Activity(false)),
                button("Activity timeline").on_press(Message::Activity(true))
            ]
            .spacing(8)
            .into()
        } else {
            row![].into()
        };
        let timeline = report.periods.iter().rev().filter_map(|p| {
            let m = &p.rows.get(&row.entity.id)?.metrics;
            Some(
                row![
                    text(timestamp(p.start)).width(155),
                    text(energy(m.watt_hours())).width(100),
                    text(format!(
                        "{} covered · {} active",
                        duration(m.measured_seconds),
                        duration(m.active_seconds)
                    ))
                    .style(text::secondary)
                ]
                .spacing(12)
                .into(),
            )
        });
        container(
            column![
                text(&row.entity.name).size(22),
                text(format!(
                    "{} · {} available / {} observed · average {} · peak {}",
                    energy(metrics.watt_hours()),
                    duration(metrics.measured_seconds),
                    duration(metrics.observed_seconds),
                    watts(metrics.average_watts()),
                    watts(metrics.peak_watts)
                )),
                controls,
                text(if activity && daily {
                    "Activity per UTC day · oldest → newest"
                } else if activity {
                    "Activity per UTC hour · oldest → newest"
                } else if daily {
                    "Energy per UTC day · oldest → newest"
                } else {
                    "Energy per UTC hour · oldest → newest"
                })
                .style(text::secondary),
                container(graph.view()).height(160).width(Fill),
                scrollable(column(timeline).spacing(4)).height(120),
            ]
            .spacing(8),
        )
        .padding(12)
        .style(container::bordered_box)
        .into()
    }
}

fn timestamp(seconds: u64) -> String {
    i64::try_from(seconds)
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .map_or_else(
            || "Unknown time".into(),
            |t| t.strftime("%Y-%m-%d %H:%M").to_string(),
        )
}

fn duration(seconds: f64) -> String {
    if seconds >= 3600.0 {
        format!("{:.1} h", seconds / 3600.0)
    } else if seconds >= 60.0 {
        format!("{:.1} min", seconds / 60.0)
    } else {
        format!("{seconds:.0} s")
    }
}

fn energy(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{v:.3} Wh"))
}
fn watts(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{v:.2} W"))
}
