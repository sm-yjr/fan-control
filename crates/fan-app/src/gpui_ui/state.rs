//! Central reactive GPUI application state shared across the Liquid Glass menu bar panel,
//! main split-view window, and interactive curve editor.
use crate::{
    gpui_ui::glass,
    gpui_ui::theme::IconTint,
    presenter::{self, CurvePreset, FanCard, FanChoice, Plan, Presentation, SmartStatus},
    sensor_list::{GroupFilter, SortOrder},
    status_bar::{MenuBarDisplayMode, StatusBarController},
    trend::{History, Sample},
    updater::Updater,
    worker::{UiSnapshot, Worker, WorkerCommand},
};
use fan_core::{
    validated_rpm_range, AdaptiveTuning, ControlMode, Curve, CurvePoint, FanConfig, SensorGroup,
    SurfaceCalibration, ThermalPolicy, FAN_OFF_PERCENT, THERMAL_DEMAND_KEY,
};
use gpui::{Context, Window};
use objc2::MainThreadMarker;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const SPEED_SETTLE: Duration = Duration::from_millis(550);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Overview,
    Fans,
    Smart,
    Details,
    Settings,
}

impl Section {
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Fans,
        Self::Smart,
        Self::Details,
        Self::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::Fans => "风扇",
            Self::Smart => "智能散热",
            Self::Details => "温度详情",
            Self::Settings => "设置",
        }
    }

    pub fn icon(self) -> gpui_kit_assets::IconName {
        use gpui_kit_assets::IconName;
        match self {
            Self::Overview => IconName::LayoutGrid,
            Self::Fans => IconName::Fan,
            Self::Smart => IconName::Sparkles,
            Self::Details => IconName::Thermometer,
            Self::Settings => IconName::Settings,
        }
    }

    pub fn tint(self) -> IconTint {
        match self {
            Self::Overview => IconTint::Blue,
            Self::Fans => IconTint::Teal,
            Self::Smart => IconTint::Purple,
            Self::Details => IconTint::Orange,
            Self::Settings => IconTint::Gray,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CurveDraft {
    pub fan_id: u8,
    pub curve: Curve,
    pub all_fans: bool,
    pub selected_point: Option<usize>,
    pub undo: Vec<Curve>,
    pub error: String,
}

impl CurveDraft {
    pub fn is_thermal_demand(&self) -> bool {
        self.curve.sensor_key == THERMAL_DEMAND_KEY
    }

    pub fn max_x(&self) -> f64 {
        if self.is_thermal_demand() {
            100.0
        } else {
            120.0
        }
    }

    pub fn x_unit(&self) -> &'static str {
        if self.is_thermal_demand() {
            "%"
        } else {
            "°C"
        }
    }
}

#[derive(Clone, Debug)]
pub struct PolicyDraft {
    pub comfort_enabled: bool,
    pub comfort_target_c: f64,
    pub dirty: bool,
    pub calibration_open: bool,
    pub cal_sensor_key: String,
    pub cal_ext_c: [f64; 2],
    pub cal_captured: [Option<(f64, f64)>; 2],
    pub notice: String,
    pub notice_is_error: bool,
}

impl Default for PolicyDraft {
    fn default() -> Self {
        Self {
            comfort_enabled: true,
            comfort_target_c: 38.0,
            dirty: false,
            calibration_open: false,
            cal_sensor_key: String::new(),
            cal_ext_c: [32.0, 40.0],
            cal_captured: [None, None],
            notice: String::new(),
            notice_is_error: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PendingSpeed {
    pub percent: f64,
    pub changed_at: Instant,
    pub epoch: u64,
}

pub struct AppState {
    pub demo: bool,
    pub worker: Worker,
    pub snapshot: UiSnapshot,
    pub history: History,
    pub section: Section,
    pub back_stack: Vec<Section>,
    pub forward_stack: Vec<Section>,
    pub menu_bar_mode: MenuBarDisplayMode,
    pub status_bar: Option<StatusBarController>,
    pub updater: Option<Updater>,

    /// Monotonically incremented whenever a global reset or plan transition occurs so
    /// older debounced fixed-speed writes can never override user intent.
    pub control_epoch: u64,
    /// Per-fan epoch counters so settling fan 0 never cancels a concurrent adjustment on fan 1.
    pub fan_epochs: BTreeMap<u8, u64>,

    // Pending debounced speed changes
    pub pending_speed: Option<PendingSpeed>,
    pub pending_fan_speeds: BTreeMap<u8, PendingSpeed>,
    /// Explicit user opt-in in the popover to switch active curve fans over to fixed speed.
    pub popover_fixed_override: bool,

    // Unified 21-step cooling preference transaction
    pub pending_policy_save: Option<mpsc::Receiver<Result<(), String>>>,
    pub pending_tuning: Option<(i8, i8)>,
    pub preference_notice: String,
    pub preference_notice_is_error: bool,

    // Smart cooling page draft
    pub policy: PolicyDraft,
    pub policy_loaded: bool,

    // Sensor details filter/sort/search
    pub sensor_group: GroupFilter,
    pub sensor_sort: SortOrder,
    pub sensor_query: String,

    // Settings UI state
    pub login_enabled: bool,
    pub settings_message: String,
    pub settings_message_is_error: bool,
    pub language_menu_open: bool,

    // Interactive curve editor modal state
    pub curve_editor: Option<CurveDraft>,

    // Cached visibility to throttle background polling when idle
    pub last_visible: bool,
}

impl AppState {
    pub fn new(demo: bool, status_bar: Option<StatusBarController>) -> Self {
        let worker = Worker::start(demo);
        let snapshot = worker.snapshot();
        let updater = if demo {
            None
        } else {
            MainThreadMarker::new().and_then(Updater::load)
        };
        let (login_enabled, settings_message, settings_message_is_error) = if demo {
            (false, String::new(), false)
        } else {
            match crate::settings::enabled() {
                Ok(v) => (v, String::new(), false),
                Err(e) => (false, e, true),
            }
        };

        let mut state = Self {
            demo,
            worker,
            snapshot,
            history: History::default(),
            section: Section::Overview,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            menu_bar_mode: MenuBarDisplayMode::load(demo),
            status_bar,
            updater,
            control_epoch: 0,
            fan_epochs: BTreeMap::new(),
            pending_speed: None,
            pending_fan_speeds: BTreeMap::new(),
            popover_fixed_override: false,
            pending_policy_save: None,
            pending_tuning: None,
            preference_notice: String::new(),
            preference_notice_is_error: false,
            policy: PolicyDraft::default(),
            policy_loaded: false,
            sensor_group: GroupFilter::All,
            sensor_sort: SortOrder::Grouped,
            sensor_query: String::new(),
            login_enabled,
            settings_message,
            settings_message_is_error,
            language_menu_open: false,
            curve_editor: None,
            last_visible: true,
        };
        // Only backfill immediately in demo mode or once the worker has loaded real config.
        if demo || state.snapshot.discovered {
            state.sync_policy_from_config(true);
        }
        state
    }

    pub fn is_fresh(&self) -> bool {
        self.snapshot.sample_age_secs + self.snapshot.published_at.elapsed().as_secs_f64()
            <= fan_core::SNAPSHOT_MAXIMUM_AGE
    }

    pub fn presentation(&self) -> Presentation {
        presenter::present(&self.snapshot, self.is_fresh(), self.demo)
    }

    pub fn fan_cards(&self) -> Vec<FanCard> {
        presenter::fan_cards(&self.snapshot, self.is_fresh())
    }

    pub fn smart_status(&self) -> SmartStatus {
        presenter::smart_status(&self.snapshot, self.is_fresh())
    }

    pub fn current_bias(&self) -> i8 {
        self.pending_tuning
            .map(|(_, next)| next)
            .unwrap_or(self.snapshot.config.adaptive_tuning.bias)
    }

    /// True when fans are in `Plan::Custom` and at least one fan uses a temperature/load curve.
    pub fn custom_uses_curves(&self) -> bool {
        self.snapshot
            .config
            .fans
            .iter()
            .any(|f| matches!(f.mode, ControlMode::Curve { .. }))
    }

    /// True when non-reset fan control writes are permitted (fresh telemetry, helper ready,
    /// not installing, not handing back, and not in isolated demo mode).
    pub fn fan_writes_allowed(&self) -> bool {
        !self.demo
            && self.snapshot.helper_ready
            && !self.snapshot.installing
            && !self.snapshot.handback_pending
            && self.is_fresh()
    }

    /// Available internal body sensors (`SensorGroup::System`) for two-point calibration.
    pub fn body_sensors(&self) -> Vec<(String, String, Option<f64>)> {
        let fresh = self.is_fresh();
        self.snapshot
            .snapshot
            .sensors
            .iter()
            .filter(|s| s.group == SensorGroup::System)
            .map(|s| {
                (
                    s.key.clone(),
                    s.name.clone(),
                    fresh.then_some(s.value).flatten().filter(|v| v.is_finite()),
                )
            })
            .collect()
    }

    /// Available control sources for the fan curve editor (Thermal Demand + CPU/GPU/System).
    pub fn curve_sources(&self) -> Vec<(String, String)> {
        let mut sources = vec![(THERMAL_DEMAND_KEY.to_string(), "热负荷（综合评估）".into())];
        for sensor in &self.snapshot.snapshot.sensors {
            if matches!(
                sensor.group,
                SensorGroup::Cpu | SensorGroup::Gpu | SensorGroup::System
            ) && !sensor.name.contains("热余量")
            {
                sources.push((
                    sensor.key.clone(),
                    format!("{} ({})", crate::i18n::translate(&sensor.name), sensor.key),
                ));
            }
        }
        sources
    }

    pub fn sync_policy_from_config(&mut self, force: bool) {
        if self.policy.dirty && !force {
            return;
        }
        let target = self.snapshot.config.thermal_policy.comfort_target_celsius;
        self.policy.comfort_enabled = target.is_some();
        self.policy.comfort_target_c = target.unwrap_or(38.0).clamp(30.0, 45.0);
        if let Some(cal) = &self.snapshot.config.thermal_policy.calibration {
            let same_machine = self
                .snapshot
                .snapshot
                .machine_id
                .as_deref()
                .is_some_and(|id| id == cal.machine_id);
            if same_machine {
                self.policy.cal_sensor_key = cal.sensor_key.clone();
                self.policy.cal_ext_c = [cal.surface_min_celsius, cal.surface_max_celsius];
                self.policy.cal_captured = [
                    Some((cal.sensor_min_celsius, cal.surface_min_celsius)),
                    Some((cal.sensor_max_celsius, cal.surface_max_celsius)),
                ];
            } else {
                // Never preload another machine's calibration points as newly re-bindable points.
                self.policy.cal_captured = [None, None];
            }
        } else if self.policy.cal_sensor_key.is_empty() {
            if let Some((key, _, _)) = self.body_sensors().first() {
                self.policy.cal_sensor_key = key.clone();
            }
        }
        self.policy.dirty = false;
        self.policy_loaded = true;
    }

    pub fn navigate(&mut self, next: Section, cx: &mut Context<Self>) {
        if self.section != next {
            self.back_stack.push(self.section);
            self.forward_stack.clear();
            self.section = next;
        }
        self.language_menu_open = false;
        if next == Section::Settings {
            self.sync_login_state();
        }
        cx.notify();
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(prev) = self.back_stack.pop() {
            self.forward_stack.push(self.section);
            self.section = prev;
            if prev == Section::Settings {
                self.sync_login_state();
            }
            cx.notify();
        }
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(next) = self.forward_stack.pop() {
            self.back_stack.push(self.section);
            self.section = next;
            if next == Section::Settings {
                self.sync_login_state();
            }
            cx.notify();
        }
    }

    fn cancel_pending_speeds(&mut self) {
        self.control_epoch = self.control_epoch.wrapping_add(1);
        for epoch in self.fan_epochs.values_mut() {
            *epoch = epoch.wrapping_add(1);
        }
        self.pending_speed = None;
        self.pending_fan_speeds.clear();
    }

    fn fan_epoch(&self, fan_id: u8) -> u64 {
        self.fan_epochs
            .get(&fan_id)
            .copied()
            .unwrap_or(self.control_epoch)
    }

    fn bump_fan_epoch(&mut self, fan_id: u8) {
        let next = self.fan_epoch(fan_id).wrapping_add(1);
        self.fan_epochs.insert(fan_id, next);
    }

    pub fn poll_tick(&mut self, any_window_visible: bool, cx: &mut Context<Self>) {
        if any_window_visible != self.last_visible {
            let _ = self
                .worker
                .send(WorkerCommand::Visibility(any_window_visible));
            self.last_visible = any_window_visible;
        }

        // Pull latest worker snapshot before checking settle eligibility.
        self.snapshot = self.worker.snapshot();
        if !self.policy_loaded && (self.snapshot.discovered || self.demo) {
            self.sync_policy_from_config(false);
        } else if self.policy.cal_sensor_key.is_empty() {
            if let Some((key, _, _)) = self.body_sensors().first() {
                self.policy.cal_sensor_key = key.clone();
            }
        }

        // Flush settled unified custom speed only if its epoch is still current and writes are allowed.
        if let Some(pending) = self.pending_speed {
            if pending.epoch != self.control_epoch {
                self.pending_speed = None;
            } else if pending.changed_at.elapsed() >= SPEED_SETTLE {
                self.pending_speed = None;
                if self.fan_writes_allowed() {
                    self.apply_plan(Plan::Custom, pending.percent, cx);
                }
            }
        }

        // Flush settled per-fan fixed speeds only if their epoch is still current and writes are allowed.
        let due_fans: Vec<(u8, PendingSpeed)> = self
            .pending_fan_speeds
            .iter()
            .filter(|(id, p)| {
                p.epoch != self.fan_epoch(**id) || p.changed_at.elapsed() >= SPEED_SETTLE
            })
            .map(|(id, p)| (*id, *p))
            .collect();
        for (id, pending) in due_fans {
            self.pending_fan_speeds.remove(&id);
            if pending.epoch == self.fan_epoch(id) && self.fan_writes_allowed() {
                self.apply_fan_choice(id, FanChoice::Fixed, pending.percent, cx);
            }
        }

        self.poll_policy_save();

        let fresh = self.is_fresh();
        let view = presenter::present(&self.snapshot, fresh, self.demo);
        if view.plan != Plan::Custom {
            self.popover_fixed_override = false;
        }
        self.history.record(Sample {
            at: Instant::now(),
            temperature: view.temperature,
            fan_percent: view.fan_percent,
        });

        self.refresh_status_bar_now(&view, fresh);
        cx.notify();
    }

    fn refresh_status_bar_now(&mut self, view: &Presentation, fresh: bool) {
        let cards = presenter::fan_cards(&self.snapshot, fresh);
        if let (Some(bar), Some(mtm)) = (self.status_bar.as_mut(), MainThreadMarker::new()) {
            bar.refresh(mtm, view, &cards, fresh, &self.snapshot.message);
        }
    }

    fn poll_policy_save(&mut self) {
        let result = self
            .pending_policy_save
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(res) => Some(res),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("保存未获确认，请重试。".into())),
            });
        if let Some(result) = result {
            self.pending_policy_save = None;
            if let Some((_prev, _next)) = self.pending_tuning.take() {
                match result {
                    Ok(()) => {
                        self.preference_notice = "已保存散热偏好。".into();
                        self.preference_notice_is_error = false;
                    }
                    Err(e) => {
                        self.preference_notice = format!("保存失败：{e}");
                        self.preference_notice_is_error = true;
                    }
                }
            } else {
                match result {
                    Ok(()) => {
                        self.snapshot = self.worker.snapshot();
                        self.sync_policy_from_config(true);
                        self.policy.notice = "智能策略已保存，等待新采样评估。".into();
                        self.policy.notice_is_error = false;
                    }
                    Err(e) => {
                        self.policy.notice = format!("保存失败：{e}");
                        self.policy.notice_is_error = true;
                    }
                }
            }
        }
    }

    pub fn apply_plan(&mut self, plan: Plan, percent: f64, cx: &mut Context<Self>) {
        // Always cancel any queued fixed-speed writes before switching modes.
        self.cancel_pending_speeds();
        let allowed = if plan == Plan::System {
            !self.demo && self.snapshot.helper_ready && !self.snapshot.installing
        } else {
            self.presentation().controls_enabled
        };
        if !allowed {
            cx.notify();
            return;
        }
        for config in presenter::configs_for_plan(&self.snapshot, plan, percent) {
            if let Err(err) = self.worker.send(WorkerCommand::Configure(config)) {
                self.settings_message = format!("发送控制指令失败：{err}");
                self.settings_message_is_error = true;
            }
        }
        cx.notify();
    }

    pub fn set_custom_percent(&mut self, percent: f64, cx: &mut Context<Self>) {
        if !self.fan_writes_allowed() {
            return;
        }
        self.pending_speed = Some(PendingSpeed {
            percent: percent.clamp(0.0, 100.0),
            changed_at: Instant::now(),
            epoch: self.control_epoch,
        });
        cx.notify();
    }

    pub fn apply_fan_choice(
        &mut self,
        fan_id: u8,
        choice: FanChoice,
        percent: f64,
        cx: &mut Context<Self>,
    ) {
        // Cancel any pending fixed speed for THIS fan only (and clear global pending speed) so
        // concurrent per-fan adjustments on other fans remain intact.
        self.pending_fan_speeds.remove(&fan_id);
        self.pending_speed = None;
        self.bump_fan_epoch(fan_id);

        let allowed = if choice == FanChoice::System {
            !self.demo && self.snapshot.helper_ready && !self.snapshot.installing
        } else {
            self.fan_writes_allowed()
        };
        if !allowed {
            cx.notify();
            return;
        }
        if let Some(config) = presenter::config_for_choice(&self.snapshot, fan_id, choice, percent)
        {
            if let Err(err) = self.worker.send(WorkerCommand::Configure(config)) {
                self.settings_message = format!("发送控制指令失败：{err}");
                self.settings_message_is_error = true;
            }
        }
        cx.notify();
    }

    pub fn set_fan_percent(&mut self, fan_id: u8, percent: f64, cx: &mut Context<Self>) {
        if !self.fan_writes_allowed() {
            return;
        }
        let epoch = self.fan_epoch(fan_id);
        self.pending_fan_speeds.insert(
            fan_id,
            PendingSpeed {
                percent: percent.clamp(0.0, 100.0),
                changed_at: Instant::now(),
                epoch,
            },
        );
        cx.notify();
    }

    pub fn save_tuning_bias(&mut self, bias: i8, cx: &mut Context<Self>) {
        if self.pending_policy_save.is_some() {
            return;
        }
        let bias = bias.clamp(-10, 10);
        let prev = self.snapshot.config.adaptive_tuning.bias;
        let (tx, rx) = mpsc::channel();
        match self.worker.send(WorkerCommand::ConfigureTuning {
            tuning: AdaptiveTuning { bias },
            reply: tx,
        }) {
            Ok(()) => {
                self.pending_policy_save = Some(rx);
                self.pending_tuning = Some((prev, bias));
                self.preference_notice = "正在保存…".into();
                self.preference_notice_is_error = false;
            }
            Err(e) => {
                self.preference_notice = format!("保存失败：{e}");
                self.preference_notice_is_error = true;
            }
        }
        cx.notify();
    }

    pub fn select_calibration_sensor(&mut self, key: String, cx: &mut Context<Self>) {
        if self.policy.cal_sensor_key != key {
            self.policy.cal_sensor_key = key;
            self.policy.cal_captured = [None, None];
            self.policy.dirty = true;
            self.policy.notice.clear();
            self.policy.notice_is_error = false;
        }
        cx.notify();
    }

    pub fn adjust_calibration_ext(&mut self, idx: usize, delta: f64, cx: &mut Context<Self>) {
        if idx < 2 {
            let next = ((self.policy.cal_ext_c[idx] + delta) * 2.0).round() / 2.0;
            self.policy.cal_ext_c[idx] = next.clamp(15.0, 60.0);
            self.policy.dirty = true;
            cx.notify();
        }
    }

    pub fn record_calibration_point(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= 2 {
            return;
        }
        if !self.is_fresh() {
            self.policy.notice = "传感器读数已过期，暂不可记录校准点。".into();
            self.policy.notice_is_error = true;
            cx.notify();
            return;
        }
        let key = self.policy.cal_sensor_key.clone();
        let reading = self
            .snapshot
            .snapshot
            .sensors
            .iter()
            .find(|s| s.key == key && s.group == SensorGroup::System)
            .and_then(|s| s.value)
            .filter(|v| v.is_finite());
        let Some(internal_c) = reading else {
            self.policy.notice = "当前参考传感器没有有效读数。".into();
            self.policy.notice_is_error = true;
            cx.notify();
            return;
        };
        let ext_c = self.policy.cal_ext_c[idx];
        self.policy.cal_captured[idx] = Some((internal_c, ext_c));
        self.policy.dirty = true;
        self.policy.notice = format!(
            "已记录校准点 {}：内部 {:.1}°C → 表面 {:.1}°C",
            idx + 1,
            internal_c,
            ext_c
        );
        self.policy.notice_is_error = false;
        cx.notify();
    }

    pub fn apply_calibration(&mut self, cx: &mut Context<Self>) {
        if self.pending_policy_save.is_some() {
            return;
        }
        let (Some((p0, s0)), Some((p1, s1))) =
            (self.policy.cal_captured[0], self.policy.cal_captured[1])
        else {
            self.policy.notice = "请先在两个不同温度状态下分别记录校准点 1 与校准点 2。".into();
            self.policy.notice_is_error = true;
            cx.notify();
            return;
        };
        let Some(machine_id) = self.snapshot.snapshot.machine_id.clone() else {
            self.policy.notice = "未识别当前机型标识，无法绑定校准。".into();
            self.policy.notice_is_error = true;
            cx.notify();
            return;
        };
        let calibration = match SurfaceCalibration::from_measurements(
            machine_id,
            self.policy.cal_sensor_key.clone(),
            p0,
            s0,
            p1,
            s1,
        ) {
            Ok(cal) => cal,
            Err(err) => {
                self.policy.notice = format!("校准无效：{err}");
                self.policy.notice_is_error = true;
                cx.notify();
                return;
            }
        };
        let mut policy = self.snapshot.config.thermal_policy.clone();
        policy.comfort_target_celsius = self
            .policy
            .comfort_enabled
            .then_some(self.policy.comfort_target_c.clamp(30.0, 45.0));
        policy.calibration = Some(calibration);
        self.dispatch_policy_save(policy, cx);
    }

    pub fn clear_calibration(&mut self, cx: &mut Context<Self>) {
        if self.pending_policy_save.is_some() {
            return;
        }
        self.policy.cal_captured = [None, None];
        let mut policy = self.snapshot.config.thermal_policy.clone();
        policy.comfort_target_celsius = self
            .policy
            .comfort_enabled
            .then_some(self.policy.comfort_target_c.clamp(30.0, 45.0));
        policy.calibration = None;
        self.dispatch_policy_save(policy, cx);
    }

    pub fn save_thermal_policy(&mut self, cx: &mut Context<Self>) {
        if self.pending_policy_save.is_some() {
            return;
        }
        let mut policy: ThermalPolicy = self.snapshot.config.thermal_policy.clone();
        policy.comfort_target_celsius = self
            .policy
            .comfort_enabled
            .then_some(self.policy.comfort_target_c.clamp(30.0, 45.0));
        self.dispatch_policy_save(policy, cx);
    }

    fn dispatch_policy_save(&mut self, policy: ThermalPolicy, cx: &mut Context<Self>) {
        // Never overwrite persisted thermal policy/calibration before initial config has loaded.
        if !self.policy_loaded {
            self.policy.notice = "配置尚未加载完成，请稍后再保存。".into();
            self.policy.notice_is_error = true;
            cx.notify();
            return;
        }
        let tuning = AdaptiveTuning {
            bias: self.current_bias(),
        };
        let (tx, rx) = mpsc::channel();
        match self.worker.send(WorkerCommand::ConfigurePolicy {
            policy,
            tuning,
            reply: tx,
        }) {
            Ok(()) => {
                self.pending_policy_save = Some(rx);
                self.policy.notice = "正在保存智能策略…".into();
                self.policy.notice_is_error = false;
            }
            Err(e) => {
                self.policy.notice = format!("保存失败：{e}");
                self.policy.notice_is_error = true;
            }
        }
        cx.notify();
    }

    pub fn send_reset(&mut self, cx: &mut Context<Self>) {
        // P0 safety: cancel any queued debounced fixed-speed writes before handing back to macOS.
        self.cancel_pending_speeds();
        self.popover_fixed_override = false;
        if let Err(err) = self.worker.send(WorkerCommand::Reset) {
            self.settings_message = format!("交还系统失败：{err}");
            self.settings_message_is_error = true;
        }
        cx.notify();
    }

    pub fn send_install(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = self.worker.send(WorkerCommand::Install) {
            self.settings_message = format!("请求启用控制服务失败：{err}");
            self.settings_message_is_error = true;
        }
        cx.notify();
    }

    pub fn send_retry(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = self.worker.send(WorkerCommand::Retry) {
            self.settings_message = format!("重新检测失败：{err}");
            self.settings_message_is_error = true;
        }
        cx.notify();
    }

    /// Dispatches uninstall AFTER the native confirmation dialog returns (outside Entity update).
    pub fn send_uninstall(&mut self, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        self.cancel_pending_speeds();
        match self.worker.send(WorkerCommand::Uninstall) {
            Ok(()) => {
                self.settings_message = "正在停止控制并移除服务…".into();
                self.settings_message_is_error = false;
            }
            Err(err) => {
                self.settings_message = format!("移除控制服务失败：{err}");
                self.settings_message_is_error = true;
            }
        }
        cx.notify();
    }

    /// Dispatches diagnostics export AFTER the native save panel returns (outside Entity update).
    pub fn send_export(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match self.worker.send(WorkerCommand::Export(path)) {
            Ok(()) => {
                self.settings_message = "已提交诊断信息导出请求。".into();
                self.settings_message_is_error = false;
            }
            Err(err) => {
                self.settings_message = format!("导出诊断信息失败：{err}");
                self.settings_message_is_error = true;
            }
        }
        cx.notify();
    }

    pub fn check_updates(&self) {
        if let Some(updater) = self.updater.as_ref() {
            updater.check();
        }
    }

    pub fn can_check_updates(&self) -> bool {
        self.updater.as_ref().is_some_and(Updater::can_check)
    }

    pub fn set_menu_bar_mode(&mut self, mode: MenuBarDisplayMode, cx: &mut Context<Self>) {
        self.menu_bar_mode = mode;
        let fresh = self.is_fresh();
        let view = presenter::present(&self.snapshot, fresh, self.demo);
        let cards = presenter::fan_cards(&self.snapshot, fresh);
        let demo = self.demo;
        let message = self.snapshot.message.clone();
        if let Some(bar) = self.status_bar.as_mut() {
            bar.set_mode(mode, demo);
            if let Some(mtm) = MainThreadMarker::new() {
                bar.refresh(mtm, &view, &cards, fresh, &message);
            }
        } else {
            mode.save(demo);
        }
        cx.notify();
    }

    pub fn sync_login_state(&mut self) {
        if self.demo {
            return;
        }
        match crate::settings::enabled() {
            Ok(v) => {
                self.login_enabled = v;
                if self.settings_message_is_error {
                    self.settings_message.clear();
                    self.settings_message_is_error = false;
                }
            }
            Err(e) => {
                self.login_enabled = false;
                self.settings_message = e;
                self.settings_message_is_error = true;
            }
        }
    }

    pub fn toggle_login(&mut self, wanted: bool, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        let bundle = glass::main_bundle_path();
        let result = crate::settings::set_enabled(wanted, std::path::Path::new(&bundle));
        let enabled = crate::settings::enabled();
        self.login_enabled = enabled.unwrap_or(false);
        match result {
            Ok(()) => {
                self.settings_message.clear();
                self.settings_message_is_error = false;
            }
            Err(e) => {
                self.settings_message = e;
                self.settings_message_is_error = true;
            }
        }
        cx.notify();
    }

    pub fn set_language(&mut self, lang: crate::i18n::Lang, cx: &mut Context<Self>) {
        self.language_menu_open = false;
        if !self.demo {
            glass::set_user_default("FanControlLanguage", lang.preference_key());
            crate::i18n::set_override(lang);
            let fresh = self.is_fresh();
            let view = presenter::present(&self.snapshot, fresh, self.demo);
            self.refresh_status_bar_now(&view, fresh);
            self.settings_message = "界面语言已切换并保存。".into();
            self.settings_message_is_error = false;
        }
        cx.notify();
    }

    pub fn open_curve_editor(&mut self, fan_id: Option<u8>, cx: &mut Context<Self>) {
        let id = fan_id
            .or_else(|| self.snapshot.snapshot.fans.first().map(|f| f.id))
            .unwrap_or(0);
        let existing = self
            .snapshot
            .config
            .fans
            .iter()
            .find(|f| f.fan_id == id)
            .and_then(|f| f.curve.clone())
            .unwrap_or_else(|| Curve::balanced("native-balanced", THERMAL_DEMAND_KEY));
        self.curve_editor = Some(CurveDraft {
            fan_id: id,
            curve: existing,
            all_fans: fan_id.is_none(),
            selected_point: Some(0),
            undo: Vec::new(),
            error: String::new(),
        });
        cx.notify();
    }

    pub fn close_curve_editor(&mut self, cx: &mut Context<Self>) {
        self.curve_editor = None;
        cx.notify();
    }

    pub fn curve_apply_preset(&mut self, preset: CurvePreset, cx: &mut Context<Self>) {
        if let Some(draft) = self.curve_editor.as_mut() {
            draft.undo.push(draft.curve.clone());
            let id = draft.curve.id.clone();
            let key = draft.curve.sensor_key.clone();
            draft.curve = presenter::preset_curve(preset, &id, &key);
            draft.selected_point = Some(0);
            draft.error.clear();
        }
        cx.notify();
    }

    pub fn curve_set_source(&mut self, sensor_key: String, cx: &mut Context<Self>) {
        if let Some(draft) = self.curve_editor.as_mut() {
            if draft.curve.sensor_key != sensor_key {
                draft.undo.push(draft.curve.clone());
                draft.curve.set_sensor_key(sensor_key);
                draft.selected_point = Some(0);
                draft.error.clear();
            }
        }
        cx.notify();
    }

    pub fn curve_nudge_hysteresis(&mut self, delta: f64, cx: &mut Context<Self>) {
        if let Some(draft) = self.curve_editor.as_mut() {
            draft.undo.push(draft.curve.clone());
            draft.curve.hysteresis = (draft.curve.hysteresis + delta).round().clamp(0.0, 30.0);
            draft.error.clear();
        }
        cx.notify();
    }

    pub fn curve_add_point(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.curve_editor.as_mut() {
            if draft.curve.points.len() < 12 {
                draft.undo.push(draft.curve.clone());
                draft
                    .curve
                    .points
                    .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
                if let Some(gap) = draft.curve.points.windows(2).max_by(|a, b| {
                    (a[1].temperature - a[0].temperature)
                        .total_cmp(&(b[1].temperature - b[0].temperature))
                }) {
                    let temp = ((gap[0].temperature + gap[1].temperature) / 2.0).round();
                    let speed = draft.curve.interpolate(temp).round();
                    draft.curve.points.push(CurvePoint {
                        temperature: temp,
                        speed_percent: speed,
                    });
                    draft
                        .curve
                        .points
                        .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
                    draft.selected_point = draft
                        .curve
                        .points
                        .iter()
                        .position(|p| (p.temperature - temp).abs() < 0.01);
                }
            }
        }
        cx.notify();
    }

    pub fn curve_remove_point(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(draft) = self.curve_editor.as_mut() {
            if draft.curve.points.len() > 2 && idx < draft.curve.points.len() {
                draft.undo.push(draft.curve.clone());
                draft.curve.points.remove(idx);
                draft.selected_point = Some(idx.min(draft.curve.points.len() - 1));
            }
        }
        cx.notify();
    }

    pub fn curve_nudge_point(
        &mut self,
        idx: usize,
        delta_x: f64,
        delta_speed: f64,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.curve_editor.as_mut() {
            if idx < draft.curve.points.len() {
                let max_x = draft.max_x();
                draft.undo.push(draft.curve.clone());
                let new_x = (draft.curve.points[idx].temperature + delta_x).clamp(0.0, max_x);
                let new_s = (draft.curve.points[idx].speed_percent + delta_speed)
                    .clamp(FAN_OFF_PERCENT, 100.0);
                draft.curve.points[idx].temperature = new_x;
                draft.curve.points[idx].speed_percent = new_s;
                draft
                    .curve
                    .points
                    .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
                draft.selected_point = draft.curve.points.iter().position(|p| {
                    (p.temperature - new_x).abs() < 0.01 && (p.speed_percent - new_s).abs() < 0.01
                });
                draft.error.clear();
            }
        }
        cx.notify();
    }

    pub fn curve_set_selected_at(
        &mut self,
        temp: f64,
        speed: f64,
        push_undo: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.curve_editor.as_mut() {
            let max_x = draft.max_x();
            let idx = draft.selected_point.unwrap_or(0);
            if idx < draft.curve.points.len() {
                if push_undo {
                    draft.undo.push(draft.curve.clone());
                }
                let new_x = temp.round().clamp(0.0, max_x);
                let new_s = speed.round().clamp(FAN_OFF_PERCENT, 100.0);
                draft.curve.points[idx].temperature = new_x;
                draft.curve.points[idx].speed_percent = new_s;
                draft
                    .curve
                    .points
                    .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
                draft.selected_point = draft.curve.points.iter().position(|p| {
                    (p.temperature - new_x).abs() < 0.01 && (p.speed_percent - new_s).abs() < 0.01
                });
                draft.error.clear();
            }
        }
        cx.notify();
    }

    pub fn curve_undo(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.curve_editor.as_mut() {
            if let Some(prev) = draft.undo.pop() {
                draft.curve = prev;
                draft.error.clear();
            }
        }
        cx.notify();
    }

    pub fn curve_save(&mut self, cx: &mut Context<Self>) {
        if self.demo || !self.snapshot.helper_ready || self.snapshot.installing {
            if let Some(draft) = self.curve_editor.as_mut() {
                draft.error = "请先启用控制服务，曲线草稿尚未保存或应用。".into();
            }
            cx.notify();
            return;
        }
        if self.snapshot.handback_pending || !self.is_fresh() {
            if let Some(draft) = self.curve_editor.as_mut() {
                draft.error = "当前采样不可用或正在交还系统，暂不可应用曲线。".into();
            }
            cx.notify();
            return;
        }
        let Some(draft) = self.curve_editor.clone() else {
            return;
        };
        if let Err(err) = draft.curve.validate() {
            if let Some(d) = self.curve_editor.as_mut() {
                d.error = format!("{err}");
            }
            cx.notify();
            return;
        }

        self.cancel_pending_speeds();
        let configs = if draft.all_fans {
            presenter::configs_for_curve(&self.snapshot, &draft.curve)
        } else {
            vec![FanConfig {
                fan_id: draft.fan_id,
                mode: ControlMode::Curve {
                    curve_id: draft.curve.id.clone(),
                },
                curve: Some(draft.curve),
            }]
        };
        if configs.is_empty() {
            if let Some(d) = self.curve_editor.as_mut() {
                d.error = "未找到可应用曲线的风扇。".into();
            }
            cx.notify();
            return;
        }
        for cfg in configs {
            if let Err(err) = self.worker.send(WorkerCommand::Configure(cfg)) {
                if let Some(d) = self.curve_editor.as_mut() {
                    d.error = format!("发送曲线配置失败：{err}");
                }
                cx.notify();
                return;
            }
        }
        self.popover_fixed_override = false;
        self.curve_editor = None;
        cx.notify();
    }

    #[allow(dead_code)]
    pub fn fan_range(&self, fan_id: u8) -> Option<(u32, u32)> {
        self.snapshot
            .snapshot
            .fans
            .iter()
            .find(|f| f.id == fan_id)
            .and_then(|f| validated_rpm_range(f.min_rpm, f.max_rpm))
    }

    pub fn handle_notice_action(&mut self, cx: &mut Context<Self>) {
        let view = self.presentation();
        match view.notice.action {
            Some(presenter::Action::EnableControl) => self.send_install(cx),
            Some(presenter::Action::Reconnect) => self.send_retry(cx),
            None => {}
        }
    }

    pub fn shutdown_safely(&self) {
        let confirmed = self.worker.shutdown();
        if !confirmed && !crate::TERMINATE_REQUEST.load(std::sync::atomic::Ordering::SeqCst) {
            crate::telemetry::event("gui.shutdown.handback.unconfirmed");
            glass::show_unconfirmed_handback_alert();
        }
    }

    #[allow(dead_code)]
    pub fn force_window_redraw(&self, window: &mut Window) {
        window.refresh();
    }
}

pub fn cooling_preference_label(bias: i8) -> String {
    match bias {
        0 => crate::i18n::translate("默认"),
        b if b < 0 => format!("{} {b}", crate::i18n::translate("更安静")),
        b => format!("{} +{b}", crate::i18n::translate("更凉快")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_draft_preserves_sensor_ranges_and_fan_off_floor() {
        let temp_curve = Curve::balanced("c1", "Tp01");
        let draft = CurveDraft {
            fan_id: 0,
            curve: temp_curve,
            all_fans: false,
            selected_point: Some(0),
            undo: Vec::new(),
            error: String::new(),
        };
        assert!(!draft.is_thermal_demand());
        assert_eq!(draft.max_x(), 120.0);
        assert_eq!(draft.x_unit(), "°C");

        let demand_curve = Curve::balanced("c2", THERMAL_DEMAND_KEY);
        let demand_draft = CurveDraft {
            fan_id: 0,
            curve: demand_curve,
            all_fans: false,
            selected_point: Some(0),
            undo: Vec::new(),
            error: String::new(),
        };
        assert!(demand_draft.is_thermal_demand());
        assert_eq!(demand_draft.max_x(), 100.0);
        assert_eq!(demand_draft.x_unit(), "%");
        assert!(demand_draft
            .curve
            .points
            .iter()
            .any(|p| (p.speed_percent - FAN_OFF_PERCENT).abs() < 0.01));
    }

    #[test]
    fn pending_speed_epoch_invalidation_prevents_stale_override() {
        let mut control_epoch = 1u64;
        let mut fan_epochs: BTreeMap<u8, u64> = BTreeMap::new();
        let fan_epoch =
            |fan_id: u8, ce: u64, map: &BTreeMap<u8, u64>| map.get(&fan_id).copied().unwrap_or(ce);

        let fan0_pending = PendingSpeed {
            percent: 65.0,
            changed_at: Instant::now() - Duration::from_secs(2),
            epoch: fan_epoch(0, control_epoch, &fan_epochs),
        };
        let fan1_pending = PendingSpeed {
            percent: 75.0,
            changed_at: Instant::now() - Duration::from_secs(2),
            epoch: fan_epoch(1, control_epoch, &fan_epochs),
        };

        // Bumping fan 0 only invalidates fan 0, leaving fan 1 eligible to flush.
        let next0 = fan_epoch(0, control_epoch, &fan_epochs).wrapping_add(1);
        fan_epochs.insert(0, next0);
        assert_ne!(fan0_pending.epoch, fan_epoch(0, control_epoch, &fan_epochs));
        assert_eq!(fan1_pending.epoch, fan_epoch(1, control_epoch, &fan_epochs));

        // Global reset/mode change invalidates both.
        control_epoch = control_epoch.wrapping_add(1);
        for epoch in fan_epochs.values_mut() {
            *epoch = epoch.wrapping_add(1);
        }
        assert_ne!(fan0_pending.epoch, fan_epoch(0, control_epoch, &fan_epochs));
        assert_ne!(fan1_pending.epoch, fan_epoch(1, control_epoch, &fan_epochs));
    }
}
