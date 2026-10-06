//! Menu bar panel: the everyday surface for checking temperature and picking
//! one cooling plan for every fan. Built from system controls in stack views
//! so text length, language and Dynamic Type never clip.
use crate::presenter::{Action, Plan, Presentation, Tone};
use crate::ui::{raw_text, text};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, sel, DefinedClass, MainThreadOnly,
    Message,
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSEdgeInsets, NSRect, NSSize};
use std::cell::RefCell;

pub(crate) mod tokens {
    pub const PANEL_WIDTH: f64 = 320.;
    pub const INSET: f64 = 16.;
    pub const CONTENT_WIDTH: f64 = PANEL_WIDTH - INSET * 2.;
    pub const HERO: f64 = 34.;
    pub const HEADLINE: f64 = 14.;
    pub const BODY: f64 = 13.;
    pub const CAPTION: f64 = 12.;
    pub const GAP: f64 = 8.;
    pub const SECTION_GAP: f64 = 14.;
    pub const RADIUS: f64 = 8.;
    pub const TINT_ALPHA: f64 = 0.14;
}

pub(crate) fn tone_color(tone: Tone) -> Retained<NSColor> {
    match tone {
        Tone::Good => NSColor::systemGreenColor(),
        Tone::Info => NSColor::controlAccentColor(),
        Tone::Warning => NSColor::systemOrangeColor(),
        Tone::Danger => NSColor::systemRedColor(),
    }
}

pub(crate) fn stack(
    mtm: MainThreadMarker,
    orientation: NSUserInterfaceLayoutOrientation,
    spacing: f64,
) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::new(), mtm);
    stack.setOrientation(orientation);
    stack.setSpacing(spacing);
    stack.setAlignment(
        if orientation == NSUserInterfaceLayoutOrientation::Vertical {
            NSLayoutAttribute::Leading
        } else {
            NSLayoutAttribute::CenterY
        },
    );
    stack.setDetachesHiddenViews(true);
    // Stacks keep their content height instead of stretching to the window.
    stack.setHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultHigh,
        NSLayoutConstraintOrientation::Vertical,
    );
    stack
}

pub(crate) fn styled_label(
    mtm: MainThreadMarker,
    value: &str,
    size: f64,
    medium: bool,
    secondary: bool,
    width: Option<f64>,
) -> Retained<NSTextField> {
    let field = match width {
        Some(_) => NSTextField::wrappingLabelWithString(&text(value), mtm),
        None => NSTextField::labelWithString(&text(value), mtm),
    };
    let font = if medium {
        NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightMedium })
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    if secondary {
        field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    }
    // Stacks hug their content at high priority; text must never lose to that.
    field.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityRequired,
        NSLayoutConstraintOrientation::Vertical,
    );
    if let Some(width) = width {
        field.setPreferredMaxLayoutWidth(width);
        field
            .widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
    }
    field.setSelectable(false);
    field
}

pub(crate) struct TintIvars {
    fill: RefCell<Option<Retained<NSColor>>>,
}

define_class!(
    /// Rounded tinted background that sizes itself from its content's
    /// constraints, unlike NSBox whose content view ignores Auto Layout.
    #[unsafe(super=NSView)]
    #[thread_kind=MainThreadOnly]
    #[ivars=TintIvars]
    pub(crate) struct TintedView;
    impl TintedView {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            if let Some(fill) = self.ivars().fill.borrow().as_ref() {
                fill.setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                    self.bounds(),
                    tokens::RADIUS,
                    tokens::RADIUS,
                )
                .fill();
            }
        }
    }
);

impl TintedView {
    pub fn set_fill(&self, color: &NSColor) {
        self.ivars().fill.replace(Some(color.retain()));
        self.setNeedsDisplay(true);
    }
}

pub(crate) fn tinted_box(
    mtm: MainThreadMarker,
    content: &NSView,
    margin: NSSize,
) -> Retained<TintedView> {
    let this = TintedView::alloc(mtm).set_ivars(TintIvars {
        fill: RefCell::new(None),
    });
    let tile: Retained<TintedView> = unsafe { msg_send![super(this), init] };
    content.setTranslatesAutoresizingMaskIntoConstraints(false);
    tile.addSubview(content);
    for constraint in [
        content
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&tile.leadingAnchor(), margin.width),
        tile.trailingAnchor()
            .constraintEqualToAnchor_constant(&content.trailingAnchor(), margin.width),
        content
            .topAnchor()
            .constraintEqualToAnchor_constant(&tile.topAnchor(), margin.height),
        tile.bottomAnchor()
            .constraintEqualToAnchor_constant(&content.bottomAnchor(), margin.height),
    ] {
        constraint.setActive(true);
    }
    tile.setContentHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultHigh,
        NSLayoutConstraintOrientation::Vertical,
    );
    tile
}

