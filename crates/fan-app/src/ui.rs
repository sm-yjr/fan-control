//! System AppKit controls owned by Rust. Main-thread state never crosses into hardware work.
use crate::{
    curve_editor::CurveEditor,
    shell::MainShell,
    sidebar::Section,
    status_bar::{MenuBarDisplayMode, StatusBarController},
    updater::Updater,
    worker::{Worker, WorkerCommand},
};
use fan_core::{Curve, CurvePoint, FanConfig, THERMAL_DEMAND_KEY};
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel, DefinedClass, MainThreadOnly,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSTimer, NSUserDefaults, NSURL,
};
use std::cell::{Cell, OnceCell, RefCell};

pub(crate) mod tokens {
    pub const BODY: f64 = crate::theme::typography::BODY;
    pub const CAPTION: f64 = crate::theme::typography::SMALL;
}

/// A slider stays still this long before its speed is sent to the fans.
const SPEED_SETTLE: std::time::Duration = std::time::Duration::from_millis(600);
const ONBOARDED_KEY: &str = "FanControlPanelIntroduced";

pub(crate) fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
pub(crate) fn text(value: &str) -> Retained<NSString> {
    NSString::from_str(&crate::i18n::translate(value))
}
pub(crate) fn raw_text(value: &str) -> Retained<NSString> {
    NSString::from_str(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TuningOrigin {
    Panel,
    PolicyPage,
}

struct Ui {
    worker: Worker,
    shell: MainShell,
    status: StatusBarController,
    panel: crate::popover::MenuPanel,
    overview: crate::overview::Overview,
    history: crate::trend::History,
    /// Custom speed chosen in the panel, applied once the slider settles.
    pending_speed: Option<(f64, std::time::Instant)>,
    fans: crate::fans::FansPage,
    /// Per-fan fixed speed from the Fans page, applied once the slider settles.
    pending_fan_speed: Option<(u8, f64, std::time::Instant)>,
    details: crate::details::DetailsPage,
    settings: crate::preferences::SettingsPage,
    updater: Option<Updater>,
    fan_ids: Vec<u8>,
    selected: Option<u8>,
    editor: Option<CurveEditor>,
    demo: bool,
    policy: Option<crate::policy::PolicyEditor>,
    pending_policy_save: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    pending_tuning: Option<(i8, i8, TuningOrigin)>,
    last_visible: bool,
    panel_opened: bool,
}

struct Ivars {
    widget_open_requested: Cell<bool>,
    ui: RefCell<Option<Ui>>,
    demo: bool,
    smoke: bool,
    timer: OnceCell<Retained<NSTimer>>,
    started: std::time::Instant,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            if urls.iter().any(|url| url.absoluteString().is_some_and(|url| url.to_string() == "fancontrol://dashboard")) {
                if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                    ui.shell.show(Section::Overview);
                } else {
                    self.ivars().widget_open_requested.set(true);
                }
            }
        }
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self, _notification: &NSNotification) {
            let ui = Ui::new(self, self.ivars().demo);
            if self.ivars().widget_open_requested.replace(false) {
                ui.shell.show(Section::Overview);
            }
            self.ivars().ui.replace(Some(ui));
            let timer = unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    0.5,
                    self,
                    sel!(tick:),
                    None,
                    true,
                )
            };
            let _ = self.ivars().timer.set(timer);
        }
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            let confirmed = self
                .ivars()
                .ui
                .borrow()
                .as_ref()
                .is_none_or(|ui| ui.worker.shutdown());
            if !confirmed {
                crate::telemetry::event("gui.shutdown.handback.unconfirmed");
                if !crate::TERMINATE_REQUEST.load(std::sync::atomic::Ordering::SeqCst) {
                    let alert = NSAlert::new(self.mtm());
                    alert.setMessageText(&text("退出前交还未获确认"));
                    alert.setInformativeText(&text(
                        "控制服务会继续重试。请检查服务状态与风扇实际模式；应用无法确认安全回退已经完成。",
                    ));
                    alert.addButtonWithTitle(&text("退出"));
                    alert.runModal();
                }
            }
            NSApplicationTerminateReply::TerminateNow
        }
    }
    impl Delegate {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            if crate::TERMINATE_REQUEST.load(std::sync::atomic::Ordering::SeqCst) {
                NSApplication::sharedApplication(self.mtm()).terminate(None);
                return;
            }
            let mut smoke_done = false;
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                if !ui.panel_opened && !self.ivars().smoke && !ui.demo {
                    ui.panel_opened = true;
                    ui.introduce_panel();
                }
                ui.refresh(self);
                if ui.policy.is_none() {
                    ui.build_policy(self);
                }
                if self.ivars().smoke && !ui.fan_ids.is_empty() {
                    ui.run_smoke_checks(self);
                    smoke_done = true;
                }
            }
            if smoke_done {
                println!("Rust AppKit main, curve, thermal policy and preferences initialized in isolated demo mode");
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            } else if self.ivars().smoke && self.ivars().started.elapsed().as_secs() > 10 {
                eprintln!("Demo UI initialization timed out");
                std::process::exit(1);
            }
        }
        #[unsafe(method(sidebarSelected:))]
        fn sidebar_selected(&self, row: isize) {
            if let Some(section) = Section::from_index(row) {
                if let Ok(ui) = self.ivars().ui.try_borrow() {
                    if let Some(ui) = ui.as_ref() {
                        ui.shell.select(section);
                        if section == Section::Settings {
                            ui.settings.sync_login_state(ui.demo);
                        }
                    }
                }
            }
        }
        #[unsafe(method(show:))]
        fn show(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                ui.panel.close();
                ui.shell.show(Section::Overview);
            }
        }
        #[unsafe(method(showFans:))]
        fn show_fans(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                ui.panel.close();
                ui.shell.show(Section::Fans);
            }
        }
        #[unsafe(method(togglePanel:))]
        fn toggle_panel(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                ui.toggle_panel();
            }
        }
        #[unsafe(method(planChanged:))]
        fn plan_changed(&self, sender: &NSSegmentedControl) {
            let plan = match sender.selectedSegment() {
                0 => crate::presenter::Plan::System,
                1 => crate::presenter::Plan::Smart,
                _ => crate::presenter::Plan::Custom,
            };
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let percent = ui.panel.slider.doubleValue();
                ui.apply_plan(plan, percent);
            }
        }
        #[unsafe(method(overviewPlanChanged:))]
        fn overview_plan_changed(&self, sender: &NSSegmentedControl) {
            let plan = match sender.selectedSegment() {
                0 => crate::presenter::Plan::System,
                1 => crate::presenter::Plan::Smart,
                _ => crate::presenter::Plan::Custom,
            };
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let percent = ui.panel.slider.doubleValue();
                ui.apply_plan(plan, percent);
            }
        }
        #[unsafe(method(customSpeed:))]
        fn custom_speed(&self, sender: &NSSlider) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let percent = sender.doubleValue().round();
                ui.panel.set_percent_label(percent);
                ui.pending_speed = Some((percent, std::time::Instant::now()));
            }
        }
        #[unsafe(method(useCurve:))]
        fn use_curve(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.panel.close();
                ui.shell.show(Section::Fans);
                ui.open_editor(self, None, false);
            }
        }
        #[unsafe(method(noticeAction:))]
        fn notice_action(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                let state = ui.worker.snapshot();
                let view = crate::presenter::present(&state, fresh(&state), ui.demo);
                let command = match view.notice.action {
                    Some(crate::presenter::Action::EnableControl) => WorkerCommand::Install,
                    Some(crate::presenter::Action::Reconnect) => WorkerCommand::Retry,
                    None => return,
                };
                let _ = ui.worker.send(command);
            }
        }
        #[unsafe(method(fanChoice:))]
        fn fan_choice(&self, sender: &NSSegmentedControl) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let index = sender.tag();
                let (Some(id), Some(choice)) = (ui.fans.card_id(index), ui.fans.choice(index))
                else {
                    return;
                };
                let percent = ui
                    .fans
                    .slider_value(index)
                    .unwrap_or(crate::presenter::DEFAULT_CUSTOM_PERCENT);
                ui.pending_fan_speed = None;
                ui.fans.forget(index);
                ui.apply_choice(id, choice, percent);
            }
        }
        #[unsafe(method(fanSpeed:))]
        fn fan_speed(&self, sender: &NSSlider) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let index = sender.tag();
                let Some(id) = ui.fans.card_id(index) else {
                    return;
                };
                let percent = sender.doubleValue().round();
                let range = ui
                    .worker
                    .snapshot()
                    .snapshot
                    .fans
                    .iter()
                    .find(|fan| fan.id == id)
                    .and_then(|fan| fan_core::validated_rpm_range(fan.min_rpm, fan.max_rpm));
                ui.fans
                    .show_fixed_value(index, percent, &crate::presenter::rpm_hint(percent, range));
                ui.pending_fan_speed = Some((id, percent, std::time::Instant::now()));
            }
        }
        #[unsafe(method(fanCurve:))]
        fn fan_curve(&self, sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let Some(id) = ui.fans.card_id(sender.tag()) else {
                    return;
                };
                ui.close_editor();
                ui.selected = Some(id);
                ui.open_editor(self, None, false);
            }
        }
        #[unsafe(method(useSmart:))]
        fn use_smart(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.apply_plan(
                    crate::presenter::Plan::Smart,
                    crate::presenter::DEFAULT_CUSTOM_PERCENT,
                );
            }
        }
        #[unsafe(method(reset:))]
        fn reset(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let _ = ui.worker.send(WorkerCommand::Reset);
            }
        }
        #[unsafe(method(install:))]
        fn install(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                let _ = ui.worker.send(WorkerCommand::Install);
            }
        }
        #[unsafe(method(retry:))]
        fn retry(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                let _ = ui.worker.send(WorkerCommand::Retry);
            }
        }
        #[unsafe(method(updates:))]
        fn updates(&self, _sender: Option<&AnyObject>) {
            let updater = self
                .ivars()
                .ui
                .borrow()
                .as_ref()
                .and_then(|ui| ui.updater.clone());
            if let Some(updater) = updater {
                updater.check();
            }
        }
        #[unsafe(method(export:))]
        fn export(&self, _sender: &NSButton) {
            let sender = self
                .ivars()
                .ui
                .borrow()
                .as_ref()
                .map(|ui| ui.worker.sender());
            if let Some(sender) = sender {
                let panel = NSSavePanel::savePanel(self.mtm());
                panel.setNameFieldStringValue(&text("FanControl-diagnostics.json"));
                if panel.runModal() == NSModalResponseOK {
                    if let Some(path) = panel.URL().and_then(|url| url.path()) {
                        let _ = sender
                            .send(WorkerCommand::Export(std::path::PathBuf::from(path.to_string())));
                    }
                }
            }
        }
        #[unsafe(method(saveCurve:))]
        fn save_curve(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let state = ui.worker.snapshot();
                if !state.helper_ready || state.installing {
                    if let Some(editor) = ui.editor.as_ref() {
                        editor
                            .error
                            .setStringValue(&text("请先启用控制服务，曲线草稿尚未保存或应用。"));
                    }
                    return;
                }
                let result = ui.editor.as_ref().map(CurveEditor::read);
                let all_fans = ui
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.all_fans.state() == NSControlStateValueOn);
                if let Some(result) = result {
                    match result {
                        Ok(fan) if all_fans => {
                            if let Some(curve) = fan.curve.as_ref() {
                                for config in crate::presenter::configs_for_curve(&state, curve) {
                                    let _ = ui.worker.send(WorkerCommand::Configure(config));
                                }
                            }
                            ui.close_editor();
                        }
                        Ok(fan) => {
                            let _ = ui.worker.send(WorkerCommand::Configure(fan));
                            ui.close_editor();
                        }
                        Err(error) => {
                            if let Some(editor) = ui.editor.as_ref() {
                                editor.error.setStringValue(&text(&error));
                            }
                        }
                    }
                }
            }
        }
        #[unsafe(method(curvePreset:))]
        fn curve_preset(&self, sender: &NSSegmentedControl) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let Some(preset) = crate::presenter::CurvePreset::ALL
                    .get(sender.selectedSegment().max(0) as usize)
                    .copied()
                else {
                    return;
                };
                if let Some(editor) = ui.editor.as_mut() {
                    let mut fan = editor.fan.clone();
                    let key = editor
                        .sources
                        .get(editor.source.indexOfSelectedItem().max(0) as usize)
                        .cloned()
                        .unwrap_or_else(|| THERMAL_DEMAND_KEY.into());
                    let id = fan
                        .curve
                        .as_ref()
                        .map(|curve| curve.id.clone())
                        .unwrap_or_else(|| "native-balanced".into());
                    fan.curve = Some(crate::presenter::preset_curve(preset, &id, &key));
                    editor.apply_draft(fan, true);
                }
            }
        }
        #[unsafe(method(curveDragged:))]
        fn curve_dragged(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let Some(editor) = ui.editor.as_mut() else {
                    return;
                };
                if !editor.drag_recorded && editor.chart.is_dragging() {
                    let source = editor.fan.curve.as_ref().map(|curve| curve.sensor_key.clone());
                    if let Ok(previous) = editor.read_values(None, source.as_deref()) {
                        if editor.undo.last() != Some(&previous) {
                            editor.undo.push(previous);
                        }
                    }
                    editor.drag_recorded = true;
                }
                if let Some(curve) = editor.chart.curve() {
                    let mut points = curve.points.clone();
                    points.sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
                    for ((x, speed), point) in editor.rows.iter().zip(&points) {
                        x.setStringValue(&raw_text(&crate::presenter::format_value(
                            point.temperature,
                        )));
                        speed.setStringValue(&raw_text(&crate::presenter::format_value(
                            point.speed_percent,
                        )));
                    }
                    editor.presets.setSelectedSegment(
                        crate::presenter::matching_preset(&curve)
                            .and_then(|preset| {
                                crate::presenter::CurvePreset::ALL
                                    .iter()
                                    .position(|p| *p == preset)
                            })
                            .map_or(-1, |index| index as isize),
                    );
                }
                editor.update_live_readout();
            }
        }
        #[unsafe(method(curveAllFans:))]
        fn curve_all_fans(&self, _sender: &NSButton) {}
        #[unsafe(method(cancelCurve:))]
        fn cancel_curve(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.close_editor();
            }
        }
        #[unsafe(method(addPoint:))]
        fn add_point(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                if let Some(editor) = ui.editor.as_mut() {
                    match editor.read() {
                        Ok(mut fan) => {
                            if let Some(curve) = fan.curve.as_mut() {
                                if curve.points.len() < 12 {
                                    curve
                                        .points
                                        .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
                                    let gap = curve.points.windows(2).max_by(|a, b| {
                                        (a[1].temperature - a[0].temperature)
                                            .total_cmp(&(b[1].temperature - b[0].temperature))
                                    });
                                    if let Some(gap) = gap {
                                        let temperature =
                                            (gap[0].temperature + gap[1].temperature) / 2.;
                                        let speed_percent = curve.interpolate(temperature);
                                        curve.points.push(CurvePoint {
                                            temperature,
                                            speed_percent,
                                        });
                                        curve.points.sort_by(|a, b| {
                                            a.temperature.total_cmp(&b.temperature)
                                        });
                                    }
                                }
                            }
                            editor.apply_draft(fan, true);
                        }
                        Err(error) => editor.error.setStringValue(&text(&error)),
                    }
                }
            }
        }
        #[unsafe(method(removePoint:))]
        fn remove_point(&self, sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let i = sender.tag().max(0) as usize;
                if let Some(editor) = ui.editor.as_mut() {
                    match editor.read_values(Some(i), None) {
                        Ok(fan) => editor.apply_draft(fan, true),
                        Err(error) => editor.error.setStringValue(&text(&error)),
                    }
                }
            }
        }
        #[unsafe(method(sourceChanged:))]
        fn source_changed(&self, _sender: &NSPopUpButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                if let Some(editor) = ui.editor.as_mut() {
                    let source = editor
                        .sources
                        .get(editor.source.indexOfSelectedItem().max(0) as usize)
                        .cloned();
                    if let Some(source) = source {
                        let previous_source = editor
                            .fan
                            .curve
                            .as_ref()
                            .map(|curve| curve.sensor_key.clone());
                        let mut fan = editor
                            .read_values(None, previous_source.as_deref())
                            .unwrap_or_else(|_| editor.fan.clone());
                        if let Some(curve) = fan.curve.as_mut() {
                            curve.set_sensor_key(source);
                        }
                        editor.apply_draft(fan, true);
                    }
                }
            }
        }
        #[unsafe(method(undoCurve:))]
        fn undo_curve(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                if let Some(editor) = ui.editor.as_mut() {
                    if let Some(fan) = editor.undo.pop() {
                        editor.apply_draft(fan, false);
                    }
                }
            }
        }
        #[unsafe(method(preferences:))]
        fn preferences(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.open_preferences();
            }
        }
        #[unsafe(method(policy:))]
        fn policy(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.open_policy(self);
            }
        }
        #[unsafe(method(coolingPreference:))]
        fn cooling_preference(&self, sender: &NSSlider) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let bias = sender.doubleValue().round().clamp(-10., 10.) as i8;
                if let Some(policy) = ui.policy.as_ref() {
                    policy.preference_changed();
                }
                ui.save_tuning_preference(bias, TuningOrigin::PolicyPage);
            }
        }
        #[unsafe(method(resetCoolingPreference:))]
        fn reset_cooling_preference(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                if let Some(policy) = ui.policy.as_ref() {
                    policy.reset_preference();
                }
                ui.save_tuning_preference(0, TuningOrigin::PolicyPage);
            }
        }
        #[unsafe(method(panelCoolingPreference:))]
        fn panel_cooling_preference(&self, sender: &NSSlider) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let bias = sender.doubleValue().round().clamp(-10., 10.) as i8;
                ui.save_tuning_preference(bias, TuningOrigin::Panel);
            }
        }
        #[unsafe(method(resetPanelCoolingPreference:))]
        fn reset_panel_cooling_preference(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.save_tuning_preference(0, TuningOrigin::Panel);
            }
        }
        #[unsafe(method(toggleCalibrationDisclosure:))]
        fn toggle_calibration_disclosure(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                if let Some(policy) = ui.policy.as_ref() {
                    policy.toggle_calibration_disclosure();
                }
            }
        }
        #[unsafe(method(comfortToggle:))]
        fn comfort_toggle(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                if let Some(policy) = ui.policy.as_mut() {
                    policy.refresh(&ui.worker.snapshot());
                }
            }
        }
        #[unsafe(method(recordCalibration:))]
        fn record_calibration(&self, sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                if let Some(policy) = ui.policy.as_ref() {
                    if let Err(error) =
                        policy.record(sender.tag().max(0) as usize, &ui.worker.snapshot())
                    {
                        policy.notice.setStringValue(&text(&error));
                    }
                }
            }
        }
        #[unsafe(method(calibrationSource:))]
        fn calibration_source(&self, _sender: &NSPopUpButton) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                if let Some(policy) = ui.policy.as_ref() {
                    policy.source_changed();
                }
            }
        }
        #[unsafe(method(clearCalibration:))]
        fn clear_calibration(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                if let Some(policy) = ui.policy.as_ref() {
                    policy.clear();
                }
            }
        }
        #[unsafe(method(cancelPolicy:))]
        fn cancel_policy(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.close_policy(self);
            }
        }
        #[unsafe(method(savePolicy:))]
        fn save_policy(&self, _sender: &NSButton) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.save_policy();
            }
        }
        #[unsafe(method(sensorFilterChanged:))]
        fn sensor_filter_changed(&self, _sender: Option<&AnyObject>) {
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                let state = ui.worker.snapshot();
                ui.details.refresh(&state, fresh(&state));
            }
        }
        #[unsafe(method(menuBarTileSelected:))]
        fn menu_bar_tile_selected(&self, index: isize) {
            let Some(mode) = MenuBarDisplayMode::ALL
                .get(index.max(0) as usize)
                .copied()
            else {
                return;
            };
            if let Some(ui) = self.ivars().ui.borrow_mut().as_mut() {
                ui.status.set_mode(mode, ui.demo);
                ui.settings.set_menu_bar_mode(mode);
                let state = ui.worker.snapshot();
                let is_fresh = fresh(&state);
                let view = crate::presenter::present(&state, is_fresh, ui.demo);
                let cards = crate::presenter::fan_cards(&state, is_fresh);
                ui.status
                    .refresh(self.mtm(), &view, &cards, is_fresh, &state.message);
            }
        }
        #[unsafe(method(login:))]
        fn login(&self, sender: &NSSwitch) {
            let demo = self.ivars().ui.borrow().as_ref().is_none_or(|ui| ui.demo);
            if demo {
                return;
            }
            let bundle = objc2_foundation::NSBundle::mainBundle()
                .bundlePath()
                .to_string();
            let result = crate::settings::set_enabled(
                sender.state() == NSControlStateValueOn,
                std::path::Path::new(&bundle),
            );
            let enabled = crate::settings::enabled();
            if let Some(ui) = self.ivars().ui.borrow().as_ref() {
                ui.settings.login.setState(if enabled.unwrap_or(false) {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                match result {
                    Ok(()) => ui.settings.set_general_message("", false),
                    Err(error) => ui.settings.set_general_message(&error, true),
                }
            }
        }
        #[unsafe(method(language:))]
        fn language(&self, sender: &NSPopUpButton) {
            let Some(language) = [
                crate::i18n::Lang::System,
                crate::i18n::Lang::Chinese,
                crate::i18n::Lang::English,
            ]
            .get(sender.indexOfSelectedItem().max(0) as usize)
            .copied()
            else {
                return;
            };
            if let Some(ui) = self.ivars().ui.borrow().as_ref().filter(|ui| !ui.demo) {
                unsafe {
                    NSUserDefaults::standardUserDefaults().setObject_forKey(
                        Some(&raw_text(language.preference_key())),
                        &raw_text("FanControlLanguage"),
                    );
                }
                ui.settings
                    .set_general_message("界面语言已保存，下次启动应用时生效。", false);
            }
        }
        #[unsafe(method(uninstall:))]
        fn uninstall(&self, _sender: Option<&AnyObject>) {
            let sender = self
                .ivars()
                .ui
                .borrow()
                .as_ref()
                .filter(|ui| !ui.demo)
                .map(|ui| ui.worker.sender());
            let Some(sender) = sender else {
                return;
            };
            let alert = NSAlert::new(self.mtm());
            alert.setMessageText(&text("移除风扇控制服务？"));
            alert.setInformativeText(&text(
                "应用将先停止自定义控制并交还系统，再请求管理员授权。移除后仍可查看温度，控制设置和诊断日志会保留。",
            ));
            alert.addButtonWithTitle(&text("移除服务"));
            alert.addButtonWithTitle(&text("取消"));
            if alert.runModal() == NSAlertFirstButtonReturn {
                let _ = sender.send(WorkerCommand::Uninstall);
            }
        }
        #[unsafe(method(about:))]
        fn about(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).orderFrontStandardAboutPanel(None);
        }
        #[unsafe(method(help:))]
        fn help(&self, _sender: Option<&AnyObject>) {
            if let Some(url) =
                objc2_foundation::NSURL::URLWithString(&text("https://github.com/sm-yjr/fan-control#readme"))
            {
                NSWorkspace::sharedWorkspace().openURL(&url);
            }
        }
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, demo: bool, smoke: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            widget_open_requested: Cell::new(false),
            ui: RefCell::new(None),
            demo,
            smoke,
            timer: OnceCell::new(),
            started: std::time::Instant::now(),
        });
        unsafe { msg_send![super(this), init] }
    }
}

