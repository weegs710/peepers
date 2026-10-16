use iced::Element;
use iced::widget::space;
use peepers_core::disk::{Disk, Kind};
use peepers_core::sensors::Kind as Sensor;
use peepers_core::telemetry::Vitals;

use super::Card;
use crate::format;
use crate::graph::{Graph, Scale, Tone, level};
use crate::history::History;
use crate::settings::Temperature;
use crate::widgets::{detail, stats};

const RATE_FLOOR: f32 = 1_048_576.0;

pub fn card(history: &History, name: &str) -> Card {
    let active = history
        .latest()
        .and_then(|vitals| find(vitals, name))
        .map_or(0.0, |disk| disk.active);
    Card {
        title: format!("Disk {name}"),
        value: format!("{active:.0}%"),
        graph: Graph::new(Scale::Fixed(100.0)).line(history.series(active_of(name)), Tone::Primary),
    }
}

pub fn view<'a, Message: 'a>(
    history: &History,
    latest: &Vitals,
    name: &str,
    unit: Temperature,
) -> Element<'a, Message> {
    let Some(disk) = find(latest, name) else {
        return space().into();
    };
    let rates = Graph::new(Scale::Fit { floor: RATE_FLOOR })
        .line(
            history.series(rate_of(name, |disk| disk.read_per_second)),
            Tone::Success,
        )
        .line(
            history.series(rate_of(name, |disk| disk.write_per_second)),
            Tone::Warning,
        );
    let temperature = disk
        .sensors
        .iter()
        .find(|reading| reading.kind == Sensor::Temperature)
        .map_or_else(format::unavailable, |reading| format::sensor(reading, unit));

    let facts = [
        ("Active time", format!("{:.0}%", disk.active)),
        (
            "Average response time",
            format!("{:.1} ms", disk.response_ms),
        ),
        ("Read speed", format::rate(disk.read_per_second)),
        ("Write speed", format::rate(disk.write_per_second)),
        ("Total read", format::bytes(disk.read_bytes)),
        ("Total written", format::bytes(disk.write_bytes)),
        ("Capacity", format::bytes(disk.size)),
        ("Type", kind(disk.kind).to_owned()),
        ("Temperature", temperature),
        (
            "I/O scheduler",
            disk.scheduler.clone().unwrap_or_else(format::unavailable),
        ),
    ];
    detail(
        format!("Disk {name}"),
        disk.model.clone(),
        rates,
        stats(facts),
    )
}

fn find<'a>(vitals: &'a Vitals, name: &str) -> Option<&'a Disk> {
    vitals.disks.iter().find(|disk| disk.name == name)
}

fn active_of(name: &str) -> impl Fn(&Vitals) -> f32 {
    move |vitals| find(vitals, name).map_or(0.0, |disk| disk.active)
}

fn rate_of(name: &str, pick: impl Fn(&Disk) -> u64) -> impl Fn(&Vitals) -> f32 {
    move |vitals| find(vitals, name).map_or(0.0, |disk| level(pick(disk)))
}

fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Nvme => "NVMe SSD",
        Kind::Ssd => "SSD",
        Kind::Hdd => "Hard disk",
    }
}
