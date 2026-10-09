//! Product text in Chinese and English, with an explicit catalog.
//!
//! Translate product labels/messages only. Raw fan/sensor names, SMC keys,
//! paths, URLs and editable values must bypass this function: a raw name can
//! coincidentally equal a catalog label. Templates keep opaque captures intact;
//! only designated product-status/value captures are translated recursively.

use std::sync::{
    atomic::{AtomicU8, Ordering},
    OnceLock,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    System,
    Chinese,
    English,
}

impl Lang {
    pub fn preference_key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Chinese => "zh",
            Self::English => "en",
        }
    }

    pub fn from_preference_key(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "zh" => Some(Self::Chinese),
            "en" => Some(Self::English),
            _ => None,
        }
    }
}

static OVERRIDE: AtomicU8 = AtomicU8::new(0);
static SYSTEM_LANGUAGE: OnceLock<Lang> = OnceLock::new();

/// Process-local override. `System` clears it; this never changes macOS settings.
/// The caller owns persistence and rebuilding already-created AppKit controls.
pub fn set_override(language: Lang) {
    OVERRIDE.store(
        match language {
            Lang::System => 0,
            Lang::Chinese => 1,
            Lang::English => 2,
        },
        Ordering::Relaxed,
    );
}

pub fn override_language() -> Lang {
    match OVERRIDE.load(Ordering::Relaxed) {
        1 => Lang::Chinese,
        2 => Lang::English,
        _ => Lang::System,
    }
}

pub fn current_language() -> Lang {
    match override_language() {
        Lang::System => system_language(),
        explicit => explicit,
    }
}

/// First supported language in macOS preferredLanguages; English is fallback.
/// Cached for this process. A system-language change takes effect after relaunch.
pub fn system_language() -> Lang {
    *SYSTEM_LANGUAGE.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            let preferred = objc2_foundation::NSLocale::preferredLanguages();
            let values: Vec<String> = preferred.iter().map(|value| value.to_string()).collect();
            language_from_preferences(values.iter().map(String::as_str))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Lang::English
        }
    })
}

/// Locale selection is pure so regional/script variants can be tested safely.
pub fn language_from_preferences<'a>(values: impl IntoIterator<Item = &'a str>) -> Lang {
    for value in values {
        let base = value.split(['-', '_']).next().unwrap_or("");
        if base.eq_ignore_ascii_case("zh") {
            return Lang::Chinese;
        }
        if base.eq_ignore_ascii_case("en") {
            return Lang::English;
        }
    }
    Lang::English
}

pub fn translate(value: &str) -> String {
    translate_for(current_language(), value)
}

/// Explicit language, independent of process-global overrides (useful for tests).
pub fn translate_for(language: Lang, value: &str) -> String {
    let language = if language == Lang::System {
        system_language()
    } else {
        language
    };
    translate_inner(language, value, 0)
}

struct Text {
    zh: &'static str,
    en: &'static str,
}

macro_rules! texts {
    ($(($zh:literal, $en:literal)),* $(,)?) => { &[$(Text { zh: $zh, en: $en }),*] };
}

