//! Turns worker state into plain-language copy for personal users.
//!
//! Everything here is pure so the wording, priorities and the unified fan plan
//! stay testable without AppKit. Strings are canonical Chinese; the UI layer
//! translates them through the i18n catalog.
use crate::worker::UiSnapshot;
use fan_core::{validated_rpm_range, ControlMode, Fan, FanConfig, HardwareMode, ThermalPressure};

/// Below this tachometer reading a fan is treated as stopped.
const STOPPED_RPM: f64 = 100.;
/// Silicon trend threshold, about 3°C per minute.
const TREND_PER_SECOND: f64 = 0.05;
pub const DEFAULT_CUSTOM_PERCENT: f64 = 40.;
/// Worker status for a fan whose takeover waits after a failed write.
pub const HELD_STATUS: &str = "暂缓接管，稍后自动重试";
const HANDBACK_PENDING_REASON: &str = "交还系统尚未确认；控制写入已暂停，正在重试。";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Good,
    Info,
    Warning,
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    EnableControl,
    Reconnect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub tone: Tone,
    pub title: &'static str,
    pub body: &'static str,
    pub action: Option<Action>,
}

/// One cooling plan applied to every fan, as chosen in the menu bar panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    System,
    Smart,
    Custom,
    /// Fans were configured separately in the main window.
    Mixed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FanLine {
    pub id: u8,
    pub name: String,
    /// Tachometer reading, never the requested target.
    pub speed: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Presentation {
    pub notice: Notice,
    pub temperature: Option<f64>,
    pub trend: &'static str,
    pub demand: &'static str,
    pub battery: Option<f64>,
    pub fans: Vec<FanLine>,
    pub plan: Plan,
    pub plan_description: &'static str,
    pub custom_percent: f64,
    /// At least one fan follows its temperature curve instead of a fixed speed.
    pub curve_active: bool,
    /// The plan can be changed now: service ready, fresh data, controllable fans.
    pub controls_enabled: bool,
    /// Fastest fan as a share of its hardware range, for the trend chart.
    pub fan_percent: Option<f64>,
}

pub fn present(state: &UiSnapshot, fresh: bool, demo: bool) -> Presentation {
    let plan = plan(state);
    let fans_present = !state.snapshot.fans.is_empty();
    let controls_enabled = fresh
        && state.helper_ready
        && !state.installing
        && !state.handback_pending
        && state.snapshot.fans.iter().any(Fan::controllable);
    Presentation {
        notice: notice(state, fresh, demo, plan),
        temperature: fresh.then(|| chip_temperature(state)).flatten(),
        trend: if !fresh {
            "数据中断"
        } else if state.thermal.adaptive.silicon_rise_celsius_per_second > TREND_PER_SECOND {
            "上升中"
        } else if state.thermal.adaptive.silicon_rise_celsius_per_second < -TREND_PER_SECOND {
            "回落中"
        } else {
            "平稳"
        },
        demand: if !fresh || !state.thermal.adaptive.available {
            "未知"
        } else if state
            .thermal
            .adaptive
            .adjusted_demand_percent
            .unwrap_or(state.thermal.adaptive.demand_percent)
            < 35.
        {
            "低"
        } else if state
            .thermal
            .adaptive
            .adjusted_demand_percent
            .unwrap_or(state.thermal.adaptive.demand_percent)
            < 70.
        {
            "中"
        } else {
            "高"
        },
        battery: state
            .battery
            .as_ref()
            .and_then(|battery| battery.charge_percent),
        fans: state
            .snapshot
            .fans
            .iter()
            .map(|fan| fan_line(state, fan, fresh))
            .collect(),
        plan,
        plan_description: match plan {
            Plan::System => "由 macOS 决定何时启动风扇，与未安装本应用时一致。",
            Plan::Smart => "按持续热负载平稳散热，热量消散后交还系统。",
            Plan::Custom => "风扇按你设定的速度或温度曲线运行，过热时安全保护仍会接管。",
            Plan::Mixed => "各风扇设置不同，可在主窗口的“风扇”页分别调整。",
        },
        custom_percent: custom_percent(state),
        curve_active: state
            .snapshot
            .fans
            .iter()
            .any(|fan| matches!(configured(state, fan.id), ControlMode::Curve { .. })),
        controls_enabled: controls_enabled && fans_present,
        fan_percent: fresh
            .then(|| {
                state
                    .snapshot
                    .fans
                    .iter()
                    .filter_map(|fan| {
                        let (minimum, maximum) = validated_rpm_range(fan.min_rpm, fan.max_rpm)?;
                        let rpm = measured_fan_rpm(fan, fresh)?;
                        Some(if rpm < STOPPED_RPM {
                            0.
                        } else {
                            percent_of_range(rpm, minimum, maximum)
                        })
                    })
                    .reduce(f64::max)
            })
            .flatten(),
    }
}

fn notice(state: &UiSnapshot, fresh: bool, demo: bool, plan: Plan) -> Notice {
    let notice = |tone, title, body, action| Notice {
        tone,
        title,
        body,
        action,
    };
    if demo {
        return notice(
            Tone::Info,
            "演示模式",
            "数据为模拟值，不会控制真实风扇。",
            None,
        );
    }
    if state.handback_pending {
        return notice(
            if fan_core::missing_input_safety_percent(
                state.snapshot.thermal_pressure,
                state.snapshot.hottest_silicon(),
            )
            .is_some()
            {
                Tone::Danger
            } else {
                Tone::Warning
            },
            "交还系统尚未确认",
            HANDBACK_PENDING_REASON,
            Some(Action::Reconnect),
        );
    }
    if state.discovered && state.snapshot.fan_count == Some(0.) {
        return notice(
            Tone::Info,
            "这台 Mac 没有风扇",
            "可以继续查看温度，散热由 macOS 管理。",
            None,
        );
    }
    if state.installing {
        return notice(
            Tone::Info,
            "等待管理员授权",
            "在系统弹出的窗口中输入密码。取消也不影响查看温度。",
            None,
        );
    }
    if !fresh {
        return notice(
            Tone::Warning,
            "温度数据暂时中断",
            "正在重新读取；在此期间风扇由 macOS 控制。",
            Some(Action::Reconnect),
        );
    }
    if matches!(
        state.snapshot.thermal_pressure,
        ThermalPressure::Serious | ThermalPressure::Critical
    ) {
        return notice(
            Tone::Danger,
            "温度偏高，正在全力散热",
            if plan == Plan::System {
                "macOS 正在限制性能以控制温度。"
            } else {
                "安全保护已临时接管，温度回落后恢复你的设置。"
            },
            None,
        );
    }
    if state.safety_active {
        return notice(
            Tone::Warning,
            "风扇已交还系统",
            "控制服务暂时没有响应，macOS 已安全接管风扇。",
            Some(Action::Reconnect),
        );
    }
    if !state.helper_ready {
        return if state.message.contains("需要更新") {
            notice(
                Tone::Info,
                "需要更新风扇控制组件",
                "更新需要一次管理员授权，温度可以照常查看。",
                Some(Action::EnableControl),
            )
        } else {
            notice(
                Tone::Info,
                "需要一次授权",
                "授权后才能调节风扇，温度可以照常查看。",
                Some(Action::EnableControl),
            )
        };
    }
    if state
        .fan_status
        .values()
        .any(|status| status == HELD_STATUS)
    {
        return notice(
            Tone::Warning,
            "风扇暂时交还系统",
            "上次控制没有成功。为避免风扇反复启停，稍后会自动再试。",
            None,
        );
    }
    if state
        .fan_status
        .values()
        .any(|status| status.contains("失败"))
    {
        return notice(
            Tone::Warning,
            "有风扇没有按设置运行",
            "已交还 macOS 控制，稍后会自动重试。",
            None,
        );
    }
    if ["失败", "无法", "未获"]
        .iter()
        .any(|word| state.configuration_notice.contains(word))
    {
        return notice(
            Tone::Warning,
            "设置可能没有保存",
            "当前设置仍在生效，但重启后可能恢复默认。详情见“设置”页。",
            None,
        );
    }
    notice(
        Tone::Good,
        "运行正常",
        match plan {
            Plan::System => "风扇由 macOS 控制。",
            Plan::Smart => "负载升高时会提前散热。",
            Plan::Custom | Plan::Mixed => "风扇按你的设置运行。",
        },
        None,
    )
}

fn chip_temperature(state: &UiSnapshot) -> Option<f64> {
    state
        .snapshot
        .input_value("Average CPU", &state.thermal)
        .or_else(|| state.snapshot.hottest_silicon())
}

fn configured(state: &UiSnapshot, id: u8) -> ControlMode {
    state
        .config
        .fans
        .iter()
        .find(|fan| fan.fan_id == id)
        .map(|fan| fan.mode.clone())
        .unwrap_or_default()
}

/// Only fresh, finite nonnegative tachometer readings are measured speeds.
/// Requested targets, confirmation and cooling preferences never enter this value.
fn measured_fan_rpm(fan: &Fan, fresh: bool) -> Option<f64> {
    fresh
        .then(|| {
            fan.current_rpm.filter(|rpm| {
                rpm.is_finite() && (0.0..=fan_core::ABSOLUTE_MAXIMUM_RPM as f64).contains(rpm)
            })
        })
        .flatten()
}

fn fan_line(state: &UiSnapshot, fan: &Fan, fresh: bool) -> FanLine {
    let speed = match measured_fan_rpm(fan, fresh) {
        None => "转速未知".to_string(),
        Some(rpm) if rpm < STOPPED_RPM => "停转".to_string(),
        Some(rpm) => format!("{} 转/分", grouped(rpm.round() as u64)),
    };
    let detail = if !fresh {
        "状态未知".to_string()
    } else {
        match (fan.mode, configured(state, fan.id)) {
            (HardwareMode::Unknown, _) => "状态未知".to_string(),
            (HardwareMode::Automatic, _) => "由系统控制".to_string(),
            (HardwareMode::Forced, ControlMode::Manual { rpm }) => {
                match validated_rpm_range(fan.min_rpm, fan.max_rpm) {
                    Some((minimum, maximum)) => format!(
                        "固定 {:.0}%",
                        percent_of_range(rpm as f64, minimum, maximum)
                    ),
                    None => "固定速度".to_string(),
                }
            }
            (HardwareMode::Forced, ControlMode::Curve { .. }) => "按温度曲线".to_string(),
            (HardwareMode::Forced, ControlMode::Adaptive) => "智能散热".to_string(),
            (HardwareMode::Forced, ControlMode::Automatic) => "正在交还系统".to_string(),
        }
    };
    FanLine {
        id: fan.id,
        name: fan.name.clone(),
        speed,
        detail,
    }
}

/// Plans are derived from saved settings of the fans that exist right now.
pub fn plan(state: &UiSnapshot) -> Plan {
    let mut plans = state
        .snapshot
        .fans
        .iter()
        .map(|fan| match configured(state, fan.id) {
            ControlMode::Automatic => Plan::System,
            ControlMode::Adaptive => Plan::Smart,
            ControlMode::Manual { .. } | ControlMode::Curve { .. } => Plan::Custom,
        });
    let Some(first) = plans.next() else {
        return Plan::System;
    };
    if plans.all(|plan| plan == first) {
        first
    } else {
        Plan::Mixed
    }
}

fn custom_percent(state: &UiSnapshot) -> f64 {
    state
        .snapshot
        .fans
        .iter()
        .find_map(|fan| match configured(state, fan.id) {
            ControlMode::Manual { rpm } => validated_rpm_range(fan.min_rpm, fan.max_rpm)
                .map(|(minimum, maximum)| percent_of_range(rpm as f64, minimum, maximum)),
            _ => None,
        })
        .unwrap_or(DEFAULT_CUSTOM_PERCENT)
}

/// 0% is the hardware minimum speed and 100% the hardware maximum.
pub fn percent_of_range(rpm: f64, minimum: u32, maximum: u32) -> f64 {
    if maximum <= minimum {
        return 0.;
    }
    ((rpm - minimum as f64) / (maximum - minimum) as f64 * 100.).clamp(0., 100.)
}

pub fn rpm_for_percent(percent: f64, minimum: u32, maximum: u32) -> u32 {
    let minimum = minimum.max(1);
    if maximum <= minimum {
        return minimum;
    }
    let percent = if percent.is_finite() {
        percent.clamp(0., 100.)
    } else {
        0.
    };
    (minimum as f64 + (maximum - minimum) as f64 * percent / 100.)
        .round()
        .clamp(minimum as f64, maximum as f64) as u32
}

/// Settings that apply one plan to every fan. Fans without a validated
/// hardware range only ever receive system control.
pub fn configs_for_plan(state: &UiSnapshot, plan: Plan, percent: f64) -> Vec<FanConfig> {
    state
        .snapshot
        .fans
        .iter()
        .filter_map(|fan| {
            let mut config = state
                .config
                .fans
                .iter()
                .find(|config| config.fan_id == fan.id)
                .cloned()
                .unwrap_or_else(|| FanConfig::automatic(fan.id));
            let range = validated_rpm_range(fan.min_rpm, fan.max_rpm);
            config.mode = match (plan, range) {
                (Plan::System, _) => ControlMode::Automatic,
                (_, None) => return None,
                (Plan::Smart, Some(_)) => ControlMode::Adaptive,
                (Plan::Custom, Some((minimum, maximum))) => ControlMode::Manual {
                    rpm: rpm_for_percent(percent, minimum, maximum),
                },
                (Plan::Mixed, Some(_)) => return None,
            };
            Some(config)
        })
        .collect()
}

/// Starting points for the temperature curve editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurvePreset {
    Quiet,
    Balanced,
    Cool,
}

