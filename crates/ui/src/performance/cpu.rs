use crate::theme::Element;
use iced::Fill;
use iced::widget::{column, container};
use peepers_core::cpu::Info;
use peepers_core::telemetry::Vitals;

use super::Card;
use crate::format;
use crate::graph::{Graph, Scale, Tone, as_percent};
use crate::history::History;
use crate::widgets::{detail, flow, stats};

/// The narrowest a logical processor's graph is drawn.
const CORE_WIDTH: f32 = 120.0;
const CORE_HEIGHT: f32 = 48.0;
const CORE_SPACING: f32 = 8.0;

pub fn card(history: &History) -> Card {
    let usage = history.latest().map_or(0.0, |vitals| vitals.cpu.usage);
    Card {
        title: "CPU".to_owned(),
        value: format!("{usage:.0}%"),
        graph: graph(history),
    }
}

pub fn view<'a, Message: 'a>(
    history: &'a History,
    latest: &'a Vitals,
    info: &Info,
    available: f32,
) -> Element<'a, Message> {
    let count = latest.cpu.cores.len();
    let cores = flow(CORE_WIDTH, CORE_SPACING, move || {
        (0..count).map(|index| core(history, index)).collect()
    });
    detail(
        "CPU",
        info.model.clone(),
        graph(history),
        available,
        column![cores, stats(facts(latest, info))]
            .spacing(18)
            .into(),
    )
}

fn graph(history: &History) -> Graph {
    Graph::new(Scale::Fixed(100.0))
        .key("cpu")
        .line(history.series(|vitals| vitals.cpu.usage), Tone::Primary)
        .named("Utilization")
        .series("utilization")
        .readout(as_percent)
}

fn core<'a, Message: 'a>(history: &History, index: usize) -> Element<'a, Message> {
    let usage =
        history.series(move |vitals| vitals.cpu.cores.get(index).map_or(0.0, |core| core.usage));
    container(
        Graph::new(Scale::Fixed(100.0))
            .key("cpu.core")
            .instance(index.to_string())
            .line(usage, Tone::Primary)
            .named(format!("Core {index}"))
            .series("utilization")
            .readout(as_percent)
            .view(),
    )
    .width(Fill)
    .height(CORE_HEIGHT)
    .into()
}

fn facts(latest: &Vitals, info: &Info) -> Vec<(&'static str, String)> {
    let cpu = &latest.cpu;
    let speeds: Vec<u64> = cpu
        .cores
        .iter()
        .filter_map(|core| core.frequency_khz)
        .collect();
    let average = speeds.iter().sum::<u64>().checked_div(speeds.len() as u64);
    let cache = |level| {
        info.caches
            .iter()
            .filter(|cache| cache.level == level)
            .map(|cache| cache.size * cache.instances as u64)
            .sum::<u64>()
    };
    let text = |value: &Option<String>| value.clone().unwrap_or_else(format::unavailable);

    vec![
        ("Utilization", format!("{:.0}%", cpu.usage)),
        ("Kernel time", format!("{:.0}%", cpu.kernel)),
        (
            "Speed",
            average.map_or_else(format::unavailable, format::frequency),
        ),
        (
            "Maximum speed",
            info.max_khz
                .map_or_else(format::unavailable, format::frequency),
        ),
        (
            "Threads",
            format!("{} running of {}", latest.load.runnable, latest.load.tasks),
        ),
        (
            "Load average",
            format!(
                "{:.2}  {:.2}  {:.2}",
                latest.load.one, latest.load.five, latest.load.fifteen
            ),
        ),
        ("Up time", format::uptime(latest.uptime)),
        ("Sockets", info.packages.to_string()),
        ("Cores", info.cores.to_string()),
        ("Logical processors", info.threads.to_string()),
        ("NUMA nodes", info.numa_nodes.to_string()),
        (
            "Virtualization",
            info.virtualization
                .map_or_else(|| "Not supported".to_owned(), str::to_owned),
        ),
        ("L1 cache", format::bytes(cache(1))),
        ("L2 cache", format::bytes(cache(2))),
        ("L3 cache", format::bytes(cache(3))),
        ("Frequency driver", text(&info.driver)),
        ("Governor", text(&info.governor)),
        ("Power preference", text(&info.preference)),
    ]
}
