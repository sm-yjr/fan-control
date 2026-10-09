use crate::{valid_temperature, ConfigError, SensorGroup, Snapshot, ThermalReading};
use serde::{Deserialize, Serialize};

pub const ADAPTIVE_LOAD_CONFIRMATION_SECONDS: f64 = 4.0;
pub const ADAPTIVE_PREDICTION_SECONDS: f64 = 20.0;
pub const ADAPTIVE_COOLING_RESIDENCE_SECONDS: f64 = 180.0;
pub const ADAPTIVE_DEMAND_RISE_SECONDS: f64 = 20.0;
pub const ADAPTIVE_DEMAND_FALL_SECONDS: f64 = 90.0;
pub const ADAPTIVE_START_DEMAND_PERCENT: f64 = 12.0;
pub const ADAPTIVE_RELEASE_DEMAND_PERCENT: f64 = 5.0;
pub const ADAPTIVE_QUIET_CONFIRMATION_SECONDS: f64 = 60.0;

/// 用户设置是温度目标，不是硬件对键盘表面温度的保证。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThermalPolicy {
    pub comfort_target_celsius: Option<f64>,
    pub calibration: Option<SurfaceCalibration>,
}
impl Default for ThermalPolicy {
    fn default() -> Self {
        Self {
            comfort_target_celsius: Some(38.0),
            calibration: None,
        }
    }
}

