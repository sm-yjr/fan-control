use serde::{Deserialize, Serialize};

pub const THERMAL_DEMAND_KEY: &str = "Thermal Demand";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThermalPressure {
    #[default]
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareMode {
    Automatic,
    Forced,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorGroup {
    Cpu,
    Gpu,
    System,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sensor {
    pub key: String,
    pub name: String,
    pub group: SensorGroup,
    /// 当前采样失败时必须为 None；不得用上一帧温度冒充新采样。
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fan {
    pub id: u8,
    pub name: String,
    /// 本轮采样的硬件边界。缺失时只能交还系统，不能构造写入上限。
    pub min_rpm: Option<f64>,
    pub max_rpm: Option<f64>,
    pub current_rpm: Option<f64>,
    pub mode: HardwareMode,
}

impl Fan {
    pub fn controllable(&self) -> bool {
        validated_rpm_range(self.min_rpm, self.max_rpm).is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// 宿主单调时钟的秒数；禁止用可被系统调整的 wall clock。
    pub sampled_at: f64,
    pub fan_count: Option<f64>,
    pub fans: Vec<Fan>,
    pub sensors: Vec<Sensor>,
    pub thermal_pressure: ThermalPressure,
    /// 当前两次 Mach CPU tick 差值所得利用率；无法读取时为 None。
    #[serde(default)]
    pub cpu_utilization_percent: Option<f64>,
    /// 本机 opaque scope；不可读取身份时禁止使用跨机器表面校准。
    #[serde(default, skip_serializing)]
    pub machine_id: Option<String>,
}

impl Snapshot {
    pub fn fresh(&self, now: f64, maximum_age: f64) -> bool {
        now.is_finite()
            && self.sampled_at.is_finite()
            && now >= self.sampled_at
            && now - self.sampled_at <= maximum_age
    }

    pub fn temperature_values(&self, group: SensorGroup) -> Vec<f64> {
        self.sensors
            .iter()
            .filter(|sensor| sensor.group == group)
            .filter_map(|sensor| sensor.value.filter(|value| valid_temperature(*value)))
            .collect()
    }

    pub fn hottest_silicon(&self) -> Option<f64> {
        self.sensors
            .iter()
            .filter(|sensor| matches!(sensor.group, SensorGroup::Cpu | SensorGroup::Gpu))
            .filter_map(|sensor| sensor.value.filter(|value| valid_temperature(*value)))
            .reduce(f64::max)
    }

    pub fn input_value(&self, key: &str, thermal: &crate::ThermalReading) -> Option<f64> {
        match key {
            THERMAL_DEMAND_KEY => thermal.available.then_some(thermal.demand_percent),
            "Average CPU" => average(&self.temperature_values(SensorGroup::Cpu)),
            "Average GPU" => average(&self.temperature_values(SensorGroup::Gpu)),
            "Hottest CPU" => self
                .temperature_values(SensorGroup::Cpu)
                .into_iter()
                .reduce(f64::max),
            "Hottest GPU" => self
                .temperature_values(SensorGroup::Gpu)
                .into_iter()
                .reduce(f64::max),
            _ => self
                .sensors
                .iter()
                .find(|sensor| sensor.key == key)
                .and_then(|sensor| sensor.value.filter(|value| valid_temperature(*value))),
        }
    }
}

fn average(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

pub fn valid_temperature(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value < 120.0
}

/// SMC fan key 内 id 只能占一个字符，FNum 必须是非负整数。
pub fn valid_fan_count(value: Option<f64>) -> Option<u8> {
    let value = value?;
    if !value.is_finite() || value < 0.0 || value.floor() != value {
        return None;
    }
    Some(value.min(10.0) as u8)
}

pub fn valid_fan_id(id: u8, count: Option<f64>) -> bool {
    valid_fan_count(count).is_some_and(|count| id < count)
}

pub const ABSOLUTE_MAXIMUM_RPM: u32 = 16_383;

pub fn validated_rpm_range(minimum: Option<f64>, maximum: Option<f64>) -> Option<(u32, u32)> {
    let minimum = minimum?;
    let maximum = maximum?;
    // 按真实浮点边界先验证，再转换。正向请求不能被截断到 0。
    if !minimum.is_finite()
        || !maximum.is_finite()
        || minimum < 0.0
        || minimum > ABSOLUTE_MAXIMUM_RPM as f64
        || maximum <= minimum
        || maximum >= u64::MAX as f64
    {
        return None;
    }
    let minimum = minimum.ceil() as u32;
    let maximum = maximum.floor().min(ABSOLUTE_MAXIMUM_RPM as f64) as u32;
    (maximum > minimum).then_some((minimum, maximum))
}

pub fn validated_rpm(
    request: i64,
    minimum: Option<f64>,
    maximum: Option<f64>,
    allow_fan_off: bool,
) -> Option<u32> {
    if request < 0 {
        return None;
    }
    let (minimum, maximum) = validated_rpm_range(minimum, maximum)?;
    if request == 0 {
        return (allow_fan_off && minimum == 0).then_some(0);
    }
    let rpm = request.clamp(minimum.max(1) as i64, maximum as i64) as u32;
    (rpm > 0).then_some(rpm)
}

pub fn target_type_encodable(data_type: &str) -> bool {
    matches!(data_type, "flt " | "fpe2")
}

pub fn requires_automatic_fallback(hardware_forced: bool, target_confirmed: bool) -> bool {
    hardware_forced && !target_confirmed
}
