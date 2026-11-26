//! Settings page styled after modern macOS System Settings: visual menu-bar
//! picker cards with selection rings, uncluttered single-line grouped rows,
//! borderless inline pickers, chevron actions, and `(i)` footnotes.
use crate::{
    components::{chevron_button, hstack, info_footnote, label, vstack, StatusBadge},
    form::{
        self, fluid_caption, fluid_group, fluid_page, settings_row, settings_section_title,
        PAGE_INSET,
    },
    presenter::Tone,
    status_bar::MenuBarDisplayMode,
    theme::{self, spacing, typography},
    ui::{raw_text, text},
};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, sel, AnyThread, DefinedClass,
    MainThreadOnly, Message,
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};

struct PreviewTileIvars {
    mode: Cell<MenuBarDisplayMode>,
    selected: Cell<bool>,
    target: RefCell<Option<Retained<AnyObject>>>,
}

define_class!(
    /// Visual macOS Settings-style thumbnail card for selecting menu bar display mode.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = PreviewTileIvars]
    struct MenuBarPreviewCard;
    impl MenuBarPreviewCard {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            let idx = MenuBarDisplayMode::ALL
                .iter()
                .position(|m| *m == self.ivars().mode.get())
                .unwrap_or(0) as isize;
            if let Some(target) = self.ivars().target.borrow().as_ref() {
                let _: () = unsafe { msg_send![&**target, menuBarTileSelected: idx] };
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            let b = self.bounds();
            let selected = self.ivars().selected.get();

            // Outer blue double-ring when selected (matches macOS Settings Appearance tiles).
            if selected {
                let ring_rect = NSRect::new(
                    NSPoint::new(b.origin.x + 1.5, b.origin.y + 1.5),
                    NSSize::new((b.size.width - 3.0).max(0.), (b.size.height - 3.0).max(0.)),
                );
                let ring = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(ring_rect, 13.0, 13.0);
                ring.setLineWidth(2.4);
                NSColor::controlAccentColor().setStroke();
                ring.stroke();
            }

            // Inner wallpaper thumbnail with blue macOS glass gradient.
            let pad = 4.5;
            let card_rect = NSRect::new(
                NSPoint::new(b.origin.x + pad, b.origin.y + pad),
                NSSize::new(
                    (b.size.width - pad * 2.0).max(0.),
                    (b.size.height - pad * 2.0).max(0.),
                ),
            );
            let card = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(card_rect, 9.5, 9.5);
            let top = NSColor::colorWithSRGBRed_green_blue_alpha(0.14, 0.57, 0.99, 1.0);
            let bottom = NSColor::colorWithSRGBRed_green_blue_alpha(0.04, 0.35, 0.80, 1.0);
            if let Some(grad) =
                NSGradient::initWithStartingColor_endingColor(NSGradient::alloc(), &top, &bottom)
            {
                grad.drawInBezierPath_angle(&card, -65.0);
            } else {
                top.setFill();
                card.fill();
            }

            // Soft diagonal light reflection ribbons inside the wallpaper.
            NSGraphicsContext::saveGraphicsState_class();
            card.addClip();
            let sheen = NSBezierPath::bezierPath();
            sheen.moveToPoint(NSPoint::new(card_rect.origin.x + card_rect.size.width * 0.22, card_rect.origin.y));
            sheen.lineToPoint(NSPoint::new(card_rect.origin.x + card_rect.size.width * 0.70, card_rect.origin.y + card_rect.size.height));
            sheen.lineToPoint(NSPoint::new(card_rect.origin.x + card_rect.size.width, card_rect.origin.y + card_rect.size.height));
            sheen.lineToPoint(NSPoint::new(card_rect.origin.x + card_rect.size.width * 0.62, card_rect.origin.y));
            sheen.closePath();
            NSColor::whiteColor().colorWithAlphaComponent(0.14).setFill();
            sheen.fill();

            let facet = NSBezierPath::bezierPath();
            facet.moveToPoint(NSPoint::new(card_rect.origin.x, card_rect.origin.y));
            facet.lineToPoint(NSPoint::new(card_rect.origin.x + card_rect.size.width * 0.36, card_rect.origin.y + card_rect.size.height));
            facet.lineToPoint(NSPoint::new(card_rect.origin.x, card_rect.origin.y + card_rect.size.height));
            facet.closePath();
            NSColor::whiteColor().colorWithAlphaComponent(0.08).setFill();
            facet.fill();

            // Translucent macOS menu bar strip across the top of the mini desktop.
            let bar_h = 22.0;
            let bar_rect = NSRect::new(
                NSPoint::new(card_rect.origin.x, card_rect.origin.y + card_rect.size.height - bar_h),
                NSSize::new(card_rect.size.width, bar_h),
            );
            NSColor::whiteColor().colorWithAlphaComponent(0.88).setFill();
            NSBezierPath::fillRect(bar_rect);

            // Mini desktop glass window card silhouette below menu bar.
            let win_rect = NSRect::new(
                NSPoint::new(card_rect.origin.x + 12.0, card_rect.origin.y + 6.0),
                NSSize::new(card_rect.size.width - 24.0, (card_rect.size.height - bar_h - 12.0).max(8.0)),
            );
            let win = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(win_rect, 5.0, 5.0);
            NSColor::whiteColor().colorWithAlphaComponent(0.24).setFill();
            win.fill();
            NSColor::whiteColor().colorWithAlphaComponent(0.32).setStroke();
            win.setLineWidth(0.5);
            win.stroke();
            NSGraphicsContext::restoreGraphicsState_class();

            // Subtle inner border around thumbnail.
            NSColor::blackColor().colorWithAlphaComponent(0.10).setStroke();
            card.setLineWidth(0.5);
            card.stroke();
        }
    }
);

