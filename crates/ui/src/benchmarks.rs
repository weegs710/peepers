mod cpu;
mod disk;
mod gpu;
mod internet;

use std::collections::{HashSet, VecDeque};
use std::env;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use iced::time;
use iced::widget::{button, column, container, row, scrollable, space, table, text};
use iced::{Element, Fill, Length, Subscription, Task};
use peepers_core::bench::{Event, Gpu, History, Job, Loaded, Outcome, Record, Run};

use crate::blocking;
use crate::format;
use crate::gpu_bench;

const POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Cpu,
    Gpu,
    Disk,
    Internet,
}

impl Section {
    const ALL: [Self; 4] = [Self::Cpu, Self::Gpu, Self::Disk, Self::Internet];
}

impl fmt::Display for Section {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cpu => "Processor",
            Self::Gpu => "Graphics",
            Self::Disk => "Disk",
            Self::Internet => "Internet",
        })
    }
}

/// What a run button starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    One(Section),
    /// The processor, the graphics and the disk. The internet test contacts a server, so it is
    /// never started without being asked for.
    All,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<Loaded, String>),
    Select(Section),
    Start(Start),
    Disk(disk::Message),
    Gpu(gpu::Message),
    Internet(internet::Message),
    Finished(Result<Vec<Gpu>, String>),
    Stop,
    Poll,
    Clear,
    Saved(Result<(), String>),
}

/// One thing the queue can run: a core benchmark polled on its own thread, or the graphics test.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Core(Job),
    /// The adapters to test, or every hardware adapter when `None`.
    Gpu(Option<HashSet<gpu_bench::Id>>),
}

struct Active {
    /// `None` for the graphics test, which reports through a message and cannot be interrupted.
    run: Option<Run>,
    stage: &'static str,
}

impl fmt::Debug for Active {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Active")
            .field("stage", &self.stage)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct Benchmarks {
    section: Section,
    history: History,
    history_path: PathBuf,
    active: Option<Active>,
    queue: VecDeque<Step>,
    notice: Option<(String, bool)>,
    disk: disk::State,
    gpu: gpu::State,
    internet: internet::State,
}

impl Default for Benchmarks {
    fn default() -> Self {
        let home = env::var("HOME").unwrap_or_default();
        let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| format!("{home}/.local/state"));
        Self {
            section: Section::Cpu,
            history: History::default(),
            history_path: PathBuf::from(state).join("peepers/benchmarks.json"),
            active: None,
            queue: VecDeque::new(),
            notice: None,
            disk: disk::State::default(),
            gpu: gpu::State::default(),
            internet: internet::State::default(),
        }
    }
}

impl Benchmarks {
    pub fn enter(&self) -> Task<Message> {
        let path = self.history_path.clone();
        Task::batch([
            blocking::run(move || History::load(&path).map_err(|error| error.to_string()))
                .map(Message::Loaded),
            self.load(self.section),
        ])
    }

    /// What a section needs from the machine before it can be shown, fetched the first time.
    fn load(&self, section: Section) -> Task<Message> {
        match section {
            Section::Cpu => Task::none(),
            Section::Disk => self.disk.load().map(Message::Disk),
            Section::Gpu => self.gpu.load().map(Message::Gpu),
            Section::Internet => self.internet.load().map(Message::Internet),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.run.is_some())
        {
            time::every(POLL).map(|_| Message::Poll)
        } else {
            Subscription::none()
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(Ok(loaded)) => {
                self.history = loaded.history;
                self.notice = (loaded.skipped > 0).then(|| {
                    (
                        format!(
                            "{} earlier results were made by an older version of these tests, which measured differently, and are not shown.",
                            loaded.skipped
                        ),
                        false,
                    )
                });
            }
            Message::Loaded(Err(error)) => {
                self.notice = Some((
                    format!("The benchmark history could not be read: {error}"),
                    true,
                ));
            }
            Message::Select(section) => {
                self.section = section;
                return self.load(section);
            }
            Message::Start(start) => {
                self.notice = None;
                self.queue = self.steps(start);
                return self.next();
            }
            Message::Disk(message) => return self.disk.update(message).map(Message::Disk),
            Message::Gpu(message) => return self.gpu.update(message).map(Message::Gpu),
            Message::Internet(message) => {
                return self.internet.update(message).map(Message::Internet);
            }
            Message::Stop => {
                self.queue.clear();
                if let Some(run) = self.active.as_ref().and_then(|active| active.run.as_ref()) {
                    run.stop();
                }
            }
            Message::Finished(result) => return self.gpu_finished(result),
            Message::Poll => return self.poll(),
            Message::Clear => {
                self.history.clear();
                return self.save();
            }
            Message::Saved(result) => {
                if let Err(error) = result {
                    self.notice = Some((
                        format!("The benchmark history was not saved: {error}"),
                        true,
                    ));
                }
            }
        }
        Task::none()
    }

