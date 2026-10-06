//! Overview page of the main window: health, three key numbers, the recent
//! trend and every fan's real state in plain language.
use crate::popover::{stack, styled_label, tinted_box, tokens, tone_color};
use crate::presenter::{Action, Plan, Presentation, Tone};
use crate::trend::{History, TrendView};
use crate::ui::{raw_text, rect, text};
use objc2::{rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSSize};
use std::time::Instant;

pub(crate) const WIDTH: f64 = crate::ui::tokens::WIDTH;
const INSET: f64 = 24.;
const CONTENT: f64 = WIDTH - INSET * 2.;
const TILE_GAP: f64 = 10.;
const TILE: f64 = (CONTENT - TILE_GAP * 2.) / 3.;
const TREND_HEIGHT: f64 = 96.;

struct Tile {
    value: Retained<NSTextField>,
}

pub(crate) struct Overview {
    pub view: Retained<NSStackView>,
    card: Retained<crate::popover::TintedView>,
    card_title: Retained<NSTextField>,
    card_body: Retained<NSTextField>,
    card_action: Retained<NSButton>,
    temperature: Tile,
    demand: Tile,
    battery: Tile,
    trend: Retained<TrendView>,
    fans: Retained<NSStackView>,
    fan_rows: Vec<(Retained<NSTextField>, Retained<NSTextField>)>,
    plan: Retained<NSTextField>,
}

fn tile(mtm: MainThreadMarker, title: &str) -> (Retained<crate::popover::TintedView>, Tile) {
    let content = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 2.);
    content.addArrangedSubview(&styled_label(
        mtm,
        title,
        tokens::CAPTION,
        false,
        true,
        None,
    ));
    let value = styled_label(mtm, "--", 22., true, false, None);
    value.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
        22.,
        unsafe { NSFontWeightMedium },
    )));
    content.addArrangedSubview(&value);
    let tile = tinted_box(mtm, &content, NSSize::new(12., 10.));
    tile.set_fill(&NSColor::quaternarySystemFillColor());
    tile.widthAnchor()
        .constraintEqualToConstant(TILE)
        .setActive(true);
    value.setAccessibilityLabel(Some(&text(title)));
    (tile, Tile { value })
}

