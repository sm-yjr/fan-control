//! Bounded everyday cooling preference; independent of surface calibration and safety.
use crate::ConfigError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveTuning {
    /// 21 positions: -10 quieter, 0 existing behavior, +10 cooler.
    #[serde(default)]
    pub bias: i8,
}
impl AdaptiveTuning {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(-10..=10).contains(&self.bias) {
            return Err(ConfigError::Invalid("散热偏好必须为 -10...10".into()));
        }
        Ok(())
    }
    pub fn normalized(self) -> f64 {
        f64::from(self.bias.clamp(-10, 10)) / 10.0
    }
}

pub fn smooth_adaptive_bias(previous: f64, target: AdaptiveTuning, elapsed: f64) -> f64 {
    let dt = if elapsed.is_finite() {
        elapsed.clamp(0.0, 5.0)
    } else {
        0.0
    };
    let target = target.normalized();
    let next = previous + (1.0 - (-dt / 10.0).exp()) * (target - previous);
    // Finish a negligible tail exactly, so restoring zero recovers the neutral path.
    if (target - next).abs() < 0.0001 {
        target
    } else {
        next
    }
}

/// Temperature gating only removes quieter reductions, never cooler additions.
pub fn tuned_adaptive_demand(demand: f64, bias: f64, hottest: f64) -> f64 {
    let demand = demand.clamp(0.0, 100.0);
    let x = demand / 100.0;
    let hot = ((hottest - 80.0) / 10.0).clamp(0.0, 1.0);
    let gate = if bias < 0.0 {
        1.0 - hot * hot * (3.0 - 2.0 * hot)
    } else {
        1.0
    };
    (demand + bias.clamp(-1.0, 1.0) * 8.0 * 4.0 * x * (1.0 - x) * gate).clamp(0.0, 100.0)
}
