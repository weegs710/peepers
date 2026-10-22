use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::pci;
use crate::sysfs;

const HWMON_DIR: &str = "/sys/class/hwmon";

/// Just under a second, so a sampler that ticks once a second still reads afresh every time.
const REFRESH: Duration = Duration::from_millis(900);

/// A sensor read that is repeated at most about once a second: a drive or GPU query takes
/// milliseconds, which a sampler running several times a second cannot afford on every tick.
#[derive(Debug)]
pub struct Cached<T> {
    value: T,
    at: Option<Instant>,
}

impl<T: Default> Default for Cached<T> {
    fn default() -> Self {
        Self {
            value: T::default(),
            at: None,
        }
    }
}

impl<T: Clone> Cached<T> {
    /// The last value, read again first when it is older than a second.
    pub fn get(&mut self, read: impl FnOnce() -> T) -> T {
        if self.at.is_none_or(|at| at.elapsed() >= REFRESH) {
            self.value = read();
            self.at = Some(Instant::now());
        }
        self.value.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Temperature,
    Voltage,
    Current,
    Power,
    Frequency,
    Fan,
}

impl Kind {
    const ALL: [Self; 6] = [
        Self::Temperature,
        Self::Voltage,
        Self::Current,
        Self::Power,
        Self::Frequency,
        Self::Fan,
    ];

    /// The attribute-name prefix hwmon uses for the kind.
    fn prefix(self) -> &'static str {
        match self {
            Self::Temperature => "temp",
            Self::Voltage => "in",
            Self::Current => "curr",
            Self::Power => "power",
            Self::Frequency => "freq",
            Self::Fan => "fan",
        }
    }

    #[must_use]
    pub fn unit(self) -> &'static str {
        match self {
            Self::Temperature => "°C",
            Self::Voltage => "V",
            Self::Current => "A",
            Self::Power => "W",
            Self::Frequency => "MHz",
            Self::Fan => "RPM",
        }
    }

    /// Divisor from the kernel's raw unit to [`Kind::unit`]: millidegrees, millivolts, milliamps,
    /// microwatts, hertz and revolutions.
    fn scale(self) -> f32 {
        match self {
            Self::Temperature | Self::Voltage | Self::Current => 1e3,
            Self::Power | Self::Frequency => 1e6,
            Self::Fan => 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub chip: String,
    pub label: String,
    pub kind: Kind,
    raw: i64,
}

impl Reading {
    /// A reading from a tool that reports in the unit of [`Kind::unit`], not from sysfs.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a sensor value is far inside the range of an i64"
    )]
    pub(crate) fn measured(chip: &str, label: &str, kind: Kind, value: f32) -> Self {
        Self {
            chip: chip.to_owned(),
            label: label.to_owned(),
            kind,
            raw: (f64::from(value) * f64::from(kind.scale())).round() as i64,
        }
    }

    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "a reading needs no more than display precision"
    )]
    pub fn value(&self) -> f32 {
        self.raw as f32 / self.kind.scale()
    }
}

/// One hwmon chip with the device behind it named where the system knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    /// The directory name, such as `hwmon3`, which tells apart chips that share a driver name.
    pub id: String,
    pub name: String,
    pub model: Option<String>,
    pub readings: Vec<Reading>,
}

/// Every hwmon chip on the machine, in the kernel's order.
#[must_use]
pub fn chips() -> Vec<Chip> {
    sysfs::numbered_dirs(HWMON_DIR, "hwmon")
        .map(|chip| {
            let device = chip.join("device");
            Chip {
                id: chip
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
                name: sysfs::read(chip.join("name")).unwrap_or_default(),
                model: sysfs::read(device.join("model")).or_else(|| pci::model(&device)),
                readings: read_chip(&chip),
            }
        })
        .collect()
}

/// Every channel of the hwmon chips under one device. Drivers differ on whether the chips sit
/// directly in the device directory (`NVMe`) or in a `hwmon` directory of their own (DRM).
#[must_use]
pub fn read_device(device: &Path) -> Vec<Reading> {
    sysfs::numbered_dirs(device, "hwmon")
        .chain(sysfs::numbered_dirs(device.join("hwmon"), "hwmon"))
        .flat_map(|chip| read_chip(&chip))
        .collect()
}

fn read_chip(chip: &Path) -> Vec<Reading> {
    let name: String = sysfs::read(chip.join("name")).unwrap_or_default();
    channels(chip)
        .into_iter()
        .filter_map(|((kind, index), file)| {
            let prefix = kind.prefix();
            Some(Reading {
                chip: name.clone(),
                label: sysfs::read(chip.join(format!("{prefix}{index}_label")))
                    .unwrap_or_else(|| format!("{prefix}{index}")),
                kind,
                raw: sysfs::read(file)?,
            })
        })
        .collect()
}

