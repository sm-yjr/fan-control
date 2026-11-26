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
    validated_rpm_range, AdaptiveTuning, Curve, CurvePoint, FanConfig, ThermalPolicy,
    THERMAL_DEMAND_KEY,
};
use gpui::{Context, Window};
use objc2::MainThreadMarker;
use std::collections::BTreeMap;
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

#[derive(Clone, Debug)]
pub struct PolicyDraft {
    pub comfort_enabled: bool,
    pub comfort_target_c: f64,
    pub calibration_open: bool,
    pub notice: String,
    pub notice_is_error: bool,
}

impl Default for PolicyDraft {
    fn default() -> Self {
        Self {
            comfort_enabled: false,
            comfort_target_c: 38.0,
            calibration_open: false,
            notice: String::new(),
            notice_is_error: false,
        }
    }
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

    // Pending debounced speed changes
    pub pending_speed: Option<(f64, Instant)>,
    pub pending_fan_speeds: BTreeMap<u8, (f64, Instant)>,

    // Unified 21-step cooling preference transaction
    pub pending_policy_save: Option<mpsc::Receiver<Result<(), String>>>,
    pub pending_tuning: Option<(i8, i8)>,
    pub preference_notice: String,
    pub preference_notice_is_error: bool,

    // Smart cooling page draft
    pub policy: PolicyDraft,
    pub policy_loaded: bool,

