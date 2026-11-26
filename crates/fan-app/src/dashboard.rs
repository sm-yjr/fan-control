//! Compact dashboard shared by the menu bar panel. Labels are native
//! accessibility elements; arcs show only verified tachometer data.
use crate::{
    components::{flex_spacer, surface_box, vstack},
    popover::{stack, styled_label, tokens},
    presenter::{FanCard, Presentation},
    theme::{self, spacing, typography, SurfaceStyle},
    trend::History,
    ui::{raw_text, text},
};
use objc2::{define_class, msg_send, rc::Retained, AnyThread, DefinedClass, MainThreadOnly};
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
            NSColor::separatorColor().colorWithAlphaComponent(0.55).setStroke();
            let axes=NSBezierPath::bezierPath(); axes.setLineWidth(0.5);
            axes.moveToPoint(NSPoint::new(4.,4.+h)); axes.lineToPoint(NSPoint::new(4.,4.)); axes.lineToPoint(NSPoint::new(4.+w,4.));
            axes.moveToPoint(NSPoint::new(4.,4.+h*0.5)); axes.lineToPoint(NSPoint::new(4.+w,4.+h*0.5));
            axes.stroke();
            let data=self.ivars().plot.borrow();
            let mut segment=Vec::new();
            let paint=|segment:&[(f64,f64)]| {
                if segment.is_empty(){return;}
                let path=NSBezierPath::bezierPath(); let first=NSPoint::new(4.+w*segment[0].0,4.+h*segment[0].1);
                path.moveToPoint(first);
                for &(x,y) in &segment[1..] { path.lineToPoint(NSPoint::new(4.+w*x,4.+h*y)); }
                if segment.len() >= 2 {
                    let fill=path.copy(); let last=segment.last().unwrap();
                    fill.lineToPoint(NSPoint::new(4.+w*last.0,4.));
                    fill.lineToPoint(NSPoint::new(first.x,4.));
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
                        NSColor::controlAccentColor().colorWithAlphaComponent(0.10).setFill();
                        fill.fill();
                    }
                }
                NSColor::controlAccentColor().setStroke();
                path.setLineWidth(1.8);
                path.setLineCapStyle(NSLineCapStyle::Round);
                path.setLineJoinStyle(NSLineJoinStyle::Round);
                path.stroke();
            };
            for &(x,y) in &data.points { if let Some(y)=y {segment.push((x,y.clamp(0.,1.)));} else {paint(&segment);segment.clear();} }
            paint(&segment);
            if let Some(&(x,Some(y)))=data.points.last() {
                let cx = 4. + w * x;
                let cy = 4. + h * y.clamp(0., 1.);
                NSColor::controlAccentColor().colorWithAlphaComponent(0.24).setFill();
                NSBezierPath::bezierPathWithOvalInRect(NSRect::new(NSPoint::new(cx-5.0,cy-5.0),NSSize::new(10.0,10.0))).fill();
                NSColor::controlAccentColor().setFill();
                NSBezierPath::bezierPathWithOvalInRect(NSRect::new(NSPoint::new(cx-2.8,cy-2.8),NSSize::new(5.6,5.6))).fill();
            }
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
            .constraintEqualToConstant(58.)
            .setActive(true);
        this
    }
}