const CATALOG: &[Text] = texts![
    ("散热偏好正在平滑调整。", "Cooling preference is adjusting gradually."),
    ("温度趋势", "Temperature trend"),
    ("风扇转速", "Fan RPM"),
    ("10 分钟前", "10 min ago"),
    ("现在", "Now"),
    ("界面验证 · 模拟数据", "UI verification · simulated data"),
    ("转速范围未知", "RPM range unavailable"),
    ("暂无温度数据", "Temperature unavailable"),
    ("正在保存散热偏好…", "Saving cooling preference…"),
    ("散热偏好已保存，等待新采样评估。", "Cooling preference saved; awaiting a fresh sample."),
    ("按散热偏好加强散热；较高偏好可能在空闲时保持主动散热。恢复默认后可随冷却交还系统。", "Cooling follows your preference. Higher settings may keep fans active while idle; Default allows control to return after cooling."),
    ("正在按散热偏好维持主动散热。恢复默认后可随冷却交还系统。", "Your cooling preference keeps the fans active. Default allows control to return after cooling."),
    ("21 档散热偏好，默认在中间。更凉快每档增加 2 个散热需求百分点，最多增加 20 个百分点，上限 100%。高档位可在空闲时保持主动散热；恢复默认后可随冷却交还系统。", "21 cooling settings with Default in the center. Each Cooler step adds 2 cooling-demand percentage points, up to 20 points, capped at 100%. Higher settings can keep fans active while idle; Default allows control to return after cooling."),
    ("无需校准。更安静会适度降低日常散热；更凉快每档增加 2 个散热需求百分点，最多增加 20 个百分点，上限 100%。高档位可在空闲时保持主动散热。高温保护始终优先。保存偏好不会切换风扇模式。", "No calibration required. Quieter gently reduces everyday cooling. Each Cooler step adds 2 cooling-demand percentage points, up to 20 points, capped at 100%. Higher settings may keep fans active while idle. Overheat protection takes priority. Saving does not change fan modes."),

    ("默认", "Default"),
    ("散热偏好必须为 -10...10", "Cooling preference must be -10...10"),
    ("散热偏好", "Cooling preference"),
    ("更安静", "Quieter"),
    ("更凉快", "Cooler"),
    ("正在保存智能策略…", "Saving smart cooling settings…"),
    ("保存未获确认，请重试。", "Save was not confirmed. Please try again."),
    ("21 档散热偏好，左侧更安静，右侧更凉快，默认在中间。保存后仅影响智能散热。", "21 cooling preference positions: quieter on the left, cooler on the right, default in the center. Applies to smart cooling after saving."),
    ("无需校准。更安静会适度降低日常散热，更凉快会加强散热；高温保护始终优先。内部温度不代表机壳表面实测温度。保存偏好不会切换风扇模式。", "No calibration needed. Quieter slightly reduces everyday cooling; cooler increases it. High-temperature protection always takes priority. Internal readings are not measured case temperatures. Saving does not switch fan modes."),
    ("GPU 温度探头", "GPU temperature probe"),
    ("CPU 能效区温度", "CPU efficiency area temperature"),
    ("CPU 温度探头", "CPU temperature probe"),
    ("CPU 超级核心区温度", "CPU super-core area temperature"),
    ("CPU 性能区温度", "CPU performance area temperature"),
    ("电源管理邻近温度", "Power management proximity temperature"),
    ("系统存储二极管温度", "System storage diode temperature"),
    ("CPU 芯片汇总温度", "CPU die aggregate temperature"),
    ("雷电接口二极管温度", "Thunderbolt diode temperature"),
    ("芯片散热器温度", "SoC heatsink temperature"),
    ("左侧雷电接口温度", "Left Thunderbolt temperature"),
    ("右侧雷电接口温度", "Right Thunderbolt temperature"),
    ("NVMe 存储温度", "NVMe storage temperature"),
    ("电源邻近温度", "Power supply proximity temperature"),
    ("系统控制器温度", "System controller temperature"),
    ("内存供电温度", "Memory power supply temperature"),
    ("嵌入式设备温度", "Embedded device temperature"),
    ("左侧雷电接口邻近温度", "Left Thunderbolt proximity temperature"),
    ("右侧雷电接口邻近温度", "Right Thunderbolt proximity temperature"),
    ("左侧风道壁温度", "Left airflow wall temperature"),
    ("右侧风道壁温度", "Right airflow wall temperature"),
    ("左前侧气流温度", "Front left airflow temperature"),
    ("右前侧气流温度", "Front right airflow temperature"),
    ("左后侧气流温度", "Rear left airflow temperature"),
    ("右后侧气流温度", "Rear right airflow temperature"),
    ("屏幕盖外侧环境温度", "Outer lid ambient temperature"),
    ("顶部环境邻近温度", "Top ambient proximity temperature"),
    ("SSD 邻近温度", "SSD proximity temperature"),
    ("芯片供电温度", "SoC power supply temperature"),
    ("虚拟芯片温度", "Virtual SoC temperature"),
    ("内存虚拟温度", "Virtual memory temperature"),
    ("内存虚拟汇总温度", "Virtual memory aggregate temperature"),
    ("内存虚拟簇温度", "Virtual memory cluster temperature"),
    ("虚拟环境温度", "Virtual ambient temperature"),
    ("虚拟传感器温度", "Virtual sensor temperature"),
    ("供电虚拟温度", "Virtual power supply temperature"),
    ("电源二极管温度", "Power supply diode temperature"),
    ("雷电接口邻近温度", "Thunderbolt proximity temperature"),
    ("供电芯片温度", "Power supply chip temperature"),
    ("供电芯片最高温度", "Power supply chip maximum temperature"),
    ("射频供电温度", "RF power supply temperature"),
    ("射频供电最高温度", "RF power supply maximum temperature"),
    ("内存温度探头", "Memory temperature probe"),
    ("SSD 温度探头", "SSD temperature probe"),
    ("SSD 控制器温度", "SSD controller temperature"),
    ("GPU 互连温度探头", "GPU fabric temperature probe"),
    ("非核心区温度", "Uncore temperature"),
    ("闪存邻近温度", "NAND proximity temperature"),
    ("SSD 汇总温度", "SSD aggregate temperature"),
    ("内存邻近温度", "Memory proximity temperature"),
    ("芯片散热区温度", "SoC cooling area temperature"),
    ("环境邻近温度", "Ambient proximity temperature"),
    ("非核心区供电温度", "Uncore power supply temperature"),
    ("供电散热器温度", "Power supply heatsink temperature"),
    ("芯片互连温度", "SoC fabric temperature"),
    ("CPU 热余量（非绝对温度）", "CPU thermal headroom (not absolute temperature)"),
    ("芯片封装温度探头", "SoC package temperature probe"),
    ("供电温度探头", "Power supply temperature probe"),
    ("未确认的传感器", "Unidentified sensor"),
    ("名称按芯片型号核对；未确认的项目保留原始标识，虚拟温度与热余量单独标明。", "Names are checked for the chip model. Unidentified readings retain their original keys; virtual temperatures and thermal headroom are labeled explicitly."),
    ("设置尚未应用。确认后点击“应用设置”。", "Changes have not been applied. Click “Apply Settings” to confirm."),
    ("登录启动设置已确认。", "Login startup settings confirmed."),
    ("移除风扇控制服务？", "Remove the fan control service?"),
    ("应用将先停止自定义控制并交还系统，再请求管理员授权。移除后仍可查看温度，控制设置和诊断日志会保留。", "The app will stop custom control and return fans to the system before requesting administrator permission. Temperature monitoring, control settings and diagnostic logs will remain available."),
    ("移除服务", "Remove Service"),
    ("取消", "Cancel"),
    ("演示模式 · 模拟数据 · 不连接风扇", "Demo mode · Simulated data · No fan connection"),
    ("让散热保持安静，让控制保持透明", "Quiet cooling, clear control"),
    ("正在检测温度与风扇…", "Detecting temperatures and fans…"),
    ("正在检查控制服务…", "Checking the control service…"),
    ("正在读取电池与电源…", "Reading battery and power information…"),
    ("系统自动", "System Automatic"),
    ("固定转速", "Fixed RPM"),
    ("智能曲线", "Smart Curve"),
    ("固定目标 RPM", "Target RPM"),
    ("应用设置", "Apply Settings"),
    ("编辑曲线…", "Edit Curve…"),
    ("实际控制状态将以硬件回读为准。", "Actual control status is confirmed by hardware readback."),
    ("热负荷是 0–100% 的控制指标。遇到过热、数据失效或服务失联时自动保护。", "Thermal demand is a 0–100% control signal. Protection activates if the system overheats, data becomes invalid or the service disconnects."),
    ("传感器详情正在加载…", "Loading sensor details…"),
    ("启用控制…", "Enable Control…"),
    ("重新检测", "Detect Again"),
    ("恢复系统自动", "Use System Control"),
    ("导出诊断…", "Export Diagnostics…"),
    ("检查更新…", "Check for Updates…"),
    ("退出", "Quit"),
    ("Fan Control：打开风扇控制", "Fan Control: Open fan controls"),
    ("关于 Fan Control", "About Fan Control"),
    ("打开 Fan Control", "Open Fan Control"),
    ("设置…", "Settings…"),
    ("帮助…", "Help…"),
    ("退出 Fan Control", "Quit Fan Control"),
    ("Fan Control 设置", "Fan Control Settings"),
    ("通用", "General"),
    ("登录时启动 Fan Control", "Start Fan Control at Login"),
    ("登录启动由 macOS 管理。移动应用后请重新确认设置。", "macOS manages login startup. Check this setting again after moving the app."),
    ("移除控制服务…", "Remove Control Service…"),
    ("使用帮助…", "User Guide…"),
    ("关于…", "About…"),
    ("等待管理员授权…", "Waiting for administrator permission…"),
    ("启用 / 修复控制…", "Enable / Repair Control…"),
    ("未知", "Unknown"),
    ("正在充电", "Charging"),
    ("已连接电源", "Connected to power"),
    ("使用电池", "Using battery"),
    ("电源状态未知", "Power state unknown"),
    ("此机型未提供电池信息", "Battery information is unavailable on this model"),
    ("暂不可用", "Unavailable"),
    ("含机身热容模型", "Includes chassis thermal capacity"),
    ("仅芯片热源模型", "Chip heat sources only"),
    ("自定义控制", "Custom Control"),
    ("模式未知", "Mode Unknown"),
    ("转速未知", "RPM Unknown"),
    ("尚未写入", "No command sent yet"),
    ("未检测到风扇。无风扇机型可继续查看温度。", "No fans detected. Temperature monitoring remains available on fanless models."),
    ("传感器暂不可用，点击重新检测可重试。", "Sensors are unavailable. Click Detect Again to retry."),
    ("请输入有效的整数 RPM。", "Enter a valid whole-number RPM."),
    ("输入硬件范围内的整数转速，再点击应用设置。", "Enter a whole-number RPM within the hardware limits, then click Apply Settings."),
    ("正在检测风扇；控制保持关闭，等待有效数据。", "Detecting fans; control remains disabled until valid data is available."),
    ("正在应用…等待硬件确认。", "Applying… Waiting for hardware confirmation."),
    ("编辑散热曲线 · 草稿", "Edit Cooling Curve · Draft"),
    ("编辑曲线", "Edit Curve"),
    ("修改仅在“保存并应用”后生效。取消可放弃本次编辑。", "Changes take effect after Save and Apply. Cancel discards this draft."),
    ("迟滞", "Hysteresis"),
    ("曲线控制源", "Curve Control Source"),
    ("迟滞，热负荷百分点", "Hysteresis, thermal demand percentage points"),
    ("迟滞，摄氏度", "Hysteresis, degrees Celsius"),
    ("热负荷百分比", "thermal demand percentage"),
    ("温度摄氏度", "temperature in degrees Celsius"),
    ("热负荷 · 0–100%", "Thermal Demand · 0–100%"),
    ("CPU 平均温度 · °C", "Average CPU Temperature · °C"),
    ("CPU 最高温度 · °C", "Hottest CPU Temperature · °C"),
    ("GPU 平均温度 · °C", "Average GPU Temperature · °C"),
    ("GPU 最高温度 · °C", "Hottest GPU Temperature · °C"),
    ("撤销", "Undo"),
    ("输入值（温度 °C / 热负荷 %）", "Input (temperature °C / thermal demand %)"),
    ("速度 %（负值低负荷 / 0 最低 RPM）", "Speed % (negative: low demand / 0: minimum RPM)"),
    ("删除", "Delete"),
    ("负值时交还系统；硬件最低 RPM 为 0 时可停转。紧急保护始终生效。", "Negative values return control to the system; fans may stop when hardware supports 0 RPM. Emergency protection remains active."),
    ("添加控制点", "Add Control Point"),
    ("恢复默认", "Restore Defaults"),
    ("保存并应用", "Save and Apply"),
    ("缺少曲线", "Curve is missing"),
    ("选择控制源", "Select a control source"),
    ("更换温度 / 热负荷单位前，请恢复对应默认曲线。", "Restore the matching default curve before switching between temperature and thermal demand units."),
    ("迟滞必须是数字", "Hysteresis must be a number"),
    ("输入值必须是数字", "Input must be a number"),
    ("速度必须是数字", "Speed must be a number"),
    ("正常", "Normal"),
    ("升高", "Elevated"),
    ("严重", "Serious"),
    ("临界", "Critical"),
    ("正在检测硬件与控制服务…", "Detecting hardware and the control service…"),
    ("正在安装或移除控制服务，请完成授权后再应用自定义设置。", "The control service is being installed or removed. Finish authorization before applying custom settings."),
    ("旧版配置已读取；首次保存前将保留备份。", "Legacy settings loaded. A backup will be kept before the first save."),
    ("设置已保存，等待硬件确认…", "Settings saved. Waiting for hardware confirmation…"),
    ("已交还系统自动控制。", "Control returned to the system."),
    ("正在交还系统并移除控制服务，等待管理员授权…", "Returning control to the system and removing the service. Waiting for administrator permission…"),
    ("控制服务已移除。应用继续以只读模式运行。", "Control service removed. The app continues in monitoring mode."),
    ("等待管理员授权。取消后仍可查看温度。", "Waiting for administrator permission. Temperature monitoring remains available if you cancel."),
    ("控制服务已安装并验证。", "Control service installed and verified."),
    ("正在重新检测…", "Detecting again…"),
    ("诊断已导出。", "Diagnostics exported."),
    ("睡眠前已交还系统。", "Control returned to the system before sleep."),
    ("睡眠前交还未获确认，请检查 helper。", "Control handback before sleep was not confirmed. Check the helper."),
    ("正在恢复唤醒前设置…", "Restoring settings after wake…"),
    ("无法注册睡眠保护，控制保持禁用；请重新启动应用。", "Sleep protection could not be registered. Control remains disabled; restart the app."),
    ("控制服务不可用，等待安全回退", "Control service unavailable; waiting for safe fallback"),
    ("硬件确认的目标与请求不一致，正在恢复系统自动。", "The hardware-confirmed target differs from the request. Restoring system automatic control."),
    ("写入失败，已请求安全回退", "Write failed; safe fallback requested"),
    ("等待控制服务", "Waiting for the control service"),
    ("控制目标已回读确认，当前 RPM 将随硬件响应变化。", "Control target confirmed by readback. Current RPM changes as the hardware responds."),
    ("安全保护已接管：温度或系统热压力过高。用户设置将在安全后恢复。", "Safety protection is active: temperature or system thermal pressure is too high. Your settings will resume when safe."),
    ("演示模式：所有控制均为模拟，无硬件写入。", "Demo mode: all control is simulated; no hardware writes."),
    ("控制服务就绪 · 数据实时更新", "Control service ready · Live data"),
    ("仅监测模式", "Monitoring only"),
    ("控制服务失联，等待安全回退", "Control service disconnected; waiting for safe fallback"),
    ("低热负荷，交还系统决定停转", "Low thermal demand; the system decides whether to stop the fans"),
    ("安全保护已接管", "Safety protection active"),
    ("数据失效，恢复系统自动", "Invalid data; restoring system automatic control"),
    ("硬件上限未知，恢复系统自动", "Hardware limit unknown; restoring system automatic control"),
    ("读取状态失败，恢复系统自动", "Status read failed; restoring system automatic control"),
    ("写入失败，恢复系统自动", "Write failed; restoring system automatic control"),
    ("睡眠前交还系统", "Returning control to the system before sleep"),
    ("退出前交还系统", "Returning control to the system before quit"),
    ("设置已生效", "Settings applied"),
    ("登录启动已注册，但需要在系统设置 → 通用 → 登录项中允许 Fan Control。", "Login startup is registered. Allow Fan Control in System Settings → General → Login Items."),
    ("系统找不到当前应用的登录启动服务。", "macOS cannot find the login startup service for this app."),
    ("无法找到系统 ServiceManagement framework。", "The system ServiceManagement framework could not be found."),
    ("当前系统不提供 SMAppService；需要 macOS 13 或更新系统。", "SMAppService is unavailable; macOS 13 or later is required."),
    ("无法获取当前应用的登录启动服务。", "The login startup service for this app could not be obtained."),
    ("登录启动属于当前用户设置，不能以 root 身份修改。", "Login startup is a current-user setting and cannot be changed as root."),
    ("请从完整的 FanControl.app 中修改登录启动。", "Change login startup from the complete FanControl.app bundle."),
    ("登录启动只能为当前运行的 FanControl.app 设置。", "Login startup can only be changed for the running FanControl.app."),
    ("应用包不完整，请重新安装 FanControl.app 后再设置登录启动。", "The app bundle is incomplete. Reinstall FanControl.app before setting login startup."),
    ("登录启动状态尚未确认，请重新查询后再试。", "Login startup status is not confirmed. Check it again before retrying."),
    ("曲线预览：横轴为控制输入，纵轴为风扇速度。精确值可在下方表格编辑。", "Curve preview: the horizontal axis is control input; the vertical axis is fan speed. Edit exact values in the table below."),
    ("语言", "Language"),
    ("系统", "System"),
    ("跟随系统", "Follow System"),
    ("系统默认", "System Default"),
    ("中文", "中文"),
    ("English", "English"),
    ("语言设置已保存。重新打开窗口后生效。", "Language preference saved. Reopen the window to apply it."),
    ("界面语言已保存，下次启动应用时生效。", "Interface language saved. It will take effect the next time the app starts."),
    ("界面语言", "Interface Language"),
    ("简体中文", "简体中文"),
    ("提前散热 · 稳定性能 · 键盘舒适", "Earlier cooling · Sustained performance · Keyboard comfort"),
    ("智能热管理", "Smart Thermal Control"),
    ("自定义曲线", "Custom Curve"),
    ("搜索传感器名称或 SMC key", "Search sensor names or SMC keys"),
    ("搜索传感器", "Search Sensors"),
    ("分组传感器详情", "Grouped Sensor Details"),
    ("没有匹配的传感器。", "No matching sensors."),
    ("数据已过期", "Stale Data"),
    ("由系统决定", "Decided by the system"),
    ("暂无 RPM 目标", "No RPM target"),
    ("CPU 温度", "CPU Temperatures"),
    ("GPU 温度", "GPU Temperatures"),
    ("内部机身传感器（非键盘表面测温）", "Internal Chassis Sensors (not keyboard surface measurements)"),
    ("其他传感器", "Other Sensors"),
    ("配置超过 1 MiB，禁止覆盖；请修复原文件。", "Settings exceed 1 MiB and cannot be overwritten. Repair the original file."),
    ("配置路径必须是普通文件，禁止覆盖。", "The settings path must be a regular file. Overwriting is blocked."),
    ("配置恢复标记不是普通文件，禁止覆盖。", "The settings recovery marker is not a regular file. Overwriting is blocked."),
    ("原配置尚无法读取和备份，已阻止覆盖。", "Existing settings cannot yet be read and backed up. Overwriting is blocked."),
    ("原配置备份路径不是普通文件，禁止覆盖原配置。", "The settings backup path is not a regular file. Existing settings cannot be overwritten."),
    ("硬件回执无法确认请求，正在恢复系统自动", "The hardware response did not confirm the request; restoring system control"),
    ("本次自定义设置未应用；恢复保存后请重新应用。", "These custom settings were not applied. Apply them again after saving is restored."),
    ("正在安装或移除控制服务，请完成后再更新智能策略。", "The control service is being installed or removed. Finish before updating the smart policy."),
    ("智能策略已保存，等待新采样评估。", "Smart policy saved; waiting for a new sample to evaluate it."),
    ("用户请求交还系统", "System control requested by the user"),
    ("正在交还系统自动控制…", "Returning control to the system…"),
    ("移除控制服务前交还系统", "Returning control to the system before removing the service"),
    ("控制服务已重新连接，正在核对硬件状态。", "The control service reconnected; checking hardware status."),
    ("控制服务失联，交还系统尚未确认", "Control service disconnected; system handback is not yet confirmed"),
    ("已确认交还系统自动", "System control handback confirmed"),
    ("已回读确认交还系统；将以新采样重新评估设置。", "System control handback confirmed by readback. Settings will be re-evaluated with a new sample."),
    ("交还系统未确认，RPM 写入已暂停", "System handback is not confirmed; RPM writes are paused"),
    ("持续负载，提前提高散热", "Sustained load; increasing cooling early"),
    ("芯片温度趋势预测，提前散热", "Chip temperature trend predicts a rise; cooling early"),
    ("持续热浸或冷却驻留", "Sustained heat soak or cooling hold"),
    ("持续热负载调节", "Sustained thermal load regulation"),
    ("已校准舒适策略", "Calibrated comfort policy"),
    ("曲线标识、名称或传感器无效", "Curve ID, name or sensor is invalid"),
    ("曲线必须有 2...64 个点，迟滞范围为 0...30", "A curve must have 2...64 points and hysteresis within 0...30"),
    ("曲线点超出有效温度或速度范围", "A curve point is outside the valid temperature or speed range"),
    ("曲线点的温度不能重复", "Curve point temperatures must be unique"),
    ("风扇 id 必须为 0...9", "Fan id must be within 0...9"),
    ("手动转速必须为 1...16383；停转只允许通过曲线策略", "Manual RPM must be within 1...16383; stopping is only allowed through a curve policy"),
    ("曲线模式与配置标识不一致", "Curve mode does not match the configured ID"),
    ("最多配置 10 个风扇", "At most 10 fans can be configured"),
    ("风扇 id 重复", "Fan id is duplicated"),
    ("配置文件超过 1 MiB", "Settings file exceeds 1 MiB"),
    ("旧配置超过风扇数量上限", "Legacy settings exceed the fan-count limit"),
    ("旧配置包含无效风扇 id", "Legacy settings contain an invalid fan id"),
    ("舒适目标必须为 30...45°C", "Comfort target must be within 30...45°C"),
    ("表面校准需要机器、传感器、真实测量偏移和有效温度范围", "Surface calibration requires a machine, sensor, offset from real measurements and a valid temperature range"),
    ("上次保存未完成，已使用系统自动并将重试保存安全配置。", "The previous save did not complete. System control is active and saving safe settings will be retried."),
    ("智能热管理与体感目标", "Smart Thermal Control and Comfort Targets"),
    ("性能与键盘舒适", "Performance and Keyboard Comfort"),
    ("启用校准后的体感温度目标", "Enable Calibrated Surface Comfort Target"),
    ("键盘体感温度目标，摄氏度", "Keyboard comfort temperature target, degrees Celsius"),
    ("°C（30–45）", "°C (30–45)"),
    ("性能策略按持续 CPU 负载、CPU/GPU 升温趋势与热浸提前增加散热。表面温度需要外部实测校准；内部传感器值不能直接当作键盘表面温度。", "The performance policy increases cooling early based on sustained CPU load, CPU/GPU temperature trends and heat soak. Surface temperature requires calibration with external measurements; internal sensor values are not keyboard surface temperatures."),
    ("内部参考传感器", "Internal Reference Sensor"),
    ("校准用内部参考传感器", "Internal reference sensor for calibration"),
    ("记录的内部温度", "Recorded Internal Temperature"),
    ("键盘表面实测 °C", "Measured Keyboard Surface °C"),
    ("未记录", "Not Recorded"),
    ("在正常使用的两个稳定温度状态分别记录。内部读数至少相差 3°C；两次记录之间可继续使用电脑。校准仅在本机和已测温度范围内有效。", "Record two stable temperature states during normal use. Internal readings must differ by at least 3°C; you can keep using the computer between records. Calibration applies only to this machine and the measured temperature range."),
    ("已保存的校准会保留；新校准需完成两个记录点后保存。", "Saved calibration is retained. Complete both new calibration points before saving."),
    ("尚未校准，当前仅启用性能与热趋势策略。", "Not calibrated; only performance and temperature-trend policies are active."),
    ("清除校准草稿", "Clear Calibration Draft"),
    ("保存目标", "Save Targets"),
    ("校准点无效。", "Invalid calibration point."),
    ("采样已过期，请等待新数据后记录。", "The sample is stale. Wait for fresh data before recording."),
    ("无法确认本机校准身份，请重新打开设置。", "This machine's calibration identity cannot be confirmed. Reopen settings."),
    ("请选择可用的内部参考传感器。", "Select an available internal reference sensor."),
    ("参考传感器暂不可用，未记录校准点。", "The reference sensor is unavailable; no calibration point was recorded."),
    ("请输入键盘表面实测温度。", "Enter the measured keyboard surface temperature."),
    ("表面实测温度必须为 15–60°C。", "Measured surface temperature must be within 15–60°C."),
    ("参考传感器已改变，请重新记录两个校准点。", "The reference sensor changed. Record both calibration points again."),
    ("舒适目标必须是数字。", "Comfort target must be a number."),
    ("本机身份发生变化，请重新记录校准。", "The machine identity changed. Record the calibration again."),
    ("请选择参考传感器。", "Select a reference sensor."),
    ("更换参考传感器后，需要两个新的校准记录点。", "Changing the reference sensor requires two new calibration points."),
    ("校准需要两个完整记录点。", "Calibration requires two complete recording points."),
    ("体感目标未启用", "Comfort target disabled"),
    ("未校准，仅启用性能策略", "Not calibrated; performance policy only"),
    ("校准不属于本机，体感调节已暂停", "Calibration belongs to another machine; comfort regulation paused"),
    ("校准传感器不可用，体感调节已暂停", "Calibration sensor unavailable; comfort regulation paused"),
    ("超出校准范围，体感调节已暂停", "Outside the calibrated range; comfort regulation paused"),
    ("高于校准范围，保守保持散热", "Above the calibrated range; conservative cooling retained"),
    ("低于校准范围，表面估计已暂停", "Below the calibrated range; surface estimate paused"),
    ("本机校准身份已更新，请重新记录两个校准点。", "This device's calibration identity was updated. Record both calibration points again."),
    ("校准范围内的温度估计", "Temperature estimate within the calibrated range"),
    ("低热负荷", "Low thermal demand"),
    ("当前温度", "Current temperature"),
    ("持续负载提前介入", "Early intervention for sustained load"),
    ("升温趋势提前介入", "Early intervention for a rising temperature trend"),
    ("机身热浸", "Chassis heat soak"),
    ("键盘体感目标", "Keyboard comfort target"),
    ("体感目标…", "Comfort Targets…"),
    ("智能热管理…", "Smart Thermal Control…"),
    ("退出前交还未获确认", "System Handback Before Quit Was Not Confirmed"),
    ("控制服务会继续重试。请检查服务状态与风扇实际模式；应用无法确认安全回退已经完成。", "The control service will keep retrying. Check its status and the actual fan mode; the app cannot confirm that safe fallback completed."),
    ("请先启用控制服务，曲线草稿尚未保存或应用。", "Enable the control service first. This curve draft has not been saved or applied."),
    ("旧版配置已迁移，原文件已备份。", "Legacy settings migrated; the original file was backed up."),
    ("上次保存未完成；现已保存系统自动配置，原文件已备份。", "The previous save did not complete. System-control settings are now saved and the original file was backed up."),
    ("目标用于“智能热管理”模式；固定转速与曲线保留独立调节。性能策略按持续 CPU 负载、CPU/GPU 升温趋势与热浸提前增加散热。表面温度需要外部实测校准。", "Targets apply to Smart Thermal Control; fixed RPM and curves keep separate settings. The performance policy increases cooling early based on sustained CPU load, CPU/GPU temperature trends and heat soak. Surface temperature requires calibration with external measurements."),
    ("实测值已修改，请重新记录对应校准点后保存。", "The measured value changed. Record the corresponding calibration point again before saving."),
    ("编辑", "Edit"),
    ("重做", "Redo"),
    ("剪切", "Cut"),
    ("拷贝", "Copy"),
    ("粘贴", "Paste"),
    ("全选", "Select All"),
    ("数据中断", "Data interrupted"),
    ("上升中", "Rising"),
    ("回落中", "Falling"),
    ("平稳", "Steady"),
    ("低", "Low"),
    ("中", "Medium"),
    ("高", "High"),
    ("由 macOS 决定何时启动风扇，与未安装本应用时一致。", "macOS decides when the fans run, the same as without this app."),
    ("按持续热负载平稳散热，热量消散后交还系统。", "Cools smoothly according to sustained thermal load and returns control to the system after heat dissipates."),
    ("风扇按你设定的速度或温度曲线运行，过热时安全保护仍会接管。", "Fans follow your speed or temperature curve. Safety protection still takes over if things get too hot."),
    ("各风扇设置不同，可在主窗口的“风扇”页分别调整。", "Your fans use different settings. Adjust each one on the Fans page of the main window."),
    ("演示模式", "Demo mode"),
    ("数据为模拟值，不会控制真实风扇。", "These are simulated values. No real fans are controlled."),
    ("这台 Mac 没有风扇", "This Mac has no fans"),
    ("可以继续查看温度，散热由 macOS 管理。", "You can still check temperatures. macOS manages cooling."),
    ("等待管理员授权", "Waiting for administrator approval"),
    ("在系统弹出的窗口中输入密码。取消也不影响查看温度。", "Enter your password in the system dialog. Canceling doesn't affect temperature monitoring."),
    ("温度数据暂时中断", "Temperature data is interrupted"),
    ("正在重新读取；在此期间风扇由 macOS 控制。", "Reading again. macOS controls the fans in the meantime."),
    ("温度偏高，正在全力散热", "Running hot, cooling at full effort"),
    ("macOS 正在限制性能以控制温度。", "macOS is limiting performance to control temperature."),
    ("安全保护已临时接管，温度回落后恢复你的设置。", "Safety protection has taken over for now. Your settings return once it cools down."),
    ("风扇已交还系统", "Fans returned to the system"),
    ("交还系统尚未确认", "System handback is not yet confirmed"),
    ("交还系统尚未确认；控制写入已暂停，正在重试。", "System handback is not yet confirmed. Control writes are paused while retrying."),
    ("控制服务暂时没有响应，macOS 已安全接管风扇。", "The control service isn't responding, so macOS has safely taken over the fans."),
    ("需要更新风扇控制组件", "Fan control component needs an update"),
    ("更新需要一次管理员授权，温度可以照常查看。", "Updating needs administrator approval once. Temperatures stay visible."),
    ("需要一次授权", "One-time approval needed"),
    ("授权后才能调节风扇，温度可以照常查看。", "Approve once to adjust the fans. Temperatures stay visible."),
    ("有风扇没有按设置运行", "A fan isn't following your settings"),
    ("已交还 macOS 控制，稍后会自动重试。", "It's back under macOS control and will retry automatically."),
    ("设置可能没有保存", "Settings may not be saved"),
    ("当前设置仍在生效，但重启后可能恢复默认。详情见“设置”页。", "Your settings are active now but may reset after a restart. See the Settings page for details."),
    ("运行正常", "Running normally"),
    ("风扇由 macOS 控制。", "macOS controls the fans."),
    ("负载升高时会提前散热。", "Cools early when load rises."),
    ("风扇按你的设置运行。", "Fans follow your settings."),
    ("停转", "Stopped"),
    ("状态未知", "Status unknown"),
    ("由系统控制", "System controlled"),
    ("固定速度", "Fixed speed"),
    ("按温度曲线", "Following curve"),
    ("智能散热", "Smart cooling"),
    ("正在交还系统", "Returning to system"),
    ("正在检测…", "Detecting…"),
    ("芯片温度", "Chip temperature"),
    ("正在读取风扇…", "Reading fans…"),
    ("启用风扇控制", "Enable fan control"),
    ("散热方式", "Cooling mode"),
    ("自定义", "Custom"),
    ("风扇速度", "Fan speed"),
    ("0% 为硬件最低转速，100% 为最高转速。松开后生效。", "0% is the hardware minimum speed and 100% the maximum. Takes effect when you let go."),
    ("最安静", "Quietest"),
    ("最凉爽", "Coolest"),
    ("按温度自动调节（温度曲线）…", "Adjust by temperature (curve)…"),
    ("提示", "Info"),
    ("需要注意", "Needs attention"),
    ("温度偏高", "Running hot"),
    ("没有可显示的风扇", "No fans to show"),
    ("重新连接", "Reconnect"),
    ("正在按温度曲线运行。拖动滑块会改为固定速度。", "Following a temperature curve. Moving the slider switches to a fixed speed."),
    ("散热需求", "Cooling demand"),
    ("电池", "Battery"),
    ("最近 10 分钟", "Last 10 minutes"),
    ("— 温度", "— Temperature"),
    ("- - 风扇", "- - Fans"),
    ("风扇", "Fans"),
    ("各风扇分别设置", "Set per fan"),
    ("当前散热方式：", "Current cooling mode: "),
    ("最近 10 分钟暂无温度数据", "No temperature data in the last 10 minutes"),
    ("在这里可以为每个风扇单独设置。菜单栏面板中的散热方式会同时作用于所有风扇。", "Set each fan individually here. The cooling mode in the menu bar panel applies to all fans at once."),
    ("选择风扇", "Choose fan"),
    ("温度曲线", "Temperature curve"),
    ("固定转速（转/分）", "Fixed speed (RPM)"),
    ("输入硬件范围内的整数转速，再点击应用到此风扇。", "Enter a whole-number speed within the hardware range, then click Apply to This Fan."),
    ("编辑温度曲线…", "Edit Temperature Curve…"),
    ("智能散热设置…", "Smart Cooling Settings…"),
    ("应用到此风扇", "Apply to This Fan"),
    ("实际状态以硬件回读为准。", "Actual state is confirmed by hardware readback."),
    ("风扇控制服务", "Fan control service"),
    ("启用 / 修复风扇控制…", "Enable / Repair Fan Control…"),
    ("全部交还系统", "Return All to System"),
    ("支持", "Support"),
    ("概览", "Overview"),
    ("设置", "Settings"),
    ("温度详情", "Temperature Details"),
    ("暂缓接管，稍后自动重试", "Takeover paused; retrying automatically"),
    ("风扇暂时交还系统", "Fans temporarily returned to the system"),
    ("上次控制没有成功。为避免风扇反复启停，稍后会自动再试。", "The last control attempt didn't succeed. To avoid starting and stopping the fans repeatedly, it will try again shortly."),
    ("已保存，智能散热会按新目标运行。", "Saved. Smart cooling now follows the new goal."),
    ("正在检查…", "Checking…"),
    ("该服务以管理员权限运行，只在硬件安全范围内写入风扇转速；应用退出、失联或电脑睡眠时会自动交还 macOS。", "This service runs with administrator privileges and only writes fan speeds within hardware-safe limits. It returns control to macOS when the app quits, loses contact, or the Mac sleeps."),
    ("正在读取设置与传感器…", "Reading settings and sensors…"),
    ("智能散热…", "Smart Cooling…"),
    ("演示模式 · 不连接控制服务", "Demo mode · Not connected to the control service"),
    ("正在等待管理员授权…", "Waiting for administrator approval…"),
    ("已启用 · 风扇可由本应用调节", "Enabled · This app can adjust the fans"),
    ("未启用 · 目前只能查看温度", "Not enabled · Temperatures only for now"),
    ("拖动图中的圆点，或在下方修改数值。点击“保存并应用”后生效。", "Drag the dots in the chart or edit the values below. Changes take effect after Save and Apply."),
    ("安静", "Quiet"),
    ("均衡", "Balanced"),
    ("凉爽", "Cool"),
    ("曲线预设", "Curve preset"),
    ("降温时延后减速的幅度，避免风扇来回变速。", "How far it waits to slow down while cooling, so fan speed doesn't bounce."),
    ("控制依据", "Based on"),
    ("曲线预览，可拖动控制点", "Curve preview with draggable points"),
    ("热负荷 →", "Thermal demand →"),
    ("温度 →", "Temperature →"),
    ("灰色区域：交还系统，风扇可能停转 · 橙线：当前值", "Gray area: returned to the system, fans may stop · Orange line: current value"),
    ("热负荷 %", "Thermal demand %"),
    ("温度 °C", "Temperature °C"),
    ("风扇速度 %", "Fan speed %"),
    ("同时应用到所有风扇", "Apply to all fans"),
    ("低于 0% 时交还系统；过热时安全保护始终生效。", "Below 0% control returns to the system. Safety protection always applies when hot."),
    ("拖动圆点调整曲线；横轴为控制输入，纵轴为风扇速度。灰色区域表示交还系统。", "Drag the dots to shape the curve. Horizontal is the control input, vertical is fan speed. The gray area means returned to the system."),
    ("负载持续升高时提前加大散热，减少降频；空闲时交还系统保持安静。以下目标只作用于“智能散热”。", "Cools early when load keeps rising to reduce throttling, and hands back to the system when idle to stay quiet. The goals below only apply to Smart cooling."),
    ("还原", "Revert"),
    ("放弃未保存的修改", "Discard unsaved changes"),
    ("保存", "Save"),
    ("校准已从草稿清除，点击保存后生效。", "Calibration cleared from the draft. Click Save to apply."),
    ("体感目标需要用外部温度计校准后才会生效；固定速度与温度曲线不受这些目标影响。", "Comfort goals take effect only after calibration with an external thermometer. Fixed speed and temperature curves aren't affected by these goals."),
    ("由 macOS 决定何时启动风扇。", "macOS decides when the fan runs."),
    ("按持续热负载平稳散热，冷却后交还系统。", "Cools smoothly according to sustained thermal load and returns control to the system after cooling."),
    ("始终保持所选速度，过热时安全保护仍会接管。", "Keeps the chosen speed. Safety protection still takes over if it gets too hot."),
    ("按散热需求自动调节，可编辑曲线。", "Adjusts automatically with cooling demand. The curve can be edited."),
    ("暂无数据", "No data"),
    ("正在等待温度数据。", "Waiting for temperature data."),
    ("负载较低，风扇交给系统，保持安静。", "Load is low, so the fans are left to the system and stay quiet."),
    ("正在按持续热负载平稳调节风扇。", "Adjusting the fans smoothly to sustained thermal load."),
    ("检测到持续高负载，正在逐步增加散热。", "Sustained heavy load detected. Increasing cooling gradually."),
    ("检测到持续升温趋势，逐步增加散热。", "A sustained rising temperature trend was detected. Increasing cooling gradually."),
    ("散热需求较低，待持续冷却后交还系统。", "Cooling demand is low. Control will return to the system after sustained cooling."),
    ("温度或系统热压力过高，安全保护正在加大散热。", "Temperature or thermal pressure is too high. Safety protection is increasing cooling."),
    ("机身积累了热量，保持适度散热帮助降温。", "Heat has built up in the body. Keeping moderate cooling to bring it down."),
    ("为让键盘区域保持舒适，正在加大散热。", "Increasing cooling to keep the keyboard area comfortable."),
    ("转速", "speed"),
    ("菜单栏面板会把同一种散热方式用于所有风扇；在这里可以为每个风扇单独设置。", "The menu bar panel applies one cooling mode to all fans. Here you can set each fan separately."),
    ("转速以风扇实测为准。温度过高时，安全保护会临时接管所有风扇。", "Speeds are measured from the fans. If it gets too hot, safety protection temporarily takes over all fans."),
    ("处理器", "CPU"),
    ("图形处理器", "GPU"),
    ("机身内部（不代表键盘表面温度）", "Inside the body (not keyboard surface temperature)"),
    ("其他", "Other"),
    ("系统热压力", "Thermal pressure"),
    ("机身升温", "Body warming"),
    ("全部传感器", "All Sensors"),
    ("颜色表示温度高低：绿色较凉，黄色温热，橙色偏热，红色过热。芯片在高负载下短时达到 90°C 以上属于正常现象。", "Colors show how warm each part is: green is cool, yellow warm, orange hot and red too hot. Chips briefly reaching 90°C or more under heavy load is normal."),
    ("正在读取传感器…", "Reading sensors…"),
    ("登录时启动", "Open at Login"),
    ("开机登录后自动在菜单栏运行，继续使用你的散热设置。", "Starts in the menu bar when you log in and keeps your cooling settings."),
    ("重新打开应用后生效。", "Takes effect after reopening the app."),
    ("启用…", "Enable…"),
    ("立即交还系统", "Return Control to System"),
    ("所有风扇马上回到 macOS 自动控制，直到你再次选择散热方式。", "All fans return to automatic macOS control until you choose a cooling mode again."),
    ("全部交还", "Return All"),
    ("移除…", "Remove…"),
    ("移除控制服务", "Remove Control Service"),
    ("需要管理员授权。移除后仍可查看温度，你的设置会保留。", "Requires administrator approval. You can still view temperatures afterwards, and your settings are kept."),
    ("软件更新", "Software Update"),
    ("诊断信息", "Diagnostics"),
    ("遇到问题时导出诊断文件，附在反馈里帮助排查。", "If something goes wrong, export a diagnostics file and attach it to your report."),
    ("导出…", "Export…"),
    ("使用帮助", "User Guide"),
    ("关于", "About"),
    ("帮助与关于", "Help & About"),
    ("不连接控制服务，也不会改变风扇。", "Not connected to the control service; fans are not changed."),
    ("在系统弹出的窗口中输入密码。", "Enter your password in the system dialog."),
    ("已启用", "Enabled"),
    ("风扇可以由本应用调节。应用退出、失联或电脑睡眠时会自动交还 macOS。", "This app can adjust the fans. Control returns to macOS automatically when the app quits, loses contact or the Mac sleeps."),
    ("未启用", "Not Enabled"),
    ("目前只能查看温度。启用需要一次管理员授权。", "Only temperatures can be viewed for now. Enabling requires administrator approval once."),
    ("改用智能散热", "Use Smart Cooling"),
    ("智能散热综合持续负载、机身蓄热和温升趋势平稳调节风扇；短时温度波动不急升急降，持续冷却后交还系统。", "Smart cooling adjusts the fans smoothly using sustained load, retained chassis heat and temperature trends. Brief temperature fluctuations do not cause abrupt speed changes. Control returns to the system after sustained cooling."),
    ("键盘体感目标（可选）", "Keyboard Comfort Target (Optional)"),
    ("启用体感温度目标", "Enable comfort temperature target"),
    ("让键盘区域不超过", "Keep the keyboard area below"),
    ("参考传感器", "Reference sensor"),
    ("选一个靠近键盘的机身传感器。", "Choose a body sensor close to the keyboard."),
    ("记录", "Record"),
    ("温度计读数", "Thermometer"),
    ("电脑内部没有键盘表面的温度计，所以需要校准：在两个不同的使用状态下，用外部温度计测量键盘中央并记录，两次内部读数至少相差 3°C。校准只对本机和测量过的温度范围有效；固定速度与温度曲线不受这个目标影响。", "The Mac has no thermometer on the keyboard surface, so calibration is needed: in two different usage states, measure the middle of the keyboard with an external thermometer and record it. The two internal readings must differ by at least 3°C. Calibration only applies to this Mac and the measured range; fixed speed and temperature curves aren't affected."),
    ("清除校准", "Clear Calibration"),
    ("当前散热需求：", "Cooling demand: "),
    ("当前选择：默认", "Current selection: Default"),
    ("当前选择：", "Current selection: "),
    ("处理器使用率", "CPU usage"),
    ("芯片温度趋向", "Chip heading to"),
    ("智能散热正在使用", "Smart cooling is on"),
    ("当前没有使用智能散热。", "Smart cooling is not in use."),
    ("关闭时只按负载和芯片温度散热。", "When off, cooling follows load and chip temperature only."),
    ("估计当前", "Estimated now"),
    ("内部读数", "Internal"),
    ("实测", "measured"),
];

