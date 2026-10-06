use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SNAPSHOT_MAXIMUM_AGE: f64 = 15.0;
pub const MISSING_INPUT_MAXIMUM_HOLD: f64 = 30.0;
pub const MINIMUM_FAN_OFF_RESIDENCE: f64 = 90.0;
pub const MINIMUM_FAN_RUN_RESIDENCE: f64 = 180.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    SetAutomatic,
    SetRpm { rpm: u32, allow_fan_off: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionReason {
    UserRequest,
    Curve,
    AdaptivePerformance,
    AdaptiveTrend,
    AdaptiveHeatSoak,
    AdaptiveTemperature,
    SurfaceComfort,
    LowDemandSystem,
    Emergency,
    MissingInput,
    StaleSnapshot,
    InvalidBounds,
    UnknownHardwareMode,
    ModeReconcile,
    RpmReconcile,
    Sleep,
    Shutdown,
    WriteFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: u64,
    pub fan_id: u8,
    pub command: Command,
    pub reason: ActionReason,
}

#[derive(Debug, Clone, Default)]
pub struct FanStatus {
    /// 用户/曲线当轮目标，允许与 ramp 后写入值不同。
    pub desired_rpm: Option<u32>,
    /// 最近一次宿主确认成功的写入值；不代表实时 tachometer 数值。
    pub applied_rpm: Option<u32>,
    pub pending: Option<Action>,
    pub last_failure: Option<String>,
    pub safety_override: bool,
    pub status_reason: Option<ActionReason>,
}

#[derive(Debug, Clone)]
struct Runtime {
    status: FanStatus,
    generation: u64,
    pending_generation: u64,
    pending_was_off: bool,
    dirty: bool,
    needs_failure_hand_back: bool,
    last_write_at: Option<f64>,
    last_auto_request_at: Option<f64>,
    last_mode_reconcile_at: Option<f64>,
    mismatch_started_at: Option<f64>,
    last_rpm_reconcile_at: Option<f64>,
    off_since: Option<f64>,
    run_since: Option<f64>,
    adaptive_run_since: Option<f64>,
    input_missing_since: Option<f64>,
    safety_missing_since: Option<f64>,
    last_input: Option<f64>,
    last_speed_percent: f64,
    rising: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            status: FanStatus::default(),
            generation: 0,
            pending_generation: 0,
            pending_was_off: false,
            dirty: true,
            needs_failure_hand_back: false,
            last_write_at: None,
            last_auto_request_at: None,
            last_mode_reconcile_at: None,
            mismatch_started_at: None,
            last_rpm_reconcile_at: None,
            off_since: None,
            run_since: None,
            adaptive_run_since: None,
            input_missing_since: None,
            safety_missing_since: None,
            last_input: None,
            last_speed_percent: 0.0,
            rising: true,
        }
    }
}

pub struct Controller {
    config: Config,
    runtime: BTreeMap<u8, Runtime>,
    known_fans: BTreeSet<u8>,
    next_action_id: u64,
    estimator: ThermalEstimator,
    adaptive_estimator: AdaptiveEstimator,
    thermal: ThermalReading,
    last_sample_at: Option<f64>,
    suspended: bool,
}

