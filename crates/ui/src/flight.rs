use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::time;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, slider, space, text, text_input,
};
use iced::{Element, Fill, Subscription, Task};
use peepers_core::telemetry::Vitals;
use peepers_core::trace::{Frame, Recorder, Trace};

use crate::blocking;
use crate::format;
use crate::graph::{Graph, Scale, Tone, decimate, level};
use crate::storage;

const TICK: Duration = Duration::from_millis(100);
/// The most points a replay graph draws, however long the recording.
const POINTS: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speed {
    X1,
    X2,
    X5,
    X10,
}

impl Speed {
    const ALL: [Self; 4] = [Self::X1, Self::X2, Self::X5, Self::X10];

    fn factor(self) -> f32 {
        match self {
            Self::X1 => 1.0,
            Self::X2 => 2.0,
            Self::X5 => 5.0,
            Self::X10 => 10.0,
        }
    }
}

impl fmt::Display for Speed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}x", self.factor())
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Directory(String),
    Start,
    Stop,
    Saved(Result<PathBuf, String>),
    Files(Vec<PathBuf>),
    Open(PathBuf),
    Loaded(Result<Box<Trace>, String>),
    Close,
    Play,
    Seek(u32),
    Speed(Speed),
    Sensor(String),
    Tick,
}

#[derive(Debug)]
struct Replay {
    trace: Trace,
    /// Seconds into the recording.
    time: f32,
    playing: bool,
    speed: Speed,
    sensor: usize,
}

impl Replay {
    fn length(&self) -> f32 {
        self.trace.frames.last().map_or(0.0, |frame| frame.seconds)
    }

    /// The last frame at or before the current time.
    fn index(&self) -> usize {
        self.trace
            .frames
            .partition_point(|frame| frame.seconds <= self.time)
            .saturating_sub(1)
    }
}

#[derive(Debug)]
pub struct Flight {
    recorder: Option<Recorder>,
    full: bool,
    directory: String,
    notice: Option<(String, bool)>,
    files: Vec<PathBuf>,
    replay: Option<Replay>,
}

impl Default for Flight {
    fn default() -> Self {
        Self {
            recorder: None,
            full: false,
            directory: storage::default_folder(),
            notice: None,
            files: Vec::new(),
            replay: None,
        }
    }
}

