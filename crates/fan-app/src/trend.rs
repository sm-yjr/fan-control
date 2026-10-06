//! Ten-minute temperature and fan trend for the overview page.
use objc2::{define_class, msg_send, rc::Retained, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSAccessibility, NSBezierPath, NSColor, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSString};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(600);
const SPACING: Duration = Duration::from_secs(5);
/// Temperature axis; readings outside are clamped to the edge.
const COOL: f64 = 30.;
const HOT: f64 = 100.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: Instant,
    pub temperature: Option<f64>,
    pub fan_percent: Option<f64>,
}

/// Keeps one sample per spacing interval inside a rolling window.
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
    pub fn temperature_range(&self) -> Option<(f64, f64)> {
        let mut values = self.samples.iter().filter_map(|sample| sample.temperature);
        let first = values.next()?;
        Some(values.fold((first, first), |(lo, hi), value| {
            (lo.min(value), hi.max(value))
        }))
    }
}

/// Horizontal position, then normalized temperature and fan speed.
type Plotted = (f64, Option<f64>, Option<f64>);

pub(crate) struct Ivars {
    points: RefCell<Vec<Plotted>>,
}

define_class!(
    #[unsafe(super=NSView)]
    #[thread_kind=MainThreadOnly]
    #[ivars=Ivars]
    pub(crate) struct TrendView;
    impl TrendView {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let inset = 4.;
            let w = (bounds.size.width - inset * 2.).max(1.);
            let h = (bounds.size.height - inset * 2.).max(1.);
            NSColor::separatorColor().setStroke();
            let grid = NSBezierPath::bezierPath();
            grid.setLineWidth(0.5);
            for i in 0..=2 {
                let y = inset + h * i as f64 / 2.;
                grid.moveToPoint(NSPoint::new(inset, y));
                grid.lineToPoint(NSPoint::new(inset + w, y));
            }
            grid.stroke();
            let points = self.ivars().points.borrow();
            let line = |value: fn(&Plotted) -> Option<f64>, dashed: bool| {
                let path = NSBezierPath::bezierPath();
                path.setLineWidth(2.);
                if dashed {
                    let pattern = [4.0_f64, 3.0];
                    unsafe { path.setLineDash_count_phase(pattern.as_ptr(), 2, 0.) };
                }
                let mut open = false;
                for point in points.iter() {
                    match value(point) {
                        Some(y) => {
                            let point = NSPoint::new(inset + w * point.0, inset + h * y.clamp(0., 1.));
                            if open { path.lineToPoint(point); } else { path.moveToPoint(point); open = true; }
                        }
                        None => open = false,
                    }
                }
                path
            };
            NSColor::systemOrangeColor().setStroke();
            line(|p| p.1, false).stroke();
            NSColor::systemBlueColor().setStroke();
            line(|p| p.2, true).stroke();
        }
    }
);

impl TrendView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            points: RefCell::new(Vec::new()),
        });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
    /// Normalizes the history onto 0…1 axes and updates the spoken summary.
    pub fn set_history(&self, history: &History, now: Instant) {
        let points: Vec<_> = history
            .samples()
            .map(|sample| {
                let age = now.saturating_duration_since(sample.at).as_secs_f64();
                (
                    (1. - age / WINDOW.as_secs_f64()).clamp(0., 1.),
                    sample
                        .temperature
                        .map(|value| (value - COOL) / (HOT - COOL)),
                    sample.fan_percent.map(|value| value / 100.),
                )
            })
            .collect();
        let summary = match history.temperature_range() {
            Some((lo, hi)) => format!("最近 10 分钟芯片温度 {lo:.0}–{hi:.0}°C"),
            None => "最近 10 分钟暂无温度数据".to_string(),
        };
        self.setAccessibilityLabel(Some(&NSString::from_str(&crate::i18n::translate(&summary))));
        if *self.ivars().points.borrow() != points {
            self.ivars().points.replace(points);
            self.setNeedsDisplay(true);
        }
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
