use std::env;
use std::fmt;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::Duration;

use iced::widget::{checkbox, column, container, pick_list, row, slider, text};
use iced::{Element, Subscription, Task, Theme, event, keyboard};
use serde::{Deserialize, Serialize};

use crate::Page;
use crate::blocking;
use crate::notice::Notice;
use crate::theme;

mod file;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Temperature {
    #[default]
    Celsius,
    Fahrenheit,
}

impl Temperature {
    const ALL: [Self; 2] = [Self::Celsius, Self::Fahrenheit];

    pub fn convert(self, celsius: f32) -> f32 {
        match self {
            Self::Celsius => celsius,
            Self::Fahrenheit => celsius * 9.0 / 5.0 + 32.0,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Celsius => "°C",
            Self::Fahrenheit => "°F",
        }
    }
}

impl fmt::Display for Temperature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Celsius => "Celsius",
            Self::Fahrenheit => "Fahrenheit",
        })
    }
}

const CONTROL: u32 = 200;

/// The text sizes the interface can be scaled to, in pixels: below the least the smallest captions
/// would be unreadable, and above the most the pages no longer fit a window.
const TEXT_SIZES: RangeInclusive<u8> = 12..=24;
const TEXT_STEP: u8 = 2;
/// The size the interface is drawn at without any scaling.
const TEXT_DEFAULT: u8 = 16;

/// How often the machine is read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interval {
    #[serde(rename = "100ms")]
    Millis100,
    #[serde(rename = "250ms")]
    Millis250,
    #[serde(rename = "500ms")]
    Millis500,
    #[default]
    #[serde(rename = "1s")]
    Second,
    #[serde(rename = "2s")]
    TwoSeconds,
    #[serde(rename = "5s")]
    FiveSeconds,
}

impl Interval {
    const ALL: [Self; 6] = [
        Self::Millis100,
        Self::Millis250,
        Self::Millis500,
        Self::Second,
        Self::TwoSeconds,
        Self::FiveSeconds,
    ];

    pub fn duration(self) -> Duration {
        match self {
            Self::Millis100 => Duration::from_millis(100),
            Self::Millis250 => Duration::from_millis(250),
            Self::Millis500 => Duration::from_millis(500),
            Self::Second => Duration::from_secs(1),
            Self::TwoSeconds => Duration::from_secs(2),
            Self::FiveSeconds => Duration::from_secs(5),
        }
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let duration = self.duration();
        match duration.as_secs() {
            0 => write!(formatter, "{} ms", duration.as_millis()),
            1 => formatter.write_str("1 second"),
            seconds => write!(formatter, "{seconds} seconds"),
        }
    }
}