impl Flight {
    pub fn enter(&self) -> Task<Message> {
        let directory = PathBuf::from(&self.directory);
        blocking::run(move || list(&directory)).map(Message::Files)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.replay.as_ref().is_some_and(|replay| replay.playing) {
            time::every(TICK).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// A line for the navigation rail while a recording is running.
    pub fn status(&self) -> Option<String> {
        self.recorder
            .as_ref()
            .map(|recorder| format!("Recording, {} samples", recorder.frames()))
    }

    /// Feeds the recording, if one is running, with the newest sample.
    pub fn record(&mut self, vitals: &Vitals) {
        if let Some(recorder) = &mut self.recorder
            && !recorder.record(vitals)
        {
            self.full = true;
            self.notice = Some((
                "The recording is full. Stop it to save what was captured.".to_owned(),
                true,
            ));
        }
    }

    pub fn update(&mut self, message: Message, latest: Option<&Vitals>) -> Task<Message> {
        match message {
            Message::Directory(directory) => self.directory = directory,
            Message::Start => match (latest, &self.recorder) {
                (Some(vitals), None) => {
                    self.recorder = Some(Recorder::start(vitals));
                    self.full = false;
                    self.notice = None;
                }
                _ => return Task::none(),
            },
            Message::Stop => {
                if let Some(recorder) = self.recorder.take() {
                    let trace = recorder.finish();
                    let directory = PathBuf::from(&self.directory);
                    return blocking::run(move || {
                        trace.save(&directory).map_err(|error| error.to_string())
                    })
                    .map(Message::Saved);
                }
            }
            Message::Saved(result) => {
                self.notice = Some(match result {
                    Ok(path) => (format!("Saved {}", path.display()), false),
                    Err(error) => (format!("The recording could not be saved: {error}"), true),
                });
                return self.enter();
            }
            Message::Files(files) => self.files = files,
            Message::Open(path) => {
                return blocking::run(move || {
                    Trace::load(&path)
                        .map(Box::new)
                        .map_err(|error| error.to_string())
                })
                .map(Message::Loaded);
            }
            Message::Loaded(result) => match result {
                Ok(trace) => {
                    let sensor = trace
                        .header
                        .sensors
                        .iter()
                        .position(|channel| channel.unit == "°C")
                        .unwrap_or(0);
                    self.replay = Some(Replay {
                        trace: *trace,
                        time: 0.0,
                        playing: false,
                        speed: Speed::X1,
                        sensor,
                    });
                    self.notice = None;
                }
                Err(error) => self.notice = Some((error, true)),
            },
            Message::Close => self.replay = None,
            Message::Play => {
                if let Some(replay) = &mut self.replay {
                    if replay.time >= replay.length() {
                        replay.time = 0.0;
                    }
                    replay.playing = !replay.playing;
                }
            }
            Message::Seek(index) => {
                if let Some(replay) = &mut self.replay {
                    replay.time = replay
                        .trace
                        .frames
                        .get(index as usize)
                        .map_or(0.0, |frame| frame.seconds);
                }
            }
            Message::Speed(speed) => {
                if let Some(replay) = &mut self.replay {
                    replay.speed = speed;
                }
            }
            Message::Sensor(label) => {
                if let Some(replay) = &mut self.replay
                    && let Some(index) = sensor_options(&replay.trace)
                        .iter()
                        .position(|option| *option == label)
                {
                    replay.sensor = index;
                }
            }
            Message::Tick => {
                if let Some(replay) = &mut self.replay {
                    replay.time = (replay.time + TICK.as_secs_f32() * replay.speed.factor())
                        .min(replay.length());
                    replay.playing &= replay.time < replay.length();
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let controls = match &self.recorder {
            Some(_) => button("Stop and save")
                .on_press(Message::Stop)
                .style(button::danger),
            None => button("Start recording")
                .on_press(Message::Start)
                .style(button::success),
        };
        let header = row![
            text("Flight recorder").size(24),
            controls,
            text(self.status().unwrap_or_else(|| "Not recording".to_owned()))
                .style(text::secondary),
            space::horizontal(),
            text_input("Folder for recordings", &self.directory)
                .on_input(Message::Directory)
                .width(360),
        ]
        .spacing(16)
        .align_y(iced::Center);
        let notice = self.notice.as_ref().map(|(message, failed)| {
            Element::from(text(message).style(if *failed { text::danger } else { text::success }))
        });
        column![header]
            .extend(notice)
            .push(match &self.replay {
                Some(replay) => replay_view(replay),
                None => self.recordings(),
            })
            .spacing(12)
            .padding(16)
            .into()
    }

    fn recordings(&self) -> Element<'_, Message> {
        if self.files.is_empty() {
            return text(format!("No recordings in {}", self.directory))
                .style(text::secondary)
                .into();
        }
        let rows = self.files.iter().map(|path| {
            row![
                text(
                    path.file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
                ),
                space::horizontal(),
                button("Replay")
                    .on_press(Message::Open(path.clone()))
                    .style(button::secondary),
            ]
            .align_y(iced::Center)
            .into()
        });
        scrollable(column(rows).spacing(6)).height(Fill).into()
    }
}

fn replay_view(replay: &Replay) -> Element<'_, Message> {
    let header = &replay.trace.header;
    column![
        controls(replay),
        text(format!(
            "{} on {}, kernel {}",
            header.machine,
            format::date_time(header.recorded_at),
            header.kernel
        ))
        .style(text::secondary),
        scrollable(row(cards(replay)).spacing(12).wrap().vertical_spacing(12)).height(Fill),
    ]
    .spacing(12)
    .into()
}

fn controls(replay: &Replay) -> Element<'_, Message> {
    let last = u32::try_from(replay.trace.frames.len().saturating_sub(1)).unwrap_or(u32::MAX);
    let position = u32::try_from(replay.index()).unwrap_or(0);
    let clock = |seconds: f32| format::uptime(Duration::from_secs_f32(seconds));
    row![
        button("Close")
            .on_press(Message::Close)
            .style(button::secondary),
        button(if replay.playing { "Pause" } else { "Play" }).on_press(Message::Play),
        pick_list(Speed::ALL, Some(replay.speed), Message::Speed),
        slider(0..=last, position, Message::Seek).width(Fill),
        text(format!(
            "{} of {}",
            clock(replay.time),
            clock(replay.length())
        )),
    ]
    .spacing(12)
    .align_y(iced::Center)
    .into()
}

/// Draws one recorded series per line across the whole recording, with a cursor at the replay time.
struct Plot<'a> {
    frames: &'a [Frame],
    span: usize,
    cursor: f32,
}

impl<'a> Plot<'a> {
    fn new(replay: &'a Replay) -> Self {
        let frames = replay.trace.frames.as_slice();
        let span = frames.len().min(POINTS);
        let cursor = if replay.length() > 0.0 {
            replay.time / replay.length()
        } else {
            0.0
        };
        Self {
            frames,
            span,
            cursor,
        }
    }

