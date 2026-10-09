//! Compact dashboard. Labels are native accessibility elements; arcs show only tachometer data.
use crate::{
    popover::{stack, styled_label, tokens},
    presenter::{FanCard, Presentation},
    trend::History,
    ui::{raw_text, text},
};
use objc2::{define_class, msg_send, rc::Retained, DefinedClass, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSCopying, NSPoint, NSRect, NSSize};
use std::{cell::RefCell, time::Instant};

#[derive(Clone, Debug, PartialEq)]
struct Plot {
    low: f64,
    high: f64,
    points: Vec<(f64, Option<f64>)>,
}
fn plot(history: &History, now: Instant) -> Plot {
    let samples: Vec<_> = history
        .samples()
        .filter(|s| now.saturating_duration_since(s.at).as_secs_f64() <= 600.)
        .collect();
    let values: Vec<_> = samples
        .iter()
        .filter_map(|s| s.temperature.filter(|v| v.is_finite()))
        .collect();
    let mut low = values
        .iter()
        .copied()
        .reduce(f64::min)
        .map(|v| (v / 5.).floor() * 5.)
        .unwrap_or(0.);
    let mut high = values
        .iter()
        .copied()
        .reduce(f64::max)
        .map(|v| (v / 5.).ceil() * 5.)
        .unwrap_or(10.);
    if high - low < 10. {
        let middle = (high + low) / 2.;
        low = (middle / 5.).floor() * 5. - 5.;
        high = low + 10.;
    }
    let mut points = Vec::new();
    let mut previous = None;
    for s in samples {
        let x = (1. - now.saturating_duration_since(s.at).as_secs_f64() / 600.).clamp(0., 1.);
        if previous.is_some_and(|at| s.at.saturating_duration_since(at).as_secs_f64() > 15.) {
            points.push((x, None));
        }
        points.push((
            x,
            s.temperature
                .filter(|v| v.is_finite())
                .map(|v| (v - low) / (high - low)),
        ));
        previous = Some(s.at);
    }
    Plot { low, high, points }
}
pub(crate) struct PlotIvars {
    plot: RefCell<Plot>,
}
define_class!(
    #[unsafe(super=NSView)] #[thread_kind=MainThreadOnly] #[ivars=PlotIvars]
    pub(crate) struct TemperaturePlot;
    impl TemperaturePlot {
        #[unsafe(method(drawRect:))]
        fn draw(&self,_dirty:NSRect) {
            let b=self.bounds(); let w=(b.size.width-8.).max(1.); let h=(b.size.height-8.).max(1.);
            NSColor::separatorColor().setStroke(); let axes=NSBezierPath::bezierPath(); axes.setLineWidth(0.5);
            axes.moveToPoint(NSPoint::new(4.,4.+h)); axes.lineToPoint(NSPoint::new(4.,4.)); axes.lineToPoint(NSPoint::new(4.+w,4.)); axes.stroke();
            let data=self.ivars().plot.borrow();
            let mut segment=Vec::new();
            let paint=|segment:&[(f64,f64)]| {
                if segment.is_empty(){return;}
                let path=NSBezierPath::bezierPath(); let first=NSPoint::new(4.+w*segment[0].0,4.+h*segment[0].1);
                path.moveToPoint(first);
                for &(x,y) in &segment[1..] { path.lineToPoint(NSPoint::new(4.+w*x,4.+h*y)); }
                let fill=path.copy(); let last=segment.last().unwrap(); fill.lineToPoint(NSPoint::new(4.+w*last.0,4.)); fill.lineToPoint(NSPoint::new(first.x,4.)); fill.closePath();
                NSColor::controlAccentColor().colorWithAlphaComponent(0.10).setFill(); fill.fill();
                NSColor::controlAccentColor().setStroke(); path.setLineWidth(1.8); path.setLineJoinStyle(NSLineJoinStyle::Round); path.stroke();
            };
            for &(x,y) in &data.points { if let Some(y)=y {segment.push((x,y.clamp(0.,1.)));} else {paint(&segment);segment.clear();} }
            paint(&segment);
            if let Some(&(x,Some(y)))=data.points.last() { NSColor::controlAccentColor().setFill(); NSBezierPath::bezierPathWithOvalInRect(NSRect::new(NSPoint::new(4.+w*x-2.8,4.+h*y.clamp(0.,1.)-2.8),NSSize::new(5.6,5.6))).fill(); }
        }
    }
);
impl TemperaturePlot {
    fn new(mtm: MainThreadMarker, width: f64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PlotIvars {
            plot: RefCell::new(Plot {
                low: 0.,
                high: 10.,
                points: Vec::new(),
            }),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        this.heightAnchor()
            .constraintEqualToConstant(60.)
            .setActive(true);
        this
    }
}
struct FanGauge {
    id: u8,
    root: Retained<NSStackView>,
    arc: Retained<crate::gauge::Arc>,
    name: Retained<NSTextField>,
    rpm: Retained<NSTextField>,
    detail: Retained<NSTextField>,
}
impl FanGauge {
    fn new(mtm: MainThreadMarker, id: u8, width: f64) -> Self {
        let root = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 5.);
        root.setAlignment(NSLayoutAttribute::CenterX);
        root.widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        let name = styled_label(mtm, "", tokens::BODY, true, false, Some(width));
        name.setAlignment(NSTextAlignment::Center);
        root.addArrangedSubview(&name);
        let arc = crate::gauge::Arc::new(mtm, width, tokens::GAUGE_TRACK);
        let center = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 0.);
        center.setAlignment(NSLayoutAttribute::CenterX);
        if let Some(icon) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &raw_text("fanblades"),
            None,
        ) {
            let image = NSImageView::imageViewWithImage(&icon, mtm);
            image
                .heightAnchor()
                .constraintEqualToConstant(20.)
                .setActive(true);
            image
                .widthAnchor()
                .constraintEqualToConstant(20.)
                .setActive(true);
            center.addArrangedSubview(&image);
        }
        let rpm = styled_label(
            mtm,
            "—",
            if width < 160. {
                tokens::RPM_DIGITS_COMPACT
            } else {
                tokens::RPM_DIGITS
            },
            true,
            false,
            None,
        );
        rpm.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            if width < 160. {
                tokens::RPM_DIGITS_COMPACT
            } else {
                tokens::RPM_DIGITS
            },
            unsafe { NSFontWeightSemibold },
        )));
        center.addArrangedSubview(&rpm);
        center.addArrangedSubview(&styled_label(
            mtm,
            "RPM",
            tokens::CAPTION,
            false,
            true,
            None,
        ));
        center.setTranslatesAutoresizingMaskIntoConstraints(false);
        arc.addSubview(&center);
        center
            .centerXAnchor()
            .constraintEqualToAnchor(&arc.centerXAnchor())
            .setActive(true);
        center
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&arc.bottomAnchor(), -4.)
            .setActive(true);
        root.addArrangedSubview(&arc);
        let detail = styled_label(mtm, "", tokens::DASHBOARD_CAPTION, false, true, Some(width));
        detail.setAlignment(NSTextAlignment::Center);
        root.addArrangedSubview(&detail);
        Self {
            id,
            root,
            arc,
            name,
            rpm,
            detail,
        }
    }
    fn refresh(&self, card: &FanCard) {
        self.name.setStringValue(&raw_text(&card.name));
        self.rpm.setStringValue(&raw_text(
            &card
                .measured_rpm
                .map(|rpm| crate::presenter::grouped(rpm.round() as u64))
                .unwrap_or_else(|| "—".into()),
        ));
        self.arc.set(
            card.percent.map(|v| v / 100.),
            &NSColor::controlAccentColor(),
        );
        self.detail
            .setStringValue(&text(&if card.measured_rpm.is_none() {
                "转速未知".into()
            } else if card.range.is_none() {
                "转速范围未知".into()
            } else {
                card.detail.clone()
            }));
        self.arc.setAccessibilityLabel(Some(&raw_text(&format!(
            "{} · {} · {}",
            card.name,
            crate::i18n::translate(&card.speed),
            crate::i18n::translate(&card.detail)
        ))));
        self.arc.setToolTip(Some(&raw_text(
            &card
                .range
                .map(|(lo, hi)| format!("{}–{} RPM", lo, hi))
                .unwrap_or_else(|| crate::i18n::translate("转速范围未知")),
        )));
    }
}
pub(crate) struct Dashboard {
    pub view: Retained<NSStackView>,
    temperature: Retained<NSTextField>,
    caption: Retained<NSTextField>,
    chart: Retained<TemperaturePlot>,
    high: Retained<NSTextField>,
    low: Retained<NSTextField>,
    endpoint: Retained<NSTextField>,
    fan_rows: Retained<NSStackView>,
    gauges: Vec<FanGauge>,
    width: f64,
}
impl Dashboard {
    pub fn new(mtm: MainThreadMarker, width: f64) -> Self {
        let view = stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Vertical,
            tokens::SECTION_GAP,
        );
        view.widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        let hero = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 16.);
        hero.setAlignment(NSLayoutAttribute::CenterY);
        let left = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 4.);
        left.addArrangedSubview(&styled_label(
            mtm,
            "芯片温度",
            tokens::HEADLINE,
            true,
            false,
            None,
        ));
        let temperature = styled_label(
            mtm,
            "—",
            if width < 330. {
                tokens::DASHBOARD_HERO_COMPACT
            } else {
                tokens::DASHBOARD_HERO
            },
            true,
            false,
            None,
        );
        temperature.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            if width < 330. {
                tokens::DASHBOARD_HERO_COMPACT
            } else {
                tokens::DASHBOARD_HERO
            },
            unsafe { NSFontWeightSemibold },
        )));
        let reading = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 2.);
        reading.setAlignment(NSLayoutAttribute::FirstBaseline);
        reading.addArrangedSubview(&temperature);
        reading.addArrangedSubview(&styled_label(mtm, "°C", 24., false, false, None));
        left.addArrangedSubview(&reading);
        let caption = styled_label(mtm, "正在检测…", tokens::CAPTION, false, true, Some(128.));
        left.addArrangedSubview(&caption);
        left.widthAnchor()
            .constraintEqualToConstant(128.)
            .setActive(true);
        hero.addArrangedSubview(&left);
        let chart_width = width - 144.;
        let right = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 4.);
        right.addArrangedSubview(&styled_label(
            mtm,
            "温度趋势",
            tokens::CAPTION,
            false,
            true,
            None,
        ));
        let row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 4.);
        let axis = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 30.);
        let high = styled_label(mtm, "—", tokens::PLOT_LABEL, false, true, Some(25.));
        let low = styled_label(mtm, "—", tokens::PLOT_LABEL, false, true, Some(25.));
        axis.addArrangedSubview(&high);
        axis.addArrangedSubview(&low);
        row.addArrangedSubview(&axis);
        let chart = TemperaturePlot::new(mtm, chart_width - 29.);
        row.addArrangedSubview(&chart);
        right.addArrangedSubview(&row);
        let timeline = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 2.);
        timeline
            .widthAnchor()
            .constraintEqualToConstant(chart_width)
            .setActive(true);
        timeline.addArrangedSubview(&styled_label(mtm, "10 分钟前", 10., false, true, None));
        timeline.addArrangedSubview(&spacer(mtm));
        timeline.addArrangedSubview(&styled_label(mtm, "现在", 10., false, true, None));
        right.addArrangedSubview(&timeline);
        let endpoint = styled_label(mtm, "", 10., true, true, Some(chart_width));
        endpoint.setAlignment(NSTextAlignment::Right);
        right.addArrangedSubview(&endpoint);
        hero.addArrangedSubview(&right);
        view.addArrangedSubview(&hero);
        view.addArrangedSubview(&separator(mtm, width));
        view.addArrangedSubview(&styled_label(
            mtm,
            "风扇转速",
            tokens::HEADLINE,
            true,
            false,
            None,
        ));
        let fan_rows = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 10.);
        fan_rows
            .widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        fan_rows.setAlignment(NSLayoutAttribute::CenterX);
        view.addArrangedSubview(&fan_rows);
        view.addArrangedSubview(&separator(mtm, width));
        Self {
            view,
            temperature,
            caption,
            chart,
            high,
            low,
            endpoint,
            fan_rows,
            gauges: Vec::new(),
            width,
        }
    }
    pub fn refresh(
        &mut self,
        presentation: &Presentation,
        cards: &[FanCard],
        history: &History,
        now: Instant,
    ) {
        let mtm = self.view.mtm();
        self.temperature.setStringValue(&raw_text(
            &presentation
                .temperature
                .map(|v| format!("{v:.0}"))
                .unwrap_or_else(|| "—".into()),
        ));
        self.temperature.setAccessibilityLabel(Some(&raw_text(
            &presentation
                .temperature
                .map(|v| format!("{v:.0}°C"))
                .unwrap_or_else(|| crate::i18n::translate("暂无温度数据")),
        )));
        self.caption
            .setStringValue(&text(if presentation.temperature.is_some() {
                presentation.trend
            } else {
                "暂无温度数据"
            }));
        let mut p = plot(history, now);
        if presentation.temperature.is_none() {
            p.points.push((1., None));
        }
        let has_data = p.points.iter().any(|(_, y)| y.is_some());
        self.high.setStringValue(&raw_text(&if has_data {
            format!("{:.0}°", p.high)
        } else {
            "—".into()
        }));
        self.low.setStringValue(&raw_text(&if has_data {
            format!("{:.0}°", p.low)
        } else {
            "—".into()
        }));
        self.endpoint.setStringValue(&raw_text(
            &presentation
                .temperature
                .map(|v| format!("{v:.0}°C"))
                .unwrap_or_else(|| crate::i18n::translate("暂无温度数据")),
        ));
        self.chart.setAccessibilityLabel(Some(&text(
            &history
                .temperature_range()
                .map(|(lo, hi)| format!("最近 10 分钟芯片温度 {lo:.0}–{hi:.0}°C"))
                .unwrap_or_else(|| "最近 10 分钟暂无温度数据".into()),
        )));
        if *self.chart.ivars().plot.borrow() != p {
            self.chart.ivars().plot.replace(p);
            self.chart.setNeedsDisplay(true);
        }
        if self.gauges.iter().map(|g| g.id).collect::<Vec<_>>()
            != cards.iter().map(|c| c.id).collect::<Vec<_>>()
        {
            for child in self.fan_rows.arrangedSubviews().to_vec() {
                self.fan_rows.removeArrangedSubview(&child);
                child.removeFromSuperview();
            }
            self.gauges.clear();
            if cards.is_empty() {
                self.fan_rows.addArrangedSubview(&styled_label(
                    mtm,
                    "没有可显示的风扇",
                    tokens::CAPTION,
                    false,
                    true,
                    None,
                ));
            }
            let gauge_width = (self.width - 18.) / 2.;
            for pair in cards.chunks(2) {
                let row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 18.);
                row.setAlignment(NSLayoutAttribute::Top);
                for card in pair {
                    let g = FanGauge::new(mtm, card.id, gauge_width);
                    row.addArrangedSubview(&g.root);
                    self.gauges.push(g);
                }
                self.fan_rows.addArrangedSubview(&row);
            }
        } else if cards.is_empty() && self.fan_rows.arrangedSubviews().is_empty() {
            self.fan_rows.addArrangedSubview(&styled_label(
                mtm,
                "没有可显示的风扇",
                tokens::CAPTION,
                false,
                true,
                None,
            ));
        }
        for (g, c) in self.gauges.iter().zip(cards) {
            g.refresh(c);
        }
    }
    pub fn verify_layout(&self, cards: &[FanCard]) {
        assert_eq!(self.gauges.len(), cards.len());
        for (g, c) in self.gauges.iter().zip(cards) {
            assert_eq!(g.id, c.id);
            assert_eq!(g.name.stringValue().to_string(), c.name);
            assert!(g.arc.frame().size.width > 0.);
            assert!(g.rpm.frame().size.height > 0.);
            assert!(g.root.frame().size.width <= self.width + 0.5);
        }
        if self.gauges.len() == 1 {
            let frame = self.gauges[0]
                .root
                .convertRect_toView(self.gauges[0].root.bounds(), Some(&self.fan_rows));
            assert!((frame.origin.x + frame.size.width / 2. - self.width / 2.).abs() < 2.);
        }
    }
}
pub(crate) fn spacer(mtm: MainThreadMarker) -> Retained<NSView> {
    let v = NSView::new(mtm);
    v.setContentHuggingPriority_forOrientation(1., NSLayoutConstraintOrientation::Horizontal);
    v
}
pub(crate) fn separator(mtm: MainThreadMarker, width: f64) -> Retained<NSBox> {
    let v = NSBox::new(mtm);
    v.setBoxType(NSBoxType::Separator);
    v.widthAnchor()
        .constraintEqualToConstant(width)
        .setActive(true);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trend::Sample;
    use std::time::Duration;
    #[test]
    fn chart_uses_real_range_and_breaks_missing_or_old_samples() {
        let now = Instant::now();
        let mut h = History::default();
        for (age, t) in [
            (610, Some(99.)),
            (600, Some(55.)),
            (595, None),
            (590, Some(56.)),
            (550, Some(58.)),
        ] {
            h.record(Sample {
                at: now - Duration::from_secs(age),
                temperature: t,
                fan_percent: None,
            });
        }
        let p = plot(&h, now);
        assert!(p.low <= 55. && p.high >= 58. && p.high - p.low >= 10.);
        assert_eq!(p.points.iter().filter(|(_, t)| t.is_none()).count(), 2);
        assert_eq!(p.points.iter().filter(|(_, t)| t.is_some()).count(), 3);
        assert!(p.points.iter().all(|(x, _)| *x >= 0. && *x <= 1.));
    }
}
