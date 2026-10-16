use std::collections::HashSet;

use iced::widget::{checkbox, column, progress_bar, row, text};
use iced::{Element, Fill, Task};
use peepers_core::bench::{Gpu, GpuKind, Outcome};
use peepers_core::gpu::Adapter;

use super::{Benchmarks, Message as Root, Section, Start, not_run, page};
use crate::blocking;
use crate::format;
use crate::gpu_bench::{self, Candidate, Id};

const INTRO: &str = "Runs two small programs on each graphics chip you tick, using the same Vulkan interface games use. \
One does as much arithmetic as it can, which shows how fast the chip computes. The other copies a block of the chip's own memory, which shows how fast that memory is. \
Each chip runs for a few seconds, and a run cannot be stopped once it has started.";

#[derive(Debug, Clone)]
pub enum Message {
    Candidates(Vec<Candidate>),
    Toggle(Id, bool),
}

#[derive(Debug, Default)]
pub struct State {
    candidates: Vec<Candidate>,
    chosen: HashSet<Id>,
    loaded: bool,
}

impl State {
    pub fn load(&self) -> Task<Message> {
        if self.loaded {
            return Task::none();
        }
        blocking::run(gpu_bench::candidates).map(Message::Candidates)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Candidates(candidates) => {
                self.chosen = candidates.iter().map(Candidate::id).collect();
                self.candidates = candidates;
                self.loaded = true;
            }
            Message::Toggle(id, on) => {
                if on {
                    self.chosen.insert(id);
                } else {
                    self.chosen.remove(&id);
                }
            }
        }
        Task::none()
    }

    /// The ticked adapters, or `None` before they are known, which runs them all.
    pub fn selection(&self) -> Option<HashSet<Id>> {
        self.loaded.then(|| self.chosen.clone())
    }
}

pub(super) fn view<'a>(app: &'a Benchmarks, adapters: &'a [Adapter]) -> Element<'a, Root> {
    let state = &app.gpu;
    let ready = !state.loaded || !state.chosen.is_empty();
    let mut chips = column![].spacing(6);
    for candidate in &state.candidates {
        chips = chips.push(
            checkbox(state.chosen.contains(&candidate.id()))
                .label(format!("{} ({})", candidate.name, kind(candidate.kind)))
                .on_toggle(move |on| Root::Gpu(Message::Toggle(candidate.id(), on))),
        );
    }
    for adapter in unreachable(adapters, &state.candidates) {
        chips = chips.push(
            text(format!(
                "{}: no Vulkan driver reported this one, so it cannot be tested",
                adapter.model.as_deref().unwrap_or(&adapter.card)
            ))
            .size(13)
            .style(text::danger),
        );
    }
    let controls = column![
        chips,
        if ready {
            app.run_button("Run", Start::One(Section::Gpu))
        } else {
            text("Tick at least one graphics chip to test")
                .style(text::secondary)
                .into()
        },
    ]
    .spacing(10);
    page("Graphics", INTRO, controls.into(), results(app))
}

/// Cards that the system lists but no graphics interface offered to the benchmark.
fn unreachable<'a>(adapters: &'a [Adapter], candidates: &[Candidate]) -> Vec<&'a Adapter> {
    if candidates.is_empty() {
        return Vec::new();
    }
    adapters
        .iter()
        .filter(|adapter| {
            !candidates.iter().any(|candidate| {
                candidate.id() == (u32::from(adapter.vendor.id()), u32::from(adapter.device))
            })
        })
        .collect()
}

fn kind(kind: GpuKind) -> &'static str {
    match kind {
        GpuKind::Integrated => "integrated: part of the processor, sharing system memory",
        GpuKind::Discrete => "discrete: its own chip with its own memory",
        GpuKind::Other => "other",
    }
}

/// The newest result of each adapter, in the order they last ran.
fn latest_per_adapter(app: &Benchmarks) -> Vec<&Gpu> {
    let mut seen = HashSet::new();
    app.history
        .records()
        .filter_map(|record| match &record.outcome {
            Outcome::Gpu(gpu) => Some(gpu),
            _ => None,
        })
        .filter(|gpu| seen.insert(&gpu.adapter))
        .collect()
}

