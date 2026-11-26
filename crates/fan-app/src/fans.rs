//! Fans page: one layered card per fan with a unified 180-degree RPM tachometer,
//! segmented mode picker, visible target/confirmed telemetry and live speed control.
use crate::{
    components::{flex_spacer, hstack, label, surface_box, vstack, SurfaceView},
    form::{self, fluid_caption, fluid_page},
    gauge::Arc,
    popover::styled_label,
    presenter::{FanCard, FanChoice, Presentation, Tone},
    theme::{self, spacing, typography, SurfaceStyle},
    ui::{raw_text, text},
};
use objc2::{rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSSize};

const GAUGE_WIDTH: f64 = 148.;

struct Card {
    id: u8,
    view: Retained<SurfaceView>,
    arc: Retained<Arc>,
    rpm: Retained<NSTextField>,
    range_label: Retained<NSTextField>,
    name: Retained<NSTextField>,
    speed: Retained<NSTextField>,
    target_line: Retained<NSTextField>,
    choice: Retained<NSSegmentedControl>,
    explanation: Retained<NSTextField>,
    fixed: Retained<NSStackView>,
    slider: Retained<NSSlider>,
    fixed_value: Retained<NSTextField>,
    fixed_hint: Retained<NSTextField>,
    curve: Retained<NSButton>,
    shown: Option<FanChoice>,
}

pub(crate) struct FansPage {
    pub view: Retained<NSStackView>,
    cards: Retained<NSStackView>,
    items: Vec<Card>,
    empty: Retained<NSTextField>,
    notice: Retained<NSTextField>,
    notice_box: Retained<SurfaceView>,
}

