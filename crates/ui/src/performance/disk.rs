use iced::Element;
use iced::widget::{column, row, space, text};
use peepers_core::disk::{Disk, Kind};
use peepers_core::health::{Ata, Health, Nvme};
use peepers_core::sensors::Kind as Sensor;
use peepers_core::telemetry::Vitals;

use super::Card;
use crate::format;
use crate::graph::{Graph, Scale, Tone, as_percent, as_rate, level};
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
        graph: Graph::new(Scale::Fixed(100.0))
            .line(history.series(active_of(name)), Tone::Primary)
            .named("Active time")
            .readout(as_percent),
    }
}

pub fn view<'a, Message: 'a>(
    history: &History,
    latest: &Vitals,
    name: &str,
    unit: Temperature,
    health: Option<&Result<Health, String>>,
    available: f32,
) -> Element<'a, Message> {
    let Some(disk) = find(latest, name) else {
        return space().into();
    };
    let rates = Graph::new(Scale::Fit { floor: RATE_FLOOR })
        .line(
            history.series(rate_of(name, |disk| disk.read_per_second)),
            Tone::Success,
        )
        .named("Read")
        .line(
            history.series(rate_of(name, |disk| disk.write_per_second)),
            Tone::Warning,
        )
        .named("Write")
        .readout(as_rate);
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
        available,
        column![stats(facts), wellness(health)].spacing(14).into(),
    )
}

fn wellness<'a, Message: 'a>(health: Option<&Result<Health, String>>) -> Element<'a, Message> {
    let note = |words: String| text(words).style(text::secondary).into();
    let report = match health {
        None => return note("Reading the drive's health".to_owned()),
        Some(Err(error)) => return note(format!("Health unavailable: {error}")),
        Some(Ok(report)) => report,
    };
    let concerns = report.concerns();
    let verdict = if concerns.is_empty() {
        text("No problems reported").style(text::success)
    } else {
        text(concerns.join(", ")).style(text::danger)
    };
    let facts = match report {
        Health::Nvme(nvme) => nvme_facts(nvme),
        Health::Ata(ata) => ata_facts(ata),
    };
    column![
        row![text("Health").size(18), verdict].spacing(12),
        stats(facts)
    ]
    .spacing(10)
    .into()
}

fn nvme_facts(nvme: &Nvme) -> Vec<(&'static str, String)> {
    let amount = |bytes: Option<u64>| bytes.map_or_else(format::unavailable, format::bytes);
    vec![
        ("Wear", format!("{}% used", nvme.percent_used)),
        (
            "Spare capacity",
            format!("{}% (limit {}%)", nvme.spare, nvme.spare_threshold),
        ),
        ("Data written", amount(nvme.bytes_written)),
        ("Data read", amount(nvme.bytes_read)),
        ("Powered on", hours(Some(nvme.power_on_hours))),
        ("Power cycles", nvme.power_cycles.to_string()),
        ("Unsafe shutdowns", nvme.unsafe_shutdowns.to_string()),
        ("Media errors", nvme.media_errors.to_string()),
    ]
}

fn ata_facts(ata: &Ata) -> Vec<(&'static str, String)> {
    let count =
        |count: Option<u64>| count.map_or_else(format::unavailable, |count| count.to_string());
    vec![
        ("Powered on", hours(ata.power_on_hours)),
        ("Bad sectors", count(ata.bad_sectors)),
        (
            "Attributes failing",
            count(ata.attributes_failing.map(u64::from)),
        ),
        (
            "Failed in the past",
            count(ata.attributes_failed_before.map(u64::from)),
        ),
        (
            "Last self-test",
            Some(ata.self_test.clone())
                .filter(|status| !status.is_empty())
                .unwrap_or_else(format::unavailable),
        ),
    ]
}

fn hours(hours: Option<u64>) -> String {
    hours.map_or_else(format::unavailable, |hours| format!("{hours} hours"))
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
