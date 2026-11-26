//! Native macOS menu bar status item with optional compact live readings.
//! Display preference is stored in NSUserDefaults so config.json remains unchanged.
use crate::{
    i18n::{raw_nsstring as raw_text, translated_nsstring as text},
    presenter::{FanCard, Presentation, Tone},
};
use objc2::{rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSUserDefaults};

pub const DISPLAY_MODE_KEY: &str = "FanControlMenuBarDisplay";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuBarDisplayMode {
    #[default]
    IconOnly,
    Temperature,
    FanSpeed,
    TemperatureAndFan,
}

impl MenuBarDisplayMode {
    pub const ALL: [Self; 4] = [
        Self::IconOnly,
        Self::Temperature,
        Self::FanSpeed,
        Self::TemperatureAndFan,
    ];

    pub fn preference_key(self) -> &'static str {
        match self {
            Self::IconOnly => "icon",
            Self::Temperature => "temperature",
            Self::FanSpeed => "rpm",
            Self::TemperatureAndFan => "both",
        }
    }

    pub fn from_preference_key(value: &str) -> Option<Self> {
        match value {
            "icon" => Some(Self::IconOnly),
            "temperature" => Some(Self::Temperature),
            "rpm" => Some(Self::FanSpeed),
            "both" => Some(Self::TemperatureAndFan),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn title(self) -> &'static str {
        match self {
            Self::IconOnly => "仅图标",
            Self::Temperature => "图标与芯片温度",
            Self::FanSpeed => "图标与最高转速",
            Self::TemperatureAndFan => "图标、温度与转速",
        }
    }

    pub fn load(demo: bool) -> Self {
        if demo {
            return Self::IconOnly;
        }
        NSUserDefaults::standardUserDefaults()
            .stringForKey(&raw_text(DISPLAY_MODE_KEY))
            .and_then(|value| Self::from_preference_key(&value.to_string()))
            .unwrap_or_default()
    }

    pub fn save(self, demo: bool) {
        if demo {
            return;
        }
        unsafe {
            NSUserDefaults::standardUserDefaults().setObject_forKey(
                Some(&raw_text(self.preference_key())),
                &raw_text(DISPLAY_MODE_KEY),
            );
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatusBarContent {
    pub title: String,
    pub symbol: &'static str,
    pub alpha: f64,
    pub tooltip: String,
}

fn format_compact_rpm(rpm: Option<f64>) -> String {
    match rpm {
        None => "—".into(),
        Some(v) if v < 100. => "0".into(),
        Some(v) if v >= 1000. => format!("{:.1}k", v / 1000.),
        Some(v) => format!("{:.0}", v),
    }
}

/// Pure formatting for the menu bar status item so every topology and fault
/// state is unit-testable without AppKit.
pub fn format_status_bar(
    view: &Presentation,
    cards: &[FanCard],
    fresh: bool,
    mode: MenuBarDisplayMode,
    message: &str,
) -> StatusBarContent {
    let fastest = if fresh {
        cards
            .iter()
            .filter_map(|card| card.measured_rpm)
            .reduce(f64::max)
    } else {
        None
    };
    let temp_str = if fresh {
        view.temperature
            .map(|v| format!("{v:.0}°"))
            .unwrap_or_else(|| "—".into())
    } else {
        "—".into()
    };
    let rpm_str = if cards.is_empty() {
        "—".into()
    } else {
        format_compact_rpm(fastest)
    };
    let title = match mode {
        MenuBarDisplayMode::IconOnly => String::new(),
        MenuBarDisplayMode::Temperature => format!(" {temp_str}"),
        MenuBarDisplayMode::FanSpeed => format!(" {rpm_str}"),
        MenuBarDisplayMode::TemperatureAndFan => format!(" {temp_str} · {rpm_str}"),
    };
    let danger = view.notice.tone == Tone::Danger;
    let symbol = if danger { "flame.fill" } else { "fanblades" };
    let spinning = fastest.is_some_and(|rpm| rpm >= 100.);
    let alpha = if !fresh {
        0.55
    } else if danger || spinning || mode != MenuBarDisplayMode::IconOnly {
        1.0
    } else {
        0.72
    };
    let temp_tooltip = if fresh {
        view.temperature
            .map(|v| format!("{v:.1}°C"))
            .unwrap_or_else(|| "暂不可用".into())
    } else {
        "数据已过期".into()
    };
    let tooltip = format!("Fan Control · CPU {temp_tooltip} · {message}");
    StatusBarContent {
        title,
        symbol,
        alpha,
        tooltip,
    }
}

pub(crate) struct StatusBarController {
    pub item: Retained<NSStatusItem>,
    mode: MenuBarDisplayMode,
    last_symbol: &'static str,
}

impl StatusBarController {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, demo: bool) -> Self {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let mode = MenuBarDisplayMode::load(demo);
        if let Some(button) = item.button(mtm) {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &raw_text("fanblades"),
                Some(&text("Fan Control：打开风扇控制")),
            );
            button.setImage(image.as_deref());
            button.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                12.0,
                unsafe { NSFontWeightMedium },
            )));
            button.setImagePosition(if mode == MenuBarDisplayMode::IconOnly {
                NSCellImagePosition::ImageOnly
            } else {
                NSCellImagePosition::ImageLeading
            });
            unsafe {
                button.setTarget(Some(target));
                button.setAction(Some(sel!(togglePanel:)));
            }
        }
        Self {
            item,
            mode,
            last_symbol: "fanblades",
        }
    }

    pub fn set_mode(&mut self, mode: MenuBarDisplayMode, demo: bool) {
        self.mode = mode;
        mode.save(demo);
    }

    pub fn refresh(
        &mut self,
        mtm: MainThreadMarker,
        view: &Presentation,
        cards: &[FanCard],
        fresh: bool,
        message: &str,
    ) {
        let content = format_status_bar(view, cards, fresh, self.mode, message);
        let Some(button) = self.item.button(mtm) else {
            return;
        };
        if self.last_symbol != content.symbol {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &raw_text(content.symbol),
                Some(&text("Fan Control：打开风扇控制")),
            );
            button.setImage(image.as_deref());
            self.last_symbol = content.symbol;
        }
        if content.title.is_empty() {
            button.setTitle(&raw_text(""));
            button.setImagePosition(NSCellImagePosition::ImageOnly);
        } else {
            button.setTitle(&raw_text(&content.title));
            button.setImagePosition(NSCellImagePosition::ImageLeading);
        }
        button.setAlphaValue(content.alpha);
        button.setToolTip(Some(&text(&content.tooltip)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presenter::{fan_cards, present};
    use crate::worker::UiSnapshot;
    use fan_core::{
        Config, ControlMode, Fan, FanConfig, HardwareMode, Sensor, SensorGroup, Snapshot,
        ThermalPressure, ThermalReading,
    };
    use std::collections::BTreeMap;
    use std::time::Instant;

    fn sample_state(temp: Option<f64>, rpm: Option<f64>, pressure: ThermalPressure) -> UiSnapshot {
        UiSnapshot {
            snapshot: Snapshot {
                sampled_at: 0.,
                fan_count: Some(1.),
                thermal_pressure: pressure,
                cpu_utilization_percent: Some(25.),
                machine_id: None,
                fans: vec![Fan {
                    id: 0,
                    name: "Fan 1".into(),
                    min_rpm: Some(1200.),
                    max_rpm: Some(5000.),
                    current_rpm: rpm,
                    mode: HardwareMode::Forced,
                }],
                sensors: vec![Sensor {
                    key: "Tp01".into(),
                    name: "CPU".into(),
                    group: SensorGroup::Cpu,
                    value: temp,
                }],
            },
            thermal: ThermalReading::default(),
            config: Config {
                fans: vec![FanConfig {
                    fan_id: 0,
                    mode: ControlMode::Adaptive,
                    curve: None,
                }],
                ..Config::default()
            },
            helper_ready: true,
            installing: false,
            discovered: true,
            message: "ok".into(),
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
    fn formats_all_display_modes_and_stale_states() {
        let state = sample_state(Some(54.2), Some(2430.), ThermalPressure::Nominal);
        let view = present(&state, true, false);
        let cards = fan_cards(&state, true);
        assert_eq!(
            format_status_bar(&view, &cards, true, MenuBarDisplayMode::IconOnly, "ok").title,
            ""
        );
        assert_eq!(
            format_status_bar(&view, &cards, true, MenuBarDisplayMode::Temperature, "ok").title,
            " 54°"
        );
        assert_eq!(
            format_status_bar(&view, &cards, true, MenuBarDisplayMode::FanSpeed, "ok").title,
            " 2.4k"
        );
        assert_eq!(
            format_status_bar(
                &view,
                &cards,
                true,
                MenuBarDisplayMode::TemperatureAndFan,
                "ok"
            )
            .title,
            " 54° · 2.4k"
        );

        let stale_view = present(&state, false, false);
        let stale_cards = fan_cards(&state, false);
        let stale = format_status_bar(
            &stale_view,
            &stale_cards,
            false,
            MenuBarDisplayMode::TemperatureAndFan,
            "stale",
        );
        assert_eq!(stale.title, " — · —");
        assert!(stale.alpha < 0.6);
    }

    #[test]
    fn switches_symbol_on_danger_and_handles_stopped_or_fanless() {
        let hot = sample_state(Some(101.), Some(4800.), ThermalPressure::Serious);
        let view = present(&hot, true, false);
        let cards = fan_cards(&hot, true);
        let content = format_status_bar(&view, &cards, true, MenuBarDisplayMode::Temperature, "");
        assert_eq!(content.symbol, "flame.fill");
        assert_eq!(content.title, " 101°");

        let stopped = sample_state(Some(42.), Some(0.), ThermalPressure::Nominal);
        let s_view = present(&stopped, true, false);
        let s_cards = fan_cards(&stopped, true);
        assert_eq!(
            format_status_bar(&s_view, &s_cards, true, MenuBarDisplayMode::FanSpeed, "").title,
            " 0"
        );
        assert_eq!(
            format_status_bar(&s_view, &[], true, MenuBarDisplayMode::FanSpeed, "").title,
            " —"
        );
    }

    #[test]
    fn preference_key_round_trips() {
        for mode in MenuBarDisplayMode::ALL {
            assert_eq!(
                MenuBarDisplayMode::from_preference_key(mode.preference_key()),
                Some(mode)
            );
        }
        assert_eq!(MenuBarDisplayMode::from_preference_key("unknown"), None);
    }
}
