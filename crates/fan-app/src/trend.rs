//! Rolling ten-minute temperature and fan speed history shared by the GPUI
//! Metal trend charts and desktop widgets.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(600);
const SPACING: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: Instant,
    pub temperature: Option<f64>,
    pub fan_percent: Option<f64>,
}

/// Keeps one sample per spacing interval inside a rolling 10-minute window.
#[derive(Default)]
pub struct History {
    samples: VecDeque<Sample>,
}

impl History {
    pub fn record(&mut self, sample: Sample) -> bool {
        if self
            .samples
            .back()
            .is_some_and(|last| sample.at.saturating_duration_since(last.at) < SPACING)
        {
            return false;
        }
        self.samples.push_back(sample);
        while self
            .samples
            .front()
            .is_some_and(|first| sample.at.saturating_duration_since(first.at) > WINDOW)
        {
            self.samples.pop_front();
        }
        true
    }

    pub fn samples(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter()
    }

    #[allow(dead_code)]
    pub fn temperature_range(&self) -> Option<(f64, f64)> {
        let mut values = self.samples.iter().filter_map(|sample| sample.temperature);
        let first = values.next()?;
        Some(values.fold((first, first), |(lo, hi), value| {
            (lo.min(value), hi.max(value))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_spaced_and_bounded_to_ten_minutes() {
        let start = Instant::now();
        let mut history = History::default();
        let sample = |secs: u64, temperature: f64| Sample {
            at: start + Duration::from_secs(secs),
            temperature: Some(temperature),
            fan_percent: None,
        };
        assert!(history.record(sample(0, 50.)));
        assert!(!history.record(sample(2, 90.)));
        for secs in (5..=900).step_by(5) {
            history.record(sample(secs, 40. + secs as f64 / 100.));
        }
        assert!(history.samples().count() <= 121);
        let first = history.samples().next().unwrap().at;
        assert!(first >= start + Duration::from_secs(300));
        let (lo, hi) = history.temperature_range().unwrap();
        assert!(lo >= 43. && hi <= 49.);
    }
}