impl Ui {
    fn new(delegate: &Delegate, demo: bool) -> Self {
        let mtm = delegate.mtm();

        let overview = crate::overview::Overview::new(mtm, delegate);
        let fans = crate::fans::FansPage::new(mtm, demo);
        let details = crate::details::DetailsPage::new(mtm, delegate);
        let settings = crate::preferences::SettingsPage::new(mtm, delegate, demo);

        let smart_placeholder = crate::form::fluid_page(mtm);
        smart_placeholder.addArrangedSubview(&crate::components::label(
            mtm,
            "正在读取设置与传感器…",
            tokens::BODY,
            unsafe { NSFontWeightRegular },
            true,
            true,
        ));

        let shell = MainShell::new(
            mtm,
            delegate,
            demo,
            &overview.view,
            &fans.view,
            &smart_placeholder,
            &details.view,
            &settings.view,
        );
        let panel = crate::popover::MenuPanel::new(mtm, delegate);
        let status = StatusBarController::new(mtm, delegate, demo);

        // Standard application menu preserves keyboard Quit and Window commands.
        let main_menu = NSMenu::new(mtm);
        let app_item = NSMenuItem::new(mtm);
        main_menu.addItem(&app_item);
        let app_menu = NSMenu::new(mtm);
        for (title, action, key) in [
            ("关于 Fan Control", sel!(about:), ""),
            ("打开 Fan Control", sel!(show:), "o"),
            ("设置…", sel!(preferences:), ","),
            ("智能散热…", sel!(policy:), "t"),
            ("帮助…", sel!(help:), ""),
            ("恢复系统自动", sel!(reset:), "r"),
            ("检查更新…", sel!(updates:), ""),
            ("退出 Fan Control", sel!(quit:), "q"),
        ] {
            let item = unsafe {
                app_menu.addItemWithTitle_action_keyEquivalent(
                    &text(title),
                    Some(action),
                    &text(key),
                )
            };
            unsafe {
                item.setTarget(Some(delegate));
            }
        }
        app_item.setSubmenu(Some(&app_menu));
        let edit_item = NSMenuItem::new(mtm);
        edit_item.setTitle(&text("编辑"));
        let edit_menu = NSMenu::new(mtm);
        for (title, action, key) in [
            ("撤销", sel!(undo:), "z"),
            ("重做", sel!(redo:), "Z"),
            ("剪切", sel!(cut:), "x"),
            ("拷贝", sel!(copy:), "c"),
            ("粘贴", sel!(paste:), "v"),
            ("全选", sel!(selectAll:), "a"),
        ] {
            unsafe {
                edit_menu.addItemWithTitle_action_keyEquivalent(
                    &text(title),
                    Some(action),
                    &raw_text(key),
                );
            }
        }
        edit_item.setSubmenu(Some(&edit_menu));
        main_menu.addItem(&edit_item);
        NSApplication::sharedApplication(mtm).setMainMenu(Some(&main_menu));

        let updater = if demo { None } else { Updater::load(mtm) };
        settings.update.setEnabled(updater.is_some());
        let worker = Worker::start(demo);

        let ui = Self {
            worker,
            shell,
            status,
            panel,
            overview,
            history: crate::trend::History::default(),
            pending_speed: None,
            fans,
            pending_fan_speed: None,
            details,
            settings,
            updater,
            fan_ids: Vec::new(),
            selected: None,
            editor: None,
            demo,
            policy: None,
            pending_policy_save: None,
            pending_tuning: None,
            last_visible: true,
            panel_opened: false,
        };
        if demo {
            ui.shell.show(Section::Overview);
        }
        ui
    }

