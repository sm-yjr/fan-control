//! Menu bar panel: the everyday surface for checking temperature and picking
//! one cooling plan for every fan. Built from system controls in layered cards
//! with conditional disclosure by active cooling mode.
use crate::{
    components::{surface_box, vstack, StatusBadge, SurfaceView},
    presenter::{Action, Plan, Presentation, Tone},
    theme::{self, layout, spacing, typography, SurfaceStyle},
    ui::{raw_text, text},
};
use objc2::{rc::Retained, runtime::AnyObject, sel, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSEdgeInsets, NSSize};
use std::cell::Cell;

pub(crate) mod tokens {
    pub const INSET: f64 = crate::theme::spacing::LG;
    pub const DASHBOARD_CAPTION: f64 = crate::theme::typography::SMALL;
    pub const PLOT_LABEL: f64 = crate::theme::typography::MICRO;
    pub const GAUGE_TRACK: f64 = crate::theme::layout::GAUGE_TRACK;
    pub const BODY: f64 = crate::theme::typography::BODY;
    pub const CAPTION: f64 = crate::theme::typography::CAPTION;
    pub const GAP: f64 = crate::theme::spacing::SM;
}

pub(crate) fn tone_color(tone: Tone) -> Retained<NSColor> {
    theme::tone_color(tone)
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

/// Native diagnostic: both the label frame and its required text height must
/// fit after the final panel size is applied, including wrapping translations.
pub(crate) fn verify_label_geometry(label: &NSTextField, container: &NSView) {
    let frame =
        label.convertRect_toView(label.alignmentRectForFrame(label.bounds()), Some(container));
    let bounds = container.bounds();
    assert!(
        frame.origin.x >= bounds.origin.x - 1. && frame.origin.y >= bounds.origin.y - 1.,
        "label outside panel: {} frame={frame:?} bounds={bounds:?}",
        label.stringValue()
    );
    assert!(
        frame.origin.x + frame.size.width <= bounds.origin.x + bounds.size.width + 1.
            && frame.origin.y + frame.size.height <= bounds.origin.y + bounds.size.height + 1.,
        "label outside panel: {} frame={frame:?} bounds={bounds:?}",
        label.stringValue()
    );
    let required = label.intrinsicContentSize();
    assert!(
        label.bounds().size.height + 1. >= required.height
            && label.bounds().size.width + 1. >= required.width,
        "label text clipped: {} frame={frame:?} required={required:?}",
        label.stringValue()
    );
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
    let image = theme::symbol_image(symbol, Some(label), 14.0)
        .or_else(|| {
            NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &raw_text(symbol),
                Some(&text(label)),
            )
        })
        .expect("system symbol");
    let button =
        unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    button.setBordered(false);
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    button.setToolTip(Some(&text(label)));
    button.setAccessibilityLabel(Some(&text(label)));
    button
}

/// All dashboard inputs belong to the same presentation refresh. Size is
/// committed only after both the controls and sampled readings are updated.
pub(crate) struct DashboardUpdate<'a> {
    pub cards: &'a [crate::presenter::FanCard],
    pub history: &'a crate::trend::History,
    pub bias: i8,
    pub pending: Option<i8>,
    pub saving: bool,
}

pub(crate) struct MenuPanel {
    pub popover: Retained<NSPopover>,
    root: Retained<NSStackView>,
    badge: StatusBadge,
    dashboard: crate::dashboard::Dashboard,
    preference_section: Retained<NSStackView>,
    pub preference: Retained<NSSlider>,
    preference_value: Retained<NSTextField>,
    preference_reset: Retained<NSButton>,
    preference_notice: Retained<NSTextField>,
    material: Retained<NSVisualEffectView>,
    pub plan: Retained<NSSegmentedControl>,
    description: Retained<NSTextField>,
    custom: Retained<NSStackView>,
    pub slider: Retained<NSSlider>,
    percent: Retained<NSTextField>,
    curve: Retained<NSButton>,
    card: Retained<SurfaceView>,
    card_title: Retained<NSTextField>,
    card_body: Retained<NSTextField>,
    card_action: Retained<NSButton>,
    shown_plan: Option<Plan>,
    size_revision: Cell<u64>,
}