pub(crate) struct FanGauge {
    pub id: u8,
    pub root: Retained<NSStackView>,
    pub arc: Retained<crate::gauge::Arc>,
    pub name: Retained<NSTextField>,
    pub rpm: Retained<NSTextField>,
    pub detail: Retained<NSTextField>,
}
impl FanGauge {
    pub fn new(mtm: MainThreadMarker, id: u8, width: f64) -> Self {
        let root = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 4.);
        root.setAlignment(NSLayoutAttribute::CenterX);
        root.widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        let name = styled_label(mtm, "", typography::CAPTION, true, false, Some(width));
        name.setAlignment(NSTextAlignment::Center);
        root.addArrangedSubview(&name);
        let arc = crate::gauge::Arc::new(mtm, width, tokens::GAUGE_TRACK);
        let center = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 0.);
        center.setAlignment(NSLayoutAttribute::CenterX);
        if let Some(icon) = theme::symbol_image("fanblades", None, 15.0) {
            let image = NSImageView::imageViewWithImage(&icon, mtm);
            image.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
            image
                .heightAnchor()
                .constraintEqualToConstant(18.)
                .setActive(true);
            image
                .widthAnchor()
                .constraintEqualToConstant(18.)
                .setActive(true);
            center.addArrangedSubview(&image);
        }
        let digit_size = if width < 150. {
            typography::RPM_DIGITS_COMPACT
        } else {
            typography::RPM_DIGITS
        };
        let rpm = styled_label(mtm, "—", digit_size, true, false, None);
        rpm.setFont(Some(&theme::rounded_digits(digit_size, unsafe {
            NSFontWeightSemibold
        })));
        center.addArrangedSubview(&rpm);
        center.addArrangedSubview(&styled_label(
            mtm,
            "RPM",
            typography::MICRO,
            true,
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
    pub fn refresh(&self, card: &FanCard) {
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
    fan_inner_width: f64,
}
impl Dashboard {
    pub fn new(mtm: MainThreadMarker, width: f64) -> Self {
        let view = vstack(mtm, spacing::MD);
        view.widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);

        // Card 1: Chip temperature + 10-minute trend.
        let card_pad = 12.;
        let inner = width - card_pad * 2.;
        let left_width = if width < 330. { 110. } else { 124. };
        let hero = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 12.);
        hero.setAlignment(NSLayoutAttribute::CenterY);
        let left = vstack(mtm, 3.);
        left.addArrangedSubview(&styled_label(
            mtm,
            "芯片温度",
            typography::CAPTION,
            true,
            true,
            None,
        ));
        let hero_size = if width < 330. {
            typography::DISPLAY_HERO_COMPACT
        } else {
            typography::DISPLAY_HERO
        };
        let temperature = styled_label(mtm, "—", hero_size, true, false, None);
        temperature.setFont(Some(&theme::rounded_digits(hero_size, unsafe {
            NSFontWeightSemibold
        })));
        let reading = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 2.);
        reading.setAlignment(NSLayoutAttribute::FirstBaseline);
        reading.addArrangedSubview(&temperature);
        reading.addArrangedSubview(&styled_label(
            mtm,
            "°C",
            if width < 330. { 20. } else { 22. },
            false,
            true,
            None,
        ));
        left.addArrangedSubview(&reading);
        let caption = styled_label(
            mtm,
            "正在检测…",
            typography::SMALL,
            false,
            true,
            Some(left_width),
        );
        left.addArrangedSubview(&caption);
        left.widthAnchor()
            .constraintEqualToConstant(left_width)
            .setActive(true);
        hero.addArrangedSubview(&left);

        let chart_width = (inner - left_width - 12.).max(110.);
        let right = vstack(mtm, 3.);
        right.addArrangedSubview(&styled_label(
            mtm,
            "温度趋势",
            typography::SMALL,
            false,
            true,
            None,
        ));

        let row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 4.);
        let axis = vstack(mtm, 28.);
        let high = styled_label(mtm, "—", tokens::PLOT_LABEL, false, true, Some(25.));
        let low = styled_label(mtm, "—", tokens::PLOT_LABEL, false, true, Some(25.));
        axis.addArrangedSubview(&high);
        axis.addArrangedSubview(&low);
        row.addArrangedSubview(&axis);
        let chart = TemperaturePlot::new(mtm, (chart_width - 29.).max(60.));
        row.addArrangedSubview(&chart);
        right.addArrangedSubview(&row);
        let timeline = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 2.);
        timeline
            .widthAnchor()
            .constraintEqualToConstant(chart_width)
            .setActive(true);
        timeline.addArrangedSubview(&styled_label(
            mtm,
            "10 分钟前",
            typography::MICRO,
            false,
            true,
            None,
        ));
        timeline.addArrangedSubview(&spacer(mtm));
        timeline.addArrangedSubview(&styled_label(
            mtm,
            "现在",
            typography::MICRO,
            false,
            true,
            None,
        ));
        right.addArrangedSubview(&timeline);
        let endpoint = styled_label(mtm, "", typography::MICRO, true, true, Some(chart_width));
        endpoint.setFont(Some(&theme::rounded_digits(typography::MICRO, unsafe {
            NSFontWeightSemibold
        })));
        endpoint.setAlignment(NSTextAlignment::Right);
        right.addArrangedSubview(&endpoint);
        hero.addArrangedSubview(&right);

        let thermal_card = surface_box(mtm, &hero, NSSize::new(card_pad, 12.), SurfaceStyle::Card);
        thermal_card
            .widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        view.addArrangedSubview(&thermal_card);

        // Card 2: Fan gauges.
        let fans_inner = vstack(mtm, spacing::SM);
        fans_inner.addArrangedSubview(&styled_label(
            mtm,
            "风扇转速",
            typography::CAPTION,
            true,
            true,
            None,
        ));
        let fan_rows = vstack(mtm, 10.);
        fan_rows
            .widthAnchor()
            .constraintEqualToConstant(inner)
            .setActive(true);
        fan_rows.setAlignment(NSLayoutAttribute::CenterX);
        fans_inner.addArrangedSubview(&fan_rows);
        let fans_card = surface_box(
            mtm,
            &fans_inner,
            NSSize::new(card_pad, 12.),
            SurfaceStyle::Card,
        );
        fans_card
            .widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        view.addArrangedSubview(&fans_card);

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
            fan_inner_width: inner,
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
        let temp_color = presentation
            .temperature
            .filter(|v| *v >= 85.)
            .map(crate::gauge::temperature_color)
            .unwrap_or_else(NSColor::labelColor);
        self.temperature.setTextColor(Some(&temp_color));
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
            let gap = 14.;
            let gauge_width = (self.fan_inner_width - gap) / 2.;
            for pair in cards.chunks(2) {
                let row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, gap);
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
        for label in [
            &self.temperature,
            &self.caption,
            &self.high,
            &self.low,
            &self.endpoint,
        ] {
            crate::popover::verify_label_geometry(label, &self.view);
        }
        for (g, c) in self.gauges.iter().zip(cards) {
            assert_eq!(g.id, c.id);
            for label in [&g.name, &g.rpm, &g.detail] {
                crate::popover::verify_label_geometry(label, &self.view);
            }
            assert_eq!(g.name.stringValue().to_string(), c.name);
            assert!(g.arc.frame().size.width > 0.);
            assert!(g.rpm.frame().size.height > 0.);
            assert!(g.root.frame().size.width <= self.fan_inner_width + 0.5);
        }
        if self.gauges.len() == 1 {
            let frame = self.gauges[0]
                .root
                .convertRect_toView(self.gauges[0].root.bounds(), Some(&self.fan_rows));
            assert!(
                (frame.origin.x + frame.size.width / 2. - self.fan_inner_width / 2.).abs() < 2.
            );
        }
    }
}
pub(crate) fn spacer(mtm: MainThreadMarker) -> Retained<NSView> {
    flex_spacer(mtm)
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