impl CurvePreset {
    pub const ALL: [CurvePreset; 3] = [Self::Quiet, Self::Balanced, Self::Cool];
}

/// Every preset keeps the balanced inputs and ends at full speed. Quiet softens
/// the running speeds but still hands back to the system at low load; cool
/// never stops and runs at least 15%.
pub fn preset_curve(preset: CurvePreset, id: &str, sensor_key: &str) -> fan_core::Curve {
    let mut curve = fan_core::Curve::balanced(id, sensor_key);
    let last = curve.points.len().saturating_sub(1);
    for (index, point) in curve.points.iter_mut().enumerate() {
        if index == last {
            continue;
        }
        point.speed_percent = match preset {
            CurvePreset::Balanced => point.speed_percent,
            CurvePreset::Quiet if point.speed_percent > 0. => (point.speed_percent * 0.6).round(),
            CurvePreset::Quiet => point.speed_percent,
            CurvePreset::Cool => (point.speed_percent.max(0.) + 15.).min(100.),
        };
    }
    curve
}

pub fn matching_preset(curve: &fan_core::Curve) -> Option<CurvePreset> {
    CurvePreset::ALL.into_iter().find(|preset| {
        let candidate = preset_curve(*preset, &curve.id, &curve.sensor_key);
        candidate.points == curve.points && candidate.hysteresis == curve.hysteresis
    })
}