pub(crate) fn link_button(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&text(title), Some(target), Some(action), mtm)
    };
    button.setBordered(false);
    button.setContentTintColor(Some(&NSColor::linkColor()));
    button.setFont(Some(&NSFont::systemFontOfSize(tokens::BODY)));
    button
}

fn symbol_button(
    mtm: MainThreadMarker,
    symbol: &str,
    label: &str,
    target: &AnyObject,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &raw_text(symbol),
        Some(&text(label)),
    )
    .expect("system symbol");
    let button =
        unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    button.setBordered(false);
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    button.setToolTip(Some(&text(label)));
    button.setAccessibilityLabel(Some(&text(label)));
    button
}

pub(crate) struct MenuPanel {
    pub popover: Retained<NSPopover>,
    root: Retained<NSStackView>,
    status_tile: Retained<TintedView>,
    status: Retained<NSTextField>,
    temperature: Retained<NSTextField>,
    caption: Retained<NSTextField>,
    fans: Retained<NSTextField>,
    pub plan: Retained<NSSegmentedControl>,
    description: Retained<NSTextField>,
    custom: Retained<NSStackView>,
    pub slider: Retained<NSSlider>,
    percent: Retained<NSTextField>,
    curve: Retained<NSButton>,
    card: Retained<TintedView>,
    card_title: Retained<NSTextField>,
    card_body: Retained<NSTextField>,
    card_action: Retained<NSButton>,
    shown_plan: Option<Plan>,
}