impl MenuBarPreviewCard {
    fn new(
        mtm: MainThreadMarker,
        mode: MenuBarDisplayMode,
        selected: bool,
        target: &AnyObject,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PreviewTileIvars {
            mode: Cell::new(mode),
            selected: Cell::new(selected),
            target: RefCell::new(Some(target.retain())),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), init] };
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        view.widthAnchor()
            .constraintEqualToConstant(116.0)
            .setActive(true);
        view.heightAnchor()
            .constraintEqualToConstant(62.0)
            .setActive(true);
        view.setAccessibilityElement(true);
        view.setAccessibilityLabel(Some(&text(mode.title())));

        // Mini menu bar live pill preview anchored inside the top frosted bar.
        let pill_row = hstack(mtm, 3.0);
        pill_row.setTranslatesAutoresizingMaskIntoConstraints(false);
        if let Some(icon) = theme::symbol_image("fanblades.fill", None, 9.5) {
            let img = NSImageView::imageViewWithImage(&icon, mtm);
            img.setContentTintColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
                0.10, 0.12, 0.16, 0.90,
            )));
            pill_row.addArrangedSubview(&img);
        }
        let sample = match mode {
            MenuBarDisplayMode::IconOnly => "",
            MenuBarDisplayMode::Temperature => "54°",
            MenuBarDisplayMode::FanSpeed => "2.4k",
            MenuBarDisplayMode::TemperatureAndFan => "54°·2.4k",
        };
        if !sample.is_empty() {
            let digits = NSTextField::labelWithString(&raw_text(sample), mtm);
            digits.setFont(Some(&theme::rounded_digits(9.5, unsafe {
                NSFontWeightSemibold
            })));
            digits.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
                0.10, 0.12, 0.16, 0.90,
            )));
            pill_row.addArrangedSubview(&digits);
        }
        view.addSubview(&pill_row);
        pill_row
            .centerXAnchor()
            .constraintEqualToAnchor(&view.centerXAnchor())
            .setActive(true);
        pill_row
            .topAnchor()
            .constraintEqualToAnchor_constant(&view.topAnchor(), 9.0)
            .setActive(true);
        view
    }

    fn set_selected(&self, selected: bool) {
        if self.ivars().selected.get() != selected {
            self.ivars().selected.set(selected);
            self.setNeedsDisplay(true);
        }
    }
}

struct PickerOption {
    mode: MenuBarDisplayMode,
    preview: Retained<MenuBarPreviewCard>,
    caption: Retained<NSTextField>,
}

