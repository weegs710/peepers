use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::Duration;

use iced::widget::{checkbox, column, container, pick_list, row, slider, text};
use iced::{Element, Subscription, Task, Theme, event, keyboard};
use serde::{Deserialize, Serialize};

use crate::Page;
use crate::blocking;
use crate::notice::Notice;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    Millis100,
    Millis250,
    Millis500,
    #[default]
    Second,
    TwoSeconds,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// A theme's name as iced lists it, so a file written by one version still loads in the next.
    theme: String,
    pub start_page: Page,
    pub temperature: Temperature,
    pub interval: Interval,
    pub tray: bool,
    /// The size body text is drawn at; everything else, graph lines and labels included, scales
    /// with it.
    text_size: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark.to_string(),
            start_page: Page::Processes,
            temperature: Temperature::Celsius,
            interval: Interval::Second,
            tray: true,
            text_size: TEXT_DEFAULT,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        let home = env::var("HOME").unwrap_or_default();
        PathBuf::from(env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config")))
            .join("peepers/settings.json")
    }

    /// A missing file is the defaults; a file that cannot be understood is an error.
    ///
    /// # Errors
    ///
    /// Fails when the file exists but cannot be read or parsed.
    pub fn load() -> Result<Self, String> {
        match fs::read_to_string(Self::path()) {
            Ok(text) => serde_json::from_str(&text)
                .map(Self::normalised)
                .map_err(|error| error.to_string()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Replaces the file in one step.
    fn save(&self) -> io::Result<()> {
        let path = Self::path();
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        let partial = path.with_extension("json.partial");
        fs::write(
            &partial,
            serde_json::to_string_pretty(self).map_err(io::Error::from)?,
        )?;
        fs::rename(partial, path)
    }

    /// The text size moved onto the nearest notch inside the allowed range, so a hand-edited file
    /// cannot make the interface unusable.
    fn normalised(mut self) -> Self {
        let (least, most) = (*TEXT_SIZES.start(), *TEXT_SIZES.end());
        let notches = self.text_size.clamp(least, most) - least;
        let snapped = (notches + TEXT_STEP / 2) / TEXT_STEP * TEXT_STEP;
        self.text_size = (least + snapped).min(most);
        self
    }

    /// How much larger or smaller than the default the interface is drawn.
    pub fn scale(&self) -> f32 {
        f32::from(self.text_size) / f32::from(TEXT_DEFAULT)
    }

    /// The text size one step along, held inside the allowed range.
    fn zoomed(&self, zoom: Zoom) -> u8 {
        let (least, most) = (*TEXT_SIZES.start(), *TEXT_SIZES.end());
        match zoom {
            Zoom::In => (self.text_size + TEXT_STEP).min(most),
            Zoom::Out => self.text_size.saturating_sub(TEXT_STEP).max(least),
            Zoom::Reset => TEXT_DEFAULT,
        }
    }

    /// The saved theme, or the default dark one when the name is not one this version knows.
    pub fn theme(&self) -> Theme {
        Theme::ALL
            .iter()
            .find(|theme| theme.to_string() == self.theme)
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
    Saved(Result<(), String>),
    Dismiss,
}

/// The settings and what to tell the user about saving them.
#[derive(Debug, Default)]
pub struct Preferences {
    pub settings: Settings,
    /// The text size under the pointer while the slider is held.
    dragging: Option<u8>,
    notice: Option<Notice>,
}

impl Preferences {
    /// Settings from disk; a file that cannot be read leaves the defaults and says why.
    pub fn load() -> Self {
        match Settings::load() {
            Ok(settings) => Self {
                settings,
                dragging: None,
                notice: None,
            },
            Err(error) => Self {
                settings: Settings::default(),
                dragging: None,
                notice: Some(Notice::failure(format!(
                    "The settings file could not be read, so the defaults are in use: {error}"
                ))),
            },
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Theme(theme) => self.settings.theme = theme.to_string(),
            Message::StartPage(page) => self.settings.start_page = page,
            Message::Temperature(temperature) => self.settings.temperature = temperature,
            Message::Interval(interval) => self.settings.interval = interval,
            Message::Tray(on) => self.settings.tray = on,
            Message::TextSizeDragged(size) => {
                self.dragging = Some(size);
                return Task::none();
            }
            Message::TextSizeChosen => {
                if let Some(size) = self.dragging.take() {
                    self.settings.text_size = size;
                }
            }
            Message::Zoom(zoom) => self.settings.text_size = self.settings.zoomed(zoom),
            Message::Saved(result) => {
                self.notice = result.err().map(|error| {
                    Notice::failure(format!("The settings could not be saved: {error}"))
                });
                return Task::none();
            }
            Message::Dismiss => {
                self.notice = None;
                return Task::none();
            }
        }
        let settings = self.settings.clone();
        blocking::run(move || settings.save().map_err(|error| error.to_string()))
            .map(Message::Saved)
    }

    /// `tray_error` is why the tray icon is missing when it was asked for.
    pub fn view<'a>(&'a self, tray_error: Option<&'a str>) -> Element<'a, Message> {
        column![
            text("Settings").size(24),
            setting(
                "Theme",
                pick_list(Theme::ALL, Some(self.settings.theme()), Message::Theme).width(CONTROL)
            ),
            setting(
                "Start on",
                pick_list(
                    Page::ALL,
                    Some(self.settings.start_page),
                    Message::StartPage
                )
                .width(CONTROL)
            ),
            setting(
                "Temperatures",
                pick_list(
                    Temperature::ALL,
                    Some(self.settings.temperature),
                    Message::Temperature
                )
                .width(CONTROL)
            ),
            setting(
                "Read the machine every",
                pick_list(
                    Interval::ALL,
                    Some(self.settings.interval),
                    Message::Interval
                )
                .width(CONTROL)
            ),
            setting(
                "Text size",
                row![
                    slider(
                        TEXT_SIZES,
                        self.dragging.unwrap_or(self.settings.text_size),
                        Message::TextSizeDragged
                    )
                    .step(TEXT_STEP)
                    .on_release(Message::TextSizeChosen)
                    .width(CONTROL),
                    text(self.dragging.unwrap_or(self.settings.text_size).to_string()),
                ]
                .spacing(12)
                .align_y(iced::Center)
            ),
            self.preview(),
            checkbox(self.settings.tray)
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
        let ratio = f32::from(self.dragging.unwrap_or(self.settings.text_size))
            / f32::from(self.settings.text_size);
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let size = |text_size| {
            Settings {
                text_size,
                ..Settings::default()
            }
            .normalised()
            .text_size
        };
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
        let at = |text_size| Settings {
            text_size,
            ..Settings::default()
        };
        assert_eq!(at(16).zoomed(Zoom::In), 18);
        assert_eq!(at(16).zoomed(Zoom::Out), 14);
        assert_eq!(at(24).zoomed(Zoom::In), 24);
        assert_eq!(at(12).zoomed(Zoom::Out), 12);
        assert_eq!(at(22).zoomed(Zoom::Reset), TEXT_DEFAULT);
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
    fn an_old_file_without_a_text_size_keeps_the_default() {
        let settings: Settings = serde_json::from_str(r#"{"temperature": "Fahrenheit"}"#).unwrap();
        assert_eq!(settings.text_size, TEXT_DEFAULT);
    }

    #[test]
    fn dragging_only_previews_and_letting_go_applies() {
        let mut preferences = Preferences::default();
        drop(preferences.update(Message::TextSizeDragged(22)));
        assert_eq!(preferences.settings.text_size, TEXT_DEFAULT);
        drop(preferences.update(Message::TextSizeChosen));
        assert_eq!(preferences.settings.text_size, 22);
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
        for theme in Theme::ALL {
            let settings = Settings {
                theme: theme.to_string(),
                ..Settings::default()
            };
            assert_eq!(&settings.theme(), theme);
        }
    }

    #[test]
    fn an_unknown_theme_falls_back_to_dark() {
        let settings = Settings {
            theme: "No Such Theme".to_owned(),
            ..Settings::default()
        };
        assert_eq!(settings.theme(), Theme::Dark);
    }

    #[test]
    fn a_partial_file_keeps_the_defaults_for_what_it_omits() {
        let settings: Settings = serde_json::from_str(r#"{"temperature": "Fahrenheit"}"#).unwrap();
        assert_eq!(settings.temperature, Temperature::Fahrenheit);
        assert_eq!(settings.start_page, Page::Processes);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let settings = Settings {
            start_page: Page::Pressure,
            interval: Interval::FiveSeconds,
            ..Settings::default()
        };
        let again: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(again, settings);
    }
}