#[derive(Clone, Copy)]
enum Capture {
    Opaque,
    Product,
    Number,
}

struct Template {
    zh: &'static str,
    en: &'static str,
    captures: &'static [Capture],
}
use Capture::{Number as N, Opaque as O, Product as P};
macro_rules! templates {
    ($(($zh:literal, $en:literal, [$($kind:ident),*])),* $(,)?) => { &[$(Template { zh:$zh, en:$en, captures:&[$($kind),*] }),*] };
}
const TEMPLATES: &[Template] = templates![
    ("散热偏好无效，已恢复默认：{0}", "Invalid cooling preference; restored the default: {0}", [O]),
    ("保存失败：{0}", "Save failed: {0}", [O]),
    ("目标 {0} 转/分 · 硬件已确认 {1} 转/分", "Target {0} RPM · confirmed by hardware {1} RPM", [O,O]),
    ("目标 {0} 转/分 · 等待硬件确认", "Target {0} RPM · waiting for hardware confirmation", [O]),
    ("约 {0} 转/分", "about {0} RPM", [O]),
    ("{0} · 耗电 {1} W", "{0} · using {1} W", [P,N]),
    ("{0} · 充入 {1} W", "{0} · charging at {1} W", [P,N]),
    ("健康度约 {0}%", "Health about {0}%", [N]),
    ("健康度 {0}%", "Health {0}%", [N]),
    ("已充电循环 {0} 次", "{0} charge cycles", [N]),
    ("电源适配器 {0} W", "{0} W power adapter", [N]),
    ("当前版本 {0}（{1}）", "Current version {0} ({1})", [O,O]),
    ("校准点 {0}", "Calibration point {0}", [N]),
    ("{0}°C/分", "{0}°C/min", [N]),
    ("校准点 {0} 键盘表面实测温度", "Calibration point {0}, measured keyboard surface temperature", [N]),
    ("记录点 {0}", "Record Point {0}", [N]),
    ("已记录点 {0}。两个记录点完成后，点击保存。", "Point {0} recorded. Complete both points, then click Save.", [N]),
    ("CPU 利用率 {0} · 芯片预测 {1}\n散热依据 {2} · 热浸 {3}%\n键盘表面估计 {4} · {5}", "CPU utilization {0} · Predicted chip temp {1}\nCooling reason {2} · Heat soak {3}%\nEstimated keyboard surface {4} · {5}", [P,P,P,N,P,P]),
    ("CPU  {0} · GPU  {1} · 智能需求 {2} · 系统热压力 {3}\nCPU 利用率 {4} · 芯片预测 {5} · {6}\n键盘表面估计 {7} · {8}", "CPU  {0} · GPU  {1} · Smart demand {2} · System thermal pressure {3}\nCPU utilization {4} · Predicted chip temp {5} · {6}\nEstimated keyboard surface {7} · {8}", [P,P,P,P,P,P,P,P,P]),
    ("电量 {0} · {1} · 电池功率 {2} · 适配器 {3}\n循环 {4} · 健康度 {5}", "Battery {0} · {1} · Battery power {2} · Adapter {3}\nCycles {4} · Health {5}", [P,P,P,P,P,P]),
    ("电量 {0} · {1} · 功率 {2} · 循环 {3} · 健康度 {4}", "Battery {0} · {1} · Power {2} · Cycles {3} · Health {4}", [P,P,P,P,P]),
    ("电量 {0} · {1} · 循环 {2} · 健康度 {3}", "Battery {0} · {1} · Cycles {2} · Health {3}", [P,P,P,P]),
    ("{0}%（估算）", "{0}% (estimated)", [N]),
    ("CPU  {0}     GPU  {1}     热负荷  {2}\n系统热压力  {3} · {4}", "CPU  {0}     GPU  {1}     Thermal demand  {2}\nSystem thermal pressure  {3} · {4}", [P,P,P,P,P]),
    ("实际：{0} · {1}  {2}", "Actual: {0} · {1}  {2}", [P,P,P]),
    ("RPM 必须在 {0}–{1} 之间。", "RPM must be between {0}–{1}.", [N,N]),
    ("Fan Control · CPU {0} · {1}", "Fan Control · CPU {0} · {1}", [P,P]),
    ("无法备份原有配置：{0}", "Could not back up existing settings: {0}", [O]),
    ("配置保存失败：{0}", "Could not save settings: {0}", [O]),
    ("配置读取失败，已使用系统自动。原文件保留：{0}", "Could not read settings; system automatic control is active. Original file kept: {0}", [O]),
    ("SMC 无法连接：{0}。点击重新检测可重试。", "SMC connection failed: {0}. Click Detect Again to retry.", [O]),
    ("{0} 配置保存失败，重启前请修复：{1}", "{0} Settings could not be saved; fix this before restarting: {1}", [P,P]),
    ("交还系统未获确认：{0}", "System control handback was not confirmed: {0}", [O]),
    ("移除服务未完成：{0}", "Service removal did not complete: {0}", [O]),
    ("安装未完成：{0}", "Installation did not complete: {0}", [O]),
    ("导出失败：{0}", "Export failed: {0}", [O]),
    ("控制服务需要更新（协议 {0} → {1}）。点击启用 / 修复。", "Control service needs an update (protocol {0} → {1}). Click Enable / Repair.", [O,N]),
    ("控制服务不可用：{0}。可查看温度；点击启用 / 修复以恢复控制。", "Control service unavailable: {0}. Temperature monitoring is available; click Enable / Repair to restore control.", [O]),
    ("传感器读取失败：{0}，自定义控制将安全回退。", "Sensor read failed: {0}; custom control will fall back safely.", [O]),
    ("风扇 {0} 设置失败：{1}", "Fan {0} configuration failed: {1}", [N,O]),
    ("目标已确认 · {0}", "Target confirmed · {0}", [P]),
    ("控制服务失联：{0}。服务将按租约交还系统。", "Control service disconnected: {0}. The service will return control to the system when its lease expires.", [O]),
    ("自定义控制已停止；保存失败，重启前请修复：{0}", "Custom control stopped; saving failed. Fix this before restarting: {0}", [P]),
    ("系统返回未知登录启动状态：{0}", "macOS returned an unknown login startup status: {0}", [N]),
    ("无法加载登录启动服务：{0}", "Could not load the login startup service: {0}", [O]),
    ("启用登录启动失败：{0}", "Could not enable login startup: {0}", [O]),
    ("关闭登录启动失败：{0}", "Could not disable login startup: {0}", [O]),
    ("无法读取应用位置：{0}", "Could not read the app location: {0}", [O]),
    ("无法读取当前应用位置：{0}", "Could not read the running app location: {0}", [O]),
    ("控制点 {0} 输入{1}", "Control point {0}, input: {1}", [N,P]),
    ("控制点 {0} 速度百分比", "Control point {0}, speed percentage", [N]),
    ("删除控制点 {0}", "Delete control point {0}", [N]),
    ("{0} · 暂不可用", "{0} · Unavailable", [O]),
    ("版本 {0}（{1}）", "Version {0} ({1})", [O,O]),
    ("采样数据已过期，等待新数据。{0}", "Samples are stale; waiting for fresh data. {0}", [P]),
    ("持续芯片 {0} · 内部热节点 {1} · 升温 {2}°C/min", "Sustained chip temp {0} · Internal thermal node {1} · Rise {2}°C/min", [P,P,N]),
    ("硬件：{0} · 当前 {1}\n目标 {2} · 最近确认 {3} · {4}", "Hardware: {0} · Current {1}\nTarget {2} · Last confirmed {3} · {4}", [P,P,P,P,P]),
    ("无法创建配置恢复标记，原配置未覆盖：{0}", "Could not create the settings recovery marker; existing settings were not overwritten: {0}", [O]),
    ("配置保存未获持久确认，下次启动将使用系统自动：{0}", "The settings save was not confirmed durable; system control will be used at next launch: {0}", [O]),
    ("配置已保存，但恢复标记清理未确认；下次启动可能使用系统自动：{0}", "Settings saved, but recovery marker cleanup was not confirmed; next launch may use system control: {0}", [O]),
    ("无法检查配置恢复标记：{0}", "Could not inspect the settings recovery marker: {0}", [O]),
    ("配置读取失败，已使用系统自动；原文件将先备份再保存：{0}", "Could not read settings; system control is active. The original file will be backed up before saving: {0}", [P]),
    ("原配置无法安全读取，已禁用自定义控制；禁止覆盖并将重试读取：{0}", "Existing settings cannot be read safely; custom control is disabled. Overwriting is blocked and reading will be retried: {0}", [P]),
    ("{0} 上次保存未完成，已使用系统自动并将重试保存安全配置。", "{0} The previous save did not complete. System control is active and saving safe settings will be retried.", [P]),
    ("保存失败，将重试当前设置：{0}", "Saving failed; current settings will be retried: {0}", [P]),
    ("{0} 保存失败，将重试当前设置：{1}", "{0} Saving failed; current settings will be retried: {1}", [P,P]),
    ("原配置现已可读取；为避免意外恢复，自定义设置需重新应用。 {0}", "Existing settings can now be read. Apply custom settings again to avoid an unexpected restore. {0}", [P]),
    ("原配置仍无法安全读取，禁止覆盖：{0}", "Existing settings still cannot be read safely; overwriting is blocked: {0}", [P]),
    ("写入失败：{0}", "Write failed: {0}", [O]),
    ("安全回退未获确认：{0}。已暂停续租和 RPM 写入，正在重试交还系统。", "Safe fallback was not confirmed: {0}. Lease renewal and RPM writes are paused; system handback is being retried.", [P]),
    ("配置 JSON 解析失败：{0}", "Settings JSON could not be parsed: {0}", [O]),
    ("不支持配置版本 {0}，当前版本为 {1}", "Unsupported settings version {0}; the current version is {1}", [N,N]),
    ("风扇 {0} 的旧控制模式损坏，已恢复系统自动", "Fan {0} has a damaged legacy control mode; system control restored", [N]),
    ("热策略或表面校准无效，已禁用舒适估计：{0}", "Thermal policy or surface calibration is invalid; comfort estimation disabled: {0}", [P]),
    ("风扇 {0} 配置无效，已恢复系统自动：{1}", "Fan {0} settings are invalid; system control restored: {1}", [N,P]),
    ("原配置备份无法读取：{0}", "The existing settings backup could not be read: {0}", [O]),
    ("无法检查原配置备份：{0}", "Could not inspect the existing settings backup: {0}", [O]),
    ("无法备份原有配置，禁止覆盖：{0}", "Could not back up existing settings; overwriting is blocked: {0}", [O]),
    ("原配置现在无法安全读取，已阻止覆盖：{0}", "Existing settings cannot currently be read safely; overwriting is blocked: {0}", [O]),
    ("{0} 转/分", "{0} RPM", [O]),
    ("固定 {0}%", "Fixed {0}%", [O]),
    ("最近 10 分钟芯片温度 {0}–{1}°C", "Chip temperature {0}–{1}°C in the last 10 minutes", [O,O]),
    ("已确认交还系统。为避免风扇反复启停，{0} 秒后再尝试接管。", "Return to system confirmed. To avoid starting and stopping the fans repeatedly, takeover resumes in {0} seconds.", [N]),
];