/// 仅适用于明确测量过的同一机器和传感器工作区间。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceCalibration {
    pub machine_id: String,
    pub sensor_key: String,
    pub offset_celsius: f64,
    pub sensor_min_celsius: f64,
    pub sensor_max_celsius: f64,
    pub surface_min_celsius: f64,
    pub surface_max_celsius: f64,
}
impl ThermalPolicy {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self
            .comfort_target_celsius
            .is_some_and(|target| !target.is_finite() || !(30.0..=45.0).contains(&target))
        {
            return Err(ConfigError::Invalid("舒适目标必须为 30...45°C".into()));
        }
        if let Some(calibration) = &self.calibration {
            calibration.validate()?;
        }
        Ok(())
    }
}
impl SurfaceCalibration {
    pub fn from_measurements(
        machine_id: impl Into<String>,
        sensor_key: impl Into<String>,
        proxy_a: f64,
        surface_a: f64,
        proxy_b: f64,
        surface_b: f64,
    ) -> Result<Self, ConfigError> {
        let ((proxy_a, surface_a), (proxy_b, surface_b)) = if proxy_a <= proxy_b {
            ((proxy_a, surface_a), (proxy_b, surface_b))
        } else {
            ((proxy_b, surface_b), (proxy_a, surface_a))
        };
        let calibration = Self {
            machine_id: machine_id.into(),
            sensor_key: sensor_key.into(),
            offset_celsius: ((surface_a - proxy_a) + (surface_b - proxy_b)) / 2.0,
            sensor_min_celsius: proxy_a,
            sensor_max_celsius: proxy_b,
            surface_min_celsius: surface_a,
            surface_max_celsius: surface_b,
        };
        calibration.validate()?;
        Ok(calibration)
    }
    pub fn validate(&self) -> Result<(), ConfigError> {
        let numbers = [
            self.offset_celsius,
            self.sensor_min_celsius,
            self.sensor_max_celsius,
            self.surface_min_celsius,
            self.surface_max_celsius,
        ];
        if self.machine_id.trim().is_empty()
            || self.machine_id.len() > 128
            || self.sensor_key.trim().is_empty()
            || self.sensor_key.len() > 32
            || numbers.iter().any(|number| !number.is_finite())
            || self.offset_celsius.abs() > 40.0
            || !valid_temperature(self.sensor_min_celsius)
            || !valid_temperature(self.sensor_max_celsius)
            || self.sensor_min_celsius >= self.sensor_max_celsius
            || self.sensor_max_celsius - self.sensor_min_celsius < 3.0
            || self.surface_min_celsius < 15.0
            || self.surface_max_celsius > 60.0
            || self.surface_min_celsius >= self.surface_max_celsius
            || (self.surface_min_celsius - self.sensor_min_celsius).abs() > 40.0
            || (self.surface_max_celsius - self.sensor_max_celsius).abs() > 40.0
            || ((self.surface_min_celsius - self.sensor_min_celsius)
                - (self.surface_max_celsius - self.sensor_max_celsius))
                .abs()
                > 3.0
            || (self.offset_celsius
                - ((self.surface_min_celsius - self.sensor_min_celsius)
                    + (self.surface_max_celsius - self.sensor_max_celsius))
                    / 2.0)
                .abs()
                > 0.01
        {
            return Err(ConfigError::Invalid(
                "表面校准需要机器、传感器、真实测量偏移和有效温度范围".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComfortStatus {
    #[default]
    NotRequested,
    Uncalibrated,
    MachineMismatch,
    MissingSensor,
    OutOfRange,
    AboveCalibrationRange,
    BelowCalibrationRange,
    Active,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptiveIntervention {
    #[default]
    Idle,
    Temperature,
    LoadFeedForward,
    RisingTemperature,
    HeatSoak,
    Comfort,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveReading {
    pub demand_percent: f64,
    /// After the everyday preference; safety floors are applied independently.
    #[serde(default)]
    pub adjusted_demand_percent: Option<f64>,
    /// 结合持续热负荷、负载前馈与温升趋势的平滑控制指标，单位不是瓦特。
    #[serde(default)]
    pub thermal_load_percent: f64,
    pub cpu_utilization_percent: Option<f64>,
    pub load_sustained: bool,
    pub silicon_rise_celsius_per_second: f64,
    pub predicted_silicon_celsius: Option<f64>,
    pub heat_soak_percent: f64,
    /// 校准范围内的 proxy + 实测偏移估计；不是实测键盘表面。
    pub estimated_surface_celsius: Option<f64>,
    /// 高端越界时仅保留最高有效校准端点的散热下界，不输出表面温度估计。
    #[serde(default)]
    pub comfort_demand_percent: f64,
    pub comfort_status: ComfortStatus,
    pub intervention: AdaptiveIntervention,
    pub available: bool,
}

impl AdaptiveReading {
    /// Shared low-demand prerequisite, not proof that control has been handed back.
    /// The caller supplies the effective demand after independent safety floors.
    /// The controller separately requires residence, quiet confirmation and idle RPM.
    pub fn low_demand_for_release(&self, effective_demand_percent: f64) -> bool {
        effective_demand_percent <= ADAPTIVE_RELEASE_DEMAND_PERCENT
            && self.demand_percent <= ADAPTIVE_RELEASE_DEMAND_PERCENT
            && !self.load_sustained
            && self.silicon_rise_celsius_per_second <= 0.15
            && self.comfort_demand_percent <= 0.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct AdaptiveEstimator {
    cpu_trend: SiliconTrend,
    gpu_trend: SiliconTrend,
    load_elapsed: f64,
    thermal_load: f64,
    heat_soak: f64,
}
#[derive(Debug, Clone, Default)]
struct SiliconTrend {
    previous: Option<f64>,
    filtered_rise: f64,
    rising_elapsed: f64,
}
impl SiliconTrend {
    fn update(&mut self, temperature: Option<f64>, dt: f64) -> (Option<f64>, bool) {
        if temperature.is_none() {
            *self = Self::default();
            return (None, false);
        }
        let slope = if dt > 0.0 {
            temperature
                .zip(self.previous)
                .map(|(current, previous)| ((current - previous) / dt).clamp(-2.0, 2.0))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        if slope >= 0.15 {
            self.rising_elapsed += dt;
        } else {
            self.rising_elapsed = 0.0;
        }
        self.filtered_rise += (1.0 - (-dt / 4.0).exp()) * (slope - self.filtered_rise);
        self.previous = temperature;
        let confirmed = self.rising_elapsed >= ADAPTIVE_LOAD_CONFIRMATION_SECONDS;
        (
            temperature.map(|value| {
                value
                    + if confirmed {
                        self.filtered_rise.max(0.0) * ADAPTIVE_PREDICTION_SECONDS
                    } else {
                        0.0
                    }
            }),
            confirmed,
        )
    }
}
impl AdaptiveEstimator {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn update(
        &mut self,
        snapshot: &Snapshot,
        thermal: &ThermalReading,
        policy: &ThermalPolicy,
        elapsed: f64,
    ) -> AdaptiveReading {
        let reseed = !elapsed.is_finite() || elapsed <= 0.0;
        if reseed {
            self.reset();
        }
        // 新采样的有效性由 controller 检查。调度空档不能清除此前的持续散热，
        // 也不能把无人观测的空档当作持续负载或温升的证明。
        let observation_gap = !reseed && elapsed > crate::SNAPSHOT_MAXIMUM_AGE;
        if observation_gap {
            self.cpu_trend = SiliconTrend::default();
            self.gpu_trend = SiliconTrend::default();
            self.load_elapsed = 0.0;
        }
        let dt = if reseed { 0.0 } else { elapsed.min(5.0) };
        let observation_dt = if observation_gap { 0.0 } else { dt };
        let cpu = snapshot.average_temperature(SensorGroup::Cpu);
        let gpu = snapshot.average_temperature(SensorGroup::Gpu);
        let silicon = [cpu, gpu].into_iter().flatten().reduce(f64::max);
        let load = snapshot
            .cpu_utilization_percent
            .filter(|load| load.is_finite() && (0.0..=100.0).contains(load));
        if load.is_some_and(|load| load >= 65.0) && silicon.is_some() {
            self.load_elapsed += observation_dt;
        } else {
            self.load_elapsed = 0.0;
        }
        let load_sustained = self.load_elapsed >= ADAPTIVE_LOAD_CONFIRMATION_SECONDS;
        let (cpu_prediction, cpu_rising) = self.cpu_trend.update(cpu, observation_dt);
        let (gpu_prediction, gpu_rising) = self.gpu_trend.update(gpu, observation_dt);
        let trend_confirmed = cpu_rising || gpu_rising;
        let predicted = [cpu_prediction, gpu_prediction]
            .into_iter()
            .flatten()
            .reduce(f64::max);
        // 日常调节复用芯片 30 秒、机身 90 秒滤波后的整体热负荷。
        // 原始芯片温度与严重热压力的紧急底线由 controller 独立处理。
        let temperature_demand = if thermal.available {
            thermal.demand_percent
        } else {
            0.0
        };
        let feed_forward = if load_sustained {
            25.0 + 30.0 * smooth(load.unwrap_or(0.0), 65.0, 95.0)
        } else {
            0.0
        };
        let trend_demand = if trend_confirmed {
            predicted
                .map(|temperature| 80.0 * smooth(temperature, 60.0, 90.0))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let chassis_soak = if thermal.uses_chassis_sensor {
            45.0 * smooth(thermal.chassis_temperature, 40.0, 55.0)
        } else {
            0.0
        };
        let soak_input = if silicon.is_some() {
            (70.0 * smooth(thermal.sustained_silicon_temperature, 65.0, 90.0)).max(chassis_soak)
        } else {
            0.0
        };
        if reseed {
            // 一次芯片热读数不能证明已有长期蓄热；机身节点可提供初始保守下界。
            self.heat_soak = chassis_soak;
        } else {
            let constant = if soak_input > self.heat_soak {
                30.0
            } else {
                120.0
            };
            self.heat_soak += (1.0 - (-dt / constant).exp()) * (soak_input - self.heat_soak);
        }
        let (load_input, load_reason) = [
            (temperature_demand, AdaptiveIntervention::Temperature),
            (feed_forward, AdaptiveIntervention::LoadFeedForward),
            (trend_demand, AdaptiveIntervention::RisingTemperature),
        ]
        .into_iter()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();
        let constant = if load_input > self.thermal_load {
            ADAPTIVE_DEMAND_RISE_SECONDS
        } else {
            ADAPTIVE_DEMAND_FALL_SECONDS
        };
        self.thermal_load += (1.0 - (-dt / constant).exp()) * (load_input - self.thermal_load);
        let load_reason = if self.thermal_load > load_input + 1.0 {
            AdaptiveIntervention::HeatSoak
        } else {
            load_reason
        };
        let (surface, comfort_status, comfort) = comfort_input(snapshot, policy);
        let candidates = [
            (self.thermal_load, load_reason),
            (self.heat_soak, AdaptiveIntervention::HeatSoak),
            (comfort, AdaptiveIntervention::Comfort),
        ];
        let (mut demand, mut intervention) = candidates
            .into_iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        if demand < 1.0 {
            demand = 0.0;
            intervention = AdaptiveIntervention::Idle;
        }
        AdaptiveReading {
            demand_percent: demand.clamp(0.0, 100.0),
            adjusted_demand_percent: Some(demand.clamp(0.0, 100.0)),
            thermal_load_percent: self.thermal_load.clamp(0.0, 100.0),
            cpu_utilization_percent: load,
            load_sustained,
            silicon_rise_celsius_per_second: self
                .cpu_trend
                .filtered_rise
                .max(self.gpu_trend.filtered_rise),
            predicted_silicon_celsius: predicted,
            heat_soak_percent: self.heat_soak,
            estimated_surface_celsius: surface,
            comfort_demand_percent: comfort,
            comfort_status,
            intervention,
            // Load and a calibrated proxy cannot authorize custom control when
            // live CPU/GPU safety telemetry is absent.
            available: snapshot.hottest_silicon().is_some(),
        }
    }
}
fn comfort_input(snapshot: &Snapshot, policy: &ThermalPolicy) -> (Option<f64>, ComfortStatus, f64) {
    let Some(target) = policy.comfort_target_celsius else {
        return (None, ComfortStatus::NotRequested, 0.0);
    };
    let Some(calibration) = &policy.calibration else {
        return (None, ComfortStatus::Uncalibrated, 0.0);
    };
    if policy.validate().is_err() {
        return (None, ComfortStatus::OutOfRange, 0.0);
    }
    if snapshot.machine_id.as_deref() != Some(calibration.machine_id.as_str()) {
        return (None, ComfortStatus::MachineMismatch, 0.0);
    }
    let Some(proxy) = snapshot
        .sensors
        .iter()
        .find(|sensor| sensor.key == calibration.sensor_key)
        .and_then(|sensor| sensor.value.filter(|value| valid_temperature(*value)))
    else {
        return (None, ComfortStatus::MissingSensor, 0.0);
    };
    let estimate = proxy + calibration.offset_celsius;
    if proxy > calibration.sensor_max_celsius || estimate > calibration.surface_max_celsius {
        // Only the intersection of both measured ranges is supported. A hot
        // extrapolation cannot claim a surface temperature, but crossing the
        // upper endpoint must not remove the cooling required at that endpoint.
        let supported_upper = (calibration.sensor_max_celsius + calibration.offset_celsius)
            .min(calibration.surface_max_celsius);
        return (
            None,
            ComfortStatus::AboveCalibrationRange,
            comfort_demand(supported_upper, target),
        );
    }
    if proxy < calibration.sensor_min_celsius || estimate < calibration.surface_min_celsius {
        return (None, ComfortStatus::BelowCalibrationRange, 0.0);
    }
    (
        Some(estimate),
        ComfortStatus::Active,
        comfort_demand(estimate, target),
    )
}
fn comfort_demand(surface: f64, target: f64) -> f64 {
    65.0 * smooth(surface, target - 1.0, target + 5.0)
}
fn smooth(value: f64, lower: f64, upper: f64) -> f64 {
    let value = ((value - lower) / (upper - lower)).clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}
