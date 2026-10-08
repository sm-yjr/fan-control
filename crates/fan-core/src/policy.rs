use crate::{ControlMode, ThermalPressure};

pub const ADAPTIVE_RAMP_UP_RPM_PER_SECOND: f64 = 100.0;
pub const ADAPTIVE_RAMP_DOWN_RPM_PER_SECOND: f64 = 35.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollingActivity {
    Automatic,
    Manual,
    Curve,
}

pub fn polling_interval(
    popover_presented: bool,
    pressure: ThermalPressure,
    hottest_silicon: Option<f64>,
    activity: PollingActivity,
) -> f64 {
    if popover_presented
        || pressure >= ThermalPressure::Serious
        || hottest_silicon.is_some_and(|value| value.is_finite() && value >= 90.0)
    {
        return 2.0;
    }
    match activity {
        PollingActivity::Automatic => 8.0,
        PollingActivity::Manual => 5.0,
        PollingActivity::Curve => 2.0,
    }
}

pub fn ramp_target(
    requested: u32,
    previous: u32,
    elapsed: f64,
    mode: &ControlMode,
    bypass: bool,
    starting_from_stopped: bool,
) -> u32 {
    if matches!(mode, ControlMode::Manual { .. })
        || (bypass && requested > previous)
        || (starting_from_stopped && !matches!(mode, ControlMode::Adaptive))
        || previous == requested
    {
        return requested;
    }
    let adaptive = matches!(mode, ControlMode::Adaptive);
    let elapsed = if adaptive {
        if elapsed.is_finite() {
            elapsed.clamp(0.0, 5.0)
        } else {
            0.0
        }
    } else if elapsed.is_finite() {
        elapsed.max(1.0)
    } else {
        1.0
    };
    let rate = match (adaptive, requested > previous) {
        (true, true) => ADAPTIVE_RAMP_UP_RPM_PER_SECOND,
        (true, false) => ADAPTIVE_RAMP_DOWN_RPM_PER_SECOND,
        (false, true) => 350.0,
        (false, false) => 250.0,
    };
    let delta = (rate * elapsed).min(u32::MAX as f64) as u32;
    if requested > previous {
        requested.min(previous.saturating_add(delta))
    } else {
        requested.max(previous.saturating_sub(delta))
    }
}

pub fn missing_input_safety_percent(
    pressure: ThermalPressure,
    hottest_silicon: Option<f64>,
) -> Option<f64> {
    if pressure == ThermalPressure::Critical {
        return Some(100.0);
    }
    let floor = (pressure == ThermalPressure::Serious).then_some(70.0_f64);
    if hottest_silicon.is_some_and(|value| value.is_finite() && value >= 96.0) {
        Some(floor.unwrap_or(0.0).max(80.0))
    } else {
        floor
    }
}

pub fn safety_adjusted_percent(
    requested: f64,
    input: f64,
    sensor_key: &str,
    pressure: ThermalPressure,
    hottest_silicon: Option<f64>,
) -> f64 {
    if pressure == ThermalPressure::Critical {
        return 100.0;
    }
    let mut speed = requested;
    if pressure == ThermalPressure::Serious {
        speed = speed.max(70.0);
    }
    if hottest_silicon.is_some_and(|value| value.is_finite() && value >= 96.0) {
        speed = speed.max(80.0);
    }
    if sensor_key == crate::THERMAL_DEMAND_KEY {
        if input >= 90.0 {
            speed = speed.max(75.0);
        } else if input >= 75.0 {
            speed = speed.max(45.0);
        }
    } else if input >= 95.0 {
        speed = speed.max(80.0);
    }
    speed.clamp(-20.0, 100.0)
}