/// Every pattern is static and uses sequential {0}, {1}, ... slots.
fn pattern_parts(pattern: &str) -> Option<(Vec<&str>, Vec<usize>)> {
    let mut literals = Vec::new();
    let mut slots = Vec::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        literals.push(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}')?;
        slots.push(after[..close].parse().ok()?);
        rest = &after[close + 1..];
    }
    literals.push(rest);
    Some((literals, slots))
}

fn match_pattern<'a>(pattern: &str, value: &'a str) -> Option<Vec<&'a str>> {
    let (literals, slots) = pattern_parts(pattern)?;
    let mut rest = value.strip_prefix(literals[0])?;
    let mut captures = Vec::new();
    for (index, slot) in slots.iter().enumerate() {
        if *slot != index {
            return None;
        }
        let next = literals[index + 1];
        if index + 1 == slots.len() {
            captures.push(rest.strip_suffix(next)?);
            rest = "";
        } else {
            if next.is_empty() {
                return None;
            }
            let boundary = rest.find(next)?;
            captures.push(&rest[..boundary]);
            rest = &rest[boundary + next.len()..];
        }
    }
    if !rest.is_empty() {
        return None;
    }
    Some(captures)
}

fn render(pattern: &str, captures: &[String]) -> Option<String> {
    let (literals, slots) = pattern_parts(pattern)?;
    let mut result = literals[0].to_owned();
    for (index, slot) in slots.into_iter().enumerate() {
        result.push_str(captures.get(slot)?);
        result.push_str(literals[index + 1]);
    }
    Some(result)
}