impl Controller {
    pub fn new(mut config: Config) -> Self {
        // 构造器仍 fail closed；from_json/replace_config 提供显式错误给 UI。
        if config.validate().is_err() {
            let mut ids = BTreeSet::new();
            config = Config {
                version: CONFIG_VERSION,
                thermal_policy: ThermalPolicy::default(),
                fans: config
                    .fans
                    .iter()
                    .filter(|fan| fan.fan_id <= 9 && ids.insert(fan.fan_id))
                    .map(|fan| FanConfig::automatic(fan.fan_id))
                    .collect(),
            };
        }
        let runtime = config
            .fans
            .iter()
            .map(|fan| (fan.fan_id, Runtime::default()))
            .collect();
        Self {
            config,
            runtime,
            known_fans: BTreeSet::new(),
            next_action_id: 1,
            estimator: ThermalEstimator::default(),
            adaptive_estimator: AdaptiveEstimator::default(),
            thermal: ThermalReading::default(),
            last_sample_at: None,
            suspended: false,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn thermal_reading(&self) -> &ThermalReading {
        &self.thermal
    }
    pub fn fan_status(&self, fan_id: u8) -> Option<&FanStatus> {
        self.runtime.get(&fan_id).map(|state| &state.status)
    }

    pub fn polling_activity(&self) -> PollingActivity {
        if self
            .config
            .fans
            .iter()
            .any(|fan| matches!(fan.mode, ControlMode::Curve { .. } | ControlMode::Adaptive))
        {
            PollingActivity::Curve
        } else if self
            .config
            .fans
            .iter()
            .any(|fan| matches!(fan.mode, ControlMode::Manual { .. }))
        {
            PollingActivity::Manual
        } else {
            PollingActivity::Automatic
        }
    }

    pub fn replace_config(&mut self, config: Config) -> Result<(), ConfigError> {
        config.validate()?;
        if self.config.thermal_policy != config.thermal_policy {
            self.adaptive_estimator.reset();
            self.last_sample_at = None;
            self.thermal.adaptive = AdaptiveReading::default();
            for state in self.runtime.values_mut() {
                state.generation = state.generation.wrapping_add(1);
                state.status.pending = None;
                state.dirty = true;
            }
        }
        // 被删除的已发现风扇也要交还系统；直到下次快照仍保留自动配置。
        let removed: Vec<_> = self
            .known_fans
            .iter()
            .copied()
            .filter(|id| !config.fans.iter().any(|fan| fan.fan_id == *id))
            .collect();
        for fan in &config.fans {
            self.set_fan_config(fan.clone())?;
        }
        self.config = config;
        for id in removed {
            self.set_fan_config(FanConfig::automatic(id))?;
        }
        Ok(())
    }

    pub fn set_fan_config(&mut self, config: FanConfig) -> Result<(), ConfigError> {
        config.validate()?;
        let old = self
            .config
            .fans
            .iter()
            .find(|fan| fan.fan_id == config.fan_id);
        if old == Some(&config) {
            return Ok(());
        }
        let source_changed = old
            .and_then(|fan| fan.curve.as_ref())
            .map(|curve| &curve.sensor_key)
            != config.curve.as_ref().map(|curve| &curve.sensor_key);
        let state = self.runtime.entry(config.fan_id).or_default();
        state.generation = state.generation.wrapping_add(1);
        state.dirty = true;
        state.last_input = None;
        state.last_speed_percent = 0.0;
        state.rising = true;
        if source_changed {
            state.input_missing_since = None;
        }
        state.status.desired_rpm = None;
        state.status.last_failure = None;
        state.status.safety_override = false;
        state.status.status_reason = None;
        if !matches!(config.mode, ControlMode::Adaptive) {
            state.adaptive_run_since = None;
        }
        if matches!(config.mode, ControlMode::Automatic) {
            state.off_since = None;
            state.run_since = None;
            state.adaptive_run_since = None;
        }
        if let Some(existing) = self
            .config
            .fans
            .iter_mut()
            .find(|fan| fan.fan_id == config.fan_id)
        {
            *existing = config;
        } else {
            self.config.fans.push(config);
        }
        Ok(())
    }

    /// 宿主应按产生顺序串行执行动作，不允许并行 RPM 与 hand-back。
    pub fn update(&mut self, snapshot: &Snapshot, now: f64) -> Vec<Action> {
        if self.suspended || !now.is_finite() {
            return Vec::new();
        }
        let fresh = snapshot.fresh(now, SNAPSHOT_MAXIMUM_AGE);
        if fresh && self.last_sample_at != Some(snapshot.sampled_at) {
            let elapsed = self
                .last_sample_at
                .map(|last| snapshot.sampled_at - last)
                .unwrap_or(0.0);
            self.thermal = self.estimator.update_snapshot(snapshot, elapsed);
            self.thermal.adaptive = self.adaptive_estimator.update(
                snapshot,
                &self.thermal,
                &self.config.thermal_policy,
                elapsed,
            );
            self.last_sample_at = Some(snapshot.sampled_at);
        } else if !fresh {
            self.thermal.available = false;
            self.thermal.adaptive.available = false;
            self.thermal.adaptive.estimated_surface_celsius = None;
            self.thermal.adaptive.comfort_demand_percent = 0.0;
            self.thermal.adaptive.cpu_utilization_percent = None;
            self.thermal.adaptive.predicted_silicon_celsius = None;
            self.thermal.adaptive.load_sustained = false;
            if matches!(
                self.thermal.adaptive.comfort_status,
                ComfortStatus::Active
                    | ComfortStatus::AboveCalibrationRange
                    | ComfortStatus::BelowCalibrationRange
            ) {
                self.thermal.adaptive.comfort_status = ComfortStatus::MissingSensor;
            }
        }

        // FNum 不可用视为读取失败，不把配置中的任意 id 当硬件存在证明。
        let count_valid = valid_fan_count(snapshot.fan_count).is_some();
        let fans: BTreeMap<u8, &Fan> = snapshot
            .fans
            .iter()
            .filter(|fan| valid_fan_id(fan.id, snapshot.fan_count))
            .map(|fan| (fan.id, fan))
            .collect();
        if fresh && count_valid {
            for &id in fans.keys() {
                self.known_fans.insert(id);
                self.runtime.entry(id).or_default();
                if !self.config.fans.iter().any(|fan| fan.fan_id == id) {
                    self.config.fans.push(FanConfig::automatic(id));
                }
            }
        }
        let mut actions = Vec::new();
        let ids: Vec<u8> = self.known_fans.iter().copied().collect();
        for id in ids {
            let config = self
                .config
                .fans
                .iter()
                .find(|fan| fan.fan_id == id)
                .cloned()
                .unwrap_or_else(|| FanConfig::automatic(id));
            let state = self.runtime.get_mut(&id).expect("known fan runtime");
            if state.status.pending.is_some() {
                continue;
            }
            state.status.safety_override = false;
            if state.needs_failure_hand_back {
                emit_auto(
                    state,
                    id,
                    ActionReason::WriteFailure,
                    now,
                    false,
                    &mut self.next_action_id,
                    &mut actions,
                );
                continue;
            }
            let fan = fans.get(&id).copied();
            if !fresh || !count_valid || fan.is_none() {
                state.status.desired_rpm = None;
                emit_auto(
                    state,
                    id,
                    ActionReason::StaleSnapshot,
                    now,
                    false,
                    &mut self.next_action_id,
                    &mut actions,
                );
                continue;
            }
            let fan = fan.expect("checked fan");
            if matches!(config.mode, ControlMode::Automatic) {
                state.status.desired_rpm = None;
                if state.dirty || fan.mode != HardwareMode::Automatic {
                    emit_auto(
                        state,
                        id,
                        if state.dirty {
                            ActionReason::UserRequest
                        } else {
                            ActionReason::ModeReconcile
                        },
                        now,
                        state.dirty,
                        &mut self.next_action_id,
                        &mut actions,
                    );
                } else {
                    state.status.status_reason = None;
                }
                continue;
            }
            if !fan.controllable() {
                state.status.desired_rpm = None;
                emit_auto(
                    state,
                    id,
                    ActionReason::InvalidBounds,
                    now,
                    state.dirty,
                    &mut self.next_action_id,
                    &mut actions,
                );
                continue;
            }
            if fan.mode == HardwareMode::Unknown
                || fan.current_rpm.is_none_or(|rpm| {
                    !rpm.is_finite() || rpm < 0.0 || rpm > ABSOLUTE_MAXIMUM_RPM as f64
                })
            {
                state.status.desired_rpm = None;
                emit_auto(
                    state,
                    id,
                    ActionReason::UnknownHardwareMode,
                    now,
                    state.dirty,
                    &mut self.next_action_id,
                    &mut actions,
                );
                continue;
            }
            let emergency =
                missing_input_safety_percent(snapshot.thermal_pressure, snapshot.hottest_silicon());
            if snapshot.hottest_silicon().is_none() {
                let missing_since = *state.safety_missing_since.get_or_insert(now);
                if emergency.is_none()
                    && (now - missing_since >= MISSING_INPUT_MAXIMUM_HOLD
                        || state.status.applied_rpm.is_none())
                {
                    state.status.desired_rpm = None;
                    emit_auto(
                        state,
                        id,
                        ActionReason::MissingInput,
                        now,
                        false,
                        &mut self.next_action_id,
                        &mut actions,
                    );
                    continue;
                }
            } else {
                state.safety_missing_since = None;
            }
            let (minimum, maximum) =
                validated_rpm_range(fan.min_rpm, fan.max_rpm).expect("controllable range");
            let observed_off = fan.current_rpm.is_some_and(|rpm| rpm <= 50.0);
            let currently_off =
                minimum == 0 && (state.status.applied_rpm == Some(0) || observed_off);
            let mut bypass = emergency.is_some();
            let mut reason = if state.dirty {
                ActionReason::UserRequest
            } else {
                ActionReason::Curve
            };
            let mut force = state.dirty;
            let mut comfort_floor = None;
            let mut desired;
            match &config.mode {
                ControlMode::Adaptive => {
                    let adaptive = &self.thermal.adaptive;
                    if adaptive.comfort_status == ComfortStatus::AboveCalibrationRange {
                        comfort_floor = Some(percent_rpm(
                            minimum,
                            maximum,
                            adaptive.comfort_demand_percent,
                        ));
                    }
                    let mut speed = adaptive.demand_percent;
                    if !adaptive.available && emergency.is_none() {
                        let since = *state.input_missing_since.get_or_insert(now);
                        state.status.status_reason = Some(ActionReason::MissingInput);
                        if now - since >= MISSING_INPUT_MAXIMUM_HOLD
                            || state.status.applied_rpm.is_none()
                        {
                            state.status.desired_rpm = None;
                            emit_auto(
                                state,
                                id,
                                ActionReason::MissingInput,
                                now,
                                false,
                                &mut self.next_action_id,
                                &mut actions,
                            );
                        }
                        continue;
                    }
                    state.input_missing_since = None;
                    reason = match adaptive.intervention {
                        AdaptiveIntervention::LoadFeedForward => ActionReason::AdaptivePerformance,
                        AdaptiveIntervention::RisingTemperature => ActionReason::AdaptiveTrend,
                        AdaptiveIntervention::HeatSoak => ActionReason::AdaptiveHeatSoak,
                        AdaptiveIntervention::Comfort => ActionReason::SurfaceComfort,
                        _ => ActionReason::AdaptiveTemperature,
                    };
                    if let Some(percent) = emergency {
                        speed = speed.max(percent);
                        reason = ActionReason::Emergency;
                        state.status.safety_override = true;
                    }
                    if snapshot.thermal_pressure == ThermalPressure::Fair {
                        speed = speed.max(35.0);
                    }
                    let cooling_residence = state
                        .adaptive_run_since
                        .is_some_and(|start| now - start < ADAPTIVE_COOLING_RESIDENCE_SECONDS);
                    if speed <= 0.0 && !cooling_residence {
                        state.status.desired_rpm = None;
                        state.status.status_reason = Some(ActionReason::LowDemandSystem);
                        if state.dirty || fan.mode != HardwareMode::Automatic {
                            let newly_forced = state.last_write_at.is_some_and(|write| {
                                state
                                    .last_auto_request_at
                                    .is_none_or(|automatic| write >= automatic)
                            });
                            emit_auto(
                                state,
                                id,
                                ActionReason::LowDemandSystem,
                                now,
                                state.dirty || newly_forced,
                                &mut self.next_action_id,
                                &mut actions,
                            );
                        }
                        continue;
                    }
                    desired = percent_rpm(minimum, maximum, speed);
                    if !adaptive.available && emergency.is_some() {
                        desired = desired.max(state.status.applied_rpm.unwrap_or(0));
                    }
                    // 接管时至少保留系统本轮已选择的散热；紧急热压始终独立设底线。
                    if fan.mode == HardwareMode::Automatic {
                        let system = validated_rpm(
                            fan.current_rpm.unwrap_or(0.0).ceil() as i64,
                            fan.min_rpm,
                            fan.max_rpm,
                            false,
                        )
                        .unwrap_or(0);
                        desired = desired.max(system);
                        bypass = true;
                    }
                    if cooling_residence && speed <= 0.0 {
                        reason = ActionReason::AdaptiveHeatSoak;
                    }
                }
                ControlMode::Manual { rpm } => {
                    desired = validated_rpm(*rpm as i64, fan.min_rpm, fan.max_rpm, false)
                        .expect("manual live range");
                    if let Some(percent) = emergency {
                        desired = desired.max(percent_rpm(minimum, maximum, percent));
                        reason = ActionReason::Emergency;
                        state.status.safety_override = true;
                    }
                }
                ControlMode::Curve { .. } => {
                    let curve = config.curve.as_ref().expect("validated curve config");
                    let input = snapshot.input_value(&curve.sensor_key, &self.thermal);
                    let speed = if let Some(input) = input {
                        state.input_missing_since = None;
                        if let Some(previous) = state.last_input {
                            if input - previous > 0.5 {
                                state.rising = true;
                            } else if input - previous < -0.5 {
                                state.rising = false;
                            }
                        }
                        let requested = curve.interpolate_with_hysteresis(
                            input,
                            state.last_speed_percent,
                            state.rising,
                        );
                        let adjusted = safety_adjusted_percent(
                            requested,
                            input,
                            &curve.sensor_key,
                            snapshot.thermal_pressure,
                            snapshot.hottest_silicon(),
                        );
                        state.last_input = Some(input);
                        state.last_speed_percent = adjusted;
                        if emergency.is_some() || adjusted > requested {
                            reason = ActionReason::Emergency;
                            state.status.safety_override = true;
                        }
                        bypass |= input >= 90.0 || adjusted >= 70.0;
                        adjusted
                    } else {
                        let since = *state.input_missing_since.get_or_insert(now);
                        if let Some(percent) = emergency {
                            reason = ActionReason::Emergency;
                            state.status.safety_override = true;
                            // 输入缺失时安全底线不降低先前已确认的散热目标。
                            let prior = state.status.applied_rpm.unwrap_or(0);
                            let prior_percent = if maximum > minimum {
                                (prior.saturating_sub(minimum)) as f64 * 100.0
                                    / (maximum - minimum) as f64
                            } else {
                                100.0
                            };
                            percent.max(prior_percent)
                        } else {
                            state.status.status_reason = Some(ActionReason::MissingInput);
                            if now - since >= MISSING_INPUT_MAXIMUM_HOLD
                                || state.status.applied_rpm.is_none()
                            {
                                emit_auto(
                                    state,
                                    id,
                                    ActionReason::MissingInput,
                                    now,
                                    false,
                                    &mut self.next_action_id,
                                    &mut actions,
                                );
                            }
                            continue;
                        }
                    };
                    if speed < 0.0 && minimum > 0 {
                        // 最低连续转速不证明设备支持强制停转；让系统决定低负荷行为。
                        state.status.desired_rpm = None;
                        state.status.status_reason = Some(ActionReason::LowDemandSystem);
                        if state.dirty || fan.mode != HardwareMode::Automatic {
                            // 新一轮自定义写入之后的降负荷必须立即 hand-back；失败重试仍限频。
                            let newly_forced = state.last_write_at.is_some_and(|write| {
                                state
                                    .last_auto_request_at
                                    .is_none_or(|automatic| write >= automatic)
                            });
                            emit_auto(
                                state,
                                id,
                                ActionReason::LowDemandSystem,
                                now,
                                state.dirty || newly_forced,
                                &mut self.next_action_id,
                                &mut actions,
                            );
                        }
                        continue;
                    }
                    desired = if speed < 0.0 {
                        minimum
                    } else {
                        percent_rpm(minimum, maximum, speed)
                    };
                    let urgent = bypass;
                    if desired == 0 {
                        if state
                            .run_since
                            .is_some_and(|start| now - start < MINIMUM_FAN_RUN_RESIDENCE)
                            && !urgent
                        {
                            desired = minimum.max(1);
                        }
                    } else if minimum == 0
                        && currently_off
                        && !urgent
                        && state
                            .off_since
                            .is_some_and(|start| now - start < MINIMUM_FAN_OFF_RESIDENCE)
                    {
                        desired = 0;
                    }
                    bypass |= fan
                        .current_rpm
                        .is_some_and(|rpm| rpm >= maximum as f64 * 0.95);
                }
                ControlMode::Automatic => unreachable!(),
            }
            state.status.desired_rpm = Some(desired);
            if fan.mode == HardwareMode::Automatic
                && interval_passed(state.last_mode_reconcile_at, now, 30.0)
            {
                force = true;
                state.last_mode_reconcile_at = Some(now);
                if matches!(reason, ActionReason::UserRequest | ActionReason::Curve) {
                    reason = ActionReason::ModeReconcile;
                }
            }
            let effective = state.status.applied_rpm.unwrap_or(desired);
            let tolerance = if effective == 0 {
                100.0
            } else {
                250.0_f64.max(effective as f64 * 0.12)
            };
            if (fan.current_rpm.unwrap_or(0.0) - effective as f64).abs() > tolerance {
                let mismatch = *state.mismatch_started_at.get_or_insert(now);
                if now - mismatch >= 10.0 && interval_passed(state.last_rpm_reconcile_at, now, 15.0)
                {
                    force = true;
                    state.last_rpm_reconcile_at = Some(now);
                    if matches!(reason, ActionReason::UserRequest | ActionReason::Curve) {
                        reason = ActionReason::RpmReconcile;
                    }
                }
            } else {
                state.mismatch_started_at = None;
            }
            let previous = state
                .status
                .applied_rpm
                .unwrap_or(fan.current_rpm.unwrap_or(0.0) as u32);
            let elapsed = state.last_write_at.map(|last| now - last).unwrap_or(1.0);
            let ramped = ramp_target(
                desired,
                previous,
                elapsed,
                &config.mode,
                bypass,
                previous == 0 && desired > 0,
            );
            let ramped = ramped.max(comfort_floor.unwrap_or(0));
            let target = validated_rpm(ramped as i64, fan.min_rpm, fan.max_rpm, desired == 0)
                .expect("bounded target");
            let should_write = force
                || state.status.applied_rpm.is_none()
                || state.status.applied_rpm.is_some_and(|previous| {
                    previous != target
                        && (previous.abs_diff(target) >= 75
                            || interval_passed(state.last_write_at, now, 5.0))
                });
            state.status.status_reason = if state.status.safety_override {
                Some(ActionReason::Emergency)
            } else if matches!(config.mode, ControlMode::Adaptive) {
                Some(reason)
            } else {
                None
            };
            if should_write {
                emit(
                    state,
                    id,
                    Command::SetRpm {
                        rpm: target,
                        allow_fan_off: target == 0,
                    },
                    reason,
                    currently_off,
                    &mut self.next_action_id,
                    &mut actions,
                );
            }
        }
        actions
    }

    /// 宿主暂缓（未发送）的动作：只清除挂起状态，不计为失败、不推进任何时钟，
    /// 下一轮采样重新评估。用于失败交还后的接管冷却期，避免反复启停。
    pub fn defer(&mut self, action: &Action) -> bool {
        let Some(state) = self.runtime.get_mut(&action.fan_id) else {
            return false;
        };
        if state.status.pending.as_ref() != Some(action) {
            return false;
        }
        state.status.pending = None;
        state.dirty = true;
        true
    }

    /// 只接受当前动作与 generation；成功后才推进 applied RPM、驻留及节流时钟。
    pub fn acknowledge(&mut self, action: &Action, success: bool, now: f64) -> bool {
        if !now.is_finite() {
            return false;
        }
        let adaptive =
            self.config.fans.iter().any(|fan| {
                fan.fan_id == action.fan_id && matches!(fan.mode, ControlMode::Adaptive)
            });
        let Some(state) = self.runtime.get_mut(&action.fan_id) else {
            return false;
        };
        if state.status.pending.as_ref() != Some(action) {
            return false;
        }
        state.status.pending = None;
        if state.pending_generation != state.generation {
            return false;
        }
        if !success {
            state.status.last_failure = Some(format!(
                "风扇 {} 的 {:?} 请求失败，正在交还系统",
                action.fan_id, action.command
            ));
            state.status.status_reason = Some(ActionReason::WriteFailure);
            state.dirty = true;
            if matches!(action.command, Command::SetRpm { .. }) {
                state.needs_failure_hand_back = true;
                state.last_auto_request_at = None;
            } else {
                state.dirty = false;
            }
            return true;
        }
        match action.command {
            Command::SetAutomatic => {
                state.status.applied_rpm = None;
                state.last_write_at = None;
                state.off_since = None;
                state.run_since = None;
                state.adaptive_run_since = None;
                state.mismatch_started_at = None;
                state.needs_failure_hand_back = false;
                // 自定义模式在降级自动后，下一份有效采样重新应用配置。
                state.dirty = false;
                if action.reason != ActionReason::WriteFailure {
                    state.status.last_failure = None;
                }
            }
            Command::SetRpm { rpm, .. } => {
                if adaptive && rpm > 0 {
                    state.adaptive_run_since.get_or_insert(now);
                }
                if rpm == 0 {
                    state.off_since.get_or_insert(now);
                    state.run_since = None;
                } else if state.pending_was_off || state.status.applied_rpm == Some(0) {
                    state.run_since = Some(now);
                    state.off_since = None;
                }
                state.status.applied_rpm = Some(rpm);
                state.last_write_at = Some(now);
                state.status.last_failure = None;
                state.dirty = false;
            }
        }
        true
    }

    pub fn prepare_for_sleep(&mut self, now: f64) -> Vec<Action> {
        self.hand_back(ActionReason::Sleep, now)
    }
    pub fn prepare_for_shutdown(&mut self, now: f64) -> Vec<Action> {
        self.hand_back(ActionReason::Shutdown, now)
    }

    fn hand_back(&mut self, reason: ActionReason, now: f64) -> Vec<Action> {
        self.suspended = true;
        let mut actions = Vec::new();
        for &id in &self.known_fans {
            let state = self.runtime.get_mut(&id).expect("known fan");
            state.generation = state.generation.wrapping_add(1);
            state.status.pending = None;
            emit_auto(
                state,
                id,
                reason,
                now,
                true,
                &mut self.next_action_id,
                &mut actions,
            );
        }
        actions
    }

    /// 宿主应在唤醒的多个恢复时点调用；每次都重读真实硬件模式后再 update。
    pub fn resume(&mut self) {
        self.suspended = false;
        self.estimator.reset();
        self.adaptive_estimator.reset();
        self.last_sample_at = None;
        self.thermal = ThermalReading::default();
        for state in self.runtime.values_mut() {
            let generation = state.generation.wrapping_add(1);
            *state = Runtime {
                generation,
                ..Runtime::default()
            };
        }
    }
}

fn interval_passed(last: Option<f64>, now: f64, interval: f64) -> bool {
    last.is_none_or(|last| now - last >= interval)
}

fn percent_rpm(minimum: u32, maximum: u32, percent: f64) -> u32 {
    ((minimum as f64 + percent.clamp(0.0, 100.0) / 100.0 * (maximum - minimum) as f64).ceil()
        as u32)
        .max(1)
}

fn emit_auto(
    state: &mut Runtime,
    id: u8,
    reason: ActionReason,
    now: f64,
    force: bool,
    next_id: &mut u64,
    actions: &mut Vec<Action>,
) {
    state.status.status_reason = Some(reason);
    if force || interval_passed(state.last_auto_request_at, now, 15.0) {
        state.last_auto_request_at = Some(now);
        emit(
            state,
            id,
            Command::SetAutomatic,
            reason,
            false,
            next_id,
            actions,
        );
    }
}

fn emit(
    state: &mut Runtime,
    fan_id: u8,
    command: Command,
    reason: ActionReason,
    was_off: bool,
    next_id: &mut u64,
    actions: &mut Vec<Action>,
) {
    let action = Action {
        id: *next_id,
        fan_id,
        command,
        reason,
    };
    *next_id = next_id.wrapping_add(1);
    state.pending_generation = state.generation;
    state.pending_was_off = was_off;
    state.status.pending = Some(action.clone());
    actions.push(action);
}