pub(crate) struct SettingsPage {
    pub view: Retained<NSStackView>,
    pub login: Retained<NSSwitch>,
    pub message: Retained<NSTextField>,
    message_is_error: Cell<bool>,
    picker_options: Vec<PickerOption>,
    service_badge: StatusBadge,
    pub install: Retained<NSButton>,
    pub configuration_notice: Retained<NSTextField>,
    pub update: Retained<NSButton>,
}

fn push(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    let btn = unsafe {
        NSButton::buttonWithTitle_target_action(&text(title), Some(target), Some(action), mtm)
    };
    btn.setControlSize(NSControlSize::Regular);
    btn
}

fn subtle_action(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    let btn = unsafe {
        NSButton::buttonWithTitle_target_action(&text(title), Some(target), Some(action), mtm)
    };
    btn.setBordered(false);
    btn.setFont(Some(&NSFont::systemFontOfSize_weight(
        typography::BODY,
        unsafe { NSFontWeightRegular },
    )));
    btn.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    btn
}

fn destructive(mtm: MainThreadMarker, title: &str, target: &AnyObject) -> Retained<NSButton> {
    let button = push(mtm, title, target, sel!(uninstall:));
    button.setHasDestructiveAction(true);
    let red = NSColor::systemRedColor();
    let attributes = objc2_foundation::NSDictionary::from_slices(
        &[unsafe { NSForegroundColorAttributeName }],
        &[&*red as &AnyObject],
    );
    let title = unsafe {
        objc2_foundation::NSAttributedString::initWithString_attributes(
            <objc2_foundation::NSAttributedString as objc2::AllocAnyThread>::alloc(),
            &text(title),
            Some(&attributes),
        )
    };
    button.setAttributedTitle(&title);
    button
}

fn short_mode_label(mode: MenuBarDisplayMode) -> &'static str {
    match mode {
        MenuBarDisplayMode::IconOnly => "仅图标",
        MenuBarDisplayMode::Temperature => "芯片温度",
        MenuBarDisplayMode::FanSpeed => "最高转速",
        MenuBarDisplayMode::TemperatureAndFan => "温度与转速",
    }
}

impl SettingsPage {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, demo: bool) -> Self {
        let view = fluid_page(mtm);
        view.setSpacing(spacing::SM);

        // ── Section 1: 外观与通用（Visual Menu Bar Picker + Launch at Login + Language）──
        view.addArrangedSubview(&settings_section_title(mtm, "外观与通用", 0.0));

        let current_display = MenuBarDisplayMode::load(demo);
        let picker_block = vstack(mtm, 8.0);
        picker_block.addArrangedSubview(&label(
            mtm,
            "菜单栏显示",
            typography::CAPTION,
            unsafe { NSFontWeightMedium },
            true,
            false,
        ));
        let tiles_row = hstack(mtm, spacing::MD);
        tiles_row.setDistribution(NSStackViewDistribution::EqualSpacing);
        tiles_row.setAlignment(NSLayoutAttribute::Top);
        let mut picker_options = Vec::new();
        for mode in MenuBarDisplayMode::ALL {
            let selected = mode == current_display;
            let col = vstack(mtm, 5.0);
            col.setAlignment(NSLayoutAttribute::CenterX);
            let preview = MenuBarPreviewCard::new(mtm, mode, selected, target);
            let caption = label(
                mtm,
                short_mode_label(mode),
                typography::SMALL,
                if selected {
                    unsafe { NSFontWeightSemibold }
                } else {
                    unsafe { NSFontWeightRegular }
                },
                !selected,
                false,
            );
            caption.setAlignment(NSTextAlignment::Center);
            col.addArrangedSubview(&preview);
            col.addArrangedSubview(&caption);
            tiles_row.addArrangedSubview(&col);
            picker_options.push(PickerOption {
                mode,
                preview,
                caption,
            });
        }
        picker_block.addArrangedSubview(&tiles_row);
        tiles_row
            .widthAnchor()
            .constraintEqualToAnchor(&picker_block.widthAnchor())
            .setActive(true);