fn numeric(value: &str) -> bool {
    value.parse::<f64>().is_ok_and(f64::is_finite)
}

fn translate_inner(language: Lang, value: &str, depth: usize) -> String {
    if depth > 8 {
        return value.to_owned();
    }
    for text in CATALOG {
        if value == text.zh || value == text.en {
            return if language == Lang::Chinese {
                text.zh
            } else {
                text.en
            }
            .to_owned();
        }
    }
    // Persistence notices are built from a small, explicit set of complete
    // sentences. Recognize those sentence boundaries, never arbitrary words.
    for (zh, en) in [
        (
            "旧版配置已读取；首次保存前将保留备份。",
            "Legacy settings loaded. A backup will be kept before the first save.",
        ),
        (
            "本次自定义设置未应用；恢复保存后请重新应用。",
            "These custom settings were not applied. Apply them again after saving is restored.",
        ),
        (
            "上次保存未完成，已使用系统自动并将重试保存安全配置。",
            "The previous save did not complete. System control is active and saving safe settings will be retried.",
        ),
        (
            "旧版配置已迁移，原文件已备份。",
            "Legacy settings migrated; the original file was backed up.",
        ),
        (
            "上次保存未完成；现已保存系统自动配置，原文件已备份。",
            "The previous save did not complete. System-control settings are now saved and the original file was backed up.",
        ),
    ] {
        for prefix in [zh, en] {
            if let Some(tail) = value
                .strip_prefix(prefix)
                .and_then(|tail| tail.strip_prefix(' '))
            {
                return format!(
                    "{} {}",
                    if language == Lang::Chinese { zh } else { en },
                    translate_inner(language, tail, depth + 1)
                );
            }
        }
    }
    for template in TEMPLATES {
        for source in [template.zh, template.en] {
            let Some(raw) = match_pattern(source, value) else {
                continue;
            };
            if raw.len() != template.captures.len()
                || raw
                    .iter()
                    .zip(template.captures)
                    .any(|(value, kind)| matches!(kind, Capture::Number) && !numeric(value))
            {
                continue;
            }
            let captures: Vec<String> = raw
                .iter()
                .zip(template.captures)
                .map(|(raw, kind)| match kind {
                    Capture::Product => translate_inner(language, raw, depth + 1),
                    Capture::Opaque | Capture::Number => (*raw).to_owned(),
                })
                .collect();
            if let Some(rendered) = render(
                if language == Lang::Chinese {
                    template.zh
                } else {
                    template.en
                },
                &captures,
            ) {
                return rendered;
            }
        }
    }
    // Sensor-detail rows have opaque names and a typed temperature value. This
    // also handles the UI's joined multiline list without rewriting names.
    let mut changed = false;
    let rows: Vec<String> = value
        .split('\n')
        .map(|row| {
            let Some((name, reading)) = row.rsplit_once("  ") else {
                return row.to_owned();
            };
            let stale = match reading {
                "暂不可用" | "Unavailable" => false,
                "数据已过期" | "Stale Data" => true,
                _ => return row.to_owned(),
            };
            changed = true;
            format!(
                "{name}  {}",
                match (language, stale) {
                    (Lang::Chinese, false) => "暂不可用",
                    (Lang::Chinese, true) => "数据已过期",
                    (_, false) => "Unavailable",
                    (_, true) => "Stale Data",
                }
            )
        })
        .collect();
    if changed {
        rows.join("\n")
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn macos_preference_order_and_region_script_variants_are_respected() {
        assert_eq!(
            language_from_preferences(["zh-Hans-SG", "en-US"]),
            Lang::Chinese
        );
        assert_eq!(
            language_from_preferences(["en-GB", "zh-Hant-TW"]),
            Lang::English
        );
        assert_eq!(
            language_from_preferences(["ja-JP", "zh_Hant_HK", "en"]),
            Lang::Chinese
        );
        assert_eq!(language_from_preferences(["de", "EN_us"]), Lang::English);
        assert_eq!(language_from_preferences(["zh", "en"]), Lang::Chinese);
        assert_eq!(
            language_from_preferences(["zhang", "english"]),
            Lang::English
        );
        assert_eq!(language_from_preferences(std::iter::empty()), Lang::English);
    }

    #[test]
    fn preference_keys_are_stable_and_override_is_reversible() {
        struct Restore(Lang);
        impl Drop for Restore {
            fn drop(&mut self) {
                set_override(self.0);
            }
        }
        let _restore = Restore(override_language());
        for language in [Lang::System, Lang::Chinese, Lang::English] {
            assert_eq!(
                Lang::from_preference_key(language.preference_key()),
                Some(language)
            );
        }
        assert_eq!(Lang::from_preference_key("unexpected"), None);
        set_override(Lang::English);
        assert_eq!(translate("应用设置"), "Apply Settings");
        set_override(Lang::Chinese);
        assert_eq!(translate("Apply Settings"), "应用设置");
        set_override(Lang::System);
        assert_eq!(current_language(), system_language());
    }

    #[test]
    fn catalog_is_unambiguous_and_every_pair_round_trips() {
        let mut chinese = BTreeSet::new();
        let mut english = BTreeSet::new();
        for text in CATALOG {
            assert!(
                chinese.insert(text.zh),
                "duplicate Chinese catalog entry {}",
                text.zh
            );
            assert!(
                english.insert(text.en),
                "duplicate English catalog entry {}",
                text.en
            );
            assert_eq!(translate_for(Lang::English, text.zh), text.en);
            assert_eq!(translate_for(Lang::Chinese, text.en), text.zh);
        }
    }

    #[test]
    fn all_templates_have_valid_slots_and_preserve_numeric_formatting() {
        for template in TEMPLATES {
            for pattern in [template.zh, template.en] {
                let (literals, slots) = pattern_parts(pattern).unwrap();
                assert_eq!(slots, (0..template.captures.len()).collect::<Vec<_>>());
                assert_eq!(literals.len(), template.captures.len() + 1);
            }
            let captures: Vec<_> = template
                .captures
                .iter()
                .enumerate()
                .map(|(index, kind)| {
                    if matches!(kind, Capture::Number) {
                        format!("0{index}.50")
                    } else {
                        format!("Opaque-Capture-{index}-Tp01")
                    }
                })
                .collect();
            let chinese = render(template.zh, &captures).unwrap();
            let english = render(template.en, &captures).unwrap();
            assert_eq!(
                translate_for(Lang::English, &chinese),
                english,
                "{}",
                template.zh
            );
            assert_eq!(
                translate_for(Lang::Chinese, &english),
                chinese,
                "{}",
                template.en
            );
        }
        assert_eq!(
            translate_for(Lang::English, "RPM 必须在 0001–6000 之间。"),
            "RPM must be between 0001–6000."
        );
        assert_eq!(
            translate_for(Lang::English, "97.00%（估算）"),
            "97.00% (estimated)"
        );
        assert_eq!(
            translate_for(Lang::English, "风扇名%（估算）"),
            "风扇名%（估算）"
        );
    }

    #[test]
    fn metrics_and_battery_translate_only_their_product_fields() {
        let metrics =
            "CPU  58.0°C     GPU  暂不可用     热负荷  63%\n系统热压力  严重 · 含机身热容模型";
        assert_eq!(translate_for(Lang::English, metrics), "CPU  58.0°C     GPU  Unavailable     Thermal demand  63%\nSystem thermal pressure  Serious · Includes chassis thermal capacity");
        assert_eq!(
            translate_for(
                Lang::English,
                "电量 82% · 已连接电源 · 循环 120 · 健康度 97%（估算）"
            ),
            "Battery 82% · Connected to power · Cycles 120 · Health 97% (estimated)"
        );
        assert_eq!(translate_for(Lang::English, "实际：系统自动 · 1450 RPM  目标已确认 · 低热负荷，交还系统决定停转"), "Actual: System Automatic · 1450 RPM  Target confirmed · Low thermal demand; the system decides whether to stop the fans");
    }

    #[test]
    fn technical_errors_names_and_keys_remain_exactly_intact() {
        let raw = "SMC key Tp01 failed: CPU 性能核心 / 正常 / 未知 / 左侧风扇 (code 0xe00002c7)";
        assert_eq!(
            translate_for(
                Lang::English,
                &format!("传感器读取失败：{raw}，自定义控制将安全回退。")
            ),
            format!("Sensor read failed: {raw}; custom control will fall back safely.")
        );
        assert_eq!(
            translate_for(Lang::English, &format!("风扇 2 设置失败：{raw}")),
            format!("Fan 2 configuration failed: {raw}")
        );
        for value in [
            "Tp01",
            "F0Md",
            "Thermal Demand",
            "Average CPU",
            "左侧风扇",
            "CPU 性能核心",
            "主板",
            "Unknown CPU fan",
            "系统自动风扇",
            "custom 正常 sensor",
            "/tmp/FanControl-diagnostics.json",
            "https://github.com/sm-yjr/fan-control#readme",
            "-12.7500",
            raw,
        ] {
            assert_eq!(translate_for(Lang::English, value), value);
            assert_eq!(translate_for(Lang::Chinese, value), value);
        }
    }

    #[test]
    fn sensor_details_keep_names_and_keys_in_multiline_lists() {
        let source =
            "CPU 性能核心 [Tp01]  58.0°C\n正常  暂不可用\nUnavailable  48.0°C\n主板  暂不可用\n";
        let translated = "CPU 性能核心 [Tp01]  58.0°C\n正常  Unavailable\nUnavailable  48.0°C\n主板  Unavailable\n";
        assert_eq!(translate_for(Lang::English, source), translated);
        assert_eq!(translate_for(Lang::Chinese, translated), source);
    }

    #[test]
    fn reset_persistence_failures_translate_nested_product_prefixes_only() {
        let raw = "permission denied: Tp01 / /private/tmp/Unknown";
        let source =
            format!("已交还系统自动控制。 配置保存失败，重启前请修复：配置保存失败：{raw}");
        assert_eq!(translate_for(Lang::English, &source), format!("Control returned to the system. Settings could not be saved; fix this before restarting: Could not save settings: {raw}"));
        assert_eq!(
            translate_for(
                Lang::English,
                "控制服务需要更新（协议 Some(4) → 5）。点击启用 / 修复。"
            ),
            "Control service needs an update (protocol Some(4) → 5). Click Enable / Repair."
        );
    }

    #[test]
    fn adaptive_policy_distinguishes_estimates_from_surface_measurements() {
        let status = "CPU 利用率 92.5% · 芯片预测 91.0°C\n散热依据 持续负载提前介入 · 热浸 63%\n键盘表面估计 38.1°C · 校准范围内的温度估计";
        let english = "CPU utilization 92.5% · Predicted chip temp 91.0°C\nCooling reason Early intervention for sustained load · Heat soak 63%\nEstimated keyboard surface 38.1°C · Temperature estimate within the calibrated range";
        assert_eq!(translate_for(Lang::English, status), english);
        assert_eq!(translate_for(Lang::Chinese, english), status);
        assert_eq!(
            translate_for(Lang::English, "键盘表面实测 °C"),
            "Measured Keyboard Surface °C"
        );
        assert_eq!(
            translate_for(Lang::English, "超出校准范围，体感调节已暂停"),
            "Outside the calibrated range; comfort regulation paused"
        );
        assert_eq!(
            translate_for(Lang::English, "已记录点 02。两个记录点完成后，点击保存。"),
            "Point 02 recorded. Complete both points, then click Save."
        );
        assert_eq!(
            translate_for(Lang::English, "Tm0P · 暂不可用"),
            "Tm0P · Unavailable"
        );
    }

    #[test]
    fn config_errors_translate_wrappers_but_preserve_technical_json_payloads() {
        let payload = "unknown field `键盘体感目标` at line 4 column 12: Tp01, CPU 性能核心, 38.00";
        let wrapped = format!(
            "配置读取失败，已使用系统自动；原文件将先备份再保存：配置 JSON 解析失败：{payload}"
        );
        assert_eq!(translate_for(Lang::English, &wrapped), format!("Could not read settings; system control is active. The original file will be backed up before saving: Settings JSON could not be parsed: {payload}"));
        assert_eq!(
            translate_for(Lang::English, "不支持配置版本 06，当前版本为 03"),
            "Unsupported settings version 06; the current version is 03"
        );
        assert_eq!(translate_for(Lang::English, "热策略或表面校准无效，已禁用舒适估计：舒适目标必须为 30...45°C"), "Thermal policy or surface calibration is invalid; comfort estimation disabled: Comfort target must be within 30...45°C");
    }

    #[test]
    fn persistence_notice_sequences_translate_complete_sentences() {
        let source = "旧版配置已迁移，原文件已备份。 上次保存未完成，已使用系统自动并将重试保存安全配置。 本次自定义设置未应用；恢复保存后请重新应用。";
        let english = "Legacy settings migrated; the original file was backed up. The previous save did not complete. System control is active and saving safe settings will be retried. These custom settings were not applied. Apply them again after saving is restored.";
        assert_eq!(translate_for(Lang::English, source), english);
        assert_eq!(translate_for(Lang::Chinese, english), source);
        let error = "permission denied: /tmp/Tp01";
        let source = format!("{source} 保存失败，将重试当前设置：配置保存未获持久确认，下次启动将使用系统自动：{error}");
        let english = format!("{english} Saving failed; current settings will be retried: The settings save was not confirmed durable; system control will be used at next launch: {error}");
        assert_eq!(translate_for(Lang::English, &source), english);
    }

    #[test]
    fn current_battery_and_hardware_templates_preserve_raw_measurements() {
        let battery = "电量 82.0% · 已连接电源 · 电池功率 -12.5 W · 适配器 96.0 W\n循环 00120 · 健康度 97%（估算）";
        assert_eq!(translate_for(Lang::English, battery), "Battery 82.0% · Connected to power · Battery power -12.5 W · Adapter 96.0 W\nCycles 00120 · Health 97% (estimated)");
        let actual = "硬件：模式未知 · 当前 数据已过期\n目标 1800 RPM · 最近确认 1750 RPM · 控制服务失联，交还系统尚未确认";
        assert_eq!(translate_for(Lang::English, actual), "Hardware: Mode Unknown · Current Stale Data\nTarget 1800 RPM · Last confirmed 1750 RPM · Control service disconnected; system handback is not yet confirmed");
        let sensors = "数据已过期 [Tp01]  数据已过期\n正常 [Tm0P]  暂不可用";
        assert_eq!(
            translate_for(Lang::English, sensors),
            "数据已过期 [Tp01]  Stale Data\n正常 [Tm0P]  Unavailable"
        );
    }

    #[test]
    fn main_adaptive_metrics_translate_nine_typed_fields_in_three_lines() {
        let source = "CPU  73.2°C · GPU  68.0°C · 智能需求 76% · 系统热压力 正常\nCPU 利用率 98.4% · 芯片预测 89.5°C · 升温趋势提前介入\n键盘表面估计 38.00°C · 校准范围内的温度估计";
        let english = "CPU  73.2°C · GPU  68.0°C · Smart demand 76% · System thermal pressure Normal\nCPU utilization 98.4% · Predicted chip temp 89.5°C · Early intervention for a rising temperature trend\nEstimated keyboard surface 38.00°C · Temperature estimate within the calibrated range";
        assert_eq!(translate_for(Lang::English, source), english);
        assert_eq!(translate_for(Lang::Chinese, english), source);
        let stale = "CPU  数据已过期 · GPU  暂不可用 · 智能需求 暂不可用 · 系统热压力 升高\nCPU 利用率 暂不可用 · 芯片预测 暂不可用 · 当前温度\n键盘表面估计 暂不可用 · 未校准，仅启用性能策略";
        assert_eq!(translate_for(Lang::English, stale), "CPU  Stale Data · GPU  Unavailable · Smart demand Unavailable · System thermal pressure Elevated\nCPU utilization Unavailable · Predicted chip temp Unavailable · Current temperature\nEstimated keyboard surface Unavailable · Not calibrated; performance policy only");
    }

    #[test]
    fn edit_menu_calibration_dirty_notice_and_mode_scope_are_localized() {
        for (zh, en) in [
            ("编辑", "Edit"),
            ("撤销", "Undo"),
            ("重做", "Redo"),
            ("剪切", "Cut"),
            ("拷贝", "Copy"),
            ("粘贴", "Paste"),
            ("全选", "Select All"),
        ] {
            assert_eq!(translate_for(Lang::English, zh), en);
            assert_eq!(translate_for(Lang::Chinese, en), zh);
        }
        for shortcut in ["z", "Z", "x", "c", "v", "a", "\r", "\u{1b}"] {
            assert_eq!(translate_for(Lang::English, shortcut), shortcut);
            assert_eq!(translate_for(Lang::Chinese, shortcut), shortcut);
        }
        assert_eq!(translate_for(Lang::English, "实测值已修改，请重新记录对应校准点后保存。"), "The measured value changed. Record the corresponding calibration point again before saving.");
        let explanation = "目标用于“智能热管理”模式；固定转速与曲线保留独立调节。性能策略按持续 CPU 负载、CPU/GPU 升温趋势与热浸提前增加散热。表面温度需要外部实测校准。";
        let english = translate_for(Lang::English, explanation);
        assert!(english.starts_with(
            "Targets apply to Smart Thermal Control; fixed RPM and curves keep separate settings."
        ));
        assert!(english
            .ends_with("Surface temperature requires calibration with external measurements."));
        assert_eq!(translate_for(Lang::Chinese, &english), explanation);
    }

    // Check actual source literals so a new product label cannot silently be
    // omitted. Exclusions are data names, deliberately preserved in both languages.
    #[test]
    fn current_product_sources_have_explicit_catalog_or_template_coverage() {
        let preserved_names = [
            "左侧风扇",
            "右侧风扇",
            "CPU 性能核心",
            "主板",
            // presenter.rs matches these fragments of worker text; never shown.
            "需要更新",
            "失败",
            "无法",
            "未获",
        ];
        for (file, source) in [
            ("ui.rs", include_str!("ui.rs")),
            ("worker.rs", include_str!("worker.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("chart.rs", include_str!("chart.rs")),
            ("presenter.rs", include_str!("presenter.rs")),
            ("popover.rs", include_str!("popover.rs")),
            ("overview.rs", include_str!("overview.rs")),
            ("trend.rs", include_str!("trend.rs")),
            ("fans.rs", include_str!("fans.rs")),
            ("details.rs", include_str!("details.rs")),
            ("form.rs", include_str!("form.rs")),
            ("gauge.rs", include_str!("gauge.rs")),
            ("preferences.rs", include_str!("preferences.rs")),
        ] {
            check_source_coverage(
                file,
                source.split("#[cfg(test)]").next().unwrap_or(source),
                &preserved_names,
            );
        }
        for (file, source) in [
            (
                "fan-core/config.rs",
                include_str!("../../fan-core/src/config.rs"),
            ),
            (
                "fan-core/adaptive.rs",
                include_str!("../../fan-core/src/adaptive.rs"),
            ),
            (
                "fan-core/tuning.rs",
                include_str!("../../fan-core/src/tuning.rs"),
            ),
        ] {
            check_source_coverage(
                file,
                source.split("#[cfg(test)]").next().unwrap_or(source),
                &preserved_names,
            );
        }
        // policy.rs is being added independently. Read it when present so its
        // latest product messages are checked without changing other modules.
        let manifest = option_env!("CARGO_MANIFEST_DIR").unwrap_or("crates/fan-app");
        let policy = std::path::Path::new(manifest).join("src/policy.rs");
        if policy.exists() {
            let source =
                std::fs::read_to_string(policy).expect("read policy source for catalog audit");
            check_source_coverage(
                "policy.rs",
                source.split("#[cfg(test)]").next().unwrap_or(&source),
                &preserved_names,
            );
        }
    }

    fn check_source_coverage(file: &str, source: &str, preserved_names: &[&str]) {
        for literal in string_literals(source) {
            if !literal
                .chars()
                .any(|ch| ('\u{3400}'..='\u{9fff}').contains(&ch))
                || preserved_names.contains(&literal.as_str())
            {
                continue;
            }
            let canonical = canonical_format(&literal);
            // A leading space is an append-only worker suffix, covered as
            // the explicit composite reset/persistence template.
            let covered = CATALOG.iter().any(|text| text.zh == literal)
                || TEMPLATES.iter().any(|template| {
                    template.zh == canonical
                        || template.zh.strip_prefix("{0}").map(canonical_format)
                            == Some(canonical.clone())
                });
            assert!(covered, "uncovered product text in {file}: {literal}");
        }
    }

    fn string_literals(source: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut chars = source.chars();
        while let Some(ch) = chars.next() {
            if ch != '"' {
                continue;
            }
            let mut value = String::new();
            while let Some(ch) = chars.next() {
                match ch {
                    '"' => break,
                    '\\' => {
                        if let Some(escaped) = chars.next() {
                            value.push(match escaped {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                other => other,
                            });
                        }
                    }
                    other => value.push(other),
                }
            }
            result.push(value);
        }
        result
    }

    fn canonical_format(source: &str) -> String {
        let mut rest = source;
        let mut result = String::new();
        let mut slot = 0;
        while let Some(open) = rest.find('{') {
            result.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else {
                return source.to_owned();
            };
            result.push_str(&format!("{{{slot}}}"));
            slot += 1;
            rest = &after[close + 1..];
        }
        result.push_str(rest);
        result
    }
}
