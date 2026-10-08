use crate::{valid_temperature, SensorGroup, Snapshot, ThermalPressure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ThermalReading {
    pub demand_percent: f64,
    pub sustained_silicon_temperature: f64,
    pub chassis_temperature: f64,
    pub chassis_rise_per_minute: f64,
    pub pressure: ThermalPressure,
    pub uses_chassis_sensor: bool,
    /// 只有当前采样有可用的 silicon/chassis 输入才为 true。
    pub available: bool,
    #[serde(default)]
    pub adaptive: crate::AdaptiveReading,
}

#[derive(Debug, Clone, Default)]
pub struct ThermalEstimator {
    sustained_silicon: Option<f64>,
    silicon_weight: f64,
    filtered_chassis: Option<f64>,
    silicon_missing_elapsed: f64,
    chassis_missing_elapsed: f64,
}

impl ThermalEstimator {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn update_snapshot(&mut self, snapshot: &Snapshot, elapsed: f64) -> ThermalReading {
        let cpu = snapshot.temperature_values(SensorGroup::Cpu);
        let gpu = snapshot.temperature_values(SensorGroup::Gpu);
        let average = |values: &[f64]| {
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
        };
        let silicon = [average(&cpu), average(&gpu)]
            .into_iter()
            .flatten()
            .reduce(f64::max);
        // Airport 的局部热量不代表机身热容量，保留 Swift 的排除条件。
        let chassis: Vec<f64> = snapshot
            .sensors
            .iter()
            .filter(|sensor| sensor.group == SensorGroup::System && sensor.key != "TW0P")
            .filter_map(|sensor| sensor.value.filter(|value| valid_temperature(*value)))
            .collect();
        self.update(
            silicon,
            representative_temperature(&chassis),
            snapshot.hottest_silicon(),
            snapshot.thermal_pressure,
            elapsed,
        )
    }

    pub fn update(
        &mut self,
        silicon: Option<f64>,
        chassis: Option<f64>,
        emergency_silicon: Option<f64>,
        pressure: ThermalPressure,
        elapsed: f64,
    ) -> ThermalReading {
        let silicon = silicon.filter(|value| valid_temperature(*value));
        let chassis = chassis.filter(|value| valid_temperature(*value));
        let emergency = emergency_silicon
            .filter(|value| valid_temperature(*value))
            .or(silicon);
        let reseed = !elapsed.is_finite() || elapsed <= 0.0 || elapsed > 30.0;
        let dt = if elapsed.is_finite() {
            elapsed.clamp(0.5, 10.0)
        } else {
            0.5
        };
        let missing_elapsed = if elapsed.is_finite() {
            elapsed.max(0.5)
        } else {
            0.5
        };
        if reseed {
            self.sustained_silicon = None;
            self.silicon_weight = 0.0;
            self.filtered_chassis = chassis;
            self.silicon_missing_elapsed = 0.0;
            self.chassis_missing_elapsed = 0.0;
        }
        if let Some(sample) = silicon {
            self.silicon_missing_elapsed = 0.0;
            // 首样本只代表已观测的一小段时间，不能占满整个持续热源窗口。
            // 权重建立后收敛到常规 30 秒低通；启动尖峰能被后续冷读纠正。
            let decay = (-dt / 30.0).exp();
            self.silicon_weight = decay * self.silicon_weight + (1.0 - decay);
            self.sustained_silicon = Some(
                self.sustained_silicon
                    .map(|previous| {
                        previous + (1.0 - decay) / self.silicon_weight * (sample - previous)
                    })
                    .unwrap_or(sample),
            );
        } else {
            self.silicon_missing_elapsed += missing_elapsed;
            if self.silicon_missing_elapsed >= crate::MISSING_INPUT_MAXIMUM_HOLD {
                self.sustained_silicon = None;
                self.silicon_weight = 0.0;
            }
        }
        let previous_chassis = self.filtered_chassis;
        if let Some(sample) = chassis {
            self.chassis_missing_elapsed = 0.0;
            self.filtered_chassis = Some(low_pass(self.filtered_chassis, sample, dt, 90.0));
        } else {
            self.chassis_missing_elapsed += missing_elapsed;
            if self.chassis_missing_elapsed >= crate::MISSING_INPUT_MAXIMUM_HOLD {
                self.filtered_chassis = None;
            }
        }
        let sustained = self.sustained_silicon.unwrap_or(0.0);
        let filtered_chassis = self.filtered_chassis.unwrap_or(0.0);
        let rise = if reseed {
            0.0
        } else {
            previous_chassis
                .map(|previous| ((filtered_chassis - previous) * 60.0 / dt).max(0.0))
                .unwrap_or(0.0)
        };
        let silicon_load = smooth_step(sustained, 50.0, 95.0);
        let chassis_load = smooth_step(filtered_chassis, 32.0, 55.0);
        let rising_load = smooth_step(rise, 0.2, 2.0);
        let mut demand = if self.filtered_chassis.is_some() {
            100.0 * (0.35 * silicon_load + 0.55 * chassis_load + 0.10 * rising_load)
        } else {
            100.0 * silicon_load
        };
        demand = demand.max(match pressure {
            ThermalPressure::Nominal => 0.0,
            ThermalPressure::Fair => 35.0,
            ThermalPressure::Serious => 75.0,
            ThermalPressure::Critical => 100.0,
        });
        if let Some(emergency) = emergency.filter(|value| *value >= 96.0) {
            demand = demand.max(75.0 + 25.0 * ((emergency - 96.0) / 9.0).clamp(0.0, 1.0));
        }
        ThermalReading {
            demand_percent: demand.clamp(0.0, 100.0),
            sustained_silicon_temperature: sustained,
            chassis_temperature: filtered_chassis,
            chassis_rise_per_minute: rise,
            pressure,
            uses_chassis_sensor: self.filtered_chassis.is_some(),
            available: silicon.is_some() || chassis.is_some(),
            adaptive: crate::AdaptiveReading::default(),
        }
    }
}

pub fn representative_temperature(values: &[f64]) -> Option<f64> {
    let mut sorted: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| valid_temperature(*value))
        .collect();
    sorted.sort_by(f64::total_cmp);
    if sorted.is_empty() {
        return None;
    }
    let position = (sorted.len() - 1) as f64 * 0.75;
    let low = position.floor() as usize;
    let high = position.ceil() as usize;
    Some(sorted[low] + (position - low as f64) * (sorted[high] - sorted[low]))
}

fn low_pass(previous: Option<f64>, sample: f64, elapsed: f64, constant: f64) -> f64 {
    previous
        .map(|previous| previous + (1.0 - (-elapsed / constant).exp()) * (sample - previous))
        .unwrap_or(sample)
}

fn smooth_step(value: f64, lower: f64, upper: f64) -> f64 {
    let x = ((value - lower) / (upper - lower)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
