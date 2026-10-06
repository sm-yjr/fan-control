//! Fans page: one card per fan with a speed ring and its own setting. Changes
//! apply right away, like the menu bar panel; a slider waits until it settles.
use crate::form::{self, caption, spacer, CONTENT};
use crate::gauge::Ring;
use crate::popover::{stack, styled_label, tinted_box, tone_color, TintedView};
use crate::presenter::{FanCard, FanChoice, Presentation, Tone};
use crate::ui::{raw_text, text};
use objc2::{rc::Retained, runtime::AnyObject, sel, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSSize};

const RING: f64 = 92.;
const DETAILS: f64 = CONTENT - RING - 28. - 20.;

struct Card {
    id: u8,
    view: Retained<TintedView>,
    ring: Retained<Ring>,
    percent: Retained<NSTextField>,
    name: Retained<NSTextField>,
    speed: Retained<NSTextField>,
    choice: Retained<NSPopUpButton>,
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
    notice_box: Retained<TintedView>,
}

fn card(mtm: MainThreadMarker, target: &AnyObject, index: usize) -> Card {
    let ring = Ring::new(mtm, RING, 9.);
    let percent = styled_label(mtm, "--", 20., true, false, None);
    percent.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
        20.,
        unsafe { NSFontWeightMedium },
    )));
    let unit = styled_label(mtm, "转速", form::CAPTION, false, true, None);
    let center = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 0.);
    center.setAlignment(NSLayoutAttribute::CenterX);
    center.addArrangedSubview(&percent);
    center.addArrangedSubview(&unit);
    center.setTranslatesAutoresizingMaskIntoConstraints(false);
    ring.addSubview(&center);
    center
        .centerXAnchor()
        .constraintEqualToAnchor(&ring.centerXAnchor())
        .setActive(true);
    center
        .centerYAnchor()
        .constraintEqualToAnchor(&ring.centerYAnchor())
        .setActive(true);

    let details = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 6.);
    let name = styled_label(mtm, "", 15., true, false, None);
    let speed = styled_label(mtm, "", form::BODY, false, true, None);
    details.addArrangedSubview(&name);
    details.addArrangedSubview(&speed);
    details.setCustomSpacing_afterView(10., &speed);

    let choice_row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.);
    choice_row.addArrangedSubview(&styled_label(
        mtm,
        "散热方式",
        form::BODY,
        false,
        false,
        None,
    ));
    let choice = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        crate::ui::rect(0., 0., 150., 26.),
        false,
    );
    if let Some(menu) = choice.menu() {
        menu.setAutoenablesItems(false);
    }
    for option in FanChoice::ALL {
        choice.addItemWithTitle(&text(option.title()));
    }
    choice.setTag(index as isize);
    unsafe {
        choice.setTarget(Some(target));
        choice.setAction(Some(sel!(fanChoice:)));
    }
    choice_row.addArrangedSubview(&choice);
    details.addArrangedSubview(&choice_row);
    let explanation = caption(mtm, "", DETAILS);
    details.addArrangedSubview(&explanation);

    let slider_row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 10.);
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
    slider
        .widthAnchor()
        .constraintEqualToConstant(DETAILS - 60.)
        .setActive(true);
    slider.setAccessibilityHelp(Some(&text(
        "0% 为硬件最低转速，100% 为最高转速。松开后生效。",
    )));
    let fixed_value = styled_label(mtm, "", form::BODY, true, false, None);
    fixed_value.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
        form::BODY,
        unsafe { NSFontWeightMedium },
    )));
    slider_row.addArrangedSubview(&slider);
    slider_row.addArrangedSubview(&fixed_value);
    let range = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.);
    range.addArrangedSubview(&styled_label(
        mtm,
        "最安静",
        form::CAPTION,
        false,
        true,
        None,
    ));
    range.addArrangedSubview(&spacer(mtm));
    let fixed_hint = styled_label(mtm, "", form::CAPTION, false, true, None);
    range.addArrangedSubview(&fixed_hint);
    range.addArrangedSubview(&spacer(mtm));
    range.addArrangedSubview(&styled_label(
        mtm,
        "最凉爽",
        form::CAPTION,
        false,
        true,
        None,
    ));
    range
        .widthAnchor()
        .constraintEqualToConstant(DETAILS - 60.)
        .setActive(true);
    let fixed = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 2.);
    fixed.addArrangedSubview(&slider_row);
    fixed.addArrangedSubview(&range);
    details.addArrangedSubview(&fixed);

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

    let content = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 20.);
    content.setAlignment(NSLayoutAttribute::Top);
    content.addArrangedSubview(&ring);
    content.addArrangedSubview(&details);
    content.addArrangedSubview(&spacer(mtm));
    let view = tinted_box(mtm, &content, NSSize::new(14., 14.));
    view.set_fill(&NSColor::quaternarySystemFillColor());
    view.widthAnchor()
        .constraintEqualToConstant(CONTENT)
        .setActive(true);
    Card {
        id: 0,
        view,
        ring,
        percent,
        name,
        speed,
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
        let view = form::page(mtm);
        view.addArrangedSubview(&caption(
            mtm,
            if demo {
                "演示模式 · 模拟数据 · 不连接风扇"
            } else {
                "菜单栏面板会把同一种散热方式用于所有风扇；在这里可以为每个风扇单独设置。"
            },
            CONTENT,
        ));
        let cards = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 12.);
        view.addArrangedSubview(&cards);
        let empty = styled_label(mtm, "正在读取风扇…", form::BODY, false, true, Some(CONTENT));
        view.addArrangedSubview(&empty);
        let notice = styled_label(mtm, "", form::CAPTION, false, false, Some(CONTENT - 24.));
        let notice_box = tinted_box(mtm, &notice, NSSize::new(12., 8.));
        notice_box
            .widthAnchor()
            .constraintEqualToConstant(CONTENT)
            .setActive(true);
        view.addArrangedSubview(&notice_box);
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
            .get(card.choice.indexOfSelectedItem().max(0) as usize)
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

    /// Returns true when cards were added or removed, so the window can resize.
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
            card.speed.setToolTip(
                if data.target.is_empty() {
                    None
                } else {
                    Some(text(&data.target))
                }
                .as_deref(),
            );
            card.ring.set(
                data.percent.map(|value| value / 100.),
                &NSColor::controlAccentColor(),
            );
            card.percent.setStringValue(&raw_text(
                &data
                    .percent
                    .map(|value| format!("{value:.0}%"))
                    .unwrap_or_else(|| "--".into()),
            ));
            card.ring.setAccessibilityElement(true);
            card.ring.setAccessibilityLabel(Some(&raw_text(&format!(
                "{} {}",
                crate::i18n::translate(&data.name),
                crate::i18n::translate(&data.speed)
            ))));
            let controllable = data.range.is_some();
            card.choice.setEnabled(enabled);
            for index in 1..FanChoice::ALL.len() {
                if let Some(item) = card.choice.itemAtIndex(index as isize) {
                    item.setEnabled(controllable && fresh);
                }
            }
            card.choice.setAccessibilityLabel(Some(&raw_text(&format!(
                "{} {}",
                crate::i18n::translate(&data.name),
                crate::i18n::translate("散热方式")
            ))));
            if card.shown != Some(data.choice) {
                if let Some(index) = FanChoice::ALL.iter().position(|c| *c == data.choice) {
                    card.choice.selectItemAtIndex(index as isize);
                }
                card.shown = Some(data.choice);
            }
            let choice = FanChoice::ALL
                .get(card.choice.indexOfSelectedItem().max(0) as usize)
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
        let tint = tone_color(view.notice.tone);
        let fill = if attention {
            tint.colorWithAlphaComponent(crate::popover::tokens::TINT_ALPHA)
        } else {
            NSColor::quaternarySystemFillColor()
        };
        self.notice_box.set_fill(&fill);
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
