use std::collections::VecDeque;

use peepers_core::telemetry::Vitals;

/// Seconds of history kept, at one sample per second.
pub const CAPACITY: usize = 120;

#[derive(Debug, Default)]
pub struct History(VecDeque<Vitals>);

impl History {
    pub fn push(&mut self, vitals: Vitals) {
        if self.0.len() == CAPACITY {
            self.0.pop_front();
        }
        self.0.push_back(vitals);
    }

    pub fn latest(&self) -> Option<&Vitals> {
        self.0.back()
    }

    pub fn series(&self, metric: impl Fn(&Vitals) -> f32) -> Vec<f32> {
        self.0.iter().map(metric).collect()
    }

    pub fn optional_series(&self, metric: impl Fn(&Vitals) -> Option<f32>) -> Vec<Option<f32>> {
        self.0.iter().map(metric).collect()
    }
}

#[cfg(test)]
mod tests {
    use peepers_core::telemetry::Sampler;

    use super::*;

    fn vitals() -> Vitals {
        Sampler::default().sample().unwrap().vitals
    }

    #[test]
    fn the_oldest_sample_makes_room_at_capacity() {
        let mut history = History::default();
        let sample = vitals();
        for _ in 0..=CAPACITY {
            history.push(sample.clone());
        }
        assert_eq!(history.series(|_| 1.0).len(), CAPACITY);
    }

    #[test]
    fn series_follow_arrival_order() {
        let mut history = History::default();
        for second in [10, 20, 30] {
            let mut sample = vitals();
            sample.uptime = std::time::Duration::from_secs(second);
            history.push(sample);
        }
        assert_eq!(
            history.series(|v| v.uptime.as_secs_f32()),
            [10.0, 20.0, 30.0]
        );
        assert_eq!(history.latest().map(|v| v.uptime.as_secs()), Some(30));
    }
}
