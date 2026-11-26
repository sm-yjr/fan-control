//! Settings page: fluid System Settings-style grouped cards for general options,
//! menu bar readout, the fan control service and support.
use crate::{
    components::{hstack, SurfaceView},
    form::{self, fluid_caption, fluid_group, fluid_page, fluid_row, section_title},
    status_bar::MenuBarDisplayMode,
    theme::spacing,
    ui::text,
};
use objc2::{rc::Retained, runtime::AnyObject, sel, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::MainThreadMarker;

pub(crate) struct SettingsPage {
    pub view: Retained<NSStackView>,
    pub login: Retained<NSSwitch>,
    pub message: Retained<NSTextField>,
    service: form::Row,
    pub install: Retained<NSButton>,
    pub configuration_notice: Retained<NSTextField>,
    pub update: Retained<NSButton>,
    /// Groups followed by a caption that is hidden when empty.
    captioned: [(Retained<SurfaceView>, Retained<NSTextField>); 2],
}

fn push(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    unsafe {
        NSButton::buttonWithTitle_target_action(&text(title), Some(target), Some(action), mtm)
    }
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

impl SettingsPage {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, demo: bool) -> Self {
        let view = fluid_page(mtm);

        view.addArrangedSubview(&section_title(mtm, "通用"));
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
        let login_row = fluid_row(
            mtm,
            "登录时启动",
            "开机登录后自动在菜单栏运行，继续使用你的散热设置。",
            &[&login],
        );

        let menu_bar_display = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            crate::ui::rect(0., 0., 170., 26.),
            false,
        );
        for mode in MenuBarDisplayMode::ALL {
            menu_bar_display.addItemWithTitle(&text(mode.title()));
        }
        let current_display = MenuBarDisplayMode::load(demo);
        if let Some(index) = MenuBarDisplayMode::ALL
            .iter()
            .position(|m| *m == current_display)
        {
            menu_bar_display.selectItemAtIndex(index as isize);
        }
        menu_bar_display.setAccessibilityLabel(Some(&text("菜单栏显示")));
        unsafe {
            menu_bar_display.setTarget(Some(target));
            menu_bar_display.setAction(Some(sel!(menuBarDisplay:)));
        }
        let menu_bar_row = fluid_row(
            mtm,
            "菜单栏显示",
            "在状态栏图标旁可选展示实时芯片温度或最高转速。",
            &[&menu_bar_display],
        );

        let language = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            crate::ui::rect(0., 0., 170., 26.),
            false,
        );
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
        let language_row = fluid_row(mtm, "界面语言", "重新打开应用后生效。", &[&language]);
        let general = fluid_group(
            mtm,
            &[&login_row.view, &menu_bar_row.view, &language_row.view],
        );
        view.addArrangedSubview(&general);
        general
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);
        let message = fluid_caption(
            mtm,
            &login_status
                .map(|_| String::new())
                .unwrap_or_else(|error| error),
        );
        view.addArrangedSubview(&message);
        view.setCustomSpacing_afterView(form::SECTION_GAP, &message);

        view.addArrangedSubview(&section_title(mtm, "风扇控制服务"));
        let install = push(mtm, "启用…", target, sel!(install:));
        install.setEnabled(!demo);
        let retry = push(mtm, "重新检测", target, sel!(retry:));
        let service = fluid_row(mtm, "正在检查…", "", &[&install, &retry]);
        service
            .title
            .setFont(Some(&NSFont::systemFontOfSize_weight(form::BODY, unsafe {
                NSFontWeightSemibold
            })));
        let reset_row = fluid_row(
            mtm,
            "立即交还系统",
            "所有风扇马上回到 macOS 自动控制，直到你再次选择散热方式。",
            &[&push(mtm, "全部交还", target, sel!(reset:))],
        );
        let uninstall = destructive(mtm, "移除…", target);
        uninstall.setEnabled(!demo);
        let uninstall_row = fluid_row(
            mtm,
            "移除控制服务",
            "需要管理员授权。移除后仍可查看温度，你的设置会保留。",
            &[&uninstall],
        );
        let service_group =
            fluid_group(mtm, &[&service.view, &reset_row.view, &uninstall_row.view]);
        view.addArrangedSubview(&service_group);
        service_group
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);
        let configuration_notice = fluid_caption(mtm, "");
        view.addArrangedSubview(&configuration_notice);
        view.setCustomSpacing_afterView(form::SECTION_GAP, &configuration_notice);

        view.addArrangedSubview(&section_title(mtm, "支持"));
        let update = push(mtm, "检查更新…", target, sel!(updates:));
        let version = crate::app_version::current();
        let update_row = fluid_row(
            mtm,
            "软件更新",
            &format!("当前版本 {}（{}）", version.version, version.build),
            &[&update],
        );
        let export_row = fluid_row(
            mtm,
            "诊断信息",
            "遇到问题时导出诊断文件，附在反馈里帮助排查。",
            &[&push(mtm, "导出…", target, sel!(export:))],
        );
        let help_buttons = hstack(mtm, spacing::SM);
        help_buttons.addArrangedSubview(&push(mtm, "使用帮助", target, sel!(help:)));
        help_buttons.addArrangedSubview(&push(mtm, "关于", target, sel!(about:)));
        let help_row = fluid_row(mtm, "帮助与关于", "", &[&help_buttons]);
        let support_group = fluid_group(mtm, &[&update_row.view, &export_row.view, &help_row.view]);
        view.addArrangedSubview(&support_group);
        support_group
            .widthAnchor()
            .constraintEqualToAnchor_constant(&view.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        Self {
            view,
            login,
            message: message.clone(),
            service,
            install,
            configuration_notice: configuration_notice.clone(),
            update,
            captioned: [
                (general, message.clone()),
                (service_group, configuration_notice),
            ],
        }
    }

    pub fn refresh(&self, demo: bool, installing: bool, ready: bool, notice: &str) {
        let (title, detail, color) = if demo {
            (
                "演示模式",
                "不连接控制服务，也不会改变风扇。",
                NSColor::labelColor(),
            )
        } else if installing {
            (
                "等待管理员授权",
                "在系统弹出的窗口中输入密码。",
                NSColor::systemOrangeColor(),
            )
        } else if ready {
            (
                "已启用",
                "风扇可以由本应用调节。应用退出、失联或电脑睡眠时会自动交还 macOS。",
                NSColor::systemGreenColor(),
            )
        } else {
            (
                "未启用",
                "目前只能查看温度。启用需要一次管理员授权。",
                NSColor::labelColor(),
            )
        };
        self.service.title.setStringValue(&text(title));
        form::set_detail(&self.service, detail);
        self.service.title.setTextColor(Some(&color));
        self.message
            .setHidden(self.message.stringValue().length() == 0);
        self.install.setHidden(demo || ready || installing);
        self.configuration_notice.setStringValue(&text(notice));
        self.configuration_notice.setHidden(notice.is_empty());
        for (group, caption) in &self.captioned {
            self.view.setCustomSpacing_afterView(
                if caption.isHidden() {
                    form::SECTION_GAP
                } else {
                    8.
                },
                group,
            );
        }
    }
}