        let login = NSSwitch::new(mtm);
        unsafe {
            login.setTarget(Some(target));
            login.setAction(Some(sel!(login:)));
        }
        login.setEnabled(!demo);
        let login_status = if demo {
            Ok(false)
        } else {
            crate::settings::enabled()
        };
        login.setState(if login_status.as_ref().is_ok_and(|enabled| *enabled) {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        login.setAccessibilityLabel(Some(&text("登录时启动 Fan Control")));
        let (login_row, _) = settings_row(mtm, "登录时启动", &[&login]);

        // Borderless inline pop-up picker matching macOS Settings ("Always show ⌄").
        let language = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            crate::ui::rect(0., 0., 140., 24.),
            false,
        );
        language.setBordered(false);
        for title in ["跟随系统", "简体中文", "English"] {
            language.addItemWithTitle(&text(title));
        }
        language.selectItemAtIndex(match crate::i18n::override_language() {
            crate::i18n::Lang::System => 0,
            crate::i18n::Lang::Chinese => 1,
            crate::i18n::Lang::English => 2,
        });
        language.setEnabled(!demo);
        language.setAccessibilityLabel(Some(&text("界面语言")));
        unsafe {
            language.setTarget(Some(target));
            language.setAction(Some(sel!(language:)));
        }
        let (language_row, _) = settings_row(mtm, "界面语言", &[&language]);

        let general = fluid_group(mtm, &[&picker_block, &login_row, &language_row]);
        view.addArrangedSubview(&general);
        general
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -PAGE_INSET * 2.)
            .setActive(true);