    fn steps(&self, start: Start) -> VecDeque<Step> {
        let step = |section| match section {
            Section::Cpu => Step::Core(Job::Cpu),
            Section::Gpu => Step::Gpu(self.gpu.selection()),
            Section::Disk => Step::Core(self.disk.job()),
            Section::Internet => Step::Core(self.internet.job()),
        };
        match start {
            Start::One(section) => VecDeque::from([step(section)]),
            Start::All => [Section::Cpu, Section::Gpu, Section::Disk].map(step).into(),
        }
    }

    /// Starts the next queued step unless one is already running.
    fn next(&mut self) -> Task<Message> {
        if self.active.is_some() {
            return Task::none();
        }
        match self.queue.pop_front() {
            Some(Step::Core(job)) => {
                self.active = Some(Active {
                    run: Some(Run::start(job)),
                    stage: "Starting",
                });
                Task::none()
            }
            Some(Step::Gpu(selection)) => {
                self.active = Some(Active {
                    run: None,
                    stage: "Testing graphics compute and memory",
                });
                blocking::run(move || gpu_bench::run(selection.as_ref())).map(Message::Finished)
            }
            None => Task::none(),
        }
    }

    fn gpu_finished(&mut self, result: Result<Vec<Gpu>, String>) -> Task<Message> {
        self.active = None;
        match result {
            Ok(adapters) => {
                for adapter in adapters {
                    self.history.push(Outcome::Gpu(adapter));
                }
                Task::batch([self.next(), self.save()])
            }
            Err(error) => {
                self.queue.clear();
                self.notice = Some((error, true));
                Task::none()
            }
        }
    }

    fn poll(&mut self) -> Task<Message> {
        let events = self
            .active
            .as_ref()
            .and_then(|active| active.run.as_ref())
            .map(Run::poll)
            .unwrap_or_default();
        let mut finished = None;
        for event in events {
            match event {
                Event::Stage(stage) => {
                    if let Some(active) = &mut self.active {
                        active.stage = stage;
                    }
                }
                Event::Finished(result) => finished = Some(result),
            }
        }
        let Some(result) = finished else {
            return Task::none();
        };
        self.active = None;
        match result {
            Ok(outcome) => {
                self.history.push(outcome);
                Task::batch([self.next(), self.save()])
            }
            Err(error) => {
                self.queue.clear();
                self.notice = Some((error, true));
                Task::none()
            }
        }
    }

    fn save(&self) -> Task<Message> {
        let (history, path) = (self.history.clone(), self.history_path.clone());
        blocking::run(move || history.save(&path).map_err(|error| error.to_string()))
            .map(Message::Saved)
    }