/// The `_input` file of each channel, or its `_average` when only that exists.
fn channels(chip: &Path) -> BTreeMap<(Kind, u32), PathBuf> {
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(chip)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        if let Some((channel, instantaneous)) = entry.file_name().to_str().and_then(parse_channel) {
            if instantaneous {
                found.insert(channel, entry.path());
            } else {
                found.entry(channel).or_insert_with(|| entry.path());
            }
        }
    }
    found
}

/// Splits `temp1_input` into its channel and whether the file is the instantaneous one.
fn parse_channel(name: &str) -> Option<((Kind, u32), bool)> {
    let (channel, suffix) = name.split_once('_')?;
    let instantaneous = match suffix {
        "input" => true,
        "average" => false,
        _ => return None,
    };
    Kind::ALL.into_iter().find_map(|kind| {
        let index = channel.strip_prefix(kind.prefix())?.parse().ok()?;
        Some(((kind, index), instantaneous))
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_fresh_value_is_not_read_again() {
        let reads = Cell::new(0);
        let mut cached = Cached::default();
        let mut read = || {
            reads.set(reads.get() + 1);
            vec![reads.get()]
        };
        assert_eq!(cached.get(&mut read), [1]);
        assert_eq!(cached.get(&mut read), [1]);
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn channel_names_split_into_kind_and_index() {
        assert_eq!(
            parse_channel("temp3_input"),
            Some(((Kind::Temperature, 3), true))
        );
        assert_eq!(
            parse_channel("power1_average"),
            Some(((Kind::Power, 1), false))
        );
        assert_eq!(parse_channel("in0_input"), Some(((Kind::Voltage, 0), true)));
    }

    #[test]
    fn other_attributes_are_not_channels() {
        assert_eq!(parse_channel("temp1_max"), None);
        assert_eq!(parse_channel("name"), None);
        assert_eq!(parse_channel("fanx_input"), None);
    }

    #[test]
    fn raw_values_scale_to_their_unit() {
        let reading = Reading {
            chip: "k10temp".into(),
            label: "Tctl".into(),
            kind: Kind::Temperature,
            raw: 45_500,
        };
        assert!((reading.value() - 45.5).abs() < f32::EPSILON);
    }

    #[test]
    fn a_measured_reading_reads_back_in_its_unit() {
        let reading = Reading::measured("nvidia", "power draw", Kind::Power, 23.5);
        assert!((reading.value() - 23.5).abs() < 1e-4);
        let reading = Reading::measured("nvidia", "graphics clock", Kind::Frequency, 1950.0);
        assert!((reading.value() - 1950.0).abs() < 1e-3);
    }

    /// The attributes the kernel documents for the `i915` driver on a discrete card: only the
    /// `_input` ones are readings, the limits and the energy counter are not.
    #[test]
    fn an_intel_card_shows_its_temperature_fan_and_voltage_and_none_of_its_limits() {
        let directory = std::env::current_exe()
            .unwrap()
            .with_file_name(format!("sensors-i915-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        for (file, content) in [
            ("name", "i915"),
            ("temp1_input", "61000"),
            ("fan1_input", "1450"),
            ("in0_input", "950"),
            ("power1_max", "190000000"),
            ("power1_rated_max", "225000000"),
            ("energy1_input", "123456"),
        ] {
            fs::write(directory.join(file), format!("{content}\n")).unwrap();
        }
        let readings = read_chip(&directory);
        fs::remove_dir_all(&directory).unwrap();
        let value = |kind| {
            readings
                .iter()
                .find(|reading| reading.kind == kind)
                .map(Reading::value)
                .unwrap()
        };
        assert_eq!(readings.len(), 3);
        assert!((value(Kind::Temperature) - 61.0).abs() < 1e-3);
        assert!((value(Kind::Voltage) - 0.95).abs() < 1e-3);
        assert!((value(Kind::Fan) - 1450.0).abs() < 1e-3);
        assert!(readings.iter().all(|reading| reading.chip == "i915"));
    }

    #[test]
    fn this_machine_has_a_chip_with_a_temperature_sensor() {
        let chips = chips();
        assert!(chips.iter().any(|chip| {
            chip.readings
                .iter()
                .any(|reading| reading.kind == Kind::Temperature)
        }));
        assert!(chips.iter().all(|chip| chip.id.starts_with("hwmon")));
    }
}