        let (general_note_row, _) = info_footnote(
            mtm,
            "开机后自动在菜单栏运行；切换界面语言需重新打开应用生效。",
        );
        general_note_row.setEdgeInsets(NSEdgeInsets {
            top: 2.,
            left: 6.,
            bottom: 0.,
            right: 6.,
        });
        view.addArrangedSubview(&general_note_row);
        general_note_row
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -PAGE_INSET * 2.)
            .setActive(true);

        let message = fluid_caption(
            mtm,
            &login_status
                .map(|_| String::new())
                .unwrap_or_else(|error| error),
        );
        message.setTextColor(Some(&NSColor::systemOrangeColor()));
        message.setHidden(message.stringValue().length() == 0);
        view.addArrangedSubview(&message);
        message
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -PAGE_INSET * 2.)
            .setActive(true);
        view.setCustomSpacing_afterView(form::SECTION_GAP, &message);

        // ── Section 2: 风扇控制服务 ──
        view.addArrangedSubview(&settings_section_title(mtm, "风扇控制服务", 14.0));
        let service_badge = StatusBadge::new(mtm, "正在检查…", Tone::Info);
        let install = push(mtm, "启用…", target, sel!(install:));
        install.setEnabled(!demo);
        let retry = subtle_action(mtm, "重新检测", target, sel!(retry:));
        let (service_row, _) = settings_row(
            mtm,
            "控制服务状态",
            &[&service_badge.view, &install, &retry],
        );

        let reset_btn = push(mtm, "全部交还", target, sel!(reset:));
        let (reset_row, _) = settings_row(mtm, "立即交还系统", &[&reset_btn]);

        let uninstall = destructive(mtm, "移除…", target);
        uninstall.setEnabled(!demo);
        let (uninstall_row, _) = settings_row(mtm, "移除控制服务", &[&uninstall]);

        let service_group = fluid_group(mtm, &[&service_row, &reset_row, &uninstall_row]);
        view.addArrangedSubview(&service_group);
        service_group
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -PAGE_INSET * 2.)
            .setActive(true);

        let (service_note_row, _) = info_footnote(
            mtm,
            "应用退出、失联或电脑睡眠时会自动交还 macOS；移除服务后仍可查看温度，你的设置会保留。",
        );
        service_note_row.setEdgeInsets(NSEdgeInsets {
            top: 2.,
            left: 6.,
            bottom: 0.,
            right: 6.,
        });
        view.addArrangedSubview(&service_note_row);
        service_note_row
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -PAGE_INSET * 2.)
            .setActive(true);

        let configuration_notice = fluid_caption(mtm, "");
        configuration_notice.setHidden(true);
        view.addArrangedSubview(&configuration_notice);
        configuration_notice
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -PAGE_INSET * 2.)
            .setActive(true);
        view.setCustomSpacing_afterView(form::SECTION_GAP, &configuration_notice);

        // ── Section 3: 支持与关于 ──
        view.addArrangedSubview(&settings_section_title(mtm, "支持与关于", 14.0));
        let version = crate::app_version::current();
        let version_label = label(
            mtm,
            &format!("{} ({})", version.version, version.build),
            typography::CAPTION,
            unsafe { NSFontWeightRegular },
            true,
            false,
        );
        version_label.setFont(Some(&theme::rounded_digits(typography::CAPTION, unsafe {
            NSFontWeightRegular
        })));
        let update = push(mtm, "检查更新…", target, sel!(updates:));
        let (update_row, _) = settings_row(mtm, "软件更新", &[&version_label, &update]);

        let export_action = subtle_action(mtm, "导出…", target, sel!(export:));
        let export_chevron = chevron_button(mtm, "导出诊断信息", target, sel!(export:));
        let (export_row, _) = settings_row(mtm, "诊断信息", &[&export_action, &export_chevron]);

        let help_chevron = chevron_button(mtm, "使用帮助", target, sel!(help:));
        let (help_row, _) = settings_row(mtm, "使用帮助", &[&help_chevron]);

        let about_chevron = chevron_button(mtm, "关于 Fan Control", target, sel!(about:));
        let (about_row, _) = settings_row(mtm, "关于 Fan Control", &[&about_chevron]);

        let support_group = fluid_group(mtm, &[&update_row, &export_row, &help_row, &about_row]);
        view.addArrangedSubview(&support_group);
        support_group
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -PAGE_INSET * 2.)
            .setActive(true);

        let initial_error = message.stringValue().length() > 0;
        Self {
            view,
            login,
            message,
            message_is_error: Cell::new(initial_error),
            picker_options,
            service_badge,
            install,
            configuration_notice,
            update,
        }
    }

    pub fn set_menu_bar_mode(&self, active: MenuBarDisplayMode) {
        for option in &self.picker_options {
            let selected = option.mode == active;
            option.preview.set_selected(selected);
            option
                .caption
                .setFont(Some(&NSFont::systemFontOfSize_weight(
                    typography::SMALL,
                    if selected {
                        unsafe { NSFontWeightSemibold }
                    } else {
                        unsafe { NSFontWeightRegular }
                    },
                )));
            let color = if selected {
                NSColor::labelColor()
            } else {
                NSColor::secondaryLabelColor()
            };
            option.caption.setTextColor(Some(&color));
        }
    }

    pub fn set_general_message(&self, value: &str, is_error: bool) {
        self.message.setStringValue(&text(value));
        let color = if is_error {
            NSColor::systemOrangeColor()
        } else {
            NSColor::secondaryLabelColor()
        };
        self.message.setTextColor(Some(&color));
        self.message_is_error.set(is_error && !value.is_empty());
        self.message.setHidden(value.is_empty());
    }

    pub fn sync_login_state(&self, demo: bool) {
        if demo {
            return;
        }
        match crate::settings::enabled() {
            Ok(enabled) => {
                self.login.setState(if enabled {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                if self.message_is_error.get() {
                    self.set_general_message("", false);
                }
            }
            Err(error) => {
                self.login.setState(NSControlStateValueOff);
                self.set_general_message(&error, true);
            }
        }
    }

    pub fn refresh(&self, demo: bool, installing: bool, ready: bool, notice: &str) {
        let (tone, title) = if demo {
            (Tone::Info, "演示模式")
        } else if installing {
            (Tone::Warning, "等待管理员授权")
        } else if ready {
            (Tone::Good, "已启用")
        } else {
            (Tone::Info, "未启用")
        };
        self.service_badge.set(tone, title);
        self.message
            .setHidden(self.message.stringValue().length() == 0);
        self.install.setHidden(demo || ready || installing);
        self.configuration_notice.setStringValue(&text(notice));
        self.configuration_notice.setHidden(notice.is_empty());
    }
}