    /// The newest result of one kind, which `pick` pulls out of an outcome.
    fn latest<'a, T>(&'a self, pick: impl Fn(&'a Outcome) -> Option<&'a T>) -> Option<&'a T> {
        self.history
            .records()
            .find_map(|record| pick(&record.outcome))
    }

    fn run_button(&self, label: &'static str, start: Start) -> Element<'_, Message> {
        button(label)
            .on_press_maybe(self.active.is_none().then_some(Message::Start(start)))
            .into()
    }

    pub fn view<'a>(&'a self, adapters: &'a [peepers_core::gpu::Adapter]) -> Element<'a, Message> {
        let header = row![
            text("Benchmarks").size(24),
            space::horizontal(),
            match &self.active {
                Some(active) => row![
                    text(active.stage).style(text::secondary),
                    button("Stop").on_press(Message::Stop).style(button::danger)
                ]
                .spacing(12)
                .align_y(iced::Center),
                None => row![self.run_button("Run processor, graphics and disk", Start::All)],
            },
        ]
        .align_y(iced::Center);

        let menu = column(Section::ALL.map(|section| {
            button(text(section.to_string()))
                .width(Fill)
                .on_press(Message::Select(section))
                .style(if section == self.section {
                    button::primary
                } else {
                    button::text
                })
                .into()
        }))
        .spacing(4)
        .width(160);
        let detail = match self.section {
            Section::Cpu => cpu::view(self),
            Section::Gpu => gpu::view(self, adapters),
            Section::Disk => disk::view(self),
            Section::Internet => internet::view(self),
        };

        column![
            header,
            self.notice_line(),
            row![menu, scrollable(detail).width(Fill).height(Fill)]
                .spacing(16)
                .height(Length::FillPortion(3)),
            container(self.history_table()).height(Length::FillPortion(1)),
        ]
        .spacing(12)
        .padding(16)
        .into()
    }

    fn notice_line(&self) -> Element<'_, Message> {
        self.notice.as_ref().map_or_else(
            || space::vertical().height(0).into(),
            |(message, failed)| {
                text(message)
                    .style(if *failed { text::danger } else { text::success })
                    .into()
            },
        )
    }

    fn history_table(&self) -> Element<'_, Message> {
        if self.history.is_empty() {
            return text("Benchmark runs will appear here as they complete.")
                .style(text::secondary)
                .into();
        }
        let columns = [
            table::column("When", |record: &Record| text(format::date_time(record.at))),
            table::column("Test", |record: &Record| {
                text(match record.outcome {
                    Outcome::Cpu(_) => "Processor",
                    Outcome::Gpu(_) => "Graphics",
                    Outcome::Internet(_) => "Internet",
                    Outcome::Disk(_) => "Disk",
                })
            }),
            table::column("Result", |record: &Record| text(summary(&record.outcome))).width(Fill),
        ];
        column![
            row![
                text("History").size(18),
                space::horizontal(),
                button("Clear history")
                    .on_press(Message::Clear)
                    .style(button::secondary)
            ]
            .align_y(iced::Center),
            scrollable(
                table(columns, self.history.records())
                    .padding_x(10)
                    .padding_y(2)
            )
            .width(Fill)
            .height(Fill),
        ]
        .spacing(8)
        .into()
    }
}

/// A section's page: what the test does, what runs it, and the newest result below.
fn page<'a>(
    title: &'a str,
    intro: &'a str,
    controls: Element<'a, Message>,
    result: Element<'a, Message>,
) -> Element<'a, Message> {
    column![
        text(title).size(22),
        text(intro).style(text::secondary),
        controls,
        result,
    ]
    .spacing(14)
    .padding(iced::Padding::new(0.0).left(8.0).right(24.0))
    .into()
}

/// A measured figure with what it means in plain words.
fn figure<'a>(label: &'a str, value: String, hint: &'a str) -> Element<'a, Message> {
    row![
        text(label).width(190),
        column![
            text(value).size(18),
            text(hint).size(12).style(text::secondary)
        ]
        .spacing(2)
        .width(Fill),
    ]
    .spacing(16)
    .into()
}

fn not_run<'a>() -> Element<'a, Message> {
    text("Not run yet").style(text::secondary).into()
}

fn summary(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Cpu(cpu) => cpu::summary(cpu),
        Outcome::Disk(disk) => disk::summary(disk),
        Outcome::Gpu(gpu) => gpu::summary(gpu),
        Outcome::Internet(internet) => internet::summary(internet),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_everything_queues_the_processor_then_the_graphics_then_the_disk() {
        let steps = Benchmarks::default().steps(Start::All);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0], Step::Core(Job::Cpu));
        assert_eq!(steps[1], Step::Gpu(None));
        assert!(matches!(steps[2], Step::Core(Job::Disk { .. })));
    }

    #[test]
    fn the_internet_test_is_only_run_when_asked_for() {
        let steps = Benchmarks::default().steps(Start::One(Section::Internet));
        assert_eq!(steps.len(), 1);
        assert!(matches!(
            steps[0],
            Step::Core(Job::Internet { server: None })
        ));
    }
}
