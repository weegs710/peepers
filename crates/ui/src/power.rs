use iced::widget::{column, container, row, rule, scrollable, space, text};
use iced::{Element, Fill};
use peepers_core::gpu::Adapter;
use peepers_core::sensors::{Chip, Kind, Reading};
use peepers_core::telemetry::Vitals;

use crate::format;
use crate::graph::{Graph, Scale, Tone, as_frequency};
use crate::history::History;
use crate::settings::Temperature;
use crate::widgets::flow;

/// The narrowest a logical processor's clock is drawn.
const CLOCK_WIDTH: f32 = 110.0;
const CLOCK_SPACING: f32 = 12.0;

/// The part of the machine a chip measures, in the order the Performance page lists its devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Processor,
    Memory,
    Storage,
    Network,
    Graphics,
    Other,
}

impl Group {
    const ALL: [Self; 6] = [
        Self::Processor,
        Self::Memory,
        Self::Storage,
        Self::Network,
        Self::Graphics,
        Self::Other,
    ];

    fn of(chip: &Chip) -> Self {
        match chip.name.as_str() {
            "k10temp" | "coretemp" | "zenpower" => Self::Processor,
            "spd5118" | "jc42" => Self::Memory,
            "nvme" | "drivetemp" => Self::Storage,
            "amdgpu" | "radeon" | "nouveau" | "i915" | "xe" | "nvidia" => Self::Graphics,
            _ if chip.model.as_deref().is_some_and(is_network) => Self::Network,
            _ => Self::Other,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Processor => "Processor",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Network => "Network",
            Self::Graphics => "Graphics",
            Self::Other => "Other sensors",
        }
    }
}

/// A network chip is told by its PCI name, because hwmon names the driver and drivers vary too much to list.
fn is_network(model: &str) -> bool {
    let model = model.to_lowercase();
    ["wireless", "ethernet", "network"]
        .iter()
        .any(|word| model.contains(word))
}

/// `notice` goes above everything, so a problem that brought the window to this page is the first
/// thing on it; the energy history goes last, because it is long.
pub fn view<'a, Message: 'a>(
    history: &'a History,
    unit: Temperature,
    notice: Option<Element<'a, Message>>,
    energy: Element<'a, Message>,
) -> Element<'a, Message> {
    let sensors: Element<'a, Message> = match history.latest() {
        Some(latest) => column![
            text("Live sensors").size(28),
            column(groups(history, latest, unit)).spacing(32)
        ]
        .spacing(24)
        .into(),
        None => text("Waiting for live sensor readings")
            .style(text::secondary)
            .into(),
    };
    scrollable(
        column(notice)
            .push(sensors)
            .push(rule::horizontal(2))
            .push(energy)
            .spacing(24)
            .padding(16),
    )
    .width(Fill)
    .height(Fill)
    .into()
}

fn groups<'a, Message: 'a>(
    history: &'a History,
    latest: &'a Vitals,
    unit: Temperature,
) -> impl Iterator<Item = Element<'a, Message>> {
    Group::ALL.into_iter().filter_map(move |group| {
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if group == Group::Processor {
            parts.push(processor(history, latest));
            parts.push(domains(latest));
        }
        parts.extend(
            latest
                .chips
                .iter()
                .filter(|chip| !chip.readings.is_empty() && Group::of(chip) == group)
                .map(|chip| section(history, chip, unit)),
        );
        if group == Group::Graphics {
            parts.extend(
                latest
                    .adapters
                    .iter()
                    .filter(|adapter| adapter.sensors.is_empty())
                    .map(silent),
            );
        }
        (!parts.is_empty()).then(|| header(group, parts))
    })
}

/// A group's name, set apart so it can be found in a long page, over what it holds.
fn header<'a, Message: 'a>(group: Group, parts: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column![
        text(group.title()).size(28).style(text::primary),
        rule::horizontal(2),
        column(parts)
            .spacing(24)
            .padding(iced::Padding::ZERO.top(8)),
    ]
    .spacing(6)
    .into()
}

/// A graphics card with no readings, so its absence is not taken for a gap.
fn silent<'a, Message: 'a>(adapter: &Adapter) -> Element<'a, Message> {
    let name = adapter
        .model
        .clone()
        .unwrap_or_else(|| adapter.card.clone());
    column![
        text(name).size(20),
        text("No sensor readings are available for it").style(text::secondary),
    ]
    .spacing(4)
    .into()
}

fn processor<'a, Message: 'a>(history: &History, latest: &'a Vitals) -> Element<'a, Message> {
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
                    .named("Average clock")
                    .readout(as_frequency)
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
        flow(CLOCK_WIDTH, CLOCK_SPACING, move || {
            latest
                .cpu
                .cores
                .iter()
                .map(|core| {
                    column![
                        text(format!("CPU {}", core.id))
                            .size(12)
                            .style(text::secondary),
                        text(
                            core.frequency_khz
                                .map_or_else(format::unavailable, format::frequency)
                        ),
                    ]
                    .into()
                })
                .collect()
        }),
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
        "k10temp" | "coretemp" | "zenpower" => "Temperatures".to_owned(),
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
    let kind = reading.kind;
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
            .named(reading.label.clone())
            .readout(move |value| format::sensor_value(kind, value, unit))
            .view()
        )
        .width(Fill)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn chip(name: &str, model: Option<&str>) -> Chip {
        Chip {
            id: "hwmon0".to_owned(),
            name: name.to_owned(),
            model: model.map(str::to_owned),
            readings: Vec::new(),
        }
    }

    #[test]
    fn a_chip_is_grouped_by_what_it_measures() {
        assert_eq!(Group::of(&chip("k10temp", None)), Group::Processor);
        assert_eq!(Group::of(&chip("spd5118", None)), Group::Memory);
        assert_eq!(Group::of(&chip("nvme", None)), Group::Storage);
        for driver in ["amdgpu", "radeon", "nouveau", "i915", "xe", "nvidia"] {
            assert_eq!(Group::of(&chip(driver, None)), Group::Graphics);
        }
        assert_eq!(Group::of(&chip("acpitz", None)), Group::Other);
    }

    #[test]
    fn a_network_chip_is_told_by_its_pci_name() {
        let wifi = chip(
            "mt7921_phy0",
            Some("MT7921 802.11ax PCIe Wireless Network Adapter [Filogic 330]"),
        );
        assert_eq!(Group::of(&wifi), Group::Network);
        assert_eq!(Group::of(&chip("mt7921_phy0", None)), Group::Other);
    }

    #[test]
    fn the_groups_follow_the_order_of_the_performance_page() {
        assert_eq!(
            Group::ALL.map(Group::title),
            [
                "Processor",
                "Memory",
                "Storage",
                "Network",
                "Graphics",
                "Other sensors"
            ]
        );
    }
}
