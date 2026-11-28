use crate::theme::Element;

use iced::widget::space;
use peepers_core::gpu::{Adapter, Pool, Vendor};
use peepers_core::telemetry::Vitals;

use super::Card;
use crate::format;
use crate::graph::{Graph, Scale, Tone, as_percent};
use crate::history::History;
use crate::settings::Temperature;
use crate::widgets::{detail, stats};

pub fn card(history: &History, name: &str) -> Card {
    let adapter = history.latest().and_then(|vitals| find(vitals, name));
    Card {
        title: adapter.map_or_else(
            || name.to_owned(),
            |adapter| format::marketing_name(&title(adapter)).to_owned(),
        ),
        value: adapter
            .and_then(|adapter| adapter.busy)
            .map_or_else(format::unavailable, |busy| format!("{busy:.0}%")),
        graph: graph(history, name),
    }
}

pub fn view<'a, Message: 'a>(
    history: &History,
    latest: &Vitals,
    name: &str,
    unit: Temperature,
    available: f32,
) -> Element<'a, Message> {
    let Some(adapter) = find(latest, name) else {
        return space().into();
    };
    let pool = |pool: Option<Pool>| {
        pool.map_or_else(format::unavailable, |pool| {
            format!(
                "{} of {}",
                format::bytes(pool.used),
                format::bytes(pool.total)
            )
        })
    };
    let mut facts = vec![
        (
            if adapter.vendor == Vendor::Intel {
                "Utilization (accessible processes)"
            } else {
                "Utilization"
            }
            .to_owned(),
            adapter
                .busy
                .map_or_else(format::unavailable, |busy| format!("{busy:.0}%")),
        ),
        (
            "Driver".to_owned(),
            adapter.driver.clone().unwrap_or_else(format::unavailable),
        ),
        ("Video memory".to_owned(), pool(adapter.vram)),
        ("Shared memory".to_owned(), pool(adapter.gtt)),
        (
            "PCIe link".to_owned(),
            match (&adapter.link_speed, adapter.link_width) {
                (Some(speed), Some(width)) => format!("{speed} x{width}"),
                _ => format::unavailable(),
            },
        ),
        (
            "Primary adapter".to_owned(),
            if adapter.primary { "Yes" } else { "No" }.to_owned(),
        ),
    ];
    facts.extend(
        adapter
            .sensors
            .iter()
            .map(|reading| (reading.label.clone(), format::sensor(reading, unit))),
    );
    detail(
        title(adapter),
        Some(adapter.card.clone()),
        graph(history, name),
        available,
        stats(facts),
    )
}

fn graph(history: &History, name: &str) -> Graph {
    let utilization =
        history.optional_series(|vitals| find(vitals, name).and_then(|adapter| adapter.busy));
    let memory = history.optional_series(|vitals| {
        find(vitals, name)
            .and_then(|adapter| adapter.vram)
            .map(|vram| 100.0 * pool_ratio(vram))
    });
    Graph::new(Scale::Fixed(100.0))
        .key("gpu")
        .instance(name)
        .optional_line(utilization, Tone::Primary)
        .named("Utilization")
        .series("utilization")
        .optional_line(memory, Tone::Warning)
        .named("Video memory")
        .series("memory")
        .readout(as_percent)
}

fn pool_ratio(pool: Pool) -> f32 {
    crate::graph::level(pool.used) / crate::graph::level(pool.total.max(1))
}

fn find<'a>(vitals: &'a Vitals, name: &str) -> Option<&'a Adapter> {
    vitals.adapters.iter().find(|adapter| adapter.card == name)
}

/// The database name, or the vendor and PCI device ID when the database has none.
fn title(adapter: &Adapter) -> String {
    adapter.model.clone().unwrap_or_else(|| {
        let vendor = match adapter.vendor {
            Vendor::Amd => "AMD".to_owned(),
            Vendor::Nvidia => "NVIDIA".to_owned(),
            Vendor::Intel => "Intel".to_owned(),
            Vendor::Other(id) => format!("{id:04x}"),
        };
        format!("{vendor} GPU {:04x}", adapter.device)
    })
}