    // Sensor details filter/sort
    pub sensor_group: GroupFilter,
    pub sensor_sort: SortOrder,

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
            pending_speed: None,
            pending_fan_speeds: BTreeMap::new(),
            pending_policy_save: None,
            pending_tuning: None,
            preference_notice: String::new(),
            preference_notice_is_error: false,
            policy: PolicyDraft::default(),
            policy_loaded: false,
            sensor_group: GroupFilter::All,
            sensor_sort: SortOrder::Grouped,
            login_enabled,
            settings_message,
            settings_message_is_error,
            language_menu_open: false,
            curve_editor: None,
            last_visible: true,
        };
        state.sync_policy_from_config();
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

    pub fn sync_policy_from_config(&mut self) {
        let target = self.snapshot.config.thermal_policy.comfort_target_celsius;
        self.policy.comfort_enabled = target.is_some();
        self.policy.comfort_target_c = target.unwrap_or(38.0);
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

    pub fn poll_tick(&mut self, any_window_visible: bool, cx: &mut Context<Self>) {
        if any_window_visible != self.last_visible {
            let _ = self
                .worker
                .send(WorkerCommand::Visibility(any_window_visible));
            self.last_visible = any_window_visible;
        }

        // Flush settled unified custom speed.
        if let Some((percent, changed)) = self.pending_speed {
            if changed.elapsed() >= SPEED_SETTLE {
                self.pending_speed = None;
                self.apply_plan(Plan::Custom, percent, cx);
            }
        }

        // Flush settled per-fan fixed speeds.
        let due_fans: Vec<(u8, f64)> = self
            .pending_fan_speeds
            .iter()
            .filter(|(_, (_, at))| at.elapsed() >= SPEED_SETTLE)
            .map(|(id, (pct, _))| (*id, *pct))
            .collect();
        for (id, pct) in due_fans {
            self.pending_fan_speeds.remove(&id);
            self.apply_fan_choice(id, FanChoice::Fixed, pct, cx);
        }

        self.poll_policy_save();

        self.snapshot = self.worker.snapshot();
        if !self.policy_loaded && (self.snapshot.discovered || self.demo) {
            self.sync_policy_from_config();
        }

        let fresh = self.is_fresh();
        let view = presenter::present(&self.snapshot, fresh, self.demo);
        self.history.record(Sample {
            at: Instant::now(),
            temperature: view.temperature,
            fan_percent: view.fan_percent,
        });

        let cards = presenter::fan_cards(&self.snapshot, fresh);
        if let (Some(bar), Some(mtm)) = (self.status_bar.as_mut(), MainThreadMarker::new()) {
            bar.refresh(mtm, &view, &cards, fresh, &self.snapshot.message);
        }

        cx.notify();
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
        let view = self.presentation();
        if !view.controls_enabled {
            return;
        }
        for config in presenter::configs_for_plan(&self.snapshot, plan, percent) {
            let _ = self.worker.send(WorkerCommand::Configure(config));
        }
        self.pending_speed = None;
        cx.notify();
    }

    pub fn set_custom_percent(&mut self, percent: f64, cx: &mut Context<Self>) {
        self.pending_speed = Some((percent.clamp(0.0, 100.0), Instant::now()));
        cx.notify();
    }

    pub fn apply_fan_choice(
        &mut self,
        fan_id: u8,
        choice: FanChoice,
        percent: f64,
        cx: &mut Context<Self>,
    ) {
        if self.demo || !self.snapshot.helper_ready || self.snapshot.installing {
            return;
        }
        if let Some(config) = presenter::config_for_choice(&self.snapshot, fan_id, choice, percent)
        {
            let _ = self.worker.send(WorkerCommand::Configure(config));
        }
        cx.notify();
    }

    pub fn set_fan_percent(&mut self, fan_id: u8, percent: f64, cx: &mut Context<Self>) {
        self.pending_fan_speeds
            .insert(fan_id, (percent.clamp(0.0, 100.0), Instant::now()));
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

    pub fn save_thermal_policy(&mut self, cx: &mut Context<Self>) {
        if self.pending_policy_save.is_some() {
            return;
        }
        let mut policy: ThermalPolicy = self.snapshot.config.thermal_policy.clone();
        policy.comfort_target_celsius = self
            .policy
            .comfort_enabled
            .then_some(self.policy.comfort_target_c.clamp(30.0, 48.0));
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
        let _ = self.worker.send(WorkerCommand::Reset);
        cx.notify();
    }

    pub fn send_install(&mut self, cx: &mut Context<Self>) {
        let _ = self.worker.send(WorkerCommand::Install);
        cx.notify();
    }

    pub fn send_retry(&mut self, cx: &mut Context<Self>) {
        let _ = self.worker.send(WorkerCommand::Retry);
        cx.notify();
    }

    pub fn request_uninstall(&mut self, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        if glass::confirm_uninstall_dialog() {
            let _ = self.worker.send(WorkerCommand::Uninstall);
        }
        cx.notify();
    }

    pub fn export_diagnostics(&mut self) {
        if let Some(path) = glass::save_diagnostics_dialog() {
            let _ = self.worker.send(WorkerCommand::Export(path));
        }
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
            self.settings_message = "界面语言已保存，下次启动应用时生效。".into();
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
        delta_temp: f64,
        delta_speed: f64,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.curve_editor.as_mut() {
            if idx < draft.curve.points.len() {
                draft.undo.push(draft.curve.clone());
                let p = &mut draft.curve.points[idx];
                p.temperature = (p.temperature + delta_temp).clamp(0.0, 100.0);
                p.speed_percent = (p.speed_percent + delta_speed).clamp(0.0, 100.0);
                draft
                    .curve
                    .points
                    .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
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
        if !self.snapshot.helper_ready || self.snapshot.installing {
            if let Some(draft) = self.curve_editor.as_mut() {
                draft.error = "请先启用控制服务，曲线草稿尚未保存或应用。".into();
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
        if draft.all_fans {
            for cfg in presenter::configs_for_curve(&self.snapshot, &draft.curve) {
                let _ = self.worker.send(WorkerCommand::Configure(cfg));
            }
        } else {
            let cfg = FanConfig {
                fan_id: draft.fan_id,
                mode: fan_core::ControlMode::Curve {
                    curve_id: draft.curve.id.clone(),
                },
                curve: Some(draft.curve),
            };
            let _ = self.worker.send(WorkerCommand::Configure(cfg));
        }
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