/// The settings file, one table for each of these.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    appearance: Appearance,
    pub startup: Startup,
    pub sampling: Sampling,
    pub units: Units,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Appearance {
    /// A theme's name as iced lists it, so a file written by one version still loads in the next.
    theme: String,
    /// The size body text is drawn at; everything else, graph lines and labels included, scales
    /// with it.
    text_size: u8,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: Theme::Dark.to_string(),
            text_size: TEXT_DEFAULT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Startup {
    pub page: Page,
    pub tray: bool,
}

impl Default for Startup {
    fn default() -> Self {
        Self {
            page: Page::Processes,
            tray: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sampling {
    pub interval: Interval,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Units {
    pub temperature: Temperature,
}

impl Settings {
    pub fn path() -> PathBuf {
        let home = env::var("HOME").unwrap_or_default();
        PathBuf::from(env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config")))
            .join("peepers/settings.toml")
    }

    /// The text size moved onto the nearest notch inside the allowed range, so a hand-edited file
    /// cannot make the interface unusable.
    fn normalised(mut self) -> Self {
        let (least, most) = (*TEXT_SIZES.start(), *TEXT_SIZES.end());
        let notches = self.appearance.text_size.clamp(least, most) - least;
        let snapped = (notches + TEXT_STEP / 2) / TEXT_STEP * TEXT_STEP;
        self.appearance.text_size = (least + snapped).min(most);
        self
    }

    /// How much larger or smaller than the default the interface is drawn.
    pub fn scale(&self) -> f32 {
        f32::from(self.appearance.text_size) / f32::from(TEXT_DEFAULT)
    }

    /// The text size one step along, held inside the allowed range.
    fn zoomed(&self, zoom: Zoom) -> u8 {
        let (least, most) = (*TEXT_SIZES.start(), *TEXT_SIZES.end());
        let size = self.appearance.text_size;
        match zoom {
            Zoom::In => (size + TEXT_STEP).min(most),
            Zoom::Out => size.saturating_sub(TEXT_STEP).max(least),
            Zoom::Reset => TEXT_DEFAULT,
        }
    }

    /// The saved theme, or the default dark one when the name is not one this version knows.
    pub fn theme(&self) -> Theme {
        theme::all()
            .iter()
            .find(|theme| theme.to_string() == self.appearance.theme)
            .cloned()
            .unwrap_or(Theme::Dark)
    }
}

/// A change of the text size made from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

impl Zoom {
    fn from_key(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Self> {
        if !modifiers.control() {
            return None;
        }
        match key.as_ref() {
            keyboard::Key::Character("+" | "=") => Some(Self::In),
            keyboard::Key::Character("-" | "_") => Some(Self::Out),
            keyboard::Key::Character("0") => Some(Self::Reset),
            _ => None,
        }
    }
}

/// Ctrl with plus, minus or zero resizes the text from any page, the way a browser does.
pub fn keys() -> Subscription<Message> {
    event::listen_with(|event, _status, _window| match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key,
            modifiers,
            ..
        }) => Zoom::from_key(&modified_key, modifiers).map(Message::Zoom),
        _ => None,
    })
}

#[derive(Debug, Clone)]
pub enum Message {
    Theme(Theme),
    StartPage(Page),
    Temperature(Temperature),
    Interval(Interval),
    Tray(bool),
    /// The slider is being dragged: only the preview follows, so the slider does not move under
    /// the pointer.
    TextSizeDragged(u8),
    TextSizeChosen,
    Zoom(Zoom),
    /// What was written, or why it could not be.
    Saved(Result<Settings, String>),
    Dismiss,
}

/// The settings and what to tell the user about saving them.
#[derive(Debug, Default)]
pub struct Preferences {
    pub settings: Settings,
    /// The settings as last read or written, which a save is measured against.
    base: Settings,
    /// Where changes are written; without one they stay in memory.
    path: Option<PathBuf>,
    /// The text size under the pointer while the slider is held.
    dragging: Option<u8>,
    notice: Option<Notice>,
}

impl Preferences {
    /// Settings from disk; a file that cannot be read leaves the defaults and says why.
    pub fn load() -> Self {
        let path = Settings::path();
        match file::read(&path) {
            Ok(settings) => Self {
                base: settings.clone(),
                settings,
                path: Some(path),
                dragging: None,
                notice: None,
            },
            Err(error) => Self {
                settings: Settings::default(),
                base: Settings::default(),
                path: Some(path),
                dragging: None,
                notice: Some(Notice::failure(format!(
                    "The settings file could not be read, so the defaults are in use: {error}"
                ))),
            },
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Theme(theme) => self.settings.appearance.theme = theme.to_string(),
            Message::StartPage(page) => self.settings.startup.page = page,
            Message::Temperature(temperature) => self.settings.units.temperature = temperature,
            Message::Interval(interval) => self.settings.sampling.interval = interval,
            Message::Tray(on) => self.settings.startup.tray = on,
            Message::TextSizeDragged(size) => {
                self.dragging = Some(size);
                return Task::none();
            }
            Message::TextSizeChosen => {
                if let Some(size) = self.dragging.take() {
                    self.settings.appearance.text_size = size;
                }
            }
            Message::Zoom(zoom) => self.settings.appearance.text_size = self.settings.zoomed(zoom),
            Message::Saved(result) => {
                self.notice = match result {
                    Ok(saved) => {
                        self.base = saved;
                        None
                    }
                    Err(error) => Some(Notice::failure(format!(
                        "The settings could not be saved: {error}"
                    ))),
                };
                return Task::none();
            }
            Message::Dismiss => {
                self.notice = None;
                return Task::none();
            }
        }
        self.write()
            .map_or_else(Task::none, |write| blocking::run(write).map(Message::Saved))
    }

    /// The write a change calls for, or none when there is no file to write to.
    fn write(&self) -> Option<impl FnOnce() -> Result<Settings, String> + Send + 'static> {
        let path = self.path.clone()?;
        let (ours, base) = (self.settings.clone(), self.base.clone());
        Some(move || file::write(&path, &ours, &base).map(|()| ours))
    }

    /// `tray_error` is why the tray icon is missing when it was asked for.
    pub fn view<'a>(&'a self, tray_error: Option<&'a str>) -> Element<'a, Message> {
        column![
            text("Settings").size(24),
            setting(
                "Theme",
                pick_list(theme::all(), Some(self.settings.theme()), Message::Theme).width(CONTROL)
            ),
            setting(
                "Start on",
                pick_list(
                    Page::ALL,
                    Some(self.settings.startup.page),
                    Message::StartPage
                )
                .width(CONTROL)
            ),
            setting(
                "Temperatures",
                pick_list(
                    Temperature::ALL,
                    Some(self.settings.units.temperature),
                    Message::Temperature
                )
                .width(CONTROL)
            ),
            setting(
                "Read the machine every",
                pick_list(
                    Interval::ALL,
                    Some(self.settings.sampling.interval),
                    Message::Interval
                )
                .width(CONTROL)
            ),
            setting(
                "Text size",
                row![
                    slider(
                        TEXT_SIZES,
                        self.dragging.unwrap_or(self.settings.appearance.text_size),
                        Message::TextSizeDragged
                    )
                    .step(TEXT_STEP)
                    .on_release(Message::TextSizeChosen)
                    .width(CONTROL),
                    text(
                        self.dragging
                            .unwrap_or(self.settings.appearance.text_size)
                            .to_string()
                    ),
                ]
                .spacing(12)
                .align_y(iced::Center)
            ),
            self.preview(),
            checkbox(self.settings.startup.tray)
                .label("Show a tray icon with the processor and memory load")
                .on_toggle(Message::Tray),
            text(format!("Saved to {}", Settings::path().display()))
                .size(12)
                .style(text::secondary),
        ]
        .extend(tray_error.map(|error| text(error).style(text::danger).into()))
        .extend(
            self.notice
                .as_ref()
                .map(|notice| notice.view(Message::Dismiss)),
        )
        .spacing(16)
        .padding(16)
        .into()
    }
}

impl Preferences {
    /// Sample text at the size being chosen, which is drawn in the units of the size in use now.
    fn preview(&self) -> Element<'_, Message> {
        let current = self.settings.appearance.text_size;
        let ratio = f32::from(self.dragging.unwrap_or(current)) / f32::from(current);
        let size = |base: f32| base * ratio;
        container(
            column![
                text("Peepers").size(size(24.0)),
                text("Processes, performance and every other page read like this.")
                    .size(size(16.0)),
                text("Captions and the small figures beside them read like this.")
                    .size(size(12.0))
                    .style(text::secondary),
                text("100%   75%   50%   25%   the scale down the side of a graph")
                    .size(size(11.0))
                    .style(text::secondary),
            ]
            .spacing(size(6.0)),
        )
        .padding(12)
        .style(container::bordered_box)
        .into()
    }
}

fn setting<'a>(
    label: &'static str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![text(label).width(220), control.into()]
        .spacing(16)
        .align_y(iced::Center)
        .into()
}

/// A directory of its own for one test, removed when the test ends.
#[cfg(test)]
struct Scratch(PathBuf);

#[cfg(test)]
impl Scratch {
    fn new(test: &str) -> Self {
        let path = env::temp_dir().join(format!("peepers-{test}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn text_size(text_size: u8) -> Settings {
        Settings {
            appearance: Appearance {
                text_size,
                ..Appearance::default()
            },
            ..Settings::default()
        }
    }

    fn theme(theme: &str) -> Settings {
        Settings {
            appearance: Appearance {
                theme: theme.to_owned(),
                ..Appearance::default()
            },
            ..Settings::default()
        }
    }

    #[test]
    fn fahrenheit_converts_from_celsius() {
        assert!((Temperature::Fahrenheit.convert(100.0) - 212.0).abs() < f32::EPSILON);
        assert!((Temperature::Celsius.convert(37.5) - 37.5).abs() < f32::EPSILON);
    }

    #[test]
    fn the_default_size_leaves_the_interface_as_drawn() {
        assert!((Settings::default().scale() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_text_size_is_moved_onto_a_notch_inside_the_range() {
        let size = |size| text_size(size).normalised().appearance.text_size;
        assert_eq!(size(0), 12);
        assert_eq!(size(11), 12);
        assert_eq!(size(13), 14);
        assert_eq!(size(15), 16);
        assert_eq!(size(16), 16);
        assert_eq!(size(23), 24);
        assert_eq!(size(200), 24);
    }

    #[test]
    fn zooming_steps_the_text_size_and_stops_at_the_ends() {
        assert_eq!(text_size(16).zoomed(Zoom::In), 18);
        assert_eq!(text_size(16).zoomed(Zoom::Out), 14);
        assert_eq!(text_size(24).zoomed(Zoom::In), 24);
        assert_eq!(text_size(12).zoomed(Zoom::Out), 12);
        assert_eq!(text_size(22).zoomed(Zoom::Reset), TEXT_DEFAULT);
    }

    #[test]
    fn control_with_plus_minus_or_zero_zooms() {
        let character = |text: &str| keyboard::Key::Character(text.into());
        let control = keyboard::Modifiers::CTRL;
        assert_eq!(Zoom::from_key(&character("+"), control), Some(Zoom::In));
        assert_eq!(Zoom::from_key(&character("="), control), Some(Zoom::In));
        assert_eq!(Zoom::from_key(&character("-"), control), Some(Zoom::Out));
        assert_eq!(Zoom::from_key(&character("0"), control), Some(Zoom::Reset));
        assert_eq!(Zoom::from_key(&character("c"), control), None);
    }

    #[test]
    fn plus_without_control_is_only_typing() {
        let plus = keyboard::Key::Character("+".into());
        assert_eq!(Zoom::from_key(&plus, keyboard::Modifiers::empty()), None);
    }

    #[test]
    fn dragging_only_previews_and_letting_go_applies() {
        let mut preferences = Preferences::default();
        drop(preferences.update(Message::TextSizeDragged(22)));
        assert_eq!(preferences.settings.appearance.text_size, TEXT_DEFAULT);
        drop(preferences.update(Message::TextSizeChosen));
        assert_eq!(preferences.settings.appearance.text_size, 22);
        assert!(preferences.dragging.is_none());
    }

    #[test]
    fn intervals_read_in_milliseconds_or_seconds() {
        assert_eq!(Interval::Millis250.to_string(), "250 ms");
        assert_eq!(Interval::Second.to_string(), "1 second");
        assert_eq!(Interval::FiveSeconds.to_string(), "5 seconds");
        assert_eq!(Interval::TwoSeconds.duration(), Duration::from_secs(2));
    }

    #[test]
    fn every_theme_name_finds_its_theme_again() {
        for name in crate::theme::all() {
            assert_eq!(&theme(&name.to_string()).theme(), name);
        }
    }

    #[test]
    fn an_unknown_theme_falls_back_to_dark() {
        assert_eq!(theme("No Such Theme").theme(), Theme::Dark);
    }

    #[test]
    fn every_page_is_written_under_the_name_the_command_line_uses() {
        for page in Page::ALL {
            let text = format!("[startup]\npage = \"{}\"\n", page.slug());
            let settings: Settings = toml_edit::de::from_str(&text).unwrap();
            assert_eq!(settings.startup.page, page);
        }
    }

    #[test]
    fn every_interval_is_written_the_way_it_is_typed() {
        let spellings = [
            ("100ms", Interval::Millis100),
            ("250ms", Interval::Millis250),
            ("500ms", Interval::Millis500),
            ("1s", Interval::Second),
            ("2s", Interval::TwoSeconds),
            ("5s", Interval::FiveSeconds),
        ];
        assert_eq!(spellings.len(), Interval::ALL.len());
        for (spelling, interval) in spellings {
            let text = format!("[sampling]\ninterval = \"{spelling}\"\n");
            let settings: Settings = toml_edit::de::from_str(&text).unwrap();
            assert_eq!(settings.sampling.interval, interval);
        }
    }

    #[test]
    fn a_change_with_no_file_to_write_to_is_not_saved() {
        let mut preferences = Preferences::default();
        assert_eq!(preferences.update(Message::Tray(false)).units(), 0);
    }

    #[test]
    fn a_change_is_written_to_the_file_the_settings_belong_to() {
        let scratch = Scratch::new("settings-written");
        let path = scratch.0.join("settings.toml");
        let mut preferences = Preferences {
            path: Some(path.clone()),
            ..Preferences::default()
        };
        preferences.settings.startup.tray = false;
        preferences.write().unwrap()().unwrap();
        assert_eq!(file::read(&path), Ok(preferences.settings));
    }

    /// What a task sends back when it is run to the end, as the application would receive it.
    fn messages_of(task: Task<Message>) -> Vec<Message> {
        let actions = iced_runtime::task::into_stream(task).expect("the change has a task to run");
        smol::block_on(smol::stream::StreamExt::collect::<Vec<_>>(actions))
            .into_iter()
            .filter_map(|action| match action {
                iced_runtime::Action::Output(message) => Some(message),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_change_made_in_the_window_reaches_the_file_with_its_comments_around_it() {
        let scratch = Scratch::new("settings-window");
        let path = scratch.0.join("settings.toml");
        let before = "# mine\n\n[appearance]\n# bigger\ntext_size = 16 # the default\n\n[alerts]\nrule = []\n";
        fs::write(&path, before).unwrap();
        let settings = file::read(&path).unwrap();
        let mut preferences = Preferences {
            base: settings.clone(),
            settings,
            path: Some(path.clone()),
            ..Preferences::default()
        };
        let task = preferences.update(Message::Zoom(Zoom::In));
        for message in messages_of(task) {
            drop(preferences.update(message));
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before.replace("= 16", "= 18")
        );
        assert_eq!(preferences.base, preferences.settings);
        assert!(preferences.notice.is_none());
    }

    #[test]
    fn once_saved_only_what_changed_since_is_written_again() {
        let scratch = Scratch::new("settings-measured");
        let path = scratch.0.join("settings.toml");
        let mut preferences = Preferences {
            path: Some(path.clone()),
            ..Preferences::default()
        };
        preferences.settings.startup.tray = false;
        let saved = preferences.write().unwrap()().unwrap();
        drop(preferences.update(Message::Saved(Ok(saved))));
        fs::write(&path, "[startup]\ntray = true\n").unwrap();
        preferences.settings.units.temperature = Temperature::Fahrenheit;
        preferences.write().unwrap()().unwrap();
        let after = file::read(&path).unwrap();
        assert!(after.startup.tray);
        assert_eq!(after.units.temperature, Temperature::Fahrenheit);
    }
}
