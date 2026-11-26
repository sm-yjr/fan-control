//! Overview command center: health banner, fluid KPI tiles, 10-minute dual trend,
//! unified 180-degree RPM gauges and direct cooling plan switching.
use crate::{
    components::{flex_spacer, hstack, label, surface_box, vstack, MetricTile, SurfaceView},
    dashboard::FanGauge,
    form::{self, fluid_page},
    gauge::temperature_color,
    popover::{link_button, styled_label, tone_color},
    presenter::{Action, FanCard, Plan, Presentation, Tone},
    theme::{self, spacing, typography, SurfaceStyle},
    trend::{History, TrendView},
    ui::{raw_text, rect, text},
};
use objc2::{rc::Retained, runtime::AnyObject, sel, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSSize};
use std::time::Instant;

const TREND_HEIGHT: f64 = 124.;

pub(crate) struct Overview {
    pub view: Retained<NSStackView>,
    card: Retained<SurfaceView>,
    card_icon: Retained<NSImageView>,
    card_title: Retained<NSTextField>,
    card_body: Retained<NSTextField>,
    card_action: Retained<NSButton>,
    temperature: MetricTile,
    demand: MetricTile,
    battery: MetricTile,
    trend: Retained<TrendView>,
    trend_range: Retained<NSTextField>,
    pub plan: Retained<NSSegmentedControl>,
    plan_note: Retained<NSTextField>,
    fan_rows: Retained<NSStackView>,
    gauges: Vec<FanGauge>,
    shown_plan: Option<Plan>,
}

impl Overview {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        let view = fluid_page(mtm);
        view.setSpacing(spacing::LG);

        // Health / action banner.
        let card_stack = hstack(mtm, spacing::MD);
        let card_icon = NSImageView::new(mtm);
        card_icon.setTranslatesAutoresizingMaskIntoConstraints(false);
        card_icon
            .widthAnchor()
            .constraintEqualToConstant(20.)
            .setActive(true);
        card_icon
            .heightAnchor()
            .constraintEqualToConstant(20.)
            .setActive(true);
        let words = vstack(mtm, 2.);
        let card_title = label(
            mtm,
            "正在检测…",
            typography::HEADLINE,
            unsafe { NSFontWeightSemibold },
            false,
            true,
        );
        let card_body = label(
            mtm,
            "",
            typography::CAPTION,
            unsafe { NSFontWeightRegular },
            true,
            true,
        );
        words.addArrangedSubview(&card_title);
        words.addArrangedSubview(&card_body);
        words.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let card_action = unsafe {
            NSButton::buttonWithTitle_target_action(
                &text("启用风扇控制"),
                Some(target),
                Some(sel!(noticeAction:)),
                mtm,
            )
        };
        card_action.setControlSize(NSControlSize::Regular);
        card_stack.addArrangedSubview(&card_icon);
        card_stack.addArrangedSubview(&words);
        card_stack.addArrangedSubview(&flex_spacer(mtm));
        card_stack.addArrangedSubview(&card_action);
        let card = surface_box(
            mtm,
            &card_stack,
            NSSize::new(14., 12.),
            SurfaceStyle::Banner(Tone::Info),
        );
        view.addArrangedSubview(&card);
        card.widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Fluid KPI tiles (desktop Macs without battery automatically distribute 2 tiles).
        let tiles = hstack(mtm, spacing::MD);
        tiles.setDistribution(NSStackViewDistribution::FillEqually);
        tiles.setAlignment(NSLayoutAttribute::Top);
        let temperature = MetricTile::new(mtm, "芯片温度", true);
        let demand = MetricTile::new(mtm, "散热需求", true);
        let battery = MetricTile::new(mtm, "电池", true);
        tiles.addArrangedSubview(&temperature.view);
        tiles.addArrangedSubview(&demand.view);
        tiles.addArrangedSubview(&battery.view);
        view.addArrangedSubview(&tiles);
        tiles
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // 10-minute dual trend card.
        let trend_body = vstack(mtm, spacing::SM);
        let legend = hstack(mtm, spacing::MD);
        legend.addArrangedSubview(&label(
            mtm,
            "最近 10 分钟",
            typography::HEADLINE,
            unsafe { NSFontWeightSemibold },
            false,
            false,
        ));
        let trend_range = label(
            mtm,
            "",
            typography::SMALL,
            unsafe { NSFontWeightMedium },
            true,
            false,
        );
        trend_range.setFont(Some(&theme::rounded_digits(typography::SMALL, unsafe {
            NSFontWeightMedium
        })));
        legend.addArrangedSubview(&trend_range);
        legend.addArrangedSubview(&flex_spacer(mtm));
        let temperature_key = styled_label(mtm, "— 温度", typography::SMALL, true, false, None);
        temperature_key.setTextColor(Some(&NSColor::controlAccentColor()));
        let fan_key = styled_label(mtm, "- - 风扇", typography::SMALL, true, false, None);
        fan_key.setTextColor(Some(&NSColor::systemTealColor()));
        legend.addArrangedSubview(&temperature_key);
        legend.addArrangedSubview(&fan_key);
        trend_body.addArrangedSubview(&legend);
        legend
            .widthAnchor()
            .constraintEqualToAnchor(&trend_body.widthAnchor())
            .setActive(true);

