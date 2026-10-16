use std::time::Duration;

/// `part` over `whole`, or zero when `whole` is zero.
#[expect(
    clippy::cast_precision_loss,
    reason = "counter deltas between samples stay far below 2^24"
)]
pub fn ratio(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        part as f32 / whole as f32
    }
}

/// `part` as a percentage of `whole`, or zero when nothing has elapsed.
pub fn percent(part: u64, whole: u64) -> f32 {
    100.0 * ratio(part, whole)
}

pub fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// `amount` spread over `elapsed`, or zero when no time has passed. Microseconds, so a read that
/// finishes within a millisecond still has a rate.
pub fn per_second(amount: u64, elapsed: Duration) -> u64 {
    (u128::from(amount) * 1_000_000)
        .checked_div(elapsed.as_micros())
        .map_or(0, |rate| u64::try_from(rate).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_whole_reads_zero() {
        assert!(percent(5, 0).abs() < f32::EPSILON);
    }

    #[test]
    fn half_reads_fifty_percent() {
        assert!((percent(1, 2) - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rates_scale_to_one_second() {
        assert_eq!(per_second(500, Duration::from_millis(500)), 1000);
        assert_eq!(per_second(500, Duration::ZERO), 0);
    }

    #[test]
    fn a_rate_over_less_than_a_millisecond_is_not_zero() {
        assert_eq!(
            per_second(8 << 20, Duration::from_micros(800)),
            10_485_760_000
        );
    }

    #[test]
    fn an_enormous_rate_saturates() {
        assert_eq!(per_second(u64::MAX, Duration::from_micros(1)), u64::MAX);
    }

    #[test]
    fn ratio_is_the_plain_quotient() {
        assert!((ratio(3, 4) - 0.75).abs() < f32::EPSILON);
    }
}
