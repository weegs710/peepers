use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::time;
use iced::widget::{button, column, container, pick_list, row, slider, space, text, text_input};
use iced::{Element, Fill, Subscription, Task};
use peepers_core::telemetry::Vitals;
use peepers_core::trace::{Frame, Recorder, Trace};

use crate::blocking;
use crate::format;
use crate::graph::{Graph, LABELLED_HEIGHT, Scale, Tone, as_percent, as_rate, decimate, level};
use crate::storage;
use crate::widgets::{flow, scrolling};

const TICK: Duration = Duration::from_millis(100);
/// The narrowest a replay's graph card is drawn.
const CARD_WIDTH: f32 = 430.0;
const CARD_SPACING: f32 = 12.0;
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
    Label(String),
    Ask(Doomed),
    Cancel,
    Confirm,
    Deleted(Result<usize, String>),
    Open(PathBuf),
    Loaded(Result<Box<Trace>, String>),
    Close,
    Play,
    Seek(u32),
    Speed(Speed),
    Sensor(String),
    Tick,
}

/// What the user has been asked to confirm deleting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doomed {
    One(PathBuf),
    All,
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

    /// Applies a playback control; anything else is not for the replay.
    fn update(&mut self, message: Message) {
        match message {
            Message::Play => {
                if self.time >= self.length() {
                    self.time = 0.0;
                }
                self.playing = !self.playing;
            }
            Message::Seek(index) => {
                self.time = self
                    .trace
                    .frames
                    .get(index as usize)
                    .map_or(0.0, |frame| frame.seconds);
            }
            Message::Speed(speed) => self.speed = speed,
            Message::Sensor(label) => {
                if let Some(index) = sensor_options(&self.trace)
                    .iter()
                    .position(|option| *option == label)
                {
                    self.sensor = index;
                }
            }
            Message::Tick => {
                self.time =
                    (self.time + TICK.as_secs_f32() * self.speed.factor()).min(self.length());
                self.playing &= self.time < self.length();
            }
            _ => {}
        }
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
    /// The user's name for the recording that is running, which goes into its file name.
    label: String,
    notice: Option<(String, bool)>,
    files: Vec<PathBuf>,
    doomed: Option<Doomed>,
    replay: Option<Replay>,
}

impl Default for Flight {
    fn default() -> Self {
        Self {
            recorder: None,
            full: false,
            directory: storage::default_folder(),
            label: String::new(),
            notice: None,
            files: Vec::new(),
            doomed: None,
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

    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// Drops the recording that is running, with nothing saved.
    pub fn discard(&mut self) {
        self.recorder = None;
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
                    let stem = stem(&trace, &self.label);
                    self.label.clear();
                    let directory = PathBuf::from(&self.directory);
                    return blocking::run(move || {
                        trace
                            .save(&directory, &stem)
                            .map_err(|error| error.to_string())
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
            Message::Label(label) => self.label = label,
            Message::Ask(doomed) => self.doomed = Some(doomed),
            Message::Cancel => self.doomed = None,
            Message::Confirm => {
                let paths = self
                    .doomed
                    .take()
                    .map_or_else(Vec::new, |doomed| self.listed(&doomed));
                return blocking::run(move || remove(&paths)).map(Message::Deleted);
            }
            Message::Deleted(result) => {
                self.notice = Some(match result {
                    Ok(1) => ("Deleted 1 recording".to_owned(), false),
                    Ok(count) => (format!("Deleted {count} recordings"), false),
                    Err(error) => (error, true),
                });
                return self.enter();
            }
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
            Message::Play
            | Message::Seek(_)
            | Message::Speed(_)
            | Message::Sensor(_)
            | Message::Tick => {
                if let Some(replay) = &mut self.replay {
                    replay.update(message);
                }
            }
        }
        Task::none()
    }

    /// The files `doomed` stands for that are in the list, so nothing else can be deleted.
    fn listed(&self, doomed: &Doomed) -> Vec<PathBuf> {
        match doomed {
            Doomed::One(path) => self
                .files
                .iter()
                .filter(|file| *file == path)
                .cloned()
                .collect(),
            Doomed::All => self.files.clone(),
        }
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
            text_input("Name this recording (optional)", &self.label)
                .on_input(Message::Label)
                .width(260),
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
            let actions = if self.doomed == Some(Doomed::One(path.clone())) {
                confirmation("Delete this recording?".to_owned())
            } else {
                row![
                    button("Replay")
                        .on_press(Message::Open(path.clone()))
                        .style(button::secondary),
                    button("Delete")
                        .on_press(Message::Ask(Doomed::One(path.clone())))
                        .style(button::secondary),
                ]
                .spacing(8)
                .into()
            };
            row![
                text(
                    path.file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
                ),
                space::horizontal(),
                actions,
            ]
            .align_y(iced::Center)
            .into()
        });
        let everything = if self.doomed == Some(Doomed::All) {
            confirmation(format!("Delete all {} recordings?", self.files.len()))
        } else {
            button("Delete all")
                .on_press(Message::Ask(Doomed::All))
                .style(button::secondary)
                .into()
        };
        column![
            row![
                text(format!("{} recordings", self.files.len())).style(text::secondary),
                space::horizontal(),
                everything,
            ]
            .align_y(iced::Center),
            scrolling(column(rows).spacing(6)).height(Fill),
        ]
        .spacing(12)
        .into()
    }
}

/// A question with the button that confirms it and the one that backs out.
fn confirmation<'a>(question: String) -> Element<'a, Message> {
    row![
        text(question).style(text::danger),
        button("Delete")
            .on_press(Message::Confirm)
            .style(button::danger),
        button("Keep")
            .on_press(Message::Cancel)
            .style(button::secondary),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
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
        scrolling(flow(CARD_WIDTH, CARD_SPACING, move || cards(replay))).height(Fill),
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
                .line(plot.series(|frame| frame.cpu), Tone::Primary)
                .named("Utilization")
                .readout(as_percent),
        ),
        card(
            "Memory",
            format!("{:.0}%", at.memory),
            plot.graph(percent)
                .line(plot.series(|frame| frame.memory), Tone::Primary)
                .named("In use")
                .readout(as_percent),
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
                .named("Read")
                .line(
                    plot.series(|frame| level(frame.disks[index].write)),
                    Tone::Warning,
                )
                .named("Write")
                .readout(as_rate),
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
                .named("Receive")
                .line(
                    plot.series(|frame| level(frame.interfaces[index].send)),
                    Tone::Warning,
                )
                .named("Send")
                .readout(as_rate),
        ));
    }
    for (index, name) in trace.header.gpus.iter().enumerate() {
        cards.push(card(
            &format!("GPU {name}"),
            format!("{:.0}%", at.gpus[index].busy),
            plot.graph(percent)
                .line(plot.series(|frame| frame.gpus[index].busy), Tone::Primary)
                .named("Utilization")
                .line(plot.series(|frame| frame.gpus[index].memory), Tone::Warning)
                .named("Video memory")
                .readout(as_percent),
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
    let unit = channel.unit.clone();
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
                    .line(plot.series(|frame| frame.sensors[index]), Tone::Primary)
                    .named(channel.label.clone())
                    .readout(move |value| format!("{value:.1} {unit}")),
            ),
        ]
        .width(Fill)
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
            container(graph.view()).height(LABELLED_HEIGHT),
        ]
        .spacing(6),
    )
    .width(Fill)
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

