use crate::{ThermalPolicy, ABSOLUTE_MAXIMUM_RPM, THERMAL_DEMAND_KEY};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt;

pub const CONFIG_VERSION: u32 = 2;
pub const PRESET_VERSION: u32 = 2;
pub const FAN_OFF_PERCENT: f64 = -20.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ControlMode {
    #[default]
    Automatic,
    Adaptive,
    Manual {
        rpm: u32,
    },
    Curve {
        curve_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temperature: f64,
    #[serde(alias = "fanSpeed")]
    pub speed_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub id: String,
    pub name: String,
    #[serde(alias = "sensorKey")]
    pub sensor_key: String,
    pub points: Vec<CurvePoint>,
    #[serde(default = "default_hysteresis")]
    pub hysteresis: f64,
    #[serde(default, alias = "presetVersion")]
    pub preset_version: Option<u32>,
}

fn default_hysteresis() -> f64 {
    3.0
}

impl Curve {
    pub fn balanced(id: impl Into<String>, sensor_key: impl Into<String>) -> Self {
        let sensor_key = sensor_key.into();
        let thermal = sensor_key == THERMAL_DEMAND_KEY;
        let points: &[(f64, f64)] = if thermal {
            &[
                (0.0, -20.0),
                (18.0, -20.0),
                (28.0, 0.0),
                (45.0, 12.0),
                (60.0, 25.0),
                (75.0, 45.0),
                (88.0, 70.0),
                (100.0, 100.0),
            ]
        } else {
            &[
                (35.0, -20.0),
                (45.0, -20.0),
                (55.0, 0.0),
                (65.0, 15.0),
                (75.0, 35.0),
                (85.0, 65.0),
                (95.0, 100.0),
            ]
        };
        Self {
            id: id.into(),
            name: if thermal {
                "Balanced Thermal"
            } else {
                "Balanced Temperature"
            }
            .into(),
            sensor_key,
            points: points
                .iter()
                .map(|&(temperature, speed_percent)| CurvePoint {
                    temperature,
                    speed_percent,
                })
                .collect(),
            hysteresis: if thermal { 8.0 } else { 4.0 },
            preset_version: Some(PRESET_VERSION),
        }
    }

    /// 源尺度切换时恢复对应预设，避免把 demand 0...100 点映射到摄氏温度。
    pub fn set_sensor_key(&mut self, sensor_key: impl Into<String>) {
        let sensor_key = sensor_key.into();
        if (self.sensor_key == THERMAL_DEMAND_KEY) != (sensor_key == THERMAL_DEMAND_KEY) {
            *self = Self::balanced(self.id.clone(), sensor_key);
        } else {
            self.sensor_key = sensor_key;
        }
    }

    pub fn interpolate(&self, temperature: f64) -> f64 {
        if self.points.len() < 2
            || !temperature.is_finite()
            || self
                .points
                .iter()
                .any(|point| !point.temperature.is_finite() || !point.speed_percent.is_finite())
        {
            return 100.0;
        }
        let mut points: Vec<&CurvePoint> = self.points.iter().collect();
        points.sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
        if temperature <= points[0].temperature {
            return points[0].speed_percent;
        }
        if temperature >= points[points.len() - 1].temperature {
            return points[points.len() - 1].speed_percent;
        }
        for segment in points.windows(2) {
            let (low, high) = (segment[0], segment[1]);
            if high.temperature > low.temperature
                && temperature >= low.temperature
                && temperature <= high.temperature
            {
                let ratio = (temperature - low.temperature) / (high.temperature - low.temperature);
                let speed = low.speed_percent + ratio * (high.speed_percent - low.speed_percent);
                return if speed.is_finite() { speed } else { 100.0 };
            }
        }
        100.0
    }

    pub fn interpolate_with_hysteresis(
        &self,
        temperature: f64,
        last_speed: f64,
        rising: bool,
    ) -> f64 {
        if rising {
            self.interpolate(temperature)
        } else {
            self.interpolate(temperature + self.hysteresis)
                .min(last_speed)
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.id.is_empty()
            || self.id.len() > 128
            || self.name.is_empty()
            || self.name.len() > 256
            || self.sensor_key.is_empty()
            || self.sensor_key.len() > 128
        {
            return Err(ConfigError::Invalid("曲线标识、名称或传感器无效".into()));
        }
        if !(2..=64).contains(&self.points.len())
            || !self.hysteresis.is_finite()
            || !(0.0..=30.0).contains(&self.hysteresis)
        {
            return Err(ConfigError::Invalid(
                "曲线必须有 2...64 个点，迟滞范围为 0...30".into(),
            ));
        }
        let maximum_temperature = if self.sensor_key == THERMAL_DEMAND_KEY {
            100.0
        } else {
            120.0
        };
        let mut temperatures = Vec::new();
        for point in &self.points {
            if !point.temperature.is_finite()
                || !(0.0..=maximum_temperature).contains(&point.temperature)
                || !point.speed_percent.is_finite()
                || !(FAN_OFF_PERCENT..=100.0).contains(&point.speed_percent)
            {
                return Err(ConfigError::Invalid("曲线点超出有效温度或速度范围".into()));
            }
            if temperatures.contains(&point.temperature) {
                return Err(ConfigError::Invalid("曲线点的温度不能重复".into()));
            }
            temperatures.push(point.temperature);
        }
        Ok(())
    }

    pub fn migrate_legacy_default(&mut self) -> bool {
        const LEGACY: &[(f64, f64)] = &[
            (35.0, -20.0),
            (42.0, -20.0),
            (50.0, 0.0),
            (60.0, 25.0),
            (70.0, 45.0),
            (80.0, 70.0),
            (90.0, 100.0),
        ];
        let mut points = self.points.clone();
        points.sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
        if self.sensor_key == "Average CPU"
            && self.name == "Default"
            && (self.hysteresis - 3.0).abs() < 0.001
            && points.len() == LEGACY.len()
            && points.iter().zip(LEGACY).all(|(point, expected)| {
                (point.temperature - expected.0).abs() < 0.001
                    && (point.speed_percent - expected.1).abs() < 0.001
            })
        {
            *self = Self::balanced(self.id.clone(), THERMAL_DEMAND_KEY);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FanConfig {
    pub fan_id: u8,
    #[serde(default)]
    pub mode: ControlMode,
    #[serde(default)]
    pub curve: Option<Curve>,
}

impl FanConfig {
    pub fn adaptive(fan_id: u8) -> Self {
        Self {
            fan_id,
            mode: ControlMode::Adaptive,
            curve: None,
        }
    }
    pub fn automatic(fan_id: u8) -> Self {
        Self {
            fan_id,
            mode: ControlMode::Automatic,
            curve: None,
        }
    }
    pub fn balanced(fan_id: u8) -> Self {
        let id = format!("balanced-fan-{fan_id}");
        Self {
            fan_id,
            mode: ControlMode::Curve {
                curve_id: id.clone(),
            },
            curve: Some(Curve::balanced(id, THERMAL_DEMAND_KEY)),
        }
    }
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.fan_id > 9 {
            return Err(ConfigError::Invalid("风扇 id 必须为 0...9".into()));
        }
        if let Some(curve) = &self.curve {
            curve.validate()?;
        }
        match &self.mode {
            ControlMode::Manual { rpm } if *rpm == 0 || *rpm > ABSOLUTE_MAXIMUM_RPM => Err(
                ConfigError::Invalid("手动转速必须为 1...16383；停转只允许通过曲线策略".into()),
            ),
            ControlMode::Curve { curve_id }
                if self
                    .curve
                    .as_ref()
                    .is_none_or(|curve| &curve.id != curve_id) =>
            {
                Err(ConfigError::Invalid("曲线模式与配置标识不一致".into()))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub fans: Vec<FanConfig>,
    #[serde(default)]
    pub thermal_policy: ThermalPolicy,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            fans: Vec::new(),
            thermal_policy: ThermalPolicy::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigLoad {
    pub config: Config,
    pub migrated: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Json(serde_json::Error),
    UnsupportedVersion(u32),
    Invalid(String),
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "配置 JSON 解析失败：{error}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "不支持配置版本 {version}，当前版本为 {CONFIG_VERSION}")
            }
            Self::Invalid(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for ConfigError {}
impl From<serde_json::Error> for ConfigError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(self.version));
        }
        if self.fans.len() > 10 {
            return Err(ConfigError::Invalid("最多配置 10 个风扇".into()));
        }
        self.thermal_policy.validate()?;
        let mut ids = BTreeSet::new();
        for fan in &self.fans {
            if !ids.insert(fan.fan_id) {
                return Err(ConfigError::Invalid("风扇 id 重复".into()));
            }
            fan.validate()?;
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, ConfigError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(input: &str) -> Result<ConfigLoad, ConfigError> {
        if input.len() > 1024 * 1024 {
            return Err(ConfigError::Invalid("配置文件超过 1 MiB".into()));
        }
        let value: Value = serde_json::from_str(input)?;
        let mut warnings = Vec::new();
        let legacy = value.is_array();
        let mut config = if let Some(fans) = value.as_array() {
            if fans.len() > 10 {
                return Err(ConfigError::Invalid("旧配置超过风扇数量上限".into()));
            }
            let mut config = Config::default();
            for fan in fans {
                let id = fan
                    .get("fanId")
                    .and_then(Value::as_u64)
                    .filter(|id| *id <= 9)
                    .ok_or_else(|| ConfigError::Invalid("旧配置包含无效风扇 id".into()))?
                    as u8;
                let curve = fan
                    .get("curveConfig")
                    .filter(|value| !value.is_null())
                    .map(|value| serde_json::from_value::<Curve>(value.clone()))
                    .transpose()?;
                let mode = fan.get("mode");
                // Swift Codable associated-value enum 格式：{"manual":{"rpm":3000}}。
                let parsed = if let Some(rpm) = mode
                    .and_then(|mode| mode.get("manual"))
                    .and_then(|manual| manual.get("rpm"))
                    .and_then(Value::as_u64)
                    .filter(|rpm| *rpm <= u32::MAX as u64)
                {
                    ControlMode::Manual { rpm: rpm as u32 }
                } else if let Some(id) = mode
                    .and_then(|mode| mode.get("curve"))
                    .and_then(|curve| curve.get("configId"))
                    .and_then(Value::as_str)
                {
                    ControlMode::Curve {
                        curve_id: id.into(),
                    }
                } else {
                    if mode.is_some_and(|mode| !mode.is_null() && mode.get("automatic").is_none()) {
                        warnings.push(format!("风扇 {id} 的旧控制模式损坏，已恢复系统自动"));
                    }
                    ControlMode::Automatic
                };
                config.fans.push(FanConfig {
                    fan_id: id,
                    mode: parsed,
                    curve,
                });
            }
            config
        } else {
            serde_json::from_value::<Config>(value)?
        };
        let version_one = config.version == 1;
        if version_one {
            config.version = CONFIG_VERSION;
        } else if config.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(config.version));
        }
        let mut migrated = legacy || version_one;
        if let Err(error) = config.thermal_policy.validate() {
            warnings.push(format!("热策略或表面校准无效，已禁用舒适估计：{error}"));
            config.thermal_policy = ThermalPolicy::default();
            migrated = true;
        }
        for fan in &mut config.fans {
            if let Some(curve) = &mut fan.curve {
                migrated |= curve.migrate_legacy_default();
                // 老版本未标记自定义曲线 presetVersion；保留其点和 id。
            }
            if let Err(error) = fan.validate() {
                warnings.push(format!(
                    "风扇 {} 配置无效，已恢复系统自动：{error}",
                    fan.fan_id
                ));
                fan.mode = ControlMode::Automatic;
                if fan
                    .curve
                    .as_ref()
                    .is_some_and(|curve| curve.validate().is_err())
                {
                    fan.curve = None;
                }
                migrated = true;
            }
        }
        config.validate()?;
        Ok(ConfigLoad {
            config,
            migrated,
            warnings,
        })
    }
}