impl Overview {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        let view = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, tokens::GAP);
        view.setEdgeInsets(NSEdgeInsets {
            top: 20.,
            left: INSET,
            bottom: INSET,
            right: INSET,
        });
        view.widthAnchor()
            .constraintEqualToConstant(WIDTH)
            .setActive(true);

        let card_stack = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 12.);
        let words = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 2.);
        let card_title = styled_label(
            mtm,
            "正在检测…",
            tokens::HEADLINE,
            true,
            false,
            Some(CONTENT - 190.),
        );
        let card_body = styled_label(mtm, "", tokens::BODY, false, true, Some(CONTENT - 190.));
        words.addArrangedSubview(&card_title);
        words.addArrangedSubview(&card_body);
        card_stack.addArrangedSubview(&words);
        let spacer = NSView::new(mtm);
        spacer.setContentHuggingPriority_forOrientation(
            1.,
            NSLayoutConstraintOrientation::Horizontal,
        );
        card_stack.addArrangedSubview(&spacer);
        let card_action = unsafe {
            NSButton::buttonWithTitle_target_action(
                &text("启用风扇控制"),
                Some(target),
                Some(sel!(noticeAction:)),
                mtm,
            )
        };
        card_stack.addArrangedSubview(&card_action);
        let card = tinted_box(mtm, &card_stack, NSSize::new(14., 12.));
        card.widthAnchor()
            .constraintEqualToConstant(CONTENT)
            .setActive(true);
        view.addArrangedSubview(&card);
        view.setCustomSpacing_afterView(16., &card);

        let tiles = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, TILE_GAP);
        let (temperature_box, temperature) = tile(mtm, "芯片温度");
        let (demand_box, demand) = tile(mtm, "散热需求");
        let (battery_box, battery) = tile(mtm, "电池");
        tiles.addArrangedSubview(&temperature_box);
        tiles.addArrangedSubview(&demand_box);
        tiles.addArrangedSubview(&battery_box);
        view.addArrangedSubview(&tiles);
        view.setCustomSpacing_afterView(16., &tiles);

        let legend = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 12.);
        legend.addArrangedSubview(&styled_label(
            mtm,
            "最近 10 分钟",
            tokens::CAPTION,
            false,
            true,
            None,
        ));
        let legend_spacer = NSView::new(mtm);
        legend_spacer.setContentHuggingPriority_forOrientation(
            1.,
            NSLayoutConstraintOrientation::Horizontal,
        );
        legend.addArrangedSubview(&legend_spacer);
        let temperature_key = styled_label(mtm, "— 温度", tokens::CAPTION, false, false, None);
        temperature_key.setTextColor(Some(&NSColor::systemOrangeColor()));
        let fan_key = styled_label(mtm, "- - 风扇", tokens::CAPTION, false, false, None);
        fan_key.setTextColor(Some(&NSColor::systemBlueColor()));
        legend.addArrangedSubview(&temperature_key);
        legend.addArrangedSubview(&fan_key);
        legend
            .widthAnchor()
            .constraintEqualToConstant(CONTENT)
            .setActive(true);
        view.addArrangedSubview(&legend);
        let trend = TrendView::new(mtm, rect(0., 0., CONTENT, TREND_HEIGHT));
        trend
            .widthAnchor()
            .constraintEqualToConstant(CONTENT)
            .setActive(true);
        trend
            .heightAnchor()
            .constraintEqualToConstant(TREND_HEIGHT)
            .setActive(true);
        view.addArrangedSubview(&trend);
        view.setCustomSpacing_afterView(16., &trend);

        view.addArrangedSubview(&styled_label(
            mtm,
            "风扇",
            tokens::CAPTION,
            false,
            true,
            None,
        ));
        let fans = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 6.);
        view.addArrangedSubview(&fans);
        let plan = styled_label(mtm, "", tokens::CAPTION, false, true, Some(CONTENT));
        view.addArrangedSubview(&plan);
        Self {
            view,
            card,
            card_title,
            card_body,
            card_action,
            temperature,
            demand,
            battery,
            trend,
            fans,
            fan_rows: Vec::new(),
            plan,
        }
    }

    fn sync_rows(&mut self, mtm: MainThreadMarker, count: usize) {
        while self.fan_rows.len() < count {
            let row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.);
            let name = styled_label(mtm, "", tokens::BODY, false, false, None);
            let spacer = NSView::new(mtm);
            spacer.setContentHuggingPriority_forOrientation(
                1.,
                NSLayoutConstraintOrientation::Horizontal,
            );
            let value = styled_label(mtm, "", tokens::BODY, false, true, None);
            row.addArrangedSubview(&name);
            row.addArrangedSubview(&spacer);
            row.addArrangedSubview(&value);
            row.widthAnchor()
                .constraintEqualToConstant(CONTENT)
                .setActive(true);
            self.fans.addArrangedSubview(&row);
            self.fan_rows.push((name, value));
        }
        for (index, (name, _)) in self.fan_rows.iter().enumerate() {
            if let Some(row) = unsafe { name.superview() } {
                row.setHidden(index >= count);
            }
        }
    }

    pub fn refresh(&mut self, view: &Presentation, history: &History) {
        let mtm = MainThreadMarker::new().expect("UI main thread");
        let tint = tone_color(view.notice.tone);
        self.card
            .set_fill(&tint.colorWithAlphaComponent(tokens::TINT_ALPHA));
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
        self.temperature.value.setStringValue(&raw_text(
            &view
                .temperature
                .map(|value| format!("{value:.0}°C"))
                .unwrap_or_else(|| "--".into()),
        ));
        self.demand.value.setStringValue(&text(view.demand));
        self.battery.value.setStringValue(&raw_text(
            &view
                .battery
                .map(|value| format!("{value:.0}%"))
                .unwrap_or_else(|| "--".into()),
        ));
        self.trend.set_history(history, Instant::now());
        self.sync_rows(mtm, view.fans.len());
        for (fan, (name, value)) in view.fans.iter().zip(&self.fan_rows) {
            name.setStringValue(&text(&fan.name));
            value.setStringValue(&raw_text(&format!(
                "{} · {}",
                crate::i18n::translate(&fan.speed),
                crate::i18n::translate(&fan.detail)
            )));
        }
        let plan = match view.plan {
            Plan::System => "系统默认",
            Plan::Smart => "智能散热",
            Plan::Custom => "自定义",
            Plan::Mixed => "各风扇分别设置",
        };
        self.plan.setStringValue(&raw_text(&format!(
            "{}{}",
            crate::i18n::translate("当前散热方式："),
            crate::i18n::translate(plan)
        )));
    }
}
