use jiff::Timestamp;
use jiff::tz::TimeZone;
use peepers_core::sensors::{Kind, Reading};

use crate::settings::Temperature;

pub use peepers_core::format::{bytes, frequency, marketing_name, megabits, rate, uptime};

/// A sensor value with its unit, temperatures in the unit the user chose.
pub fn sensor_value(kind: Kind, value: f32, temperature: Temperature) -> String {
    match kind {
        Kind::Temperature => format!("{:.0} {}", temperature.convert(value), temperature.symbol()),
        Kind::Power | Kind::Voltage | Kind::Current => format!("{value:.2} {}", kind.unit()),
        Kind::Frequency | Kind::Fan => format!("{value:.0} {}", kind.unit()),
    }
}

pub fn sensor(reading: &Reading, temperature: Temperature) -> String {
    sensor_value(reading.kind, reading.value(), temperature)
}

pub fn unavailable() -> String {
    "n/a".to_owned()
}

/// `YYYY-MM-DD HH:MM` in the system time zone from seconds since the Unix epoch.
pub fn date_time(epoch: u64) -> String {
    stamp(epoch, "%Y-%m-%d %H:%M", &TimeZone::system())
}

fn stamp(epoch: u64, pattern: &str, zone: &TimeZone) -> String {
    i64::try_from(epoch)
        .ok()
        .and_then(|seconds| Timestamp::from_second(seconds).ok())
        .map_or_else(String::new, |at| {
            at.to_zoned(zone.clone()).strftime(pattern).to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATE_TIME: &str = "%Y-%m-%d %H:%M";

    #[test]
    fn dates_come_out_of_epoch_seconds() {
        let utc = TimeZone::UTC;
        assert_eq!(stamp(0, DATE_TIME, &utc), "1970-01-01 00:00");
        assert_eq!(stamp(1_760_568_000, DATE_TIME, &utc), "2025-10-15 22:40");
        assert_eq!(stamp(951_782_400, DATE_TIME, &utc), "2000-02-29 00:00");
    }

    #[test]
    fn a_zone_moves_the_clock() {
        let eastern = TimeZone::fixed(jiff::tz::offset(-4));
        assert_eq!(
            stamp(1_760_568_000, DATE_TIME, &eastern),
            "2025-10-15 18:40"
        );
    }
}