fn results(app: &Benchmarks) -> Element<'_, Root> {
    let gpus = latest_per_adapter(app);
    if gpus.is_empty() {
        return not_run();
    }
    let best_compute = gpus.iter().map(|gpu| gpu.gigaflops).fold(0.0, f32::max);
    let best_memory = gpus.iter().map(|gpu| gpu.bandwidth).max().unwrap_or(0);
    let mut cards = column![
        text("Compute is how many calculations the chip finishes a second, in teraflops (trillions). It decides how much visual work a game or a render can ask of it. Memory speed is how fast the chip moves data in its own memory, which limits high resolutions and big textures.")
            .size(13)
            .style(text::secondary),
    ]
    .spacing(14);
    for gpu in &gpus {
        cards = cards.push(
            column![
                text(&gpu.adapter).size(18),
                text(kind(gpu.kind)).size(12).style(text::secondary),
                bar(
                    "Compute",
                    format!("{:.2} teraflops", gpu.gigaflops / 1000.0),
                    gpu.gigaflops,
                    best_compute,
                ),
                bar(
                    "Memory speed",
                    format::rate(gpu.bandwidth),
                    f32_of(gpu.bandwidth),
                    f32_of(best_memory),
                ),
            ]
            .spacing(6),
        );
    }
    for line in comparisons(&gpus) {
        cards = cards.push(text(line));
    }
    cards.into()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "bar lengths need no more than a few digits"
)]
fn f32_of(value: u64) -> f32 {
    value as f32
}

fn bar(label: &str, value: String, amount: f32, best: f32) -> Element<'_, Root> {
    row![
        text(label).width(110),
        progress_bar(0.0..=best.max(f32::EPSILON), amount)
            .length(Fill)
            .girth(10),
        text(value).width(150),
    ]
    .spacing(12)
    .align_y(iced::Center)
    .into()
}

/// How much faster the strongest chip is than each of the others, in words.
fn comparisons(gpus: &[&Gpu]) -> Vec<String> {
    let Some(best) = gpus
        .iter()
        .max_by(|a, b| a.gigaflops.total_cmp(&b.gigaflops))
    else {
        return Vec::new();
    };
    gpus.iter()
        .filter(|gpu| gpu.adapter != best.adapter && gpu.gigaflops > 0.0 && gpu.bandwidth > 0)
        .map(|gpu| {
            format!(
                "{} computes {:.1}x faster and moves memory {:.1}x faster than {}.",
                best.adapter,
                best.gigaflops / gpu.gigaflops,
                f32_of(best.bandwidth) / f32_of(gpu.bandwidth),
                gpu.adapter
            )
        })
        .collect()
}

pub(super) fn summary(gpu: &Gpu) -> String {
    format!(
        "{}: {:.2} teraflops, memory {}",
        gpu.adapter,
        gpu.gigaflops / 1000.0,
        format::rate(gpu.bandwidth)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(name: &str, gigaflops: f32, bandwidth: u64) -> Gpu {
        Gpu {
            adapter: name.to_owned(),
            kind: GpuKind::Discrete,
            gigaflops,
            bandwidth,
        }
    }

    #[test]
    fn the_strongest_chip_is_compared_with_each_of_the_others() {
        let (fast, slow) = (gpu("Fast", 4000.0, 160), gpu("Slow", 1000.0, 80));
        assert_eq!(
            comparisons(&[&slow, &fast]),
            ["Fast computes 4.0x faster and moves memory 2.0x faster than Slow."]
        );
        assert!(comparisons(&[&fast]).is_empty());
    }

    #[test]
    fn ticking_and_unticking_changes_the_selection() {
        let mut state = State::default();
        assert_eq!(state.selection(), None);
        drop(state.update(Message::Candidates(Vec::new())));
        assert_eq!(state.selection(), Some(HashSet::new()));
        drop(state.update(Message::Toggle((1, 2), true)));
        assert_eq!(state.selection(), Some(HashSet::from([(1, 2)])));
        drop(state.update(Message::Toggle((1, 2), false)));
        assert_eq!(state.selection(), Some(HashSet::new()));
    }

    #[test]
    fn a_summary_reads_in_teraflops() {
        assert!(summary(&gpu("Chip", 3721.0, 160_000_000_000)).contains("3.72 teraflops"));
    }
}