fn card(mtm: MainThreadMarker, target: &AnyObject, index: usize) -> Card {
    // Left column: unified 180° RPM tachometer + hardware bounds.
    let left = vstack(mtm, 4.);
    left.setAlignment(NSLayoutAttribute::CenterX);
    left.widthAnchor()
        .constraintEqualToConstant(GAUGE_WIDTH)
        .setActive(true);

    let arc = Arc::new(mtm, GAUGE_WIDTH, theme::layout::GAUGE_TRACK);
    let center = vstack(mtm, 0.);
    center.setAlignment(NSLayoutAttribute::CenterX);
    if let Some(icon) = theme::symbol_image("fanblades", None, 14.0) {
        let img = NSImageView::imageViewWithImage(&icon, mtm);
        img.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
        img.widthAnchor()
            .constraintEqualToConstant(16.)
            .setActive(true);
        img.heightAnchor()
            .constraintEqualToConstant(16.)
            .setActive(true);
        center.addArrangedSubview(&img);
    }
    let rpm = styled_label(mtm, "—", typography::RPM_DIGITS_COMPACT, true, false, None);
    rpm.setFont(Some(&theme::rounded_digits(
        typography::RPM_DIGITS_COMPACT,
        unsafe { NSFontWeightSemibold },
    )));
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
    left.addArrangedSubview(&arc);

    let range_label = styled_label(mtm, "", typography::MICRO, false, true, None);
    range_label.setFont(Some(&theme::rounded_digits(typography::MICRO, unsafe {
        NSFontWeightRegular
    })));
    left.addArrangedSubview(&range_label);

    // Right column: title, live state, target telemetry, 4-segment mode picker & controls.
    let details = vstack(mtm, spacing::SM);
    details.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityDefaultLow,
        NSLayoutConstraintOrientation::Horizontal,
    );

    let title_row = hstack(mtm, spacing::SM);
    let name = label(
        mtm,
        "",
        typography::SECTION,
        unsafe { NSFontWeightSemibold },
        false,
        false,
    );
    let speed = label(
        mtm,
        "",
        typography::CAPTION,
        unsafe { NSFontWeightRegular },
        true,
        false,
    );
    title_row.addArrangedSubview(&name);
    title_row.addArrangedSubview(&flex_spacer(mtm));
    title_row.addArrangedSubview(&speed);
    details.addArrangedSubview(&title_row);
    title_row
        .widthAnchor()
        .constraintEqualToAnchor(&details.widthAnchor())
        .setActive(true);

    let target_line = label(
        mtm,
        "",
        typography::SMALL,
        unsafe { NSFontWeightRegular },
        true,
        true,
    );
    target_line.setFont(Some(&theme::rounded_digits(typography::SMALL, unsafe {
        NSFontWeightRegular
    })));
    target_line.setHidden(true);
    details.addArrangedSubview(&target_line);

    let labels: Vec<_> = FanChoice::ALL
        .iter()
        .map(|option| text(option.title()))
        .collect();
    let choice = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&labels),
            NSSegmentSwitchTracking::SelectOne,
            Some(target),
            Some(sel!(fanChoice:)),
            mtm,
        )
    };
    choice.setControlSize(NSControlSize::Small);
    choice.setFont(Some(&NSFont::systemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    choice.setSegmentDistribution(NSSegmentDistribution::Fit);
    choice.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityDefaultLow,
        NSLayoutConstraintOrientation::Horizontal,
    );
    choice.setTag(index as isize);
    details.addArrangedSubview(&choice);
    choice
        .widthAnchor()
        .constraintLessThanOrEqualToAnchor(&details.widthAnchor())
        .setActive(true);

    let explanation = fluid_caption(mtm, "");
    details.addArrangedSubview(&explanation);

    // Fixed speed slider block.
    let slider_row = hstack(mtm, spacing::MD);
    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(
            40.,
            0.,
            100.,
            Some(target),
            Some(sel!(fanSpeed:)),
            mtm,
        )
    };
    slider.setTag(index as isize);
    slider.setNumberOfTickMarks(5);
    slider.setAccessibilityHelp(Some(&text(
        "0% 为硬件最低转速，100% 为最高转速。松开后生效。",
    )));
    let fixed_value = styled_label(mtm, "", form::BODY, true, false, None);
    fixed_value.setFont(Some(&theme::rounded_digits(form::BODY, unsafe {
        NSFontWeightSemibold
    })));
    fixed_value
        .widthAnchor()
        .constraintGreaterThanOrEqualToConstant(42.)
        .setActive(true);
    fixed_value.setAlignment(NSTextAlignment::Right);
    slider_row.addArrangedSubview(&slider);
    slider_row.addArrangedSubview(&fixed_value);

    let range = hstack(mtm, spacing::SM);
    range.addArrangedSubview(&styled_label(
        mtm,
        "最安静",
        form::CAPTION,
        false,
        true,
        None,
    ));
    range.addArrangedSubview(&flex_spacer(mtm));
    let fixed_hint = styled_label(mtm, "", form::CAPTION, false, true, None);
    fixed_hint.setFont(Some(&theme::rounded_digits(form::CAPTION, unsafe {
        NSFontWeightRegular
    })));
    range.addArrangedSubview(&fixed_hint);
    range.addArrangedSubview(&flex_spacer(mtm));
    range.addArrangedSubview(&styled_label(
        mtm,
        "最凉爽",
        form::CAPTION,
        false,
        true,
        None,
    ));

    let fixed = vstack(mtm, 3.);
    fixed.addArrangedSubview(&slider_row);
    slider_row
        .widthAnchor()
        .constraintEqualToAnchor(&fixed.widthAnchor())
        .setActive(true);
    fixed.addArrangedSubview(&range);
    range
        .widthAnchor()
        .constraintEqualToAnchor(&fixed.widthAnchor())
        .setActive(true);
    details.addArrangedSubview(&fixed);
    fixed
        .widthAnchor()
        .constraintEqualToAnchor(&details.widthAnchor())
        .setActive(true);

    let curve = unsafe {
        NSButton::buttonWithTitle_target_action(
            &text("编辑温度曲线…"),
            Some(target),
            Some(sel!(fanCurve:)),
            mtm,
        )
    };
    curve.setTag(index as isize);
    details.addArrangedSubview(&curve);

    let content = hstack(mtm, spacing::XL);
    content.setAlignment(NSLayoutAttribute::Top);
    content.addArrangedSubview(&left);
    content.addArrangedSubview(&details);

    let view = surface_box(mtm, &content, NSSize::new(16., 14.), SurfaceStyle::Card);
    Card {
        id: 0,
        view,
        arc,
        rpm,
        range_label,
        name,
        speed,
        target_line,
        choice,
        explanation,
        fixed,
        slider,
        fixed_value,
        fixed_hint,
        curve,
        shown: None,
    }
}