        let trend = TrendView::new(mtm, rect(0., 0., 500., TREND_HEIGHT));
        trend
            .heightAnchor()
            .constraintEqualToConstant(TREND_HEIGHT)
            .setActive(true);
        trend_body.addArrangedSubview(&trend);
        trend
            .widthAnchor()
            .constraintEqualToAnchor(&trend_body.widthAnchor())
            .setActive(true);

        let trend_card = surface_box(mtm, &trend_body, NSSize::new(14., 12.), SurfaceStyle::Card);
        view.addArrangedSubview(&trend_card);
        trend_card
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Unified fan gauges + quick cooling mode switcher card.
        let fans_body = vstack(mtm, spacing::MD);
        let fans_header = hstack(mtm, spacing::MD);
        fans_header.addArrangedSubview(&label(
            mtm,
            "风扇与散热方式",
            typography::HEADLINE,
            unsafe { NSFontWeightSemibold },
            false,
            false,
        ));
        fans_header.addArrangedSubview(&flex_spacer(mtm));
        let plan_labels: Vec<_> = ["系统默认", "智能散热", "自定义"]
            .iter()
            .map(|name| text(name))
            .collect();
        let plan = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&plan_labels),
                NSSegmentSwitchTracking::SelectOne,
                Some(target),
                Some(sel!(overviewPlanChanged:)),
                mtm,
            )
        };
        plan.setSegmentDistribution(NSSegmentDistribution::FillEqually);
        plan.setAccessibilityLabel(Some(&text("散热方式")));
        fans_header.addArrangedSubview(&plan);
        fans_body.addArrangedSubview(&fans_header);
        fans_header
            .widthAnchor()
            .constraintEqualToAnchor(&fans_body.widthAnchor())
            .setActive(true);

        let fan_rows = vstack(mtm, spacing::MD);
        fan_rows.setAlignment(NSLayoutAttribute::CenterX);
        fans_body.addArrangedSubview(&fan_rows);
        fan_rows
            .widthAnchor()
            .constraintEqualToAnchor(&fans_body.widthAnchor())
            .setActive(true);

        let footer = hstack(mtm, spacing::SM);
        let plan_note = form::fluid_caption(mtm, "");
        let manage_fans = link_button(mtm, "单独配置各风扇…", target, sel!(showFans:));
        manage_fans.setFont(Some(&NSFont::systemFontOfSize(typography::CAPTION)));
        footer.addArrangedSubview(&plan_note);
        footer.addArrangedSubview(&flex_spacer(mtm));
        footer.addArrangedSubview(&manage_fans);
        fans_body.addArrangedSubview(&footer);
        footer
            .widthAnchor()
            .constraintEqualToAnchor(&fans_body.widthAnchor())
            .setActive(true);

        let fans_card = surface_box(mtm, &fans_body, NSSize::new(14., 14.), SurfaceStyle::Card);
        view.addArrangedSubview(&fans_card);
        fans_card
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        Self {
            view,
            card,
            card_icon,
            card_title,
            card_body,
            card_action,
            temperature,
            demand,
            battery,
            trend,
            trend_range,
            plan,
            plan_note,
            fan_rows,
            gauges: Vec::new(),
            shown_plan: None,
        }
    }

    pub fn refresh(
        &mut self,
        view: &Presentation,
        cards: &[FanCard],
        demand_percent: Option<f64>,
        history: &History,
    ) {
        let mtm = self.view.mtm();
        let tint = tone_color(view.notice.tone);
        self.card.set_style(SurfaceStyle::Banner(view.notice.tone));
        if let Some(icon) = theme::symbol_image(theme::tone_symbol(view.notice.tone), None, 18.0) {
            self.card_icon.setImage(Some(&icon));
        }
        self.card_icon.setContentTintColor(Some(&tint));
        self.card_title.setStringValue(&text(view.notice.title));
        let title_color = if view.notice.tone == Tone::Good {
            NSColor::labelColor()
        } else {
            tint
        };
        self.card_title.setTextColor(Some(&title_color));
        self.card_body.setStringValue(&text(view.notice.body));
        match view.notice.action {
            Some(action) => {
                self.card_action.setHidden(false);
                self.card_action.setTitle(&text(match action {
                    Action::EnableControl => "启用风扇控制",
                    Action::Reconnect => "重新连接",
                }));
            }
            None => self.card_action.setHidden(true),
        }

        // KPI 1: Chip temperature
        match view.temperature {
            Some(v) => {
                self.temperature
                    .value
                    .setStringValue(&raw_text(&format!("{v:.0}°C")));
                self.temperature.set_subtitle(view.trend);
                if let Some(bar) = &self.temperature.bar {
                    bar.set(Some(((v - 20.) / 90.).clamp(0., 1.)), &temperature_color(v));
                }
            }
            None => {
                self.temperature.value.setStringValue(&raw_text("—"));
                self.temperature.set_subtitle("暂无温度数据");
                if let Some(bar) = &self.temperature.bar {
                    bar.set(None, &NSColor::secondaryLabelColor());
                }
            }
        }

        // KPI 2: Cooling demand
        self.demand.value.setStringValue(&text(view.demand));
        if let Some(pct) = demand_percent {
            self.demand
                .subtitle
                .setStringValue(&raw_text(&format!("{pct:.0}%")));
            self.demand.subtitle.setHidden(false);
            let demand_color = if pct >= 70. {
                NSColor::systemOrangeColor()
            } else if pct >= 35. {
                NSColor::systemYellowColor()
            } else {
                NSColor::systemGreenColor()
            };
            if let Some(bar) = &self.demand.bar {
                bar.set(Some((pct / 100.).clamp(0., 1.)), &demand_color);
            }
        } else {
            self.demand.set_subtitle("");
            if let Some(bar) = &self.demand.bar {
                bar.set(None, &NSColor::secondaryLabelColor());
            }
        }

        // KPI 3: Battery (hidden cleanly on desktop Macs)
        self.battery.view.setHidden(view.battery.is_none());
        if let Some(pct) = view.battery {
            self.battery
                .value
                .setStringValue(&raw_text(&format!("{pct:.0}%")));
            let color = if pct <= 20. {
                NSColor::systemOrangeColor()
            } else {
                NSColor::systemGreenColor()
            };
            if let Some(bar) = &self.battery.bar {
                bar.set(Some((pct / 100.).clamp(0., 1.)), &color);
            }
        }

        // Trend chart
        let range = self.trend.set_history(history, Instant::now());
        if let Some((lo, hi)) = range {
            self.trend_range
                .setStringValue(&raw_text(&format!("{lo:.0}–{hi:.0}°C")));
        } else {
            self.trend_range.setStringValue(&raw_text(""));
        }

        // Quick plan segmented control
        if self.shown_plan != Some(view.plan) {
            match view.plan {
                Plan::System => self.plan.setSelectedSegment(0),
                Plan::Smart => self.plan.setSelectedSegment(1),
                Plan::Custom => self.plan.setSelectedSegment(2),
                Plan::Mixed => {
                    for index in 0..3 {
                        self.plan.setSelected_forSegment(false, index);
                    }
                }
            }
            self.shown_plan = Some(view.plan);
        }
        self.plan.setEnabled(view.controls_enabled);
        self.plan_note.setStringValue(&text(view.plan_description));

        // Fan gauges (rebuild only on topology change)
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
                    typography::CAPTION,
                    false,
                    true,
                    None,
                ));
            }
            for pair in cards.chunks(2) {
                let row = hstack(mtm, 28.);
                row.setAlignment(NSLayoutAttribute::Top);
                for card in pair {
                    let gauge = FanGauge::new(mtm, card.id, 176.);
                    row.addArrangedSubview(&gauge.root);
                    self.gauges.push(gauge);
                }
                self.fan_rows.addArrangedSubview(&row);
            }
        } else if cards.is_empty() && self.fan_rows.arrangedSubviews().is_empty() {
            self.fan_rows.addArrangedSubview(&styled_label(
                mtm,
                "没有可显示的风扇",
                typography::CAPTION,
                false,
                true,
                None,
            ));
        }
        for (gauge, card) in self.gauges.iter().zip(cards) {
            gauge.refresh(card);
        }
    }

    pub fn forget_plan(&mut self) {
        self.shown_plan = None;
    }
}