/// What a recording is called on disk: when it began, how long it ran and the user's name for it.
fn stem(trace: &Trace, label: &str) -> String {
    let seconds = trace.frames.last().map_or(0.0, |frame| frame.seconds);
    name(trace.header.recorded_at, seconds, label)
}

fn name(recorded_at: u64, seconds: f32, label: &str) -> String {
    let seconds = Duration::try_from_secs_f32(seconds).map_or(0, |length| length.as_secs());
    let length = if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}min", seconds / 60)
    };
    let mut stem = format!("peepers-{}-{length}", format::file_stamp(recorded_at));
    let label = slug(label);
    if !label.is_empty() {
        stem.push('-');
        stem.push_str(&label);
    }
    stem
}

/// The words of `label` joined by hyphens, which a file name can hold whatever was typed.
fn slug(label: &str) -> String {
    let words: Vec<&str> = label
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let slug: String = words.join("-").chars().take(40).collect();
    slug.trim_end_matches('-').to_owned()
}

/// Deletes each file, stopping at the first that cannot be, and says how many went.
fn remove(paths: &[PathBuf]) -> Result<usize, String> {
    for path in paths {
        fs::remove_file(path)
            .map_err(|error| format!("{} could not be deleted: {error}", path.display()))?;
    }
    Ok(paths.len())
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

    #[test]
    fn a_recording_is_named_for_when_how_long_and_what() {
        let stamp = format::file_stamp(1_760_568_000);
        assert_eq!(
            name(1_760_568_000, 725.4, "Gaming, 4K!"),
            format!("peepers-{stamp}-12min-Gaming-4K")
        );
        assert_eq!(
            name(1_760_568_000, 45.0, ""),
            format!("peepers-{stamp}-45s")
        );
    }

    #[test]
    fn a_label_becomes_a_safe_short_slug() {
        assert_eq!(slug("  ../etc/passwd  "), "etc-passwd");
        assert_eq!(slug("!!!"), "");
        let long = slug(&"word ".repeat(30));
        assert!(long.len() <= 40 && !long.ends_with('-'));
    }

    #[test]
    fn only_listed_files_can_be_deleted() {
        let mut flight = Flight::default();
        let kept = PathBuf::from("/listed/peepers-a.json");
        flight.files = vec![kept.clone(), PathBuf::from("/listed/peepers-b.json")];
        assert_eq!(flight.listed(&Doomed::One(kept.clone())), [kept]);
        assert!(
            flight
                .listed(&Doomed::One(PathBuf::from("/etc/passwd")))
                .is_empty()
        );
        assert_eq!(flight.listed(&Doomed::All).len(), 2);
    }

    #[test]
    fn removing_deletes_each_file_and_stops_at_a_failure() {
        let directory = std::env::current_exe()
            .unwrap()
            .with_file_name(format!("flight-remove-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let (one, two) = (
            directory.join("peepers-1.json"),
            directory.join("peepers-2.json"),
        );
        fs::write(&one, "{}").unwrap();
        fs::write(&two, "{}").unwrap();
        assert_eq!(remove(&[one.clone(), two.clone()]), Ok(2));
        assert!(!one.exists() && !two.exists());
        let error = remove(&[one]).unwrap_err();
        assert!(error.contains("could not be deleted"), "{error}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deleting_asks_first_and_can_be_called_off() {
        let mut flight = Flight::default();
        drop(flight.update(Message::Ask(Doomed::All), None));
        assert_eq!(flight.doomed, Some(Doomed::All));
        drop(flight.update(Message::Cancel, None));
        assert_eq!(flight.doomed, None);
    }
}