impl FansPage {
    pub fn new(mtm: MainThreadMarker, demo: bool) -> Self {
        let view = fluid_page(mtm);
        view.setSpacing(spacing::MD);
        let intro = fluid_caption(
            mtm,
            if demo {
                "演示模式 · 模拟数据 · 不连接风扇"
            } else {
                "菜单栏面板会把同一种散热方式用于所有风扇；在这里可以为每个风扇单独设置。"
            },
        );
        view.addArrangedSubview(&intro);

        let cards = vstack(mtm, spacing::MD);
        view.addArrangedSubview(&cards);
        cards
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -crate::form::PAGE_INSET * 2.)
            .setActive(true);

        let empty = label(
            mtm,
            "正在读取风扇…",
            form::BODY,
            unsafe { NSFontWeightRegular },
            true,
            true,
        );
        view.addArrangedSubview(&empty);

        let notice = label(
            mtm,
            "",
            form::CAPTION,
            unsafe { NSFontWeightRegular },
            false,
            true,
        );
        let notice_box = surface_box(mtm, &notice, NSSize::new(14., 10.), SurfaceStyle::Well);
        view.addArrangedSubview(&notice_box);
        notice_box
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -crate::form::PAGE_INSET * 2.)
            .setActive(true);
        Self {
            view,
            cards,
            items: Vec::new(),
            empty,
            notice,
            notice_box,
        }
    }

    pub fn card_id(&self, index: isize) -> Option<u8> {
        self.items
            .get(usize::try_from(index).ok()?)
            .map(|card| card.id)
    }

    pub fn choice(&self, index: isize) -> Option<FanChoice> {
        let card = self.items.get(usize::try_from(index).ok()?)?;
        FanChoice::ALL
            .get(card.choice.selectedSegment().max(0) as usize)
            .copied()
    }

    pub fn slider_value(&self, index: isize) -> Option<f64> {
        let card = self.items.get(usize::try_from(index).ok()?)?;
        Some(card.slider.doubleValue().round())
    }

    pub fn show_fixed_value(&self, index: isize, percent: f64, hint: &str) {
        if let Some(card) = usize::try_from(index).ok().and_then(|i| self.items.get(i)) {
            set_fixed_value(card, percent, hint);
        }
    }

    /// Forces the next refresh to show the saved choice again.
    pub fn forget(&mut self, index: isize) {
        if let Some(card) = usize::try_from(index)
            .ok()
            .and_then(|i| self.items.get_mut(i))
        {
            card.shown = None;
        }
    }

    pub fn refresh(
        &mut self,
        target: &AnyObject,
        view: &Presentation,
        cards: &[FanCard],
        enabled: bool,
        fresh: bool,
        pending: Option<(u8, f64)>,
    ) -> bool {
        let mtm = MainThreadMarker::new().expect("UI main thread");
        let mut resized = false;
        while self.items.len() < cards.len() {
            let card = card(mtm, target, self.items.len());
            self.cards.addArrangedSubview(&card.view);
            card.view
                .widthAnchor()
                .constraintEqualToAnchor(&self.cards.widthAnchor())
                .setActive(true);
            self.items.push(card);
            resized = true;
        }
        while self.items.len() > cards.len() {
            if let Some(card) = self.items.pop() {
                card.view.removeFromSuperview();
                resized = true;
            }
        }
        self.empty.setHidden(!cards.is_empty());
        if cards.is_empty() {
            self.empty.setStringValue(&text(view.notice.body));
        }
        for (card, data) in self.items.iter_mut().zip(cards) {
            if card.id != data.id {
                card.id = data.id;
                card.shown = None;
            }
            card.name.setStringValue(&text(&data.name));
            card.speed.setStringValue(&raw_text(&format!(
                "{} · {}",
                crate::i18n::translate(&data.speed),
                crate::i18n::translate(&data.detail)
            )));
            card.target_line.setStringValue(&text(&data.target));
            card.target_line.setHidden(data.target.is_empty());
            card.arc.set(
                data.percent.map(|value| value / 100.),
                &NSColor::controlAccentColor(),
            );
            card.rpm.setStringValue(&raw_text(
                &data
                    .measured_rpm
                    .map(|rpm| crate::presenter::grouped(rpm.round() as u64))
                    .unwrap_or_else(|| "—".into()),
            ));
            card.range_label.setStringValue(&raw_text(
                &data
                    .range
                    .map(|(lo, hi)| {
                        format!(
                            "{}–{} RPM",
                            crate::presenter::grouped(u64::from(lo)),
                            crate::presenter::grouped(u64::from(hi))
                        )
                    })
                    .unwrap_or_else(|| crate::i18n::translate("转速范围未知")),
            ));
            card.arc.setAccessibilityElement(true);
            card.arc.setAccessibilityLabel(Some(&raw_text(&format!(
                "{} {}",
                crate::i18n::translate(&data.name),
                crate::i18n::translate(&data.speed)
            ))));
            let controllable = data.range.is_some();
            card.choice.setEnabled(enabled);
            for index in 1..FanChoice::ALL.len() {
                card.choice
                    .setEnabled_forSegment(enabled && controllable && fresh, index as isize);
            }
            card.choice.setAccessibilityLabel(Some(&raw_text(&format!(
                "{} {}",
                crate::i18n::translate(&data.name),
                crate::i18n::translate("散热方式")
            ))));
            if card.shown != Some(data.choice) {
                if let Some(index) = FanChoice::ALL.iter().position(|c| *c == data.choice) {
                    card.choice.setSelectedSegment(index as isize);
                }
                card.shown = Some(data.choice);
            }
            let choice = FanChoice::ALL
                .get(card.choice.selectedSegment().max(0) as usize)
                .copied()
                .unwrap_or(data.choice);
            card.explanation.setStringValue(&text(choice.explanation()));
            card.fixed.setHidden(choice != FanChoice::Fixed);
            card.curve.setHidden(choice != FanChoice::Curve);
            card.curve.setEnabled(controllable);
            card.slider.setEnabled(enabled && controllable);
            card.slider.setAccessibilityLabel(Some(&raw_text(&format!(
                "{} {}",
                crate::i18n::translate(&data.name),
                crate::i18n::translate("风扇速度")
            ))));
            let percent = match pending {
                Some((id, percent)) if id == data.id => percent,
                _ => {
                    card.slider.setDoubleValue(data.fixed_percent);
                    data.fixed_percent
                }
            };
            set_fixed_value(
                card,
                percent,
                &crate::presenter::rpm_hint(percent, data.range),
            );
        }
        let attention = view.notice.tone != Tone::Good;
        self.notice_box.set_style(if attention {
            SurfaceStyle::Banner(view.notice.tone)
        } else {
            SurfaceStyle::Well
        });
        self.notice.setStringValue(&raw_text(&if attention {
            format!(
                "{}\n{}",
                crate::i18n::translate(view.notice.title),
                crate::i18n::translate(view.notice.body)
            )
        } else {
            crate::i18n::translate("转速以风扇实测为准。温度过高时，安全保护会临时接管所有风扇。")
        }));
        resized
    }
}

fn set_fixed_value(card: &Card, percent: f64, hint: &str) {
    card.fixed_value
        .setStringValue(&raw_text(&format!("{percent:.0}%")));
    card.fixed_hint.setStringValue(&text(hint));
}
