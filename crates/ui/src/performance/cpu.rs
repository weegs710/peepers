use iced::Element;
use iced::widget::{column, container, row};
use peepers_core::cpu::Info;
use peepers_core::telemetry::Vitals;

use super::Card;
use crate::format;
use crate::graph::{Graph, Scale, Tone};
use crate::history::History;
use crate::widgets::{detail, stats};

pub fn card(history: &History) -> Card {
    let usage = history.latest().map_or(0.0, |vitals| vitals.cpu.usage);
    Card {
        title: "CPU".to_owned(),
        value: format!("{usage:.0}%"),
        graph: graph(history),
    }
}

pub fn view<'a, Message: 'a>(
    history: &History,
    latest: &Vitals,
    info: &Info,
) -> Element<'a, Message> {
    let cores = row((0..latest.cpu.cores.len()).map(|index| core(history, index)))
        .spacing(8)
        .wrap()
        .vertical_spacing(8);
    detail(
        "CPU",
        info.model.clone(),
        graph(history),
        column![cores, stats(facts(latest, info))]
            .spacing(18)
            .into(),
    )
}

fn graph(history: &History) -> Graph {
    Graph::new(Scale::Fixed(100.0)).line(history.series(|vitals| vitals.cpu.usage), Tone::Primary)
}

fn core<'a, Message: 'a>(history: &History, index: usize) -> Element<'a, Message> {
    let usage =
        history.series(move |vitals| vitals.cpu.cores.get(index).map_or(0.0, |core| core.usage));
    container(
        Graph::new(Scale::Fixed(100.0))
            .line(usage, Tone::Primary)
            .view(),
    )
    .width(120)
    .height(48)
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