impl MenuPanel {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        use tokens::*;
        let root = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, GAP);
        root.setEdgeInsets(NSEdgeInsets {
            top: INSET,
            left: INSET,
            bottom: INSET - 4.,
            right: INSET,
        });
        root.setTranslatesAutoresizingMaskIntoConstraints(false);
        root.widthAnchor()
            .constraintEqualToConstant(PANEL_WIDTH)
            .setActive(true);

        // Header: product name and one-word health.
        let header = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, GAP);
        header
            .widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);
        if let Some(icon) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &raw_text("fanblades"),
            None,
        ) {
            header.addArrangedSubview(&NSImageView::imageViewWithImage(&icon, mtm));
        }
        header.addArrangedSubview(&styled_label(
            mtm,
            "Fan Control",
            HEADLINE,
            true,
            false,
            None,
        ));
        let spacer = NSView::new(mtm);
        spacer.setContentHuggingPriority_forOrientation(
            1.,
            NSLayoutConstraintOrientation::Horizontal,
        );
        header.addArrangedSubview(&spacer);
        let status = styled_label(mtm, "正在检测…", CAPTION, true, false, None);
        let status_tile = tinted_box(mtm, &status, NSSize::new(8., 2.));
        header.addArrangedSubview(&status_tile);
        root.addArrangedSubview(&header);
        root.setCustomSpacing_afterView(SECTION_GAP, &header);

        // Hero temperature with a plain-language trend.
        let hero = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, GAP);
        hero.setAlignment(NSLayoutAttribute::CenterY);
        hero.setHuggingPriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Vertical,
        );
        let temperature = styled_label(mtm, "--°", HERO, true, false, None);
        temperature.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            HERO,
            unsafe { NSFontWeightMedium },
        )));
        temperature
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(HERO * 1.25)
            .setActive(true);
        let caption = styled_label(mtm, "芯片温度", CAPTION, false, true, None);
        hero.addArrangedSubview(&temperature);
        hero.addArrangedSubview(&caption);
        root.addArrangedSubview(&hero);
        let fans = styled_label(
            mtm,
            "正在读取风扇…",
            CAPTION,
            false,
            true,
            Some(CONTENT_WIDTH),
        );
        root.addArrangedSubview(&fans);
        root.setCustomSpacing_afterView(SECTION_GAP, &fans);

        // Problem card, only shown when something needs attention.
        let card_stack = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 4.);
        let card_title = styled_label(mtm, "", BODY, true, false, Some(CONTENT_WIDTH - 24.));
        let card_body = styled_label(mtm, "", CAPTION, false, false, Some(CONTENT_WIDTH - 24.));
        let card_action = unsafe {
            NSButton::buttonWithTitle_target_action(
                &text("启用风扇控制"),
                Some(target),
                Some(sel!(noticeAction:)),
                mtm,
            )
        };
        card_action.setControlSize(NSControlSize::Small);
        card_stack.addArrangedSubview(&card_title);
        card_stack.addArrangedSubview(&card_body);
        card_stack.addArrangedSubview(&card_action);
        let card = tinted_box(mtm, &card_stack, NSSize::new(12., 10.));
        card.widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);
        root.addArrangedSubview(&card);
        root.setCustomSpacing_afterView(SECTION_GAP, &card);

        // One plan for every fan.
        root.addArrangedSubview(&styled_label(mtm, "散热方式", CAPTION, false, true, None));
        let labels: Vec<_> = ["系统默认", "智能散热", "自定义"]
            .iter()
            .map(|name| text(name))
            .collect();
        let plan = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&labels),
                NSSegmentSwitchTracking::SelectOne,
                Some(target),
                Some(sel!(planChanged:)),
                mtm,
            )
        };
        for (index, symbol) in ["apple.logo", "sparkles", "slider.horizontal.3"]
            .iter()
            .enumerate()
        {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &raw_text(symbol),
                None,
            );
            plan.setImage_forSegment(image.as_deref(), index as isize);
        }
        // Longer translations drop the symbols rather than overflow the panel.
        if plan.intrinsicContentSize().width > CONTENT_WIDTH {
            for index in 0..3 {
                plan.setImage_forSegment(None, index);
            }
        }
        if plan.intrinsicContentSize().width > CONTENT_WIDTH {
            plan.setControlSize(NSControlSize::Small);
            plan.setFont(Some(&NSFont::systemFontOfSize(
                NSFont::smallSystemFontSize(),
            )));
        }
        plan.setSegmentDistribution(NSSegmentDistribution::FillEqually);
        plan.widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);
        plan.setAccessibilityLabel(Some(&text("散热方式")));
        root.addArrangedSubview(&plan);
        let description = styled_label(mtm, "", CAPTION, false, true, Some(CONTENT_WIDTH));
        root.addArrangedSubview(&description);

        let custom = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 4.);
        let speed_row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, GAP);
        speed_row.addArrangedSubview(&styled_label(mtm, "风扇速度", BODY, false, false, None));
        let speed_spacer = NSView::new(mtm);
        speed_spacer.setContentHuggingPriority_forOrientation(
            1.,
            NSLayoutConstraintOrientation::Horizontal,
        );
        speed_row.addArrangedSubview(&speed_spacer);
        let percent = styled_label(mtm, "40%", BODY, true, false, None);
        percent.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            BODY,
            unsafe { NSFontWeightMedium },
        )));
        speed_row.addArrangedSubview(&percent);
        speed_row
            .widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);
        let slider = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(
                40.,
                0.,
                100.,
                Some(target),
                Some(sel!(customSpeed:)),
                mtm,
            )
        };
        slider.setNumberOfTickMarks(5);
        slider.setAllowsTickMarkValuesOnly(false);
        slider
            .widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);
        slider.setAccessibilityLabel(Some(&text("风扇速度")));
        slider.setAccessibilityHelp(Some(&text(
            "0% 为硬件最低转速，100% 为最高转速。松开后生效。",
        )));
        let range = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, GAP);
        range.addArrangedSubview(&styled_label(mtm, "最安静", CAPTION, false, true, None));
        let range_spacer = NSView::new(mtm);
        range_spacer.setContentHuggingPriority_forOrientation(
            1.,
            NSLayoutConstraintOrientation::Horizontal,
        );
        range.addArrangedSubview(&range_spacer);
        range.addArrangedSubview(&styled_label(mtm, "最凉爽", CAPTION, false, true, None));
        range
            .widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);
        custom.addArrangedSubview(&speed_row);
        custom.addArrangedSubview(&slider);
        custom.addArrangedSubview(&range);
        root.addArrangedSubview(&custom);
        let curve = link_button(mtm, "按温度自动调节（温度曲线）…", target, sel!(useCurve:));
        root.addArrangedSubview(&curve);

        // Footer.
        let separator = NSBox::new(mtm);
        separator.setBoxType(NSBoxType::Separator);
        separator
            .widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);
        root.addArrangedSubview(&separator);
        root.setCustomSpacing_afterView(4., &separator);
        let footer = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 12.);
        footer.addArrangedSubview(&link_button(mtm, "打开 Fan Control", target, sel!(show:)));
        let footer_spacer = NSView::new(mtm);
        footer_spacer.setContentHuggingPriority_forOrientation(
            1.,
            NSLayoutConstraintOrientation::Horizontal,
        );
        footer.addArrangedSubview(&footer_spacer);
        footer.addArrangedSubview(&symbol_button(
            mtm,
            "gearshape",
            "设置",
            target,
            sel!(preferences:),
        ));
        footer.addArrangedSubview(&symbol_button(
            mtm,
            "power",
            "退出 Fan Control",
            target,
            sel!(quit:),
        ));
        footer
            .widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);
        root.addArrangedSubview(&footer);

        let controller = NSViewController::new(mtm);
        controller.setView(&root);
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setContentViewController(Some(&controller));
        Self {
            popover,
            root,
            status_tile,
            status,
            temperature,
            caption,
            fans,
            plan,
            description,
            custom,
            slider,
            percent,
            curve,
            card,
            card_title,
            card_body,
            card_action,
            shown_plan: None,
        }
    }

    pub fn show(&self, button: &NSStatusBarButton) {
        if self.popover.isShown() {
            self.close();
        } else {
            self.popover.showRelativeToRect_ofView_preferredEdge(
                button.bounds(),
                button,
                objc2_foundation::NSRectEdge::MinY,
            );
        }
    }

    pub fn close(&self) {
        unsafe { self.popover.performClose(None) };
    }

    pub fn set_percent_label(&self, percent: f64) {
        self.percent
            .setStringValue(&raw_text(&format!("{percent:.0}%")));
    }

    /// `pending` keeps a slider drag from being overwritten by the last saved value.
    pub fn refresh(&mut self, view: &Presentation, pending: Option<f64>) {
        let healthy = view.notice.tone == Tone::Good;
        let tint = tone_color(view.notice.tone);
        self.status.setStringValue(&text(match view.notice.tone {
            Tone::Good => "运行正常",
            Tone::Info => "提示",
            Tone::Warning => "需要注意",
            Tone::Danger => "温度偏高",
        }));
        self.status.setTextColor(Some(&tint));
        self.status_tile
            .set_fill(&tint.colorWithAlphaComponent(tokens::TINT_ALPHA));
        self.temperature.setStringValue(&raw_text(
            &view
                .temperature
                .map(|value| format!("{value:.0}°"))
                .unwrap_or_else(|| "--°".into()),
        ));
        let tr = crate::i18n::translate;
        self.caption.setStringValue(&raw_text(&format!(
            "{} · {}",
            tr("芯片温度"),
            tr(view.trend)
        )));
        let fans = view
            .fans
            .iter()
            .map(|fan| format!("{} {} · {}", tr(&fan.name), tr(&fan.speed), tr(&fan.detail)))
            .collect::<Vec<_>>()
            .join("\n");
        self.fans.setStringValue(&raw_text(&if fans.is_empty() {
            tr("没有可显示的风扇")
        } else {
            fans
        }));

        self.card.setHidden(healthy);
        if !healthy {
            self.card
                .set_fill(&tint.colorWithAlphaComponent(tokens::TINT_ALPHA));
            self.card_title.setStringValue(&text(view.notice.title));
            self.card_title.setTextColor(Some(&tint));
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
        }

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
        self.description
            .setStringValue(&text(view.plan_description));
        let custom = view.plan == Plan::Custom;
        self.custom.setHidden(!custom);
        self.curve.setHidden(!custom);
        let percent = pending.unwrap_or(view.custom_percent);
        if pending.is_none() {
            self.slider.setDoubleValue(percent);
        }
        self.slider.setEnabled(view.controls_enabled);
        self.set_percent_label(percent);
        if custom && view.curve_active {
            self.description
                .setStringValue(&text("正在按温度曲线运行。拖动滑块会改为固定速度。"));
        }
        // Hidden sections shrink the panel instead of leaving blank space.
        self.root.layoutSubtreeIfNeeded();
        let size = self.root.fittingSize();
        let current = self.popover.contentSize();
        if (size.height - current.height).abs() > 0.5 || (size.width - current.width).abs() > 0.5 {
            self.popover.setContentSize(size);
        }
    }

    /// Forces the next refresh to re-select the plan after a user change is rejected.
    pub fn forget_plan(&mut self) {
        self.shown_plan = None;
    }
}
