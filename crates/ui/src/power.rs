use iced::widget::{column, container, row, scrollable, space, text};
use iced::{Element, Fill};
use peepers_core::sensors::{Chip, Kind, Reading};
use peepers_core::telemetry::Vitals;

use crate::format;
use crate::graph::{Graph, Scale, Tone};
use crate::history::History;
use crate::settings::Temperature;

pub fn view<'a, Message: 'a>(history: &History, unit: Temperature) -> Element<'a, Message> {
    let Some(latest) = history.latest() else {
        return container(text("Waiting for the first sample").style(text::secondary))
            .center(Fill)
            .into();
    };
    let sections = std::iter::once(processor(history, latest))
        .chain(std::iter::once(domains(latest)))
        .chain(
            latest
                .chips
                .iter()
                .filter(|chip| !chip.readings.is_empty())
                .map(|chip| section(history, chip, unit)),
        );
    scrollable(column(sections).spacing(24).padding(16))
        .width(Fill)
        .height(Fill)
        .into()
}

fn processor<'a, Message: 'a>(history: &History, latest: &Vitals) -> Element<'a, Message> {
    let frequencies: Vec<u64> = latest
        .cpu
        .cores
        .iter()
        .filter_map(|core| core.frequency_khz)
        .collect();
    let average = |frequencies: &[u64]| {
        frequencies
            .iter()
            .sum::<u64>()
            .checked_div(frequencies.len() as u64)
    };
    let line = history.series(|vitals| {
        let khz: Vec<u64> = vitals
            .cpu
            .cores
            .iter()
            .filter_map(|core| core.frequency_khz)
            .collect();
        crate::graph::level(average(&khz).unwrap_or(0))
    });
    let current = average(&frequencies).map_or_else(format::unavailable, format::frequency);
    let fastest = frequencies
        .iter()
        .max()
        .map_or_else(format::unavailable, |&khz| format::frequency(khz));
    column![
        text("Processor clocks").size(20),
        row![
            container(
                Graph::new(Scale::Fit { floor: 1_000_000.0 })
                    .line(line, Tone::Primary)
                    .view()
            )
            .width(240)
            .height(48),
            column![
                text(format!("Average {current}")),
                text(format!("Fastest core {fastest}")).style(text::secondary)
            ]
            .spacing(2),
        ]
        .spacing(16),
        row(latest.cpu.cores.iter().map(|core| {
            column![
                text(format!("CPU {}", core.id))
                    .size(12)
                    .style(text::secondary),
                text(
                    core.frequency_khz
                        .map_or_else(format::unavailable, format::frequency)
                ),
            ]
            .width(110)
            .into()
        }))
        .spacing(12)
        .wrap()
        .vertical_spacing(8),
    ]
    .spacing(10)
    .into()
}

fn domains<'a, Message: 'a>(latest: &Vitals) -> Element<'a, Message> {
    let rows = latest.power.iter().map(|domain| {
        let value = match (domain.readable, domain.watts) {
            (true, Some(watts)) => format!("{watts:.1} W"),
            (true, None) => "Measuring".to_owned(),
            (false, _) => "Needs read permission, which by default only root has".to_owned(),
        };
        row![text(domain.name.clone()).width(180), text(value)]
            .spacing(16)
            .into()
    });
    column![text("Package power").size(20), column(rows).spacing(4)]
        .spacing(10)
        .into()
}

fn section<'a, Message: 'a>(
    history: &History,
    chip: &Chip,
    unit: Temperature,
) -> Element<'a, Message> {
    let title = title(chip);
    let rows = chip
        .readings
        .iter()
        .map(|reading| reading_row(history, &chip.id, reading, unit));
    column![
        row![
            text(title).size(20),
            space::horizontal(),
            text(format!("{} ({})", chip.name, chip.id)).style(text::secondary)
        ],
        column(rows).spacing(6),
    ]
    .spacing(10)
    .into()
}

/// What the chip measures. The PCI name of a processor's or memory module's sensor function is
/// the name of a bridge on the chipset, so those drivers get a plain role instead.
fn title(chip: &Chip) -> String {
    match chip.name.as_str() {
        "k10temp" | "coretemp" | "zenpower" => "Processor".to_owned(),
        "spd5118" | "jc42" => "Memory module".to_owned(),
        "acpitz" => "Firmware thermal zone".to_owned(),
        _ => chip.model.clone().unwrap_or_else(|| chip.name.clone()),
    }
}

fn reading_row<'a, Message: 'a>(
    history: &History,
    chip: &str,
    reading: &Reading,
    unit: Temperature,
) -> Element<'a, Message> {
    let values = history.series(|vitals| {
        vitals
            .chips
            .iter()
            .find(|candidate| candidate.id == chip)
            .and_then(|candidate| {
                candidate
                    .readings
                    .iter()
                    .find(|other| other.kind == reading.kind && other.label == reading.label)
            })
            .map_or(0.0, Reading::value)
    });
    let (low, high) = values
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), &value| {
            (low.min(value), high.max(value))
        });
    row![
        text(reading.label.clone()).width(200),
        text(format::sensor(reading, unit)).width(130),
        text(format!(
            "{} to {}",
            format::sensor_value(reading.kind, low, unit),
            format::sensor_value(reading.kind, high, unit)
        ))
        .style(text::secondary)
        .width(170),
        container(
            Graph::new(Scale::Fit {
                floor: floor(reading.kind)
            })
            .line(values, Tone::Primary)
            .view()
        )
        .width(240)
        .height(30),
    ]
    .spacing(12)
    .align_y(iced::Center)
    .into()
}

/// The smallest span a graph shows, so a steady sensor does not look like a storm.
fn floor(kind: Kind) -> f32 {
    match kind {
        Kind::Temperature => 60.0,
        Kind::Power => 10.0,
        Kind::Frequency | Kind::Fan => 1000.0,
        Kind::Voltage | Kind::Current => 1.0,
    }
}
