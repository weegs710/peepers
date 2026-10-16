use iced::Element;
use iced::widget::column;
use peepers_core::bench::{Cpu, Level, Outcome, Pair};

use super::{Benchmarks, Message, Section, Start, figure, not_run, page};
use crate::format;

const INTRO: &str = "Runs fixed pieces of work and times them, first on one core and then on every thread at once. \
Nothing here is an invented score: each figure is a rate or a time that means the same thing on any machine, so it can be compared with another computer or with this one on another day. It takes about ten seconds.";

pub(super) fn view(app: &Benchmarks) -> Element<'_, Message> {
    page(
        "Processor",
        INTRO,
        app.run_button("Run", Start::One(Section::Cpu)),
        app.latest(|outcome| match outcome {
            Outcome::Cpu(cpu) => Some(cpu),
            _ => None,
        })
        .map_or_else(not_run, results),
    )
}

fn results(cpu: &Cpu) -> Element<'_, Message> {
    let operations = |pair: Pair<f32>, unit: &str| {
        format!(
            "{:.1} {unit} on one core, {:.1} on all {} threads",
            pair.single, pair.multi, cpu.threads
        )
    };
    let mut figures = column![
        figure(
            "Integer math",
            operations(cpu.integer, "billion operations a second"),
            "Whole-number adds, shifts and xors finished per second: the everyday work of most software.",
        ),
        figure(
            "Floating-point math",
            operations(cpu.float, "billion operations a second"),
            "Decimal multiplies and adds (64-bit) per second, also called GFLOPS: spreadsheets, physics, audio and 3D lean on this.",
        ),
        figure(
            "Scaling across cores",
            format!(
                "{:.1}x for integers, {:.1}x for decimals",
                speed_up(cpu.integer),
                speed_up(cpu.float)
            ),
            "How many times faster every thread together is than one core. More than the number of physical cores means hyper-threading is helping.",
        ),
        figure(
            "Memory bandwidth",
            format!(
                "{} on one thread, {} on all threads",
                format::rate(cpu.bandwidth.single),
                format::rate(cpu.bandwidth.multi)
            ),
            "How fast the processor can stream data out of RAM. Large files, video and big datasets are limited by this.",
        ),
    ]
    .spacing(14);
    for latency in &cpu.latencies {
        let (label, hint) = describe(latency.level);
        figures = figures.push(figure(
            label,
            format!(
                "{:.1} ns (tested over {})",
                latency.nanoseconds,
                format::bytes(latency.bytes)
            ),
            hint,
        ));
    }
    figures.into()
}

/// Several threads' rate over one thread's, or zero when one thread measured nothing.
fn speed_up(pair: Pair<f32>) -> f32 {
    if pair.single > 0.0 {
        pair.multi / pair.single
    } else {
        0.0
    }
}

/// How the level is named, and what it is, for a reader who has not met cache levels.
fn describe(level: Level) -> (&'static str, &'static str) {
    match level {
        Level::L1 => (
            "Level 1 cache delay",
            "Time to fetch one value that the next fetch depends on, from the smallest and fastest memory, inside each core.",
        ),
        Level::L2 => (
            "Level 2 cache delay",
            "The same wait, from a bigger memory that is a little slower but still belongs to one core.",
        ),
        Level::L3 => (
            "Level 3 cache delay",
            "The same wait, from the large memory many cores share.",
        ),
        Level::Memory => (
            "Main memory delay",
            "The same wait, from RAM: by far the slowest step, which is why a program that fits in cache runs so much faster.",
        ),
    }
}

pub(super) fn summary(cpu: &Cpu) -> String {
    let memory = cpu
        .latencies
        .iter()
        .find(|latency| latency.level == Level::Memory)
        .map_or_else(format::unavailable, |latency| {
            format!("{:.0} ns", latency.nanoseconds)
        });
    format!(
        "{} threads: integer {:.0} and float {:.0} billion operations a second, memory {}, {memory} to RAM",
        cpu.threads,
        cpu.integer.multi,
        cpu.float.multi,
        format::rate(cpu.bandwidth.multi)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_is_the_multi_rate_over_the_single_rate() {
        assert!(
            (speed_up(Pair {
                single: 2.0,
                multi: 9.0
            }) - 4.5)
                .abs()
                < f32::EPSILON
        );
        assert!(
            speed_up(Pair {
                single: 0.0,
                multi: 9.0
            })
            .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn every_level_is_explained() {
        for level in [Level::L1, Level::L2, Level::L3, Level::Memory] {
            let (label, hint) = describe(level);
            assert!(!label.is_empty() && !hint.is_empty());
        }
    }
}
