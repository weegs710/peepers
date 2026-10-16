use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use iced::widget::{checkbox, column, pick_list, row, text};
use iced::{Element, Task, Theme};
use serde::{Deserialize, Serialize};

use crate::Page;
use crate::blocking;

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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark.to_string(),
            start_page: Page::Processes,
            temperature: Temperature::Celsius,
            interval: Interval::Second,
            tray: true,
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
            Ok(text) => serde_json::from_str(&text).map_err(|error| error.to_string()),
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

    /// The saved theme, or the default dark one when the name is not one this version knows.
    pub fn theme(&self) -> Theme {
        Theme::ALL
            .iter()
            .find(|theme| theme.to_string() == self.theme)
            .cloned()
            .unwrap_or(Theme::Dark)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Theme(Theme),
    StartPage(Page),
    Temperature(Temperature),
    Interval(Interval),
    Tray(bool),
    Saved(Result<(), String>),
}

/// The settings and what to tell the user about saving them.
#[derive(Debug, Default)]
pub struct Preferences {
    pub settings: Settings,
    notice: Option<(String, bool)>,
}

impl Preferences {
    /// Settings from disk; a file that cannot be read leaves the defaults and says why.
    pub fn load() -> Self {
        match Settings::load() {
            Ok(settings) => Self {
                settings,
                notice: None,
            },
            Err(error) => Self {
                settings: Settings::default(),
                notice: Some((
                    format!(
                        "The settings file could not be read, so the defaults are in use: {error}"
                    ),
                    true,
                )),
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
            Message::Saved(result) => {
                self.notice = result
                    .err()
                    .map(|error| (format!("The settings could not be saved: {error}"), true));
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
            checkbox(self.settings.tray)
                .label("Show a tray icon with the processor and memory load")
                .on_toggle(Message::Tray),
            text(format!("Saved to {}", Settings::path().display()))
                .size(12)
                .style(text::secondary),
        ]
        .extend(tray_error.map(|error| text(error).style(text::danger).into()))
        .extend(self.notice.as_ref().map(|(message, failed)| {
            text(message)
                .style(if *failed { text::danger } else { text::success })
                .into()
        }))
        .spacing(16)
        .padding(16)
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