impl MenuPanel {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        Self::with_width(mtm, target, layout::PANEL_WIDTH)
    }

    pub fn with_width(mtm: MainThreadMarker, target: &AnyObject, panel_width: f64) -> Self {
        use tokens::*;
        let content_width = panel_width - INSET * 2.;
        let card_pad = 12.;
        let control_inner = content_width - card_pad * 2.;
        let root = vstack(mtm, spacing::MD);
        root.setEdgeInsets(NSEdgeInsets {
            top: INSET,
            left: INSET,
            bottom: INSET - 2.,
            right: INSET,
        });
        root.setTranslatesAutoresizingMaskIntoConstraints(false);
        root.widthAnchor()
            .constraintEqualToConstant(panel_width)
            .setActive(true);

        // Header: product identity and semantic icon+text status pill.
        let header = stack(
            mtm,
            NSUserInterfaceLayoutOrientation::Horizontal,
            spacing::SM,
        );
        header
            .widthAnchor()
            .constraintEqualToConstant(content_width)
            .setActive(true);
        if let Some(icon) = theme::symbol_image("fanblades.fill", None, 16.0)
            .or_else(|| theme::symbol_image("fanblades", None, 16.0))
        {
            let img = NSImageView::imageViewWithImage(&icon, mtm);
            img.setContentTintColor(Some(&NSColor::controlAccentColor()));
            header.addArrangedSubview(&img);
        }
        let brand = styled_label(mtm, "Fan Control", 16., true, false, None);
        brand.setFont(Some(&NSFont::systemFontOfSize_weight(16., unsafe {
            NSFontWeightSemibold
        })));
        header.addArrangedSubview(&brand);
        header.addArrangedSubview(&crate::dashboard::spacer(mtm));
        let badge = StatusBadge::new(mtm, "正在检测…", Tone::Info);
        header.addArrangedSubview(&badge.view);
        root.addArrangedSubview(&header);

        let dashboard = crate::dashboard::Dashboard::new(mtm, content_width);
        root.addArrangedSubview(&dashboard.view);

        // Problem banner, shown only when attention or action is required.
        let card_stack = vstack(mtm, 4.);
        let card_title = styled_label(mtm, "", BODY, true, false, Some(control_inner));
        let card_body = styled_label(
            mtm,
            "",
            typography::SMALL,
            false,
            false,
            Some(control_inner),
        );
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
        let card = surface_box(
            mtm,
            &card_stack,
            NSSize::new(card_pad, 10.),
            SurfaceStyle::Banner(Tone::Info),
        );
        card.widthAnchor()
            .constraintEqualToConstant(content_width)
            .setActive(true);
        root.addArrangedSubview(&card);

        // Layered cooling control card with mode-based conditional disclosure.
        let control_stack = vstack(mtm, spacing::SM);
        control_stack.addArrangedSubview(&styled_label(
            mtm,
            "散热方式",
            typography::CAPTION,
            true,
            true,
            None,
        ));
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
        if plan.intrinsicContentSize().width > control_inner {
            for index in 0..3 {
                plan.setImage_forSegment(None, index);
            }
        }
        if plan.intrinsicContentSize().width > control_inner {
            plan.setControlSize(NSControlSize::Small);
            plan.setFont(Some(&NSFont::systemFontOfSize(
                NSFont::smallSystemFontSize(),
            )));
        }
        plan.setSegmentDistribution(NSSegmentDistribution::FillEqually);
        plan.widthAnchor()
            .constraintEqualToConstant(control_inner)
            .setActive(true);
        plan.setAccessibilityLabel(Some(&text("散热方式")));
        control_stack.addArrangedSubview(&plan);

        // Custom fixed-speed controls (only visible in Plan::Custom).
        let custom = vstack(mtm, 4.);
        let speed_row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, GAP);
        speed_row.addArrangedSubview(&styled_label(mtm, "风扇速度", BODY, false, false, None));
        speed_row.addArrangedSubview(&crate::dashboard::spacer(mtm));
        let percent = styled_label(mtm, "40%", BODY, true, false, None);
        percent.setFont(Some(&theme::rounded_digits(BODY, unsafe {
            NSFontWeightSemibold
        })));
        speed_row.addArrangedSubview(&percent);
        speed_row
            .widthAnchor()
            .constraintEqualToConstant(control_inner)
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
            .constraintEqualToConstant(control_inner)
            .setActive(true);
        slider.setAccessibilityLabel(Some(&text("风扇速度")));
        slider.setAccessibilityHelp(Some(&text(
            "0% 为硬件最低转速，100% 为最高转速。松开后生效。",
        )));
        let range = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, GAP);
        range.addArrangedSubview(&styled_label(
            mtm,
            "最安静",
            typography::SMALL,
            false,
            true,
            None,
        ));
        range.addArrangedSubview(&crate::dashboard::spacer(mtm));
        range.addArrangedSubview(&styled_label(
            mtm,
            "最凉爽",
            typography::SMALL,
            false,
            true,
            None,
        ));
        range
            .widthAnchor()
            .constraintEqualToConstant(control_inner)
            .setActive(true);
        custom.addArrangedSubview(&speed_row);
        custom.addArrangedSubview(&slider);
        custom.addArrangedSubview(&range);
        control_stack.addArrangedSubview(&custom);

        let curve = link_button(mtm, "按温度自动调节（温度曲线）…", target, sel!(useCurve:));
        curve.setFont(Some(&NSFont::systemFontOfSize(typography::CAPTION)));
        control_stack.addArrangedSubview(&curve);

        // Smart cooling 21-step preference section (disclosed only in Plan::Smart).
        let preference_section = vstack(mtm, 6.);
        let selection = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 6.);
        selection
            .widthAnchor()
            .constraintEqualToConstant(control_inner)
            .setActive(true);
        selection.addArrangedSubview(&styled_label(
            mtm,
            "散热偏好",
            typography::CAPTION,
            true,
            false,
            None,
        ));
        selection.addArrangedSubview(&crate::dashboard::spacer(mtm));
        let pref_size = if control_inner < 300. {
            10.0
        } else {
            typography::SMALL
        };
        let preference_value = styled_label(mtm, "当前选择：默认", pref_size, false, true, None);
        let pref_pill = surface_box(
            mtm,
            &preference_value,
            NSSize::new(6., 2.),
            SurfaceStyle::NeutralPill,
        );
        selection.addArrangedSubview(&pref_pill);
        preference_section.addArrangedSubview(&selection);

        let preference = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(
                0.,
                -10.,
                10.,
                Some(target),
                Some(sel!(panelCoolingPreference:)),
                mtm,
            )
        };
        preference.setNumberOfTickMarks(21);
        preference.setAllowsTickMarkValuesOnly(true);
        preference.setContinuous(false);
        preference
            .widthAnchor()
            .constraintEqualToConstant(control_inner)
            .setActive(true);
        preference.setAccessibilityLabel(Some(&text("散热偏好")));
        preference.setAccessibilityHelp(Some(&text("21 档散热偏好，默认在中间。更凉快每档增加 2 个散热需求百分点，最多增加 20 个百分点，上限 100%。高档位可在空闲时保持主动散热；恢复默认后可随冷却交还系统。")));
        preference.setToolTip(preference.accessibilityHelp().as_deref());
        preference_section.addArrangedSubview(&preference);

        let endpoints = NSView::new(mtm);
        endpoints
            .widthAnchor()
            .constraintEqualToConstant(control_inner)
            .setActive(true);
        endpoints
            .heightAnchor()
            .constraintEqualToConstant(16.)
            .setActive(true);
        for (label_text, position) in [("更安静", -1), ("默认", 0), ("更凉快", 1)] {
            let field = styled_label(mtm, label_text, typography::SMALL, false, true, None);
            field.setTranslatesAutoresizingMaskIntoConstraints(false);
            endpoints.addSubview(&field);
            field
                .centerYAnchor()
                .constraintEqualToAnchor(&endpoints.centerYAnchor())
                .setActive(true);
            match position {
                -1 => field
                    .leadingAnchor()
                    .constraintEqualToAnchor(&endpoints.leadingAnchor())
                    .setActive(true),
                0 => field
                    .centerXAnchor()
                    .constraintEqualToAnchor(&endpoints.centerXAnchor())
                    .setActive(true),
                _ => field
                    .trailingAnchor()
                    .constraintEqualToAnchor(&endpoints.trailingAnchor())
                    .setActive(true),
            }
        }
        preference_section.addArrangedSubview(&endpoints);

        let preference_reset =
            link_button(mtm, "恢复默认", target, sel!(resetPanelCoolingPreference:));
        preference_reset.setFont(Some(&NSFont::systemFontOfSize(typography::SMALL)));
        preference_section.addArrangedSubview(&preference_reset);
        control_stack.addArrangedSubview(&preference_section);

        let description =
            styled_label(mtm, "", typography::SMALL, false, true, Some(control_inner));
        control_stack.addArrangedSubview(&description);

        let preference_notice =
            styled_label(mtm, "", DASHBOARD_CAPTION, false, true, Some(control_inner));
        preference_notice.setHidden(true);
        control_stack.addArrangedSubview(&preference_notice);

        let control_card = surface_box(
            mtm,
            &control_stack,
            NSSize::new(card_pad, 12.),
            SurfaceStyle::Card,
        );
        control_card
            .widthAnchor()
            .constraintEqualToConstant(content_width)
            .setActive(true);
        root.addArrangedSubview(&control_card);

        // Footer bar.
        let footer = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 12.);
        footer.addArrangedSubview(&link_button(mtm, "打开 Fan Control", target, sel!(show:)));
        footer.addArrangedSubview(&crate::dashboard::spacer(mtm));
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
            .constraintEqualToConstant(content_width)
            .setActive(true);
        root.addArrangedSubview(&footer);

        let controller = NSViewController::new(mtm);
        let material = NSVisualEffectView::new(mtm);
        material.setMaterial(NSVisualEffectMaterial::Popover);
        material.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        material.setState(NSVisualEffectState::FollowsWindowActiveState);
        material.addSubview(&root);
        crate::components::pin_edges(&root, &material);
        controller.setView(&material);
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setContentViewController(Some(&controller));
        Self {
            popover,
            root,
            badge,
            dashboard,
            preference_section,
            preference,
            preference_value,
            preference_reset,
            preference_notice,
            material,
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
            size_revision: Cell::new(0),
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
    pub fn refresh(
        &mut self,
        view: &Presentation,
        pending: Option<f64>,
        dashboard: DashboardUpdate<'_>,
    ) {
        let healthy = view.notice.tone == Tone::Good;
        let tint = tone_color(view.notice.tone);
        self.badge.set(
            view.notice.tone,
            match view.notice.tone {
                Tone::Good => "运行正常",
                Tone::Info => "提示",
                Tone::Warning => "需要注意",
                Tone::Danger => "温度偏高",
            },
        );
        self.card.setHidden(healthy);
        if !healthy {
            self.card.set_style(SurfaceStyle::Banner(view.notice.tone));
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
        let custom = view.plan == Plan::Custom;
        let smart = view.plan == Plan::Smart;
        self.custom.setHidden(!custom);
        self.curve.setHidden(!custom);
        self.preference_section.setHidden(!smart);

        let percent = pending.unwrap_or(view.custom_percent);
        if pending.is_none() {
            self.slider.setDoubleValue(percent);
        }
        self.slider.setEnabled(view.controls_enabled);
        self.set_percent_label(percent);

        let DashboardUpdate {
            cards,
            history,
            bias,
            pending,
            saving,
        } = dashboard;
        self.dashboard
            .refresh(view, cards, history, std::time::Instant::now());
        let shown = pending.unwrap_or(bias);
        self.preference.setDoubleValue(shown as f64);
        let tr = crate::i18n::translate;
        let value = if shown == 0 {
            tr("默认")
        } else {
            format!(
                "{} {}",
                tr(if shown < 0 { "更安静" } else { "更凉快" }),
                shown.unsigned_abs()
            )
        };
        self.preference_value
            .setStringValue(&raw_text(&format!("{}{}", tr("当前选择："), value)));
        self.preference.setEnabled(!saving);
        self.preference_reset.setEnabled(!saving && shown != 0);
        let description = if smart && shown > 0 {
            "按散热偏好加强散热；较高偏好可能在空闲时保持主动散热。恢复默认后可随冷却交还系统。"
        } else if custom && view.curve_active {
            "正在按温度曲线运行。拖动滑块会改为固定速度。"
        } else {
            view.plan_description
        };
        self.description.setStringValue(&text(description));
        if pending.is_some() {
            self.preference_notice
                .setStringValue(&text("正在保存散热偏好…"));
            self.preference_notice.setHidden(false);
        }
        self.refit();
    }
    fn refit(&self) {
        self.root.layoutSubtreeIfNeeded();
        let size = self.root.fittingSize();
        let current = self.popover.contentSize();
        if (size.height - current.height).abs() > 0.5 || (size.width - current.width).abs() > 0.5 {
            self.popover.setContentSize(size);
            self.size_revision.set(self.size_revision.get() + 1);
        }
    }
    pub fn preference_result(&self, result: Result<(), String>) {
        let failed = result.is_err();
        let color = if failed {
            NSColor::systemRedColor()
        } else {
            NSColor::secondaryLabelColor()
        };
        self.preference_notice.setTextColor(Some(&color));
        self.preference_notice.setStringValue(&text(&match result {
            Ok(()) => "散热偏好已保存，等待新采样评估。".into(),
            Err(e) => format!("保存失败：{e}"),
        }));
        self.preference_notice.setHidden(false);
    }
    pub fn clear_diagnostic_notice(&self) {
        self.preference_notice.setHidden(true);
        self.refit();
    }
    pub fn mark_diagnostic(&self) {
        let label = styled_label(
            self.root.mtm(),
            "界面验证 · 模拟数据",
            tokens::PLOT_LABEL,
            false,
            true,
            None,
        );
        self.root.addArrangedSubview(&label);
    }
    pub fn diagnostic_size_revision(&self) -> u64 {
        self.size_revision.get()
    }
    pub fn diagnostic_view(&self) -> &NSView {
        &self.material
    }
    pub fn verify_layout(&self, cards: &[crate::presenter::FanCard]) {
        self.root.layoutSubtreeIfNeeded();
        self.dashboard.verify_layout(cards);
        for label in [&self.badge.label, &self.description] {
            verify_label_geometry(label, &self.root);
        }
        if !self.card.isHidden() {
            for label in [&self.card_title, &self.card_body] {
                verify_label_geometry(label, &self.root);
            }
        }
        if !self.preference_notice.isHidden() {
            verify_label_geometry(&self.preference_notice, &self.root);
        }
        if !self.custom.isHidden() {
            verify_label_geometry(&self.percent, &self.root);
        }
        if !self.preference_section.isHidden() {
            verify_label_geometry(&self.preference_value, &self.root);
            let slider = self
                .preference
                .convertRect_toView(self.preference.bounds(), Some(&self.root));
            let label = self
                .preference_value
                .convertRect_toView(self.preference_value.bounds(), Some(&self.root));
            assert!(slider.size.width > 0. && slider.size.height > 0.);
            assert!(label.size.height > 0.);
        }
    }
    pub fn forget_plan(&mut self) {
        self.shown_plan = None;
    }
}
