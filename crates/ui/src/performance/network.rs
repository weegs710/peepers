use iced::Element;
use iced::widget::space;
use peepers_core::network::{Interface, Kind};
use peepers_core::telemetry::Vitals;

use super::Card;
use crate::format;
use crate::graph::{Graph, Scale, Tone, level};
use crate::history::History;
use crate::widgets::{detail, stats};

const RATE_FLOOR: f32 = 131_072.0;

pub fn card(history: &History, name: &str) -> Card {
    let (receive, send) = history
        .latest()
        .and_then(|vitals| find(vitals, name))
        .map_or((0, 0), |interface| {
            (interface.receive_per_second, interface.send_per_second)
        });
    Card {
        title: name.to_owned(),
        value: format!("S {}  R {}", format::bytes(send), format::bytes(receive)),
        graph: graph(history, name),
    }
}

pub fn view<'a, Message: 'a>(
    history: &History,
    latest: &Vitals,
    name: &str,
) -> Element<'a, Message> {
    let Some(interface) = find(latest, name) else {
        return space().into();
    };
    let addresses = |addresses: Vec<String>| {
        if addresses.is_empty() {
            format::unavailable()
        } else {
            addresses.join(", ")
        }
    };
    let mut facts = vec![
        ("Send", format::rate(interface.send_per_second)),
        ("Receive", format::rate(interface.receive_per_second)),
        ("Total sent", format::bytes(interface.sent)),
        ("Total received", format::bytes(interface.received)),
        (
            "Link speed",
            interface
                .speed_mbps
                .map_or_else(format::unavailable, |speed| format!("{speed} Mbps")),
        ),
        (
            "State",
            interface.state.clone().unwrap_or_else(format::unavailable),
        ),
        (
            "Hardware address",
            interface.mac.clone().unwrap_or_else(format::unavailable),
        ),
        (
            "IPv4 address",
            addresses(interface.ipv4.iter().map(ToString::to_string).collect()),
        ),
        (
            "IPv6 address",
            addresses(interface.ipv6.iter().map(ToString::to_string).collect()),
        ),
    ];
    if let Some(signal) = interface.signal {
        facts.push((
            "Signal strength",
            format!("{} dBm, quality {}", signal.dbm, signal.quality),
        ));
    }
    detail(
        interface.model.clone().unwrap_or_else(|| name.to_owned()),
        Some(format!("{name}, {}", kind(interface.kind))),
        graph(history, name),
        stats(facts),
    )
}

fn graph(history: &History, name: &str) -> Graph {
    let rate = |pick: fn(&Interface) -> u64| {
        history.series(|vitals| find(vitals, name).map_or(0.0, |interface| level(pick(interface))))
    };
    Graph::new(Scale::Fit { floor: RATE_FLOOR })
        .line(
            rate(|interface| interface.receive_per_second),
            Tone::Success,
        )
        .line(rate(|interface| interface.send_per_second), Tone::Warning)
}

fn find<'a>(vitals: &'a Vitals, name: &str) -> Option<&'a Interface> {
    vitals
        .interfaces
        .iter()
        .find(|interface| interface.name == name)
}

fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Loopback => "Loopback",
        Kind::Ethernet => "Ethernet",
        Kind::Wireless => "Wi-Fi",
        Kind::Virtual => "Virtual",
    }
}