    fn open_preferences(&mut self) {
        self.panel.close();
        self.settings.sync_login_state(self.demo);
        self.shell.show(Section::Settings);
    }

    fn toggle_panel(&self) {
        let mtm = self.shell.window.mtm();
        if let Some(button) = self.status.item.button(mtm) {
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            self.panel.show(&button);
        }
    }

    fn introduce_panel(&self) {
        let defaults = NSUserDefaults::standardUserDefaults();
        if defaults.boolForKey(&raw_text(ONBOARDED_KEY)) {
            return;
        }
        defaults.setBool_forKey(true, &raw_text(ONBOARDED_KEY));
        self.toggle_panel();
    }

    fn apply_plan(&mut self, plan: crate::presenter::Plan, percent: f64) {
        let state = self.worker.snapshot();
        let view = crate::presenter::present(&state, fresh(&state), self.demo);
        if !view.controls_enabled {
            self.panel.forget_plan();
            self.overview.forget_plan();
            return;
        }
        for config in crate::presenter::configs_for_plan(&state, plan, percent) {
            let _ = self.worker.send(WorkerCommand::Configure(config));
        }
        self.pending_speed = None;
    }

    fn open_policy(&mut self, delegate: &Delegate) {
        self.panel.close();
        self.build_policy(delegate);
        self.shell.show(Section::Smart);
    }

