use iced::Element;
use peepers_core::telemetry::Vitals;

use super::Card;
use crate::format;
use crate::graph::{Graph, Scale, Tone, as_percent};
use crate::history::History;
use crate::widgets::{detail, stats};

pub fn card(history: &History) -> Card {
    Card {
        title: "Memory".to_owned(),
        value: history.latest().map_or_else(format::unavailable, |vitals| {
            format!(
                "{} of {}",
                format::bytes(vitals.memory.used()),
                format::bytes(vitals.memory.total)
            )
        }),
        graph: graph(history),
    }
}

pub fn view<'a, Message: 'a>(
    history: &History,
    latest: &Vitals,
    available: f32,
) -> Element<'a, Message> {
    let memory = &latest.memory;
    let pressure = latest
        .pressure
        .memory
        .map_or_else(format::unavailable, |pressure| {
            format!("{:.1}% stalled", pressure.some.avg10)
        });
    let mut facts = vec![
        ("In use", format::bytes(memory.used())),
        ("Available", format::bytes(memory.available)),
        ("Cached", format::bytes(memory.cached)),
        ("Free", format::bytes(memory.free)),
        ("Shared", format::bytes(memory.shared)),
        ("Kernel slab", format::bytes(memory.slab)),
        (
            "Committed",
            format!(
                "{} of {}",
                format::bytes(memory.committed),
                format::bytes(memory.commit_limit)
            ),
        ),
        ("Memory pressure", pressure),
        (
            "Pages in / out",
            format!("{} / {}", memory.pages_in, memory.pages_out),
        ),
    ];
    if memory.swap_total > 0 {
        facts.push((
            "Swap in use",
            format!(
                "{} of {}",
                format::bytes(memory.swap_used()),
                format::bytes(memory.swap_total)
            ),
        ));
    }
    detail(
        "Memory",
        Some(format::bytes(memory.total)),
        graph(history),
        available,
        stats(facts),
    )
}

fn graph(history: &History) -> Graph {
    Graph::new(Scale::Fixed(100.0))
        .line(
            history.series(|vitals| vitals.memory.used_percent()),
            Tone::Primary,
        )
        .named("In use")
        .readout(as_percent)
}
