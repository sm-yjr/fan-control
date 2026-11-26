//! Ten-minute temperature and fan trend chart with gradient area fill,
//! shared visual grammar with the compact dashboard and desktop widgets.
use objc2::{define_class, msg_send, rc::Retained, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility, NSBezierPath, NSColor, NSGradient, NSLineCapStyle, NSLineJoinStyle, NSView,
};
use objc2_foundation::{MainThreadMarker, NSCopying, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(600);
const SPACING: Duration = Duration::from_secs(5);
const GAP_BREAK_SECS: f64 = 15.;
/// Default floor/ceiling bounds when temperature readings exist.
const MIN_SPAN: f64 = 10.;

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

#[derive(Clone, Debug, Default, PartialEq)]
struct ChartData {
    low: f64,
    high: f64,
    points: Vec<Plotted>,
}

pub(crate) struct Ivars {
    data: RefCell<ChartData>,
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
            let inset_x = 6.;
            let inset_y = 6.;
            let w = (bounds.size.width - inset_x * 2.).max(1.);
            let h = (bounds.size.height - inset_y * 2.).max(1.);
            NSColor::separatorColor().colorWithAlphaComponent(0.55).setStroke();
            let grid = NSBezierPath::bezierPath();
            grid.setLineWidth(0.5);
            for i in 0..=2 {
                let y = inset_y + h * i as f64 / 2.;
                grid.moveToPoint(NSPoint::new(inset_x, y));
                grid.lineToPoint(NSPoint::new(inset_x + w, y));
            }
            grid.stroke();

            let data = self.ivars().data.borrow();
            let paint_temp = |segment: &[(f64, f64)]| {
                if segment.is_empty() {
                    return;
                }
                let path = NSBezierPath::bezierPath();
                let first = NSPoint::new(inset_x + w * segment[0].0, inset_y + h * segment[0].1);
                path.moveToPoint(first);
                for &(x, y) in &segment[1..] {
                    path.lineToPoint(NSPoint::new(inset_x + w * x, inset_y + h * y));
                }
                if segment.len() >= 2 {
                    let fill = path.copy();
                    let last = segment.last().unwrap();
                    fill.lineToPoint(NSPoint::new(inset_x + w * last.0, inset_y));
                    fill.lineToPoint(NSPoint::new(first.x, inset_y));
                    fill.closePath();
                    let top = NSColor::controlAccentColor().colorWithAlphaComponent(0.22);
                    let bottom = NSColor::controlAccentColor().colorWithAlphaComponent(0.02);
                    if let Some(gradient) = NSGradient::initWithStartingColor_endingColor(
                        NSGradient::alloc(),
                        &top,
                        &bottom,
                    ) {
                        gradient.drawInBezierPath_angle(&fill, -90.0);
                    } else {
                        NSColor::controlAccentColor()
                            .colorWithAlphaComponent(0.10)
                            .setFill();
                        fill.fill();
                    }
                }
                NSColor::controlAccentColor().setStroke();
                path.setLineWidth(2.0);
                path.setLineCapStyle(NSLineCapStyle::Round);
                path.setLineJoinStyle(NSLineJoinStyle::Round);
                path.stroke();
            };

            let mut segment = Vec::new();
            for &(x, temp, _) in &data.points {
                if let Some(y) = temp {
                    segment.push((x, y.clamp(0., 1.)));
                } else {
                    paint_temp(&segment);
                    segment.clear();
                }
            }
            paint_temp(&segment);

            // Secondary dashed fan percentage series.
            let fan_path = NSBezierPath::bezierPath();
            fan_path.setLineWidth(1.6);
            fan_path.setLineCapStyle(NSLineCapStyle::Round);
            fan_path.setLineJoinStyle(NSLineJoinStyle::Round);
            let pattern = [4.0_f64, 3.0];
            unsafe { fan_path.setLineDash_count_phase(pattern.as_ptr(), 2, 0.) };
            let mut open = false;
            for &(x, _, fan) in &data.points {
                match fan {
                    Some(y) => {
                        let p = NSPoint::new(inset_x + w * x, inset_y + h * y.clamp(0., 1.));
                        if open {
                            fan_path.lineToPoint(p);
                        } else {
                            fan_path.moveToPoint(p);
                            open = true;
                        }
                    }
                    None => open = false,
                }
            }
            NSColor::systemTealColor().setStroke();
            fan_path.stroke();

            if let Some(&(x, Some(y), _)) = data.points.last() {
                let cx = inset_x + w * x;
                let cy = inset_y + h * y.clamp(0., 1.);
                NSColor::controlAccentColor()
                    .colorWithAlphaComponent(0.22)
                    .setFill();
                NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                    NSPoint::new(cx - 5.5, cy - 5.5),
                    NSSize::new(11., 11.),
                ))
                .fill();
                NSColor::controlAccentColor().setFill();
                NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                    NSPoint::new(cx - 3.0, cy - 3.0),
                    NSSize::new(6.0, 6.0),
                ))
                .fill();
            }
        }
    }
);

impl TrendView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            data: RefCell::new(ChartData::default()),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }
    /// Normalizes the history onto adaptive temperature bounds + 0…100% fan axis
    /// and updates the spoken summary. Returns `(low_c, high_c)` when data exists.
    pub fn set_history(&self, history: &History, now: Instant) -> Option<(f64, f64)> {
        let samples: Vec<_> = history
            .samples()
            .filter(|s| now.saturating_duration_since(s.at) <= WINDOW)
            .collect();
        let temps: Vec<_> = samples
            .iter()
            .filter_map(|s| s.temperature.filter(|v| v.is_finite()))
            .collect();
        let mut low = temps
            .iter()
            .copied()
            .reduce(f64::min)
            .map(|v| (v / 5.).floor() * 5.)
            .unwrap_or(30.);
        let mut high = temps
            .iter()
            .copied()
            .reduce(f64::max)
            .map(|v| (v / 5.).ceil() * 5.)
            .unwrap_or(100.);
        if high - low < MIN_SPAN {
            let middle = (high + low) / 2.;
            low = (middle / 5.).floor() * 5. - 5.;
            high = low + MIN_SPAN;
        }
        let mut points = Vec::new();
        let mut previous = None;
        for sample in samples {
            let age = now.saturating_duration_since(sample.at).as_secs_f64();
            let x = (1. - age / WINDOW.as_secs_f64()).clamp(0., 1.);
            if previous.is_some_and(|at| {
                sample.at.saturating_duration_since(at).as_secs_f64() > GAP_BREAK_SECS
            }) {
                points.push((x, None, None));
            }
            points.push((
                x,
                sample
                    .temperature
                    .filter(|v| v.is_finite())
                    .map(|value| (value - low) / (high - low)),
                sample
                    .fan_percent
                    .filter(|v| v.is_finite())
                    .map(|value| value / 100.),
            ));
            previous = Some(sample.at);
        }
        let summary = match history.temperature_range() {
            Some((lo, hi)) => format!("最近 10 分钟芯片温度 {lo:.0}–{hi:.0}°C"),
            None => "最近 10 分钟暂无温度数据".to_string(),
        };
        self.setAccessibilityLabel(Some(&NSString::from_str(&crate::i18n::translate(&summary))));
        let next = ChartData { low, high, points };
        if *self.ivars().data.borrow() != next {
            self.ivars().data.replace(next);
            self.setNeedsDisplay(true);
        }
        (!temps.is_empty()).then_some((low, high))
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