    fn build_policy(&mut self, target: &AnyObject) {
        let state = self.worker.snapshot();
        if !state.discovered && !self.demo {
            return;
        }
        let mtm = self.shell.window.mtm();
        let policy = crate::policy::PolicyEditor::new(mtm, target, &state);
        self.shell.set_smart_view(mtm, &policy.view);
        policy.set_saving(self.pending_policy_save.is_some(), state.installing);
        self.policy = Some(policy);
    }

    /// Unified immediate-save path for the 21-step cooling preference slider
    /// across both the menu bar panel and the Smart Cooling page.
    fn save_tuning_preference(&mut self, bias: i8, origin: TuningOrigin) {
        if self.pending_policy_save.is_some() {
            return;
        }
        let previous = self.worker.snapshot().config.adaptive_tuning.bias;
        let (tx, rx) = std::sync::mpsc::channel();
        match self.worker.send(WorkerCommand::ConfigureTuning {
            tuning: fan_core::AdaptiveTuning { bias },
            reply: tx,
        }) {
            Ok(()) => {
                self.pending_policy_save = Some(rx);
                self.pending_tuning = Some((previous, bias, origin));
                if let Some(policy) = self.policy.as_ref() {
                    policy.set_saving(true, false);
                    if origin == TuningOrigin::PolicyPage {
                        policy.show_preference_saving();
                    }
                }
            }
            Err(e) => {
                self.panel.preference_result(Err(e.clone()));
                if let Some(policy) = self.policy.as_ref() {
                    policy.force_preference(previous);
                    policy.preference_result(Err(e));
                }
            }
        }
    }

