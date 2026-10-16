use std::time::Duration;

/// Binary units with one decimal, such as `15.4 GiB`.
#[must_use]
pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let unit = (value.max(1).ilog2() / 10).min(4);
    let divisor = 1_u64 << (unit * 10);
    let suffix = UNITS[unit as usize];
    if unit == 0 {
        format!("{value} {suffix}")
    } else {
        format!(
            "{}.{} {suffix}",
            value / divisor,
            value % divisor * 10 / divisor
        )
    }
}

#[must_use]
pub fn rate(bytes_per_second: u64) -> String {
    format!("{}/s", bytes(bytes_per_second))
}

/// The bracketed product name of a PCI database entry: `Navi 23 [Radeon RX 6600]` reads `Radeon RX 6600`.
#[must_use]
pub fn marketing_name(model: &str) -> &str {
    model
        .split_once('[')
        .and_then(|(_, rest)| rest.strip_suffix(']'))
        .unwrap_or(model)
}

/// Link speeds are quoted in bits: `47.5` megabytes per second is `380 Mbit/s`.
#[must_use]
pub fn megabits(bytes_per_second: u64) -> String {
    format!("{} Mbit/s", bytes_per_second * 8 / 1_000_000)
}

/// Kilohertz as gigahertz, such as `4.20 GHz`.
#[must_use]
pub fn frequency(khz: u64) -> String {
    format!("{}.{:02} GHz", khz / 1_000_000, khz % 1_000_000 / 10_000)
}

/// Whole seconds as `3d 4:05:06`, dropping the days when there are none.
#[must_use]
pub fn uptime(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let clock = format!(
        "{}:{:02}:{:02}",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    );
    match seconds / 86_400 {
        0 => clock,
        days => format!("{days}d {clock}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_pick_the_largest_whole_unit() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.0 KiB");
        assert_eq!(bytes(1536 * 1024), "1.5 MiB");
        assert_eq!(bytes(16 * 1024_u64.pow(3)), "16.0 GiB");
    }

    #[test]
    fn bytes_stop_at_tebibytes() {
        assert_eq!(bytes(2048 * 1024_u64.pow(4)), "2048.0 TiB");
    }

    #[test]
    fn rates_carry_the_per_second_suffix() {
        assert_eq!(rate(2048), "2.0 KiB/s");
    }

    #[test]
    fn link_speeds_are_in_bits() {
        assert_eq!(megabits(47_500_000), "380 Mbit/s");
    }

    #[test]
    fn frequency_keeps_two_decimals() {
        assert_eq!(frequency(4_200_000), "4.20 GHz");
        assert_eq!(frequency(400_000), "0.40 GHz");
    }

    #[test]
    fn uptime_shows_days_only_when_present() {
        assert_eq!(uptime(Duration::from_secs(3661)), "1:01:01");
        assert_eq!(uptime(Duration::from_secs(90_061)), "1d 1:01:01");
    }

    #[test]
    fn marketing_names_come_from_the_brackets() {
        assert_eq!(
            marketing_name("Navi 23 [Radeon RX 6600M]"),
            "Radeon RX 6600M"
        );
        assert_eq!(
            marketing_name("Ethernet Controller I226-V"),
            "Ethernet Controller I226-V"
        );
    }
}
