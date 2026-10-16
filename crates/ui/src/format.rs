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

/// `YYYY-MM-DD HH:MM` in UTC from seconds since the Unix epoch.
pub fn date_time(epoch: u64) -> String {
    let days = i64::try_from(epoch / 86_400).unwrap_or(0);
    let minutes = epoch % 86_400 / 60;
    // Civil-from-days, after Howard Hinnant: shift to a March-based year so leap days fall last.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        minutes / 60,
        minutes % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_come_out_of_epoch_seconds() {
        assert_eq!(date_time(0), "1970-01-01 00:00");
        assert_eq!(date_time(1_760_568_000), "2025-10-15 22:40");
        assert_eq!(date_time(951_782_400), "2000-02-29 00:00");
    }
}