    fn save_policy(&mut self) {
        if self.pending_policy_save.is_some() {
            return;
        }
        let Some(policy) = self.policy.as_ref() else {
            return;
        };
        match policy.read(&self.worker.snapshot().snapshot) {
            Ok(saved) => {
                let (tx, rx) = std::sync::mpsc::channel();
                match self.worker.send(WorkerCommand::ConfigurePolicy {
                    policy: saved,
                    tuning: policy.tuning(),
                    reply: tx,
                }) {
                    Ok(()) => {
                        self.pending_policy_save = Some(rx);
                        policy.set_saving(true, false);
                        policy.notice.setStringValue(&text("正在保存智能策略…"));
                    }
                    Err(error) => policy.notice.setStringValue(&text(&error)),
                }
            }
            Err(error) => policy.notice.setStringValue(&text(&error)),
        }
    }

    fn poll_policy_save(&mut self, target: &AnyObject) {
        let result = self
            .pending_policy_save
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("保存未获确认，请重试。".into()))
                }
            });
        if let Some(result) = result {
            self.pending_policy_save = None;
            if let Some((previous, bias, origin)) = self.pending_tuning.take() {
                if let Some(policy) = self.policy.as_ref() {
                    policy.set_saving(false, self.worker.snapshot().installing);
                    match (&result, origin) {
                        (Ok(()), TuningOrigin::Panel) => {
                            policy.sync_saved_preference(previous, bias);
                        }
                        (Ok(()), TuningOrigin::PolicyPage) => {
                            policy.force_preference(bias);
                            policy.preference_result(Ok(()));
                        }
                        (Err(e), TuningOrigin::PolicyPage) => {
                            policy.force_preference(previous);
                            policy.preference_result(Err(e.clone()));
                        }
                        (Err(_), TuningOrigin::Panel) => {}
                    }
                }
                if origin == TuningOrigin::Panel {
                    self.panel.preference_result(result);
                }
                return;
            }
            if result.is_ok() {
                self.build_policy(target);
            }
            if let Some(policy) = self.policy.as_ref() {
                policy.set_saving(false, self.worker.snapshot().installing);
                policy.notice.setStringValue(&text(&match result {
                    Ok(()) => "智能策略已保存，等待新采样评估。".into(),
                    Err(error) => format!("保存失败：{error}"),
                }));
            }
        }
    }

    fn close_policy(&mut self, target: &AnyObject) {
        self.build_policy(target);
    }

    /// Only called in --ui-smoke, where the worker has no SMC/helper/storage access.
    fn run_smoke_checks(&mut self, delegate: &Delegate) {
        assert!(self.demo);
        // 1. Verify sidebar section switching keeps main window size completely stable.
        self.shell.window.layoutIfNeeded();
        let initial_frame = self.shell.window.frame();
        assert!(
            initial_frame.size.width <= 860.,
            "main window should stay near default width: {:?}",
            initial_frame.size
        );
        for section in Section::ALL {
            self.shell.select(section);
            self.shell.window.layoutIfNeeded();
            self.shell.window.displayIfNeeded();
            let current = self.shell.window.frame();
            assert!(
                (current.size.width - initial_frame.size.width).abs() < 0.5
                    && (current.size.height - initial_frame.size.height).abs() < 0.5,
                "sidebar switch to {:?} changed window size: {:?} vs {:?}",
                section,
                current.size,
                initial_frame.size
            );
        }

        // 2. Verify native view-based sensor table (1000 rows, reuse, search, sort).
        self.shell.select(Section::Details);
        self.shell.window.layoutIfNeeded();
        self.details.verify_native_list(&self.shell.window);

        // 3. Verify curve editor updates in place without rebuilding the NSWindow.
        self.open_editor(delegate, None, false);
        let editor_win_ptr = self
            .editor
            .as_ref()
            .expect("curve editor opened")
            .window
            .as_ref() as *const NSWindow;
        if let Some(editor) = self.editor.as_mut() {
            let initial_points = editor.rows.len();
            let mut next = editor.read().expect("valid default curve");
            next.curve.as_mut().unwrap().points.push(CurvePoint {
                temperature: 92.,
                speed_percent: 88.,
            });
            editor.apply_draft(next, true);
            assert_eq!(editor.rows.len(), initial_points + 1);
            let same_ptr = editor.window.as_ref() as *const NSWindow;
            assert_eq!(
                editor_win_ptr, same_ptr,
                "curve editor must update in place"
            );
            let undone = editor.undo.pop().expect("undo entry");
            editor.apply_draft(undone, false);
            assert_eq!(editor.rows.len(), initial_points);
        }
        self.close_editor();

        // 4. Verify Smart Cooling page & unified immediate preference save + rollback.
        self.open_policy(delegate);
        self.shell.window.layoutIfNeeded();
        self.shell.window.displayIfNeeded();
        self.verify_policy_flow(delegate);

        // 5. Verify Settings page, login toggle wiring, and menu bar visual picker modes.
        self.open_preferences();
        self.shell.window.layoutIfNeeded();
        let login_frame = self.settings.login.frame();
        assert!(
            login_frame.size.width >= 32. && login_frame.size.height >= 20.,
            "login switch should have clickable native geometry: {:?}",
            login_frame.size
        );
        assert_eq!(self.settings.login.action(), Some(sel!(login:)));
        assert!(self.settings.login.target().is_some());
        self.settings.set_general_message("test notice", true);
        assert!(!self.settings.message.isHidden());
        self.settings.set_general_message("", false);
        assert!(self.settings.message.isHidden());
        for mode in MenuBarDisplayMode::ALL {
            self.status.set_mode(mode, true);
            self.settings.set_menu_bar_mode(mode);
            let state = self.worker.snapshot();
            let view = crate::presenter::present(&state, fresh(&state), true);
            let cards = crate::presenter::fan_cards(&state, fresh(&state));
            self.status
                .refresh(self.shell.window.mtm(), &view, &cards, true, &state.message);
        }
    }

    fn verify_policy_flow(&mut self, target: &AnyObject) {
        assert!(self.demo);
        self.policy
            .as_ref()
            .expect("smart page")
            .verify_preference_controls();
        let modes = self.worker.snapshot().config.fans;

        // Calibration policy save error & disconnect.
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_policy_save = Some(rx);
        self.save_policy();
        assert!(self.pending_policy_save.is_some());
        tx.send(Err("simulated disk full".into())).unwrap();
        self.poll_policy_save(target);
        assert!(self
            .policy
            .as_ref()
            .unwrap()
            .notice
            .stringValue()
            .to_string()
            .contains("simulated disk full"));
        assert_eq!(self.worker.snapshot().config.adaptive_tuning.bias, 0);

        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_policy_save = Some(rx);
        drop(tx);
        self.poll_policy_save(target);
        assert!(self.pending_policy_save.is_none());

        // Unified immediate save from PolicyPage: error rollback, disconnect rollback, success.
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_policy_save = Some(rx);
        self.pending_tuning = Some((0, -7, TuningOrigin::PolicyPage));
        self.save_tuning_preference(-10, TuningOrigin::PolicyPage);
        assert_eq!(self.pending_tuning, Some((0, -7, TuningOrigin::PolicyPage)));
        tx.send(Err("simulated disk full".into())).unwrap();
        self.poll_policy_save(target);
        assert_eq!(self.worker.snapshot().config.adaptive_tuning.bias, 0);
        assert_eq!(self.policy.as_ref().unwrap().tuning().bias, 0);

        self.save_tuning_preference(-7, TuningOrigin::PolicyPage);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while self.pending_policy_save.is_some() && std::time::Instant::now() < deadline {
            self.poll_policy_save(target);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(self.pending_policy_save.is_none());
        assert_eq!(self.worker.snapshot().config.adaptive_tuning.bias, -7);
        assert_eq!(self.policy.as_ref().unwrap().tuning().bias, -7);
        assert_eq!(self.worker.snapshot().config.fans, modes);

        // Inline panel: error/disconnect rollback, duplicate save, and acknowledged save/reset.
        let previous = self.worker.snapshot().config.adaptive_tuning.bias;
        self.policy.as_ref().unwrap().smoke_preference(10);
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_policy_save = Some(rx);
        self.pending_tuning = Some((previous, 6, TuningOrigin::Panel));
        self.save_tuning_preference(10, TuningOrigin::Panel);
        assert_eq!(
            self.pending_tuning,
            Some((previous, 6, TuningOrigin::Panel))
        );
        tx.send(Err("simulated disk full".into())).unwrap();
        self.poll_policy_save(target);
        assert_eq!(self.worker.snapshot().config.adaptive_tuning.bias, previous);
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_policy_save = Some(rx);
        self.pending_tuning = Some((previous, 6, TuningOrigin::Panel));
        drop(tx);
        self.poll_policy_save(target);
        assert_eq!(self.worker.snapshot().config.adaptive_tuning.bias, previous);
        for bias in [6, 0] {
            self.save_tuning_preference(bias, TuningOrigin::Panel);
            self.save_tuning_preference(10, TuningOrigin::Panel);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while self.pending_policy_save.is_some() && std::time::Instant::now() < deadline {
                self.poll_policy_save(target);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(self.pending_policy_save.is_none());
            assert_eq!(self.worker.snapshot().config.adaptive_tuning.bias, bias);
            assert_eq!(self.worker.snapshot().config.fans, modes);
        }
        assert_eq!(self.policy.as_ref().unwrap().tuning().bias, 10);
        self.close_policy(target);
        assert_eq!(self.policy.as_ref().unwrap().tuning().bias, 0);
        println!("Sidebar stability, curve editor in-place updates, unified immediate preference and disclosure verified in demo mode");
    }

    fn refresh(&mut self, target: &AnyObject) {
        if let Some((percent, changed)) = self.pending_speed {
            if changed.elapsed() >= SPEED_SETTLE {
                self.apply_plan(crate::presenter::Plan::Custom, percent);
            }
        }
        let visible = self.shell.window.isVisible() || self.panel.popover.isShown();
        if visible != self.last_visible {
            let _ = self.worker.send(WorkerCommand::Visibility(visible));
            self.last_visible = visible;
        }
        self.poll_policy_save(target);
        let state = self.worker.snapshot();
        if let Some(policy) = self.policy.as_mut() {
            policy.refresh(&state);
            policy.set_saving(self.pending_policy_save.is_some(), state.installing);
        }
        let is_fresh = fresh(&state);
        let view = crate::presenter::present(&state, is_fresh, self.demo);
        self.history.record(crate::trend::Sample {
            at: std::time::Instant::now(),
            temperature: view.temperature,
            fan_percent: view.fan_percent,
        });
        let cards = crate::presenter::fan_cards(&state, is_fresh);
        let pending_bias = self.pending_tuning.map(|(_, b, _)| b);
        self.panel.refresh(
            &view,
            self.pending_speed.map(|(percent, _)| percent),
            crate::popover::DashboardUpdate {
                cards: &cards,
                history: &self.history,
                bias: state.config.adaptive_tuning.bias,
                pending: pending_bias,
                saving: self.pending_policy_save.is_some() || state.installing,
            },
        );
        self.shell.refresh(&view);
        if self.shell.window.isVisible() {
            let smart = crate::presenter::smart_status(&state, is_fresh);
            self.overview
                .refresh(&view, &cards, smart.demand, &self.history);
        }
        if let Some((id, percent, changed)) = self.pending_fan_speed {
            if changed.elapsed() >= SPEED_SETTLE {
                self.pending_fan_speed = None;
                self.apply_choice(id, crate::presenter::FanChoice::Fixed, percent);
            }
        }
        let ids: Vec<_> = state.snapshot.fans.iter().map(|fan| fan.id).collect();
        if ids != self.fan_ids {
            self.fan_ids = ids;
            if !self.selected.is_some_and(|id| self.fan_ids.contains(&id)) {
                self.selected = self.fan_ids.first().copied();
            }
        }
        let resized = self.fans.refresh(
            target,
            &view,
            &cards,
            !self.demo && state.helper_ready && !state.installing,
            is_fresh,
            self.pending_fan_speed.map(|(id, percent, _)| (id, percent)),
        );
        if resized {
            let smart = crate::presenter::smart_status(&state, is_fresh);
            self.overview
                .refresh(&view, &cards, smart.demand, &self.history);
        }
        if self.shell.window.isVisible() {
            self.details.refresh(&state, is_fresh);
        }
        self.settings.refresh(
            self.demo,
            state.installing,
            state.helper_ready,
            &state.configuration_notice,
        );
        self.settings
            .update
            .setEnabled(self.updater.as_ref().is_some_and(Updater::can_check));

        if let Some(editor) = self.editor.as_mut() {
            if !editor.chart.is_dragging() {
                editor.drag_recorded = false;
                if let Ok(draft) = editor.read_draft() {
                    if let Some(curve) = draft.curve {
                        editor.chart.set_current(if is_fresh {
                            state
                                .snapshot
                                .input_value(&curve.sensor_key, &state.thermal)
                        } else {
                            None
                        });
                        editor.chart.set_curve(curve);
                    }
                }
            }
        }

        let mtm = self.shell.window.mtm();
        self.status
            .refresh(mtm, &view, &cards, is_fresh, &state.message);
    }

    fn apply_choice(&mut self, id: u8, choice: crate::presenter::FanChoice, percent: f64) {
        let state = self.worker.snapshot();
        if self.demo || !state.helper_ready || state.installing {
            return;
        }
        if let Some(config) = crate::presenter::config_for_choice(&state, id, choice, percent) {
            let _ = self.worker.send(WorkerCommand::Configure(config));
        }
    }

    fn close_editor(&mut self) {
        if let Some(editor) = self.editor.take() {
            editor.window.close();
        }
    }

    fn open_editor(&mut self, delegate: &Delegate, draft: Option<FanConfig>, record: bool) {
        let Some(id) = self.selected else {
            return;
        };
        let state = self.worker.snapshot();
        let fan = draft.unwrap_or_else(|| {
            state
                .config
                .fans
                .iter()
                .find(|f| f.fan_id == id)
                .cloned()
                .unwrap_or_else(|| FanConfig::automatic(id))
        });
        if let Some(editor) = self.editor.as_mut() {
            editor.apply_draft(fan, record);
            editor.window.makeKeyAndOrderFront(None);
            return;
        }
        let mut fan = fan;
        fan.curve
            .get_or_insert_with(|| Curve::balanced("native-balanced", THERMAL_DEMAND_KEY));
        let editor = CurveEditor::new(delegate.mtm(), delegate, fan, &state);
        editor.window.makeKeyAndOrderFront(None);
        self.editor = Some(editor);
    }
}

fn fresh(state: &crate::worker::UiSnapshot) -> bool {
    state.sample_age_secs + state.published_at.elapsed().as_secs_f64()
        <= fan_core::SNAPSHOT_MAXIMUM_AGE
}

pub fn run(demo: bool, smoke: bool) {
    let language = if demo {
        std::env::var("FAN_CONTROL_DEMO_LANGUAGE")
            .ok()
            .and_then(|value| crate::i18n::Lang::from_preference_key(&value))
            .unwrap_or_default()
    } else {
        NSUserDefaults::standardUserDefaults()
            .stringForKey(&raw_text("FanControlLanguage"))
            .and_then(|value| crate::i18n::Lang::from_preference_key(&value.to_string()))
            .unwrap_or_default()
    };
    crate::i18n::set_override(language);
    let mtm = MainThreadMarker::new().expect("GUI must start on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate = Delegate::new(mtm, demo, smoke);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