/// Whole numbers stay whole; anything else shows one decimal.
pub fn format_value(value: f64) -> String {
    let rounded = (value * 10.).round() / 10.;
    if rounded.fract() == 0. {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

/// One edited curve applied to every controllable fan. Each fan keeps its own
/// curve identity so per-fan settings remain distinct in the saved file.
pub fn configs_for_curve(state: &UiSnapshot, edited: &fan_core::Curve) -> Vec<FanConfig> {
    state
        .snapshot
        .fans
        .iter()
        .filter(|fan| fan.controllable())
        .map(|fan| {
            let mut config = state
                .config
                .fans
                .iter()
                .find(|config| config.fan_id == fan.id)
                .cloned()
                .unwrap_or_else(|| FanConfig::automatic(fan.id));
            let id = config
                .curve
                .as_ref()
                .map(|curve| curve.id.clone())
                .unwrap_or_else(|| format!("{}-fan{}", edited.id, fan.id));
            let mut curve = edited.clone();
            curve.id = id;
            config.mode = ControlMode::Curve {
                curve_id: curve.id.clone(),
            };
            config.curve = Some(curve);
            config
        })
        .collect()
}

/// How one fan is set up on the Fans page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanChoice {
    System,
    Smart,
    Fixed,
    Curve,
}

impl FanChoice {
    pub const ALL: [FanChoice; 4] = [Self::System, Self::Smart, Self::Fixed, Self::Curve];
    pub fn title(self) -> &'static str {
        match self {
            Self::System => "系统默认",
            Self::Smart => "智能散热",
            Self::Fixed => "固定速度",
            Self::Curve => "温度曲线",
        }
    }
    pub fn explanation(self) -> &'static str {
        match self {
            Self::System => "由 macOS 决定何时启动风扇。",
            Self::Smart => "按持续热负载平稳散热，冷却后交还系统。",
            Self::Fixed => "始终保持所选速度，过热时安全保护仍会接管。",
            Self::Curve => "按散热需求自动调节，可编辑曲线。",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FanCard {
    pub id: u8,
    pub name: String,
    /// Fresh tachometer value; available even when the hardware range is unknown.
    /// Separate from configured, requested or acknowledged speeds.
    pub measured_rpm: Option<f64>,
    /// Tachometer reading as a share of the hardware range; 0 when stopped.
    pub percent: Option<f64>,
    pub speed: String,
    pub detail: String,
    pub choice: FanChoice,
    /// Saved fixed speed, or the default for a fan without one.
    pub fixed_percent: f64,
    pub range: Option<(u32, u32)>,
    /// Requested and acknowledged speeds, kept apart from the measured speed.
    pub target: String,
}

pub fn fan_cards(state: &UiSnapshot, fresh: bool) -> Vec<FanCard> {
    state
        .snapshot
        .fans
        .iter()
        .map(|fan| {
            let line = fan_line(state, fan, fresh);
            let range = validated_rpm_range(fan.min_rpm, fan.max_rpm);
            let mode = configured(state, fan.id);
            FanCard {
                id: fan.id,
                name: line.name,
                measured_rpm: measured_fan_rpm(fan, fresh),
                percent: fresh
                    .then(|| {
                        let (minimum, maximum) = range?;
                        let rpm = measured_fan_rpm(fan, fresh)?;
                        Some(if rpm < STOPPED_RPM {
                            0.
                        } else {
                            percent_of_range(rpm, minimum, maximum)
                        })
                    })
                    .flatten(),
                speed: line.speed,
                detail: line.detail,
                choice: match mode {
                    ControlMode::Automatic => FanChoice::System,
                    ControlMode::Adaptive => FanChoice::Smart,
                    ControlMode::Manual { .. } => FanChoice::Fixed,
                    ControlMode::Curve { .. } => FanChoice::Curve,
                },
                fixed_percent: match (mode, range) {
                    (ControlMode::Manual { rpm }, Some((minimum, maximum))) => {
                        percent_of_range(rpm as f64, minimum, maximum).round()
                    }
                    _ => DEFAULT_CUSTOM_PERCENT,
                },
                range,
                target: match (
                    state.targets.get(&fan.id).copied().flatten(),
                    state.confirmed_targets.get(&fan.id).copied().flatten(),
                ) {
                    (Some(target), Some(confirmed)) => format!(
                        "目标 {} 转/分 · 硬件已确认 {} 转/分",
                        grouped(target as u64),
                        grouped(confirmed as u64)
                    ),
                    (Some(target), None) => {
                        format!("目标 {} 转/分 · 等待硬件确认", grouped(target as u64))
                    }
                    _ => String::new(),
                },
            }
        })
        .collect()
}

/// The saved setting for one fan after choosing `choice` on the Fans page.
/// Fans without a validated range only ever return to system control.
pub fn config_for_choice(
    state: &UiSnapshot,
    id: u8,
    choice: FanChoice,
    percent: f64,
) -> Option<FanConfig> {
    let fan = state.snapshot.fans.iter().find(|fan| fan.id == id)?;
    let mut config = state
        .config
        .fans
        .iter()
        .find(|config| config.fan_id == id)
        .cloned()
        .unwrap_or_else(|| FanConfig::automatic(id));
    let range = validated_rpm_range(fan.min_rpm, fan.max_rpm);
    config.mode = match (choice, range) {
        (FanChoice::System, _) => ControlMode::Automatic,
        (_, None) => return None,
        (FanChoice::Smart, Some(_)) => ControlMode::Adaptive,
        (FanChoice::Fixed, Some((minimum, maximum))) => ControlMode::Manual {
            rpm: rpm_for_percent(percent, minimum, maximum),
        },
        (FanChoice::Curve, Some(_)) => {
            let curve = config.curve.get_or_insert_with(|| {
                fan_core::Curve::balanced(
                    format!("native-balanced-fan{id}"),
                    fan_core::THERMAL_DEMAND_KEY,
                )
            });
            ControlMode::Curve {
                curve_id: curve.id.clone(),
            }
        }
    };
    Some(config)
}

/// Approximate speed shown under a fixed-speed slider.
pub fn rpm_hint(percent: f64, range: Option<(u32, u32)>) -> String {
    match range {
        Some((minimum, maximum)) => format!(
            "约 {} 转/分",
            grouped(rpm_for_percent(percent, minimum, maximum) as u64)
        ),
        None => String::new(),
    }
}

/// Component temperatures: calm, warm, hot, critical.
pub fn temperature_level(celsius: f64) -> Tone {
    if celsius < 65. {
        Tone::Good
    } else if celsius < 85. {
        Tone::Info
    } else if celsius < 100. {
        Tone::Warning
    } else {
        Tone::Danger
    }
}

/// Plain-language state of smart cooling for its page.
#[derive(Clone, Debug, PartialEq)]
pub struct SmartStatus {
    pub demand: Option<f64>,
    pub level: &'static str,
    pub reason: &'static str,
    pub in_use: bool,
}

pub fn smart_status(state: &UiSnapshot, fresh: bool) -> SmartStatus {
    use fan_core::AdaptiveIntervention as Why;
    let reading = &state.thermal.adaptive;
    let actual_demand = reading
        .adjusted_demand_percent
        .unwrap_or(reading.demand_percent);
    let available = fresh && reading.available && actual_demand.is_finite();
    let demand = available.then_some(actual_demand.clamp(0., 100.));
    // Match the controller's release prerequisite, including its safety floors.
    // UiSnapshot has no residence timers: this describes waiting, not completion.
    let safety_floor = fan_core::missing_input_safety_percent(
        state.snapshot.thermal_pressure,
        state.snapshot.hottest_silicon(),
    )
    .unwrap_or(0.0);
    let effective_demand = actual_demand.max(safety_floor).max(
        if state.snapshot.thermal_pressure == ThermalPressure::Fair {
            35.0
        } else {
            0.0
        },
    );
    let actively_controlled = state.snapshot.fans.iter().any(|fan| {
        configured(state, fan.id) == ControlMode::Adaptive
            && fan.mode == HardwareMode::Forced
            && state
                .confirmed_targets
                .get(&fan.id)
                .is_some_and(Option::is_some)
            && state.targets.get(&fan.id).is_some_and(Option::is_some)
    });
    let cooling_held = actively_controlled
        && !state.safety_active
        && reading.low_demand_for_release(effective_demand);
    let smart_fans: Vec<_> = state
        .snapshot
        .fans
        .iter()
        .filter(|fan| configured(state, fan.id) == ControlMode::Adaptive)
        .collect();
    let system_idle = !smart_fans.is_empty()
        && smart_fans.iter().all(|fan| {
            fan.mode == HardwareMode::Automatic && state.targets.get(&fan.id) == Some(&None)
        });
    SmartStatus {
        demand,
        level: match demand {
            None => "暂无数据",
            Some(value) if value < 35. => "低",
            Some(value) if value < 70. => "中",
            Some(_) => "高",
        },
        reason: if state.handback_pending {
            HANDBACK_PENDING_REASON
        } else if !available {
            "正在等待温度数据。"
        } else if state.safety_active
            && fan_core::missing_input_safety_percent(
                state.snapshot.thermal_pressure,
                state.snapshot.hottest_silicon(),
            )
            .is_some()
        {
            "温度或系统热压力过高，安全保护正在加大散热。"
        } else if cooling_held {
            "散热需求较低，待持续冷却后交还系统。"
        } else if system_idle && actual_demand < fan_core::ADAPTIVE_START_DEMAND_PERCENT {
            "负载较低，风扇交给系统，保持安静。"
        } else {
            match reading.intervention {
                Why::Idle if !actively_controlled => "负载较低，风扇交给系统，保持安静。",
                Why::Idle => "正在按持续热负载平稳调节风扇。",
                Why::Temperature => "正在按持续热负载平稳调节风扇。",
                Why::LoadFeedForward => "检测到持续高负载，正在逐步增加散热。",
                Why::RisingTemperature => "检测到持续升温趋势，逐步增加散热。",
                Why::HeatSoak => "机身积累了热量，保持适度散热帮助降温。",
                Why::Comfort => "为让键盘区域保持舒适，正在加大散热。",
            }
        },
        in_use: state
            .snapshot
            .fans
            .iter()
            .any(|fan| configured(state, fan.id) == ControlMode::Adaptive),
    }
}

/// Battery state and health in plain words, without raw power-source fields.
pub fn battery_lines(battery: &fan_platform::BatteryReading) -> (String, Vec<String>) {
    let state = match (battery.is_charging, battery.is_on_ac) {
        (Some(true), _) => "正在充电",
        (_, Some(true)) => "已连接电源",
        (_, Some(false)) => "使用电池",
        _ => "电源状态未知",
    };
    let state = match battery.power_watts.filter(|watts| watts.abs() >= 0.5) {
        Some(watts) if watts < 0. => format!("{} · 耗电 {:.1} W", state, -watts),
        Some(watts) => format!("{} · 充入 {:.1} W", state, watts),
        None => state.to_string(),
    };
    let mut details = Vec::new();
    if let Some(health) = battery.health_percent {
        details.push(if battery.health_is_estimate {
            format!("健康度约 {health:.0}%")
        } else {
            format!("健康度 {health:.0}%")
        });
    }
    if let Some(cycles) = battery.cycle_count {
        details.push(format!("已充电循环 {cycles} 次"));
    }
    if let Some(adapter) = battery.adapter_watts {
        details.push(format!("电源适配器 {adapter:.0} W"));
    }
    (state, details)
}

fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fan_core::{Config, Snapshot, ThermalReading};
    use std::collections::BTreeMap;
    use std::time::Instant;

    fn fan(id: u8, rpm: f64, mode: HardwareMode) -> Fan {
        Fan {
            id,
            name: format!("风扇 {id}"),
            min_rpm: Some(1500.),
            max_rpm: Some(4500.),
            current_rpm: Some(rpm),
            mode,
        }
    }

    fn state(fans: Vec<Fan>, modes: &[ControlMode]) -> UiSnapshot {
        let mut config = Config::default();
        config.fans = fans
            .iter()
            .zip(modes)
            .map(|(fan, mode)| {
                let mut config = FanConfig::automatic(fan.id);
                config.mode = mode.clone();
                config
            })
            .collect();
        UiSnapshot {
            snapshot: Snapshot {
                sampled_at: 0.,
                fan_count: Some(fans.len() as f64),
                fans,
                sensors: Vec::new(),
                thermal_pressure: ThermalPressure::Nominal,
                cpu_utilization_percent: None,
                machine_id: None,
            },
            thermal: ThermalReading::default(),
            config,
            helper_ready: true,
            installing: false,
            discovered: true,
            message: String::new(),
            fan_status: BTreeMap::new(),
            battery: None,
            configuration_notice: String::new(),
            safety_active: false,
            handback_pending: false,
            targets: BTreeMap::new(),
            confirmed_targets: BTreeMap::new(),
            sample_age_secs: 0.,
            published_at: Instant::now(),
        }
    }

    #[test]
    fn percent_maps_to_hardware_range_and_never_leaves_it() {
        assert_eq!(rpm_for_percent(0., 1500, 4500), 1500);
        assert_eq!(rpm_for_percent(50., 1500, 4500), 3000);
        assert_eq!(rpm_for_percent(100., 1500, 4500), 4500);
        assert_eq!(rpm_for_percent(250., 1500, 4500), 4500);
        assert_eq!(rpm_for_percent(-5., 1500, 4500), 1500);
        assert_eq!(rpm_for_percent(f64::NAN, 1500, 4500), 1500);
        assert_eq!(rpm_for_percent(50., 0, 0), 1);
        assert_eq!(percent_of_range(3000., 1500, 4500), 50.);
        assert_eq!(percent_of_range(9000., 1500, 4500), 100.);
    }

    #[test]
    fn unified_plan_uses_each_fans_own_range_and_skips_unbounded_fans() {
        let mut unbounded = fan(2, 0., HardwareMode::Automatic);
        unbounded.max_rpm = None;
        let mut wide = fan(1, 0., HardwareMode::Automatic);
        wide.max_rpm = Some(5500.);
        let state = state(
            vec![fan(0, 0., HardwareMode::Automatic), wide, unbounded],
            &[
                ControlMode::Automatic,
                ControlMode::Automatic,
                ControlMode::Automatic,
            ],
        );
        let custom = configs_for_plan(&state, Plan::Custom, 50.);
        assert_eq!(custom.len(), 2);
        assert_eq!(custom[0].mode, ControlMode::Manual { rpm: 3000 });
        assert_eq!(custom[1].mode, ControlMode::Manual { rpm: 3500 });
        let system = configs_for_plan(&state, Plan::System, 50.);
        assert_eq!(system.len(), 3);
        assert!(system.iter().all(|c| c.mode == ControlMode::Automatic));
        assert!(configs_for_plan(&state, Plan::Mixed, 50.).is_empty());
    }

    #[test]
    fn plan_reflects_saved_settings_and_mixed_configurations() {
        let fans = || {
            vec![
                fan(0, 0., HardwareMode::Automatic),
                fan(1, 0., HardwareMode::Automatic),
            ]
        };
        assert_eq!(
            plan(&state(
                fans(),
                &[ControlMode::Adaptive, ControlMode::Adaptive]
            )),
            Plan::Smart
        );
        assert_eq!(
            plan(&state(
                fans(),
                &[
                    ControlMode::Manual { rpm: 2000 },
                    ControlMode::Manual { rpm: 3000 }
                ]
            )),
            Plan::Custom
        );
        assert_eq!(
            plan(&state(
                fans(),
                &[ControlMode::Adaptive, ControlMode::Automatic]
            )),
            Plan::Mixed
        );
        let curve = ControlMode::Curve {
            curve_id: "c".into(),
        };
        let curved = state(fans(), &[curve.clone(), ControlMode::Manual { rpm: 2000 }]);
        assert_eq!(plan(&curved), Plan::Custom);
        assert!(present(&curved, true, false).curve_active);
    }

    #[test]
    fn fan_lines_show_tachometer_and_hardware_truth_not_targets() {
        let mut state = state(
            vec![
                fan(0, 2049.6, HardwareMode::Forced),
                fan(1, 0., HardwareMode::Automatic),
            ],
            &[
                ControlMode::Manual { rpm: 3000 },
                ControlMode::Manual { rpm: 3000 },
            ],
        );
        let shown = present(&state, true, false);
        assert_eq!(shown.fans[0].speed, "2,050 转/分");
        assert_eq!(shown.fans[0].detail, "固定 50%");
        // Settings say custom, hardware says system: show the hardware.
        assert_eq!(shown.fans[1].speed, "停转");
        assert_eq!(shown.fans[1].detail, "由系统控制");
        state.snapshot.fans[0].mode = HardwareMode::Unknown;
        assert_eq!(present(&state, true, false).fans[0].detail, "状态未知");
        let stale = present(&state, false, false);
        assert_eq!(stale.fans[0].speed, "转速未知");
        assert_eq!(stale.temperature, None);
        assert!(!stale.controls_enabled);
    }

    #[test]
    fn notices_follow_safety_priority_and_never_show_raw_errors() {
        let base = || {
            state(
                vec![fan(0, 0., HardwareMode::Automatic)],
                &[ControlMode::Adaptive],
            )
        };
        assert_eq!(present(&base(), true, false).notice.tone, Tone::Good);

        let mut missing = base();
        missing.helper_ready = false;
        missing.message = "控制服务不可用：No such file or directory (os error 2)".into();
        let shown = present(&missing, true, false);
        assert_eq!(shown.notice.title, "需要一次授权");
        assert_eq!(shown.notice.action, Some(Action::EnableControl));
        assert!(!shown.notice.body.contains("os error"));
        assert!(!shown.controls_enabled);

        missing.message = "控制服务需要更新（协议 6 → 7）。点击启用 / 修复。".into();
        assert_eq!(
            present(&missing, true, false).notice.title,
            "需要更新风扇控制组件"
        );

        let mut handback = base();
        handback.safety_active = true;
        assert_eq!(
            present(&handback, true, false).notice.action,
            Some(Action::Reconnect)
        );
        handback.snapshot.thermal_pressure = ThermalPressure::Critical;
        assert_eq!(present(&handback, true, false).notice.tone, Tone::Danger);
        assert_eq!(present(&handback, false, false).notice.tone, Tone::Warning);
        assert_eq!(present(&handback, false, true).notice.title, "演示模式");

        let mut failed = base();
        failed
            .fan_status
            .insert(0, "写入失败，已请求安全回退".into());
        assert_eq!(present(&failed, true, false).notice.tone, Tone::Warning);

        let mut held = base();
        held.fan_status.insert(0, HELD_STATUS.into());
        assert_eq!(present(&held, true, false).notice.title, "风扇暂时交还系统");

        let mut unsaved = base();
        unsaved.configuration_notice = "保存失败，将重试当前设置：disk full".into();
        assert_eq!(
            present(&unsaved, true, false).notice.title,
            "设置可能没有保存"
        );
    }

    #[test]
    fn presets_are_valid_monotone_and_recognized() {
        for key in [fan_core::THERMAL_DEMAND_KEY, "Average CPU"] {
            for preset in CurvePreset::ALL {
                let curve = preset_curve(preset, "c", key);
                let mut config = Config::default();
                let mut fan = FanConfig::automatic(0);
                fan.mode = ControlMode::Curve {
                    curve_id: "c".into(),
                };
                fan.curve = Some(curve.clone());
                config.fans = vec![fan];
                config.validate().unwrap();
                assert!(curve
                    .points
                    .windows(2)
                    .all(|pair| pair[0].speed_percent <= pair[1].speed_percent));
                assert_eq!(curve.points.last().unwrap().speed_percent, 100.);
                assert_eq!(matching_preset(&curve), Some(preset));
            }
        }
        let quiet = preset_curve(CurvePreset::Quiet, "c", fan_core::THERMAL_DEMAND_KEY);
        assert!(quiet.points.first().unwrap().speed_percent < 0.);
        let cool = preset_curve(CurvePreset::Cool, "c", fan_core::THERMAL_DEMAND_KEY);
        assert!(cool.points.iter().all(|point| point.speed_percent >= 15.));
        let mut edited = preset_curve(CurvePreset::Balanced, "c", fan_core::THERMAL_DEMAND_KEY);
        edited.points[3].speed_percent += 1.;
        assert_eq!(matching_preset(&edited), None);
    }

    #[test]
    fn values_are_shown_rounded() {
        assert_eq!(format_value(20.178694751381215), "20.2");
        assert_eq!(format_value(4.524999999999999), "4.5");
        assert_eq!(format_value(45.0), "45");
        assert_eq!(format_value(-20.0), "-20");
    }

    #[test]
    fn edited_curve_applies_to_every_fan_with_distinct_ids() {
        let mut unbounded = fan(2, 0., HardwareMode::Automatic);
        unbounded.max_rpm = None;
        let state = state(
            vec![
                fan(0, 0., HardwareMode::Automatic),
                fan(1, 0., HardwareMode::Automatic),
                unbounded,
            ],
            &[
                ControlMode::Automatic,
                ControlMode::Automatic,
                ControlMode::Automatic,
            ],
        );
        let edited = preset_curve(CurvePreset::Quiet, "edit", fan_core::THERMAL_DEMAND_KEY);
        let configs = configs_for_curve(&state, &edited);
        assert_eq!(configs.len(), 2);
        let config = Config {
            fans: configs.clone(),
            ..Config::default()
        };
        config.validate().unwrap();
        assert_ne!(
            configs[0].curve.as_ref().unwrap().id,
            configs[1].curve.as_ref().unwrap().id
        );
        for config in &configs {
            assert_eq!(config.curve.as_ref().unwrap().points, edited.points);
            assert!(matches!(config.mode, ControlMode::Curve { .. }));
        }
    }

    #[test]
    fn fanless_mac_is_explained_instead_of_offering_control() {
        let mut state = state(Vec::new(), &[]);
        state.snapshot.fan_count = Some(0.);
        let shown = present(&state, true, false);
        assert_eq!(shown.notice.title, "这台 Mac 没有风扇");
        assert!(!shown.controls_enabled);
    }

    #[test]
    fn measured_rpm_missing_stale_and_invalid_values_never_become_stopped() {
        for reading in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(f64::NEG_INFINITY),
            Some(-1.),
            Some(f64::from(fan_core::ABSOLUTE_MAXIMUM_RPM) + 1.),
        ] {
            let mut hardware = fan(7, 0., HardwareMode::Automatic);
            hardware.current_rpm = reading;
            let snapshot = state(vec![hardware], &[ControlMode::Automatic]);
            for fresh in [true, false] {
                let cards = fan_cards(&snapshot, fresh);
                assert_eq!(cards[0].measured_rpm, None);
                assert_eq!(cards[0].percent, None);
                assert_eq!(cards[0].speed, "转速未知");
                assert_eq!(present(&snapshot, fresh, false).fan_percent, None);
            }
        }
        let snapshot = state(
            vec![fan(7, 0., HardwareMode::Automatic)],
            &[ControlMode::Automatic],
        );
        assert_eq!(fan_cards(&snapshot, true)[0].measured_rpm, Some(0.));
        assert_eq!(fan_cards(&snapshot, true)[0].percent, Some(0.));
        assert_eq!(fan_cards(&snapshot, true)[0].speed, "停转");
        assert_eq!(fan_cards(&snapshot, false)[0].measured_rpm, None);
    }

    #[test]
    fn measured_rpm_keeps_real_fan_identity_and_individual_hardware_ranges() {
        let mut first = fan(7, 3000., HardwareMode::Forced);
        first.name = "机箱风扇".into();
        let mut second = fan(2, 2600., HardwareMode::Automatic);
        second.name = "主风扇".into();
        second.min_rpm = Some(1000.);
        second.max_rpm = Some(5000.);
        for fans in [
            vec![],
            vec![first.clone()],
            vec![first.clone(), second.clone()],
        ] {
            let snapshot = state(fans.clone(), &vec![ControlMode::Automatic; fans.len()]);
            let cards = fan_cards(&snapshot, true);
            assert_eq!(cards.len(), fans.len());
            for (card, hardware) in cards.iter().zip(&fans) {
                assert_eq!(card.id, hardware.id);
                assert_eq!(card.name, hardware.name);
                assert_eq!(card.measured_rpm, hardware.current_rpm);
            }
            if let Some(card) = cards.first() {
                assert_eq!(card.percent, Some(50.));
            }
            if let Some(card) = cards.get(1) {
                assert_eq!(card.percent, Some(40.));
            }
        }
        first.min_rpm = None;
        let snapshot = state(vec![first], &[ControlMode::Automatic]);
        let card = &fan_cards(&snapshot, true)[0];
        assert_eq!(card.measured_rpm, Some(3000.));
        assert_eq!(card.range, None);
        assert_eq!(card.percent, None);
        let snapshot = state(
            vec![fan(7, 7000., HardwareMode::Automatic)],
            &[ControlMode::Automatic],
        );
        assert_eq!(fan_cards(&snapshot, true)[0].measured_rpm, Some(7000.));
        assert_eq!(fan_cards(&snapshot, true)[0].percent, Some(100.));
    }

    #[test]
    fn every_preference_and_requested_target_preserves_measured_rpm() {
        let mut snapshot = state(
            vec![fan(7, 3000., HardwareMode::Forced)],
            &[ControlMode::Adaptive],
        );
        for bias in -10..=10 {
            snapshot.config.adaptive_tuning.bias = bias;
            for target in [None, Some(1500), Some(4500)] {
                snapshot.targets.insert(7, target);
                snapshot.confirmed_targets.insert(7, target);
                let card = &fan_cards(&snapshot, true)[0];
                assert_eq!(card.measured_rpm, Some(3000.));
                assert_eq!(card.percent, Some(50.));
                assert_eq!(present(&snapshot, true, false).fan_percent, Some(50.));
            }
        }
    }

    #[test]
    fn fan_cards_show_measured_share_and_saved_choice() {
        let mut fans = vec![
            fan(0, 3000., HardwareMode::Forced),
            fan(1, 0., HardwareMode::Automatic),
        ];
        fans.push(Fan {
            min_rpm: None,
            ..fan(2, 2000., HardwareMode::Automatic)
        });
        let state = state(
            fans,
            &[
                ControlMode::Manual { rpm: 2700 },
                ControlMode::Adaptive,
                ControlMode::Automatic,
            ],
        );
        let cards = fan_cards(&state, true);
        assert_eq!(cards[0].percent, Some(50.));
        assert_eq!(
            (cards[0].choice, cards[0].fixed_percent),
            (FanChoice::Fixed, 40.)
        );
        assert_eq!(
            (cards[1].percent, cards[1].choice),
            (Some(0.), FanChoice::Smart)
        );
        assert_eq!(cards[1].fixed_percent, DEFAULT_CUSTOM_PERCENT);
        assert_eq!((cards[2].percent, cards[2].range), (None, None));
        assert!(fan_cards(&state, false)
            .iter()
            .all(|card| card.percent.is_none()));
    }

    #[test]
    fn per_fan_choice_is_bounded_and_keeps_existing_curves() {
        let mut fans = vec![fan(0, 3000., HardwareMode::Automatic)];
        fans.push(Fan {
            max_rpm: None,
            ..fan(1, 2000., HardwareMode::Automatic)
        });
        let mut state = state(fans, &[ControlMode::Automatic, ControlMode::Automatic]);
        let fixed = config_for_choice(&state, 0, FanChoice::Fixed, 250.).unwrap();
        assert_eq!(fixed.mode, ControlMode::Manual { rpm: 4500 });
        let curve = config_for_choice(&state, 0, FanChoice::Curve, 0.).unwrap();
        let id = curve.curve.as_ref().unwrap().id.clone();
        assert_eq!(
            curve.mode,
            ControlMode::Curve {
                curve_id: id.clone()
            }
        );
        state.config.fans[0] = curve;
        let again = config_for_choice(&state, 0, FanChoice::Curve, 0.).unwrap();
        assert_eq!(again.curve.unwrap().id, id);
        assert!(config_for_choice(&state, 1, FanChoice::Smart, 0.).is_none());
        assert_eq!(
            config_for_choice(&state, 1, FanChoice::System, 0.)
                .unwrap()
                .mode,
            ControlMode::Automatic
        );
        assert!(config_for_choice(&state, 9, FanChoice::System, 0.).is_none());
        assert_eq!(rpm_hint(50., Some((1500, 4500))), "约 3,000 转/分");
    }

    #[test]
    fn smart_status_explains_the_reason_in_plain_words() {
        let mut state = state(
            vec![fan(0, 3000., HardwareMode::Forced)],
            &[ControlMode::Adaptive],
        );
        state.thermal.adaptive.available = true;
        state.thermal.adaptive.demand_percent = 72.;
        state.thermal.adaptive.intervention = fan_core::AdaptiveIntervention::LoadFeedForward;
        let status = smart_status(&state, true);
        assert_eq!((status.demand, status.level), (Some(72.), "高"));
        assert!(status.in_use && status.reason.contains("持续高负载"));
        let stale = smart_status(&state, false);
        assert_eq!((stale.demand, stale.level), (None, "暂无数据"));
        assert_eq!(temperature_level(50.), Tone::Good);
        assert_eq!(temperature_level(90.), Tone::Warning);
        assert_eq!(temperature_level(105.), Tone::Danger);
    }

    #[test]
    fn smart_status_explains_cooling_residence_before_system_handback() {
        let mut state = state(
            vec![fan(0, 1500., HardwareMode::Forced)],
            &[ControlMode::Adaptive],
        );
        state.thermal.adaptive.available = true;
        state.targets.insert(0, Some(1500));
        state.confirmed_targets.insert(0, Some(1500));
        assert!(smart_status(&state, true).reason.contains("持续冷却"));
        state.safety_active = true;
        state.snapshot.thermal_pressure = ThermalPressure::Serious;
        assert!(smart_status(&state, true).reason.contains("安全保护"));
        state.safety_active = false;
        state.snapshot.thermal_pressure = ThermalPressure::Nominal;
        state.snapshot.fans[0].mode = HardwareMode::Automatic;
        state.targets.insert(0, None);
        state.confirmed_targets.insert(0, None);
        assert!(smart_status(&state, true).reason.contains("交给系统"));
        assert_eq!(smart_status(&state, false).reason, "正在等待温度数据。");
    }

    fn controlled_smart_state() -> UiSnapshot {
        let mut state = state(
            vec![fan(0, 1500., HardwareMode::Forced)],
            &[ControlMode::Adaptive],
        );
        state.thermal.adaptive.available = true;
        state.targets.insert(0, Some(1500));
        state.confirmed_targets.insert(0, Some(1500));
        state
    }

    #[test]
    fn cooling_residence_checks_default_and_every_preference_position() {
        let mut state = controlled_smart_state();
        state.thermal.adaptive.demand_percent = 5.;
        for bias in -10..=10 {
            state.config.adaptive_tuning.bias = bias;
            let adjusted =
                fan_core::tuned_adaptive_demand(5., state.config.adaptive_tuning.normalized(), 60.);
            state.thermal.adaptive.adjusted_demand_percent = (bias != 0).then_some(adjusted);
            let shown = smart_status(&state, true);
            assert_eq!(shown.demand, Some(adjusted));
            assert_eq!(shown.reason.contains("持续冷却"), bias <= 0, "bias {bias}");
            if bias > 0 {
                assert!(!shown.reason.contains("交给系统"), "bias {bias}");
            }
        }
        assert!((fan_core::tuned_adaptive_demand(5., 1., 60.) - 6.52).abs() < 1e-10);
    }

    #[test]
    fn cooling_residence_keeps_raw_and_adjusted_release_boundaries() {
        let mut state = controlled_smart_state();
        for (raw, adjusted, held) in [
            (5., None, true),
            (5.0001, None, false),
            (5., Some(5.), true),
            (5., Some(5.0001), false),
            (5.0001, Some(5.), false),
            (
                8.,
                Some(fan_core::tuned_adaptive_demand(8., -1., 60.)),
                false,
            ),
        ] {
            state.thermal.adaptive.demand_percent = raw;
            state.thermal.adaptive.adjusted_demand_percent = adjusted;
            assert_eq!(smart_status(&state, true).reason.contains("持续冷却"), held);
        }
        // Unacknowledged writes are not proof of active control.
        state.thermal.adaptive.demand_percent = 0.;
        state.thermal.adaptive.adjusted_demand_percent = None;
        state.confirmed_targets.insert(0, None);
        assert!(!smart_status(&state, true).reason.contains("持续冷却"));
    }

    #[test]
    fn cooling_residence_waits_for_load_trend_and_comfort_to_subside() {
        use fan_core::AdaptiveIntervention as Why;
        let mut state = controlled_smart_state();
        state.thermal.adaptive.demand_percent = 5.;
        state.thermal.adaptive.load_sustained = true;
        state.thermal.adaptive.intervention = Why::LoadFeedForward;
        assert!(smart_status(&state, true).reason.contains("持续高负载"));
        state.thermal.adaptive.load_sustained = false;
        state.thermal.adaptive.intervention = Why::RisingTemperature;
        for (trend, held) in [(0.15, true), (0.150001, false), (-0.1, true)] {
            state.thermal.adaptive.silicon_rise_celsius_per_second = trend;
            let shown = smart_status(&state, true);
            assert_eq!(shown.reason.contains("持续冷却"), held);
            if !held {
                assert!(shown.reason.contains("升温趋势"));
            }
        }
        state.thermal.adaptive.silicon_rise_celsius_per_second = 0.;
        state.thermal.adaptive.comfort_demand_percent = 0.01;
        state.thermal.adaptive.intervention = Why::Comfort;
        assert!(smart_status(&state, true).reason.contains("保持舒适"));
    }

    #[test]
    fn cooling_residence_never_masks_missing_data_or_safety_floors() {
        let mut state = controlled_smart_state();
        state.thermal.adaptive.adjusted_demand_percent = Some(4.);
        assert_eq!(smart_status(&state, false).reason, "正在等待温度数据。");
        state.thermal.adaptive.available = false;
        assert_eq!(smart_status(&state, true).demand, None);
        assert_eq!(smart_status(&state, true).reason, "正在等待温度数据。");
        state.thermal.adaptive.available = true;
        for invalid in [f64::NAN, f64::INFINITY] {
            state.thermal.adaptive.adjusted_demand_percent = Some(invalid);
            assert_eq!(smart_status(&state, true).demand, None);
            assert_eq!(smart_status(&state, true).reason, "正在等待温度数据。");
        }
        state.thermal.adaptive.adjusted_demand_percent = Some(4.);
        for pressure in [
            ThermalPressure::Fair,
            ThermalPressure::Serious,
            ThermalPressure::Critical,
        ] {
            state.snapshot.thermal_pressure = pressure;
            assert!(!smart_status(&state, true).reason.contains("持续冷却"));
            assert!(!smart_status(&state, true).reason.contains("交给系统"));
        }
        state.safety_active = true;
        assert!(smart_status(&state, true).reason.contains("安全保护"));
        state.snapshot.thermal_pressure = ThermalPressure::Nominal;
        state.snapshot.sensors.push(fan_core::Sensor {
            key: "TC0P".into(),
            name: "CPU".into(),
            group: fan_core::SensorGroup::Cpu,
            value: Some(96.),
        });
        assert!(smart_status(&state, true).reason.contains("安全保护"));
    }

    #[test]
    fn unconfirmed_handback_overrides_old_targets_freshness_and_thermal_demand() {
        let mut state = controlled_smart_state();
        state.thermal.adaptive.demand_percent = 5.;
        state.handback_pending = true;
        state.safety_active = true;
        for pressure in [ThermalPressure::Nominal, ThermalPressure::Critical] {
            state.snapshot.thermal_pressure = pressure;
            for (fresh, available, ready) in [
                (true, true, true),
                (false, true, true),
                (true, false, false),
            ] {
                state.thermal.adaptive.available = available;
                state.helper_ready = ready;
                let smart = smart_status(&state, fresh);
                assert_eq!(smart.reason, HANDBACK_PENDING_REASON);
                let shown = present(&state, fresh, false);
                assert_eq!(shown.notice.title, "交还系统尚未确认");
                assert_eq!(shown.notice.action, Some(Action::Reconnect));
                assert_eq!(shown.notice.body, HANDBACK_PENDING_REASON);
                assert!(!shown.controls_enabled);
                assert_eq!(
                    shown.notice.tone,
                    if pressure == ThermalPressure::Critical {
                        Tone::Danger
                    } else {
                        Tone::Warning
                    }
                );
            }
        }
    }

    #[test]
    fn thermal_override_and_confirmed_return_have_distinct_statuses() {
        let mut state = controlled_smart_state();
        state.safety_active = true;
        state.snapshot.thermal_pressure = ThermalPressure::Critical;
        assert!(!state.handback_pending);
        assert!(smart_status(&state, true).reason.contains("安全保护"));
        state.safety_active = false;
        state.snapshot.thermal_pressure = ThermalPressure::Nominal;
        state.handback_pending = true;
        assert_eq!(smart_status(&state, true).reason, HANDBACK_PENDING_REASON);
        // The worker only clears its published flag once fallback is confirmed.
        state.handback_pending = false;
        assert!(smart_status(&state, true).reason.contains("持续冷却"));
        state.snapshot.fans[0].mode = HardwareMode::Automatic;
        state.targets.insert(0, None);
        state.confirmed_targets.insert(0, None);
        assert!(smart_status(&state, true).reason.contains("交给系统"));
        assert_eq!(present(&state, true, false).notice.tone, Tone::Good);
    }

    #[test]
    fn battery_lines_use_plain_words() {
        let battery = fan_platform::BatteryReading {
            charge_percent: Some(84.),
            is_charging: Some(false),
            is_on_ac: Some(false),
            cycle_count: Some(211),
            health_percent: Some(92.4),
            power_watts: Some(-13.26),
            adapter_watts: None,
            health_is_estimate: true,
        };
        let (state, details) = battery_lines(&battery);
        assert_eq!(state, "使用电池 · 耗电 13.3 W");
        assert_eq!(details, ["健康度约 92%", "已充电循环 211 次"]);
    }
}