    fn graph(&self, scale: Scale) -> Graph {
        Graph::new(scale).span(self.span).marker(self.cursor)
    }

    fn series(&self, pick: impl Fn(&Frame) -> f32) -> Vec<f32> {
        decimate(&self.frames.iter().map(pick).collect::<Vec<_>>(), POINTS)
    }
}

fn cards(replay: &Replay) -> Vec<Element<'_, Message>> {
    let (trace, plot) = (&replay.trace, Plot::new(replay));
    let at = &trace.frames[replay.index()];
    let percent = Scale::Fixed(100.0);
    let mut cards = vec![
        card(
            "CPU",
            format!("{:.0}%", at.cpu),
            plot.graph(percent)
                .line(plot.series(|frame| frame.cpu), Tone::Primary),
        ),
        card(
            "Memory",
            format!("{:.0}%", at.memory),
            plot.graph(percent)
                .line(plot.series(|frame| frame.memory), Tone::Primary),
        ),
    ];
    for (index, name) in trace.header.disks.iter().enumerate() {
        let disk = at.disks[index];
        cards.push(card(
            &format!("Disk {name}"),
            format!(
                "R {}  W {}",
                format::rate(disk.read),
                format::rate(disk.write)
            ),
            plot.graph(Scale::Fit { floor: 1_048_576.0 })
                .line(
                    plot.series(|frame| level(frame.disks[index].read)),
                    Tone::Success,
                )
                .line(
                    plot.series(|frame| level(frame.disks[index].write)),
                    Tone::Warning,
                ),
        ));
    }
    for (index, name) in trace.header.interfaces.iter().enumerate() {
        let network = at.interfaces[index];
        cards.push(card(
            name,
            format!(
                "R {}  S {}",
                format::rate(network.receive),
                format::rate(network.send)
            ),
            plot.graph(Scale::Fit { floor: 131_072.0 })
                .line(
                    plot.series(|frame| level(frame.interfaces[index].receive)),
                    Tone::Success,
                )
                .line(
                    plot.series(|frame| level(frame.interfaces[index].send)),
                    Tone::Warning,
                ),
        ));
    }
    for (index, name) in trace.header.gpus.iter().enumerate() {
        cards.push(card(
            &format!("GPU {name}"),
            format!("{:.0}%", at.gpus[index].busy),
            plot.graph(percent)
                .line(plot.series(|frame| frame.gpus[index].busy), Tone::Primary)
                .line(plot.series(|frame| frame.gpus[index].memory), Tone::Warning),
        ));
    }
    cards.extend(sensor_card(replay, &plot));
    cards
}

/// One sensor at a time, chosen from everything the recording captured.
fn sensor_card<'a>(replay: &'a Replay, plot: &Plot<'_>) -> Option<Element<'a, Message>> {
    let (trace, index) = (&replay.trace, replay.sensor);
    let channel = trace.header.sensors.get(index)?;
    let options = sensor_options(trace);
    Some(
        column![
            pick_list(
                options.clone(),
                options.get(index).cloned(),
                Message::Sensor
            )
            .width(Fill),
            card(
                &channel.label,
                format!(
                    "{:.1} {}",
                    trace.frames[replay.index()].sensors[index],
                    channel.unit
                ),
                plot.graph(Scale::Fit { floor: 10.0 })
                    .line(plot.series(|frame| frame.sensors[index]), Tone::Primary),
            ),
        ]
        .width(430)
        .into(),
    )
}

fn card<'a>(title: &str, current: String, graph: Graph) -> Element<'a, Message> {
    container(
        column![
            row![
                text(title.to_owned()),
                space::horizontal(),
                text(current).style(text::secondary)
            ],
            container(graph.view()).height(90),
        ]
        .spacing(6),
    )
    .width(430)
    .padding(10)
    .style(container::rounded_box)
    .into()
}

fn sensor_options(trace: &Trace) -> Vec<String> {
    trace
        .header
        .sensors
        .iter()
        .map(|channel| format!("{} ({})", channel.id, channel.unit))
        .collect()
}

/// The recordings in `directory`, newest first.
fn list(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("peepers-"))
        })
        .collect();
    files.sort_by(|a, b| b.cmp(a));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_read_as_multipliers() {
        assert_eq!(Speed::X5.to_string(), "5x");
    }
}
