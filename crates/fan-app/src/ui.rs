//! System AppKit controls owned by Rust. Main-thread state never crosses into hardware work.
use crate::{
    updater::Updater,
    worker::{Worker, WorkerCommand},
};
use fan_core::{Config, ControlMode, Curve, CurvePoint, FanConfig, THERMAL_DEMAND_KEY};
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel, DefinedClass, MainThreadOnly,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSTimer, NSUserDefaults,
};
use std::cell::{OnceCell, RefCell};

pub(crate) mod tokens {
    pub const WIDTH: f64 = 620.0;
    pub const MARGIN: f64 = 24.0;
    pub const BODY: f64 = 13.0;
    pub const CAPTION: f64 = 11.0;
    pub const TITLE: f64 = 22.0;
    pub const ROW: f64 = 30.0;
}
const TAB_OVERVIEW: isize = 0;
const EDITOR_WIDTH: f64 = 560.;
const EDITOR_HEIGHT: f64 = 720.;
const TAB_FANS: isize = 1;
const TAB_SMART: isize = 2;
const TAB_SETTINGS: isize = 4;
/// Smart cooling page height until settings and sensors load.
const SMART_HEIGHT: f64 = 600.;
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
pub(crate) fn label(
    parent: &NSView,
    mtm: MainThreadMarker,
    value: &str,
    frame: NSRect,
    size: f64,
) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&text(value), mtm);
    field.setFrame(frame);
    field.setFont(Some(&NSFont::systemFontOfSize(size)));
    field.setToolTip(Some(&text(value)));
    parent.addSubview(&field);
    field
}
fn button(
    parent: &NSView,
    delegate: &Delegate,
    title: &str,
    action: objc2::runtime::Sel,
    frame: NSRect,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &text(title),
            Some(delegate),
            Some(action),
            delegate.mtm(),
        )
    };
    button.setFrame(frame);
    parent.addSubview(&button);
    button
}
pub(crate) fn input(
    parent: &NSView,
    mtm: MainThreadMarker,
    value: &str,
    frame: NSRect,
) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(NSTextField::alloc(mtm), frame);
    field.setStringValue(&raw_text(value));
    parent.addSubview(&field);
    field
}
pub(crate) fn window(mtm: MainThreadMarker, title: &str, w: f64, h: f64) -> Retained<NSWindow> {
    let visible = NSScreen::mainScreen(mtm).map(|screen| screen.visibleFrame());
    let window_width = visible
        .map(|frame| w.min((frame.size.width - 40.).max(320.)))
        .unwrap_or(w);
    let window_height = visible
        .map(|frame| h.min((frame.size.height - 60.).max(320.)))
        .unwrap_or(h);
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(0., 0., window_width, window_height),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&text(title));
    window.center();
    window
}
pub(crate) fn content(
    window: &NSWindow,
    mtm: MainThreadMarker,
    w: f64,
    h: f64,
) -> Retained<NSView> {
    let parent = window.contentView().expect("window content view");
    if parent.bounds().size.width >= w && parent.bounds().size.height >= h {
        return parent;
    }
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), parent.bounds());
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    scroll.setHasVerticalScroller(true);
    scroll.setHasHorizontalScroller(true);
    scroll.setAutohidesScrollers(true);
    let document = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., w, h));
    scroll.setDocumentView(Some(&document));
    parent.addSubview(&scroll);
    scroll
        .contentView()
        .scrollToPoint(NSPoint::new(0., (h - parent.bounds().size.height).max(0.)));
    scroll.reflectScrolledClipView(&scroll.contentView());
    document
}

struct Editor {
    window: Retained<NSWindow>,
    fan: FanConfig,
    source: Retained<NSPopUpButton>,
    sources: Vec<String>,
    hysteresis: Retained<NSTextField>,
    rows: Vec<(Retained<NSTextField>, Retained<NSTextField>)>,
    error: Retained<NSTextField>,
    chart: Retained<crate::chart::CurvePreview>,
    undo: Vec<FanConfig>,
    presets: Retained<NSSegmentedControl>,
    all_fans: Retained<NSButton>,
    /// Undo is recorded once per drag gesture, not per mouse movement.
    drag_recorded: bool,
}
struct Ui {
    worker: Worker,
    window: Retained<NSWindow>,
    tabs: Retained<NSTabViewController>,
    /// Hosts the smart-cooling editor once settings and sensors are loaded.
    smart_controller: Retained<NSViewController>,
    status: Retained<NSStatusItem>,
    panel: crate::popover::MenuPanel,
    overview: crate::overview::Overview,
    history: crate::trend::History,
    /// Custom speed chosen in the panel, applied once the slider settles.
    pending_speed: Option<(f64, std::time::Instant)>,
    fans: crate::fans::FansPage,
    fans_controller: Retained<NSViewController>,
    overview_controller: Retained<NSViewController>,
    details_controller: Retained<NSViewController>,
    settings_controller: Retained<NSViewController>,
    /// Per-fan fixed speed from the Fans page, applied once the slider settles.
    pending_fan_speed: Option<(u8, f64, std::time::Instant)>,
    details: crate::details::DetailsPage,
    settings: crate::preferences::SettingsPage,
    updater: Option<Updater>,
    fan_ids: Vec<u8>,
    selected: Option<u8>,
    editor: Option<Editor>,
    demo: bool,
    policy: Option<crate::policy::PolicyEditor>,
    pending_policy_save: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    pending_panel_tuning: Option<(i8, i8)>,
    last_visible: bool,
    panel_opened: bool,
}
struct Ivars {
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
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self,_notification:&NSNotification) {
            let ui=Ui::new(self,self.ivars().demo);
            self.ivars().ui.replace(Some(ui));
            let timer=unsafe { NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(0.5,self,sel!(tick:),None,true) };
            let _=self.ivars().timer.set(timer);
        }
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self,_app:&NSApplication)->NSApplicationTerminateReply {
            let confirmed=self.ivars().ui.borrow().as_ref().is_none_or(|ui|ui.worker.shutdown());
            if !confirmed {
                crate::telemetry::event("gui.shutdown.handback.unconfirmed");
                if !crate::TERMINATE_REQUEST.load(std::sync::atomic::Ordering::SeqCst) {
                    let alert=NSAlert::new(self.mtm());alert.setMessageText(&text("退出前交还未获确认"));alert.setInformativeText(&text("控制服务会继续重试。请检查服务状态与风扇实际模式；应用无法确认安全回退已经完成。"));alert.addButtonWithTitle(&text("退出"));alert.runModal();
                }
            }
            NSApplicationTerminateReply::TerminateNow
        }
    }
    impl Delegate {
        #[unsafe(method(tick:))]
        fn tick(&self,_timer:&NSTimer) { if crate::TERMINATE_REQUEST.load(std::sync::atomic::Ordering::SeqCst) {NSApplication::sharedApplication(self.mtm()).terminate(None);return;}
            let mut smoke_done=false;
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                if !ui.panel_opened && !self.ivars().smoke && !ui.demo {
                    ui.panel_opened=true;
                    ui.introduce_panel();
                }
                ui.refresh(self);
                if ui.policy.is_none() { ui.build_policy(self); }
                if self.ivars().smoke && !ui.fan_ids.is_empty() {
                    ui.open_editor(self,None,false);ui.open_policy(self);ui.open_preferences(self);
                    ui.tabs.setSelectedTabViewItemIndex(3);
                    ui.details.verify_native_list(&ui.window);
                    ui.tabs.setSelectedTabViewItemIndex(TAB_SMART);
                    ui.window.displayIfNeeded();
                    ui.verify_policy_flow(self);
                    smoke_done=true;
                }
            }
            if smoke_done {
                println!("Rust AppKit main, curve, thermal policy and preferences initialized in isolated demo mode");
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            } else if self.ivars().smoke && self.ivars().started.elapsed().as_secs()>10 {
                eprintln!("Demo UI initialization timed out");std::process::exit(1);
            }
        }
        #[unsafe(method(show:))]
        fn show(&self,_sender:Option<&AnyObject>) {
            if let Some(ui)=self.ivars().ui.borrow().as_ref() {
                ui.panel.close();
                ui.show_tab(TAB_OVERVIEW);
            }
        }
        #[unsafe(method(togglePanel:))]
        fn toggle_panel(&self,_sender:Option<&AnyObject>) {
            if let Some(ui)=self.ivars().ui.borrow().as_ref() { ui.toggle_panel(); }
        }
        #[unsafe(method(planChanged:))]
        fn plan_changed(&self,sender:&NSSegmentedControl) {
            let plan=match sender.selectedSegment() {0=>crate::presenter::Plan::System,1=>crate::presenter::Plan::Smart,_=>crate::presenter::Plan::Custom};
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() { let percent=ui.panel.slider.doubleValue(); ui.apply_plan(plan,percent); }
        }
        #[unsafe(method(customSpeed:))]
        fn custom_speed(&self,sender:&NSSlider) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let percent=sender.doubleValue().round();
                ui.panel.set_percent_label(percent);
                ui.pending_speed=Some((percent,std::time::Instant::now()));
            }
        }
        #[unsafe(method(useCurve:))]
        fn use_curve(&self,_sender:Option<&AnyObject>) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                ui.panel.close();
                ui.show_tab(TAB_FANS);
                ui.open_editor(self,None,false);
            }
        }
        #[unsafe(method(noticeAction:))]
        fn notice_action(&self,_sender:Option<&AnyObject>) {
            if let Some(ui)=self.ivars().ui.borrow().as_ref() {
                let state=ui.worker.snapshot();
                let view=crate::presenter::present(&state,fresh(&state),ui.demo);
                let command=match view.notice.action {
                    Some(crate::presenter::Action::EnableControl)=>WorkerCommand::Install,
                    Some(crate::presenter::Action::Reconnect)=>WorkerCommand::Retry,
                    None=>return,
                };
                let _=ui.worker.send(command);
            }
        }
        #[unsafe(method(fanChoice:))]
        fn fan_choice(&self,sender:&NSPopUpButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let index=sender.tag();
                let (Some(id),Some(choice))=(ui.fans.card_id(index),ui.fans.choice(index)) else {return;};
                let percent=ui.fans.slider_value(index).unwrap_or(crate::presenter::DEFAULT_CUSTOM_PERCENT);
                ui.pending_fan_speed=None;
                ui.fans.forget(index);
                ui.apply_choice(id,choice,percent);
            }
        }
        #[unsafe(method(fanSpeed:))]
        fn fan_speed(&self,sender:&NSSlider) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let index=sender.tag();
                let Some(id)=ui.fans.card_id(index) else {return;};
                let percent=sender.doubleValue().round();
                let range=ui.worker.snapshot().snapshot.fans.iter().find(|fan|fan.id==id).and_then(|fan|fan_core::validated_rpm_range(fan.min_rpm,fan.max_rpm));
                ui.fans.show_fixed_value(index,percent,&crate::presenter::rpm_hint(percent,range));
                ui.pending_fan_speed=Some((id,percent,std::time::Instant::now()));
            }
        }
        #[unsafe(method(fanCurve:))]
        fn fan_curve(&self,sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let Some(id)=ui.fans.card_id(sender.tag()) else {return;};
                ui.close_editor();
                ui.selected=Some(id);
                ui.open_editor(self,None,false);
            }
        }
        #[unsafe(method(useSmart:))]
        fn use_smart(&self,_sender:Option<&AnyObject>) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() { ui.apply_plan(crate::presenter::Plan::Smart,crate::presenter::DEFAULT_CUSTOM_PERCENT); }
        }
        #[unsafe(method(reset:))]
        fn reset(&self,_sender:Option<&AnyObject>) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() { let _=ui.worker.send(WorkerCommand::Reset); }
        }
        #[unsafe(method(install:))]
        fn install(&self,_sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow().as_ref() { let _=ui.worker.send(WorkerCommand::Install); }
        }
        #[unsafe(method(retry:))]
        fn retry(&self,_sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow().as_ref() { let _=ui.worker.send(WorkerCommand::Retry); }
        }
        #[unsafe(method(updates:))]
        fn updates(&self,_sender:Option<&AnyObject>) {
            let updater=self.ivars().ui.borrow().as_ref().and_then(|ui|ui.updater.clone());
            if let Some(updater)=updater { updater.check(); }
        }
        #[unsafe(method(export:))]
        fn export(&self,_sender:&NSButton) {
            let sender=self.ivars().ui.borrow().as_ref().map(|ui|ui.worker.sender());
            if let Some(sender)=sender {
                let panel=NSSavePanel::savePanel(self.mtm());
                panel.setNameFieldStringValue(&text("FanControl-diagnostics.json"));
                if panel.runModal()==NSModalResponseOK {
                    if let Some(path)=panel.URL().and_then(|url|url.path()) { let _=sender.send(WorkerCommand::Export(std::path::PathBuf::from(path.to_string()))); }
                }
            }
        }
        #[unsafe(method(editCurve:))]
        fn edit_curve(&self,_sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() { ui.open_editor(self,None,false); }
        }
        #[unsafe(method(saveCurve:))]
        fn save_curve(&self,_sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let state=ui.worker.snapshot();
                if !state.helper_ready || state.installing {
                    if let Some(editor)=ui.editor.as_ref(){editor.error.setStringValue(&text("请先启用控制服务，曲线草稿尚未保存或应用。"));}return;
                }
                let result=ui.editor.as_ref().map(Editor::read);
                let all_fans=ui.editor.as_ref().is_some_and(|editor|editor.all_fans.state()==NSControlStateValueOn);
                if let Some(result)=result {
                    match result {
                        Ok(fan) if all_fans => {
                            if let Some(curve)=fan.curve.as_ref() {
                                for config in crate::presenter::configs_for_curve(&state,curve) { let _=ui.worker.send(WorkerCommand::Configure(config)); }
                            }
                            ui.close_editor();
                        }
                        Ok(fan) => { let _=ui.worker.send(WorkerCommand::Configure(fan)); ui.close_editor(); }
                        Err(error)=>if let Some(editor)=ui.editor.as_ref() { editor.error.setStringValue(&text(&error)); },
                    }
                }
            }
        }
        #[unsafe(method(curvePreset:))]
        fn curve_preset(&self,sender:&NSSegmentedControl) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let Some(preset)=crate::presenter::CurvePreset::ALL.get(sender.selectedSegment().max(0) as usize).copied() else {return;};
                let draft=ui.editor.as_ref().map(|editor|{
                    let mut fan=editor.fan.clone();
                    let key=editor.sources.get(editor.source.indexOfSelectedItem().max(0) as usize).cloned().unwrap_or_else(||THERMAL_DEMAND_KEY.into());
                    let id=fan.curve.as_ref().map(|curve|curve.id.clone()).unwrap_or_else(||"native-balanced".into());
                    fan.curve=Some(crate::presenter::preset_curve(preset,&id,&key));
                    fan
                });
                if let Some(fan)=draft { ui.open_editor(self,Some(fan),true); }
            }
        }
        #[unsafe(method(curveDragged:))]
        fn curve_dragged(&self,_sender:Option<&AnyObject>) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let Some(editor)=ui.editor.as_mut() else {return;};
                if !editor.drag_recorded {
                    let source=editor.fan.curve.as_ref().map(|curve|curve.sensor_key.clone());
                    if let Ok(previous)=editor.read_values(None,source.as_deref()) {
                        if editor.undo.last()!=Some(&previous) { editor.undo.push(previous); }
                    }
                    editor.drag_recorded=true;
                }
                if let Some(curve)=editor.chart.curve() {
                    let mut points=curve.points.clone();
                    points.sort_by(|a,b|a.temperature.total_cmp(&b.temperature));
                    for ((x,speed),point) in editor.rows.iter().zip(&points) {
                        x.setStringValue(&raw_text(&crate::presenter::format_value(point.temperature)));
                        speed.setStringValue(&raw_text(&crate::presenter::format_value(point.speed_percent)));
                    }
                    editor.presets.setSelectedSegment(crate::presenter::matching_preset(&curve).and_then(|preset|crate::presenter::CurvePreset::ALL.iter().position(|p|*p==preset)).map_or(-1,|index|index as isize));
                }
            }
        }
        #[unsafe(method(curveAllFans:))]
        fn curve_all_fans(&self,_sender:&NSButton) {}
        #[unsafe(method(cancelCurve:))]
        fn cancel_curve(&self,_sender:&NSButton) { if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() { ui.close_editor(); } }
        #[unsafe(method(addPoint:))]
        fn add_point(&self,_sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let result=ui.editor.as_ref().map(Editor::read);
                if let Some(Ok(mut fan))=result {
                    if let Some(curve)=fan.curve.as_mut() { if curve.points.len()<12 { curve.points.sort_by(|a,b|a.temperature.total_cmp(&b.temperature)); let gap=curve.points.windows(2).max_by(|a,b|(a[1].temperature-a[0].temperature).total_cmp(&(b[1].temperature-b[0].temperature))); if let Some(gap)=gap {let temperature=(gap[0].temperature+gap[1].temperature)/2.;let speed_percent=curve.interpolate(temperature);curve.points.push(CurvePoint {temperature,speed_percent});curve.points.sort_by(|a,b|a.temperature.total_cmp(&b.temperature));} } }
                    ui.open_editor(self,Some(fan),true);
                } else if let Some(Err(error))=result {
                    if let Some(editor)=ui.editor.as_ref() {editor.error.setStringValue(&text(&error));}
                }
            }
        }
        #[unsafe(method(removePoint:))]
        fn remove_point(&self,sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let i=sender.tag().max(0) as usize;
                let result=ui.editor.as_ref().map(|editor|editor.read_values(Some(i),None));
                if let Some(Ok(fan))=result {
                    ui.open_editor(self,Some(fan),true);
                } else if let Some(Err(error))=result {
                    if let Some(editor)=ui.editor.as_ref() {editor.error.setStringValue(&text(&error));}
                }
            }
        }
        #[unsafe(method(sourceChanged:))]
        fn source_changed(&self,_sender:&NSPopUpButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                if let Some(editor)=ui.editor.as_ref() {
                    let source=editor.sources.get(editor.source.indexOfSelectedItem().max(0) as usize).cloned();
                    if let Some(source)=source {
                        let previous_source=editor.fan.curve.as_ref().map(|curve|curve.sensor_key.as_str());
                        let mut fan=editor.read_values(None,previous_source).unwrap_or_else(|_|editor.fan.clone());
                        if let Some(curve)=fan.curve.as_mut() {curve.set_sensor_key(source);}
                        ui.open_editor(self,Some(fan),true);
                    }
                }
            }
        }
        #[unsafe(method(resetCurve:))]
        fn reset_curve(&self,_sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                if let Some(editor)=ui.editor.as_ref() { let mut fan=editor.fan.clone(); let source=editor.sources.get(editor.source.indexOfSelectedItem().max(0) as usize).cloned().unwrap_or_else(||THERMAL_DEMAND_KEY.into()); fan.curve=Some(Curve::balanced("native-balanced", source)); ui.open_editor(self,Some(fan),true); }
            }
        }
        #[unsafe(method(undoCurve:))]
        fn undo_curve(&self,_sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {
                let fan=ui.editor.as_mut().and_then(|editor|editor.undo.pop());
                if let Some(fan)=fan {ui.open_editor(self,Some(fan),false);}
            }
        }
        #[unsafe(method(preferences:))]
        fn preferences(&self,_sender:Option<&AnyObject>) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut() {ui.open_preferences(self);}
        }
        #[unsafe(method(policy:))]
        fn policy(&self,_sender:Option<&AnyObject>) {if let Some(ui)=self.ivars().ui.borrow_mut().as_mut(){ui.open_policy(self);}}
        #[unsafe(method(coolingPreference:))]
        fn cooling_preference(&self,_sender:&NSSlider){if let Some(ui)=self.ivars().ui.borrow().as_ref(){if let Some(policy)=ui.policy.as_ref(){policy.preference_changed();}}}
        #[unsafe(method(resetCoolingPreference:))]
        fn reset_cooling_preference(&self,_sender:&NSButton){if let Some(ui)=self.ivars().ui.borrow().as_ref(){if let Some(policy)=ui.policy.as_ref(){policy.reset_preference();}}}
        #[unsafe(method(panelCoolingPreference:))]
        fn panel_cooling_preference(&self,sender:&NSSlider){if let Some(ui)=self.ivars().ui.borrow_mut().as_mut(){ui.save_panel_tuning(sender.doubleValue().round().clamp(-10.,10.) as i8);}}
        #[unsafe(method(resetPanelCoolingPreference:))]
        fn reset_panel_cooling_preference(&self,_sender:&NSButton){if let Some(ui)=self.ivars().ui.borrow_mut().as_mut(){ui.save_panel_tuning(0);}}
        #[unsafe(method(comfortToggle:))]
        fn comfort_toggle(&self,_sender:Option<&AnyObject>) {if let Some(ui)=self.ivars().ui.borrow_mut().as_mut(){if let Some(policy)=ui.policy.as_mut(){policy.refresh(&ui.worker.snapshot());}}}
        #[unsafe(method(recordCalibration:))]
        fn record_calibration(&self,sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow().as_ref(){if let Some(policy)=ui.policy.as_ref(){if let Err(error)=policy.record(sender.tag().max(0) as usize,&ui.worker.snapshot()){policy.notice.setStringValue(&text(&error));}}}
        }
        #[unsafe(method(calibrationSource:))]
        fn calibration_source(&self,_sender:&NSPopUpButton){if let Some(ui)=self.ivars().ui.borrow().as_ref(){if let Some(policy)=ui.policy.as_ref(){policy.source_changed();}}}
        #[unsafe(method(clearCalibration:))]
        fn clear_calibration(&self,_sender:&NSButton){if let Some(ui)=self.ivars().ui.borrow().as_ref(){if let Some(policy)=ui.policy.as_ref(){policy.clear();}}}
        #[unsafe(method(cancelPolicy:))]
        fn cancel_policy(&self,_sender:&NSButton){if let Some(ui)=self.ivars().ui.borrow_mut().as_mut(){ui.close_policy(self);}}
        #[unsafe(method(savePolicy:))]
        fn save_policy(&self,_sender:&NSButton) {
            if let Some(ui)=self.ivars().ui.borrow_mut().as_mut(){ui.save_policy();}
        }
        #[unsafe(method(login:))]
        fn login(&self,sender:&NSSwitch) {
            let demo=self.ivars().ui.borrow().as_ref().is_none_or(|ui|ui.demo);
            if demo {return;}
            let bundle=objc2_foundation::NSBundle::mainBundle().bundlePath().to_string();
            let result=crate::settings::set_enabled(sender.state()==NSControlStateValueOn,std::path::Path::new(&bundle));
            let enabled=crate::settings::enabled();
            if let Some(ui)=self.ivars().ui.borrow().as_ref() {
                ui.settings.login.setState(if enabled.unwrap_or(false){NSControlStateValueOn}else{NSControlStateValueOff});
                ui.settings.message.setStringValue(&text(&result.map(|()|String::new()).unwrap_or_else(|e|e)));
            }
        }
        #[unsafe(method(language:))]
        fn language(&self,sender:&NSPopUpButton) {
            let Some(language)=[crate::i18n::Lang::System,crate::i18n::Lang::Chinese,crate::i18n::Lang::English].get(sender.indexOfSelectedItem().max(0) as usize).copied() else{return;};
            if let Some(ui)=self.ivars().ui.borrow().as_ref().filter(|ui|!ui.demo) {
                unsafe {NSUserDefaults::standardUserDefaults().setObject_forKey(Some(&raw_text(language.preference_key())),&raw_text("FanControlLanguage"));}
                ui.settings.message.setStringValue(&text("界面语言已保存，下次启动应用时生效。"));
            }
        }
        #[unsafe(method(uninstall:))]
        fn uninstall(&self,_sender:Option<&AnyObject>) {
            let sender=self.ivars().ui.borrow().as_ref().filter(|ui|!ui.demo).map(|ui|ui.worker.sender());
            let Some(sender)=sender else{return;};
            let alert=NSAlert::new(self.mtm());alert.setMessageText(&text("移除风扇控制服务？"));alert.setInformativeText(&text("应用将先停止自定义控制并交还系统，再请求管理员授权。移除后仍可查看温度，控制设置和诊断日志会保留。"));alert.addButtonWithTitle(&text("移除服务"));alert.addButtonWithTitle(&text("取消"));
            if alert.runModal()==NSAlertFirstButtonReturn {let _=sender.send(WorkerCommand::Uninstall);}
        }
        #[unsafe(method(about:))]
        fn about(&self,_sender:Option<&AnyObject>) {NSApplication::sharedApplication(self.mtm()).orderFrontStandardAboutPanel(None);}
        #[unsafe(method(help:))]
        fn help(&self,_sender:Option<&AnyObject>) {
            if let Some(url)=objc2_foundation::NSURL::URLWithString(&text("https://github.com/sm-yjr/fan-control#readme")){NSWorkspace::sharedWorkspace().openURL(&url);}
        }
        #[unsafe(method(quit:))]
        fn quit(&self,_sender:Option<&AnyObject>) { NSApplication::sharedApplication(self.mtm()).terminate(None); }
    }
);
impl Delegate {
    fn new(mtm: MainThreadMarker, demo: bool, smoke: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
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
        use tokens::*;
        let mtm = delegate.mtm();

        // Overview: plain-language health, numbers, trend and fans.
        let overview = crate::overview::Overview::new(mtm, delegate);
        let overview_controller = crate::form::controller(mtm, &overview.view);

        // Fans: one card per fan for people who want separate settings.
        let fans = crate::fans::FansPage::new(mtm, demo);
        let fans_controller = crate::form::controller(mtm, &fans.view);

        // Temperature details: components, battery and every sensor.
        let details = crate::details::DetailsPage::new(mtm);
        let details_controller = crate::form::controller(mtm, &details.view);

        // Settings: general preferences, the control service and support.
        let settings = crate::preferences::SettingsPage::new(mtm, delegate, demo);
        let settings_controller = crate::form::controller(mtm, &settings.view);

        let smart_view =
            NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., WIDTH, SMART_HEIGHT));
        label(
            &smart_view,
            mtm,
            "正在读取设置与传感器…",
            rect(MARGIN, SMART_HEIGHT - 60., WIDTH - MARGIN * 2., 24.),
            BODY,
        );
        let smart_controller = NSViewController::new(mtm);
        smart_controller.setView(&smart_view);
        smart_controller.setPreferredContentSize(NSSize::new(WIDTH, SMART_HEIGHT));

        let tabs = NSTabViewController::new(mtm);
        tabs.setTabStyle(NSTabViewControllerTabStyle::Toolbar);
        for (controller, title, symbol) in [
            (&overview_controller, "概览", "square.grid.2x2"),
            (&fans_controller, "风扇", "fanblades"),
            (&smart_controller, "智能散热", "sparkles"),
            (&details_controller, "温度详情", "thermometer.medium"),
            (&settings_controller, "设置", "gearshape"),
        ] {
            controller.setTitle(Some(&text(title)));
            let item = NSTabViewItem::tabViewItemWithViewController(controller);
            item.setLabel(&text(title));
            item.setImage(
                NSImage::imageWithSystemSymbolName_accessibilityDescription(
                    &raw_text(symbol),
                    Some(&text(title)),
                )
                .as_deref(),
            );
            tabs.addTabViewItem(&item);
        }
        tabs.setSelectedTabViewItemIndex(TAB_OVERVIEW);
        let window = NSWindow::windowWithContentViewController(&tabs);
        window.setStyleMask(
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable,
        );
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.center();

        let panel = crate::popover::MenuPanel::new(mtm, delegate);
        let status =
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = status.button(mtm) {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &text("fanblades"),
                Some(&text("Fan Control：打开风扇控制")),
            );
            button.setImage(image.as_deref());
            unsafe {
                button.setTarget(Some(delegate));
                button.setAction(Some(sel!(togglePanel:)));
            }
        }
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
            window,
            tabs,
            smart_controller,
            status,
            panel,
            overview,
            history: crate::trend::History::default(),
            pending_speed: None,
            fans,
            fans_controller: fans_controller.clone(),
            overview_controller: overview_controller.clone(),
            details_controller: details_controller.clone(),
            settings_controller: settings_controller.clone(),
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
            pending_panel_tuning: None,
            last_visible: true,
            panel_opened: false,
        };
        // A demo has no menu bar context to discover, so it opens the window.
        if demo {
            ui.window.makeKeyAndOrderFront(None);
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
        ui
    }
    fn show_tab(&self, index: isize) {
        self.tabs.setSelectedTabViewItemIndex(index);
        self.window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        NSApplication::sharedApplication(self.window.mtm()).activateIgnoringOtherApps(true);
    }
    fn open_preferences(&mut self, _delegate: &Delegate) {
        self.panel.close();
        self.show_tab(TAB_SETTINGS);
    }
    fn toggle_panel(&self) {
        let mtm = self.window.mtm();
        if let Some(button) = self.status.button(mtm) {
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            self.panel.show(&button);
        }
    }
    /// Shows the panel once, on the very first launch, so people find the
    /// menu bar entry. Later launches (such as at login) stay quiet.
    fn introduce_panel(&self) {
        let defaults = NSUserDefaults::standardUserDefaults();
        if defaults.boolForKey(&raw_text(ONBOARDED_KEY)) {
            return;
        }
        defaults.setBool_forKey(true, &raw_text(ONBOARDED_KEY));
        self.toggle_panel();
    }
    /// Applies one plan to every fan. Safety checks stay in the worker; this
    /// only refuses while the service, data or hardware range is unavailable.
    fn apply_plan(&mut self, plan: crate::presenter::Plan, percent: f64) {
        let state = self.worker.snapshot();
        let view = crate::presenter::present(&state, fresh(&state), self.demo);
        if !view.controls_enabled {
            self.panel.forget_plan();
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
        self.show_tab(TAB_SMART);
    }
    /// Builds (or, after 还原, rebuilds) the smart-cooling page from saved settings.
    fn build_policy(&mut self, target: &AnyObject) {
        let state = self.worker.snapshot();
        if !state.discovered && !self.demo {
            return;
        }
        let policy = crate::policy::PolicyEditor::new(self.window.mtm(), target, &state);
        self.smart_controller.setView(&policy.view);
        crate::form::fit(&self.smart_controller, &policy.view);
        policy.set_saving(self.pending_policy_save.is_some(), state.installing);
        self.policy = Some(policy);
    }
    fn save_panel_tuning(&mut self, bias: i8) {
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
                self.pending_panel_tuning = Some((previous, bias));
            }
            Err(e) => self.panel.preference_result(Err(e)),
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
            if let Some((previous, bias)) = self.pending_panel_tuning.take() {
                if result.is_ok() {
                    if let Some(policy) = self.policy.as_ref() {
                        policy.sync_saved_preference(previous, bias);
                    }
                }
                self.panel.preference_result(result);
                return;
            }
            if result.is_ok() {
                self.build_policy(target);
            }
            if let Some(policy) = self.policy.as_ref() {
                policy.notice.setStringValue(&text(&match result {
                    Ok(()) => "智能策略已保存，等待新采样评估。".into(),
                    Err(error) => format!("保存失败：{error}"),
                }));
            }
        }
    }
    /// Only called in --ui-smoke, where the worker has no SMC/helper/storage access.
    fn verify_policy_flow(&mut self, target: &AnyObject) {
        assert!(self.demo);
        self.policy
            .as_ref()
            .expect("smart page")
            .verify_preference_controls();
        let modes = self.worker.snapshot().config.fans;
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_policy_save = Some(rx);
        self.save_policy(); // A repeated request cannot replace an outstanding save.
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
        self.policy.as_ref().unwrap().smoke_preference(-7);
        self.save_policy();
        self.save_policy();
        self.close_policy(target); // Reopen before worker confirmation.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while self.pending_policy_save.is_some() && std::time::Instant::now() < deadline {
            self.poll_policy_save(target);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(self.pending_policy_save.is_none());
        assert_eq!(self.policy.as_ref().unwrap().tuning().bias, -7);
        assert_eq!(self.worker.snapshot().config.fans, modes);
        self.close_policy(target);
        assert_eq!(self.policy.as_ref().unwrap().tuning().bias, -7);
        self.policy.as_ref().unwrap().smoke_preference(10);
        self.close_policy(target);
        assert_eq!(self.policy.as_ref().unwrap().tuning().bias, -7);
        // Inline panel: error/disconnect roll back to accepted config, duplicate saves cannot replace it.
        let previous = self.worker.snapshot().config.adaptive_tuning.bias;
        self.policy.as_ref().unwrap().smoke_preference(10); // unrelated editor draft stays intact
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_policy_save = Some(rx);
        self.pending_panel_tuning = Some((previous, 6));
        self.save_panel_tuning(10);
        assert_eq!(self.pending_panel_tuning, Some((previous, 6)));
        tx.send(Err("simulated disk full".into())).unwrap();
        self.poll_policy_save(target);
        assert_eq!(self.worker.snapshot().config.adaptive_tuning.bias, previous);
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_policy_save = Some(rx);
        self.pending_panel_tuning = Some((previous, 6));
        drop(tx);
        self.poll_policy_save(target);
        assert_eq!(self.worker.snapshot().config.adaptive_tuning.bias, previous);
        for bias in [6, 0] {
            self.save_panel_tuning(bias);
            self.save_panel_tuning(10);
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
        println!("Inline preference: error/disconnect rollback, duplicate save, acknowledged save/reset and unrelated editor draft verified");
        println!("Smart preference: 21 native positions, reset, failure/disconnect feedback, duplicate save and reopen verified in demo mode");
    }
    fn close_policy(&mut self, target: &AnyObject) {
        self.build_policy(target);
    }
    fn refresh(&mut self, target: &AnyObject) {
        if let Some((percent, changed)) = self.pending_speed {
            if changed.elapsed() >= SPEED_SETTLE {
                self.apply_plan(crate::presenter::Plan::Custom, percent);
            }
        }
        let visible = self.window.isVisible() || self.panel.popover.isShown();
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
        let fresh = fresh(&state);
        let view = crate::presenter::present(&state, fresh, self.demo);
        self.history.record(crate::trend::Sample {
            at: std::time::Instant::now(),
            temperature: view.temperature,
            fan_percent: view.fan_percent,
        });
        let cards = crate::presenter::fan_cards(&state, fresh);
        self.panel
            .refresh(&view, self.pending_speed.map(|(percent, _)| percent));
        self.panel.refresh_dashboard(
            &view,
            &cards,
            &self.history,
            state.config.adaptive_tuning.bias,
            self.pending_panel_tuning.map(|(_, b)| b),
            self.pending_policy_save.is_some() || state.installing,
        );
        if self.window.isVisible() {
            self.overview.refresh(&view, &self.history);
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
            fresh,
            self.pending_fan_speed.map(|(id, percent, _)| (id, percent)),
        );
        if resized {
            self.overview.refresh(&view, &self.history);
        }
        if self.window.isVisible() {
            self.details.refresh(&state, fresh);
        }
        if self.window.isVisible() || resized {
            for (controller, page) in [
                (
                    &self.overview_controller,
                    Retained::into_super(self.overview.view.clone()),
                ),
                (
                    &self.fans_controller,
                    Retained::into_super(self.fans.view.clone()),
                ),
                (
                    &self.details_controller,
                    Retained::into_super(self.details.view.clone()),
                ),
                (
                    &self.settings_controller,
                    Retained::into_super(self.settings.view.clone()),
                ),
            ] {
                crate::form::refit(controller, &page);
            }
            if let Some(policy) = self.policy.as_ref() {
                crate::form::refit(&self.smart_controller, &policy.view);
            }
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
        let temperature = |value: Option<f64>| {
            if fresh {
                value
                    .map(|value| format!("{value:.1}°C"))
                    .unwrap_or_else(|| "暂不可用".into())
            } else {
                "数据已过期".into()
            }
        };
        let cpu = state.snapshot.input_value("Average CPU", &state.thermal);
        if let Some(editor) = self.editor.as_mut() {
            if !editor.chart.is_dragging() {
                editor.drag_recorded = false;
                if let Ok(draft) = editor.read_draft() {
                    if let Some(curve) = draft.curve {
                        editor.chart.set_current(if fresh {
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
        if let Some(button) = self
            .status
            .button(MainThreadMarker::new().expect("UI main thread"))
        {
            button.setToolTip(Some(&text(&format!(
                "Fan Control · CPU {} · {}",
                temperature(cpu),
                state.message
            ))));
            let fastest = state
                .snapshot
                .fans
                .iter()
                .filter_map(|fan| fan.current_rpm)
                .fold(0., f64::max);
            button.setAlphaValue(if fresh && fastest >= 100. { 1. } else { 0.6 });
        }
    }
    /// Applies one fan's choice from the Fans page.
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
        let mut undo = if draft.is_some() {
            self.editor
                .as_ref()
                .map(|editor| editor.undo.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let all_fans = self
            .editor
            .as_ref()
            .is_none_or(|editor| editor.all_fans.state() == NSControlStateValueOn);
        if record {
            if let Some(editor) = self.editor.as_ref() {
                let source = editor
                    .fan
                    .curve
                    .as_ref()
                    .map(|curve| curve.sensor_key.as_str());
                let previous = editor
                    .read_values(None, source)
                    .unwrap_or_else(|_| editor.fan.clone());
                if undo.last() != Some(&previous) {
                    undo.push(previous);
                }
                if undo.len() > 50 {
                    undo.remove(0);
                }
            }
        }
        let mut fan = draft.unwrap_or_else(|| {
            state
                .config
                .fans
                .iter()
                .find(|f| f.fan_id == id)
                .cloned()
                .unwrap_or_else(|| FanConfig::automatic(id))
        });
        let curve = fan
            .curve
            .get_or_insert_with(|| Curve::balanced("native-balanced", THERMAL_DEMAND_KEY))
            .clone();
        let frame = self.editor.as_ref().map(|editor| editor.window.frame());
        self.close_editor();
        let mtm = delegate.mtm();
        use tokens::MARGIN;
        let (width, height) = (EDITOR_WIDTH, EDITOR_HEIGHT);
        let inner = width - MARGIN * 2.;
        let window = window(mtm, "温度曲线", width, height);
        if let Some(frame) = frame {
            window.setFrameTopLeftPoint(objc2_foundation::NSPoint::new(
                frame.origin.x,
                frame.origin.y + frame.size.height,
            ));
        }
        let view = content(&window, mtm, width, height);
        let thermal = curve.sensor_key == THERMAL_DEMAND_KEY;
        label(
            &view,
            mtm,
            "温度曲线",
            rect(MARGIN, height - 46., inner, 28.),
            tokens::TITLE,
        );
        label(
            &view,
            mtm,
            "拖动图中的圆点，或在下方修改数值。点击“保存并应用”后生效。",
            rect(MARGIN, height - 70., inner, 20.),
            tokens::CAPTION,
        );
        let labels: Vec<_> = ["安静", "均衡", "凉爽"]
            .iter()
            .map(|name| text(name))
            .collect();
        let presets = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &objc2_foundation::NSArray::from_retained_slice(&labels),
                NSSegmentSwitchTracking::SelectOne,
                Some(delegate),
                Some(sel!(curvePreset:)),
                mtm,
            )
        };
        presets.setFrame(rect(MARGIN, height - 108., 240., 28.));
        presets.setSegmentDistribution(NSSegmentDistribution::FillEqually);
        presets.setAccessibilityLabel(Some(&text("曲线预设")));
        match crate::presenter::matching_preset(&curve) {
            Some(preset) => presets.setSelectedSegment(
                crate::presenter::CurvePreset::ALL
                    .iter()
                    .position(|p| *p == preset)
                    .unwrap_or(1) as isize,
            ),
            None => presets.setSelectedSegment(-1),
        }
        view.addSubview(&presets);
        label(
            &view,
            mtm,
            "迟滞",
            rect(width - MARGIN - 170., height - 103., 50., 22.),
            tokens::BODY,
        );
        let hysteresis = input(
            &view,
            mtm,
            &crate::presenter::format_value(curve.hysteresis),
            rect(width - MARGIN - 120., height - 107., 56., 26.),
        );
        label(
            &view,
            mtm,
            if thermal { "%" } else { "°C" },
            rect(width - MARGIN - 58., height - 103., 58., 22.),
            tokens::BODY,
        );
        hysteresis.setAccessibilityLabel(Some(&text(if thermal {
            "迟滞，热负荷百分点"
        } else {
            "迟滞，摄氏度"
        })));
        hysteresis.setToolTip(Some(&text("降温时延后减速的幅度，避免风扇来回变速。")));
        label(
            &view,
            mtm,
            "控制依据",
            rect(MARGIN, height - 144., 80., 22.),
            tokens::BODY,
        );
        let source = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(MARGIN + 84., height - 149., inner - 84., 30.),
            false,
        );
        let mut sources = vec![
            THERMAL_DEMAND_KEY.into(),
            "Average CPU".into(),
            "Hottest CPU".into(),
            "Average GPU".into(),
            "Hottest GPU".into(),
        ];
        sources.extend(state.snapshot.sensors.iter().map(|s| s.key.clone()));
        if !sources.contains(&curve.sensor_key) {
            sources.push(curve.sensor_key.clone());
        }
        for key in &sources {
            source.addItemWithTitle(&text(&source_title(key, &state.snapshot)));
        }
        if let Some(index) = sources.iter().position(|key| *key == curve.sensor_key) {
            source.selectItemAtIndex(index as isize);
        }
        source.setAccessibilityLabel(Some(&text("曲线控制源")));
        unsafe {
            source.setTarget(Some(delegate));
            source.setAction(Some(sel!(sourceChanged:)));
        }
        view.addSubview(&source);
        let chart_top = height - 162.;
        let chart_height = 170.;
        label(
            &view,
            mtm,
            "100%",
            rect(MARGIN, chart_top - 14., 40., 14.),
            tokens::CAPTION,
        );
        let chart = crate::chart::CurvePreview::new(
            mtm,
            rect(MARGIN, chart_top - chart_height, inner, chart_height),
            curve.clone(),
        );
        chart.set_target(delegate);
        chart.setAccessibilityLabel(Some(&text("曲线预览，可拖动控制点")));
        view.addSubview(&chart);
        let axis_y = chart_top - chart_height - 18.;
        label(
            &view,
            mtm,
            "0",
            rect(MARGIN + 12., axis_y, 40., 16.),
            tokens::CAPTION,
        );
        label(
            &view,
            mtm,
            if thermal {
                "热负荷 →"
            } else {
                "温度 →"
            },
            rect(width / 2. - 60., axis_y, 120., 16.),
            tokens::CAPTION,
        )
        .setAlignment(NSTextAlignment::Center);
        label(
            &view,
            mtm,
            if thermal { "100%" } else { "120°C" },
            rect(width - MARGIN - 52., axis_y, 40., 16.),
            tokens::CAPTION,
        )
        .setAlignment(NSTextAlignment::Right);
        label(
            &view,
            mtm,
            "灰色区域：交还系统，风扇可能停转 · 橙线：当前值",
            rect(MARGIN, axis_y - 20., inner, 16.),
            tokens::CAPTION,
        );
        let table_top = axis_y - 30.;
        label(
            &view,
            mtm,
            if thermal { "热负荷 %" } else { "温度 °C" },
            rect(MARGIN + 8., table_top - 16., 150., 16.),
            tokens::CAPTION,
        );
        label(
            &view,
            mtm,
            "风扇速度 %",
            rect(MARGIN + 176., table_top - 16., 150., 16.),
            tokens::CAPTION,
        );
        let table_height = table_top - 22. - 136.;
        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            rect(MARGIN, 136., inner, table_height),
        );
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        let document_height = (curve.points.len() as f64 * tokens::ROW).max(table_height);
        let document = NSView::initWithFrame(
            NSView::alloc(mtm),
            rect(0., 0., inner - 20., document_height),
        );
        scroll.setDocumentView(Some(&document));
        view.addSubview(&scroll);
        // Unflipped documents start at the bottom; show the first point instead.
        scroll
            .contentView()
            .scrollToPoint(objc2_foundation::NSPoint::new(
                0.,
                (document_height - table_height).max(0.),
            ));
        scroll.reflectScrolledClipView(&scroll.contentView());
        let mut rows = Vec::new();
        let mut points = curve.points.clone();
        points.sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
        for (i, point) in points.iter().enumerate() {
            let y = document_height - tokens::ROW - i as f64 * tokens::ROW;
            let x = input(
                &document,
                mtm,
                &crate::presenter::format_value(point.temperature),
                rect(8., y, 150., 25.),
            );
            let speed = input(
                &document,
                mtm,
                &crate::presenter::format_value(point.speed_percent),
                rect(176., y, 110., 25.),
            );
            x.setAccessibilityLabel(Some(&text(&format!(
                "控制点 {} 输入{}",
                i + 1,
                if thermal {
                    "热负荷百分比"
                } else {
                    "温度摄氏度"
                }
            ))));
            speed.setAccessibilityLabel(Some(&text(&format!("控制点 {} 速度百分比", i + 1))));
            let delete = button(
                &document,
                delegate,
                "删除",
                sel!(removePoint:),
                rect(300., y, 72., 26.),
            );
            delete.setTag(i as isize);
            delete.setAccessibilityLabel(Some(&text(&format!("删除控制点 {}", i + 1))));
            delete.setEnabled(points.len() > 2);
            rows.push((x, speed));
        }
        button(
            &view,
            delegate,
            "添加控制点",
            sel!(addPoint:),
            rect(MARGIN, 96., 130., 30.),
        )
        .setEnabled(rows.len() < 12);
        button(
            &view,
            delegate,
            "撤销",
            sel!(undoCurve:),
            rect(MARGIN + 138., 96., 90., 30.),
        )
        .setEnabled(!undo.is_empty());
        let all = button(
            &view,
            delegate,
            "同时应用到所有风扇",
            sel!(curveAllFans:),
            rect(width - MARGIN - 200., 98., 200., 26.),
        );
        all.setButtonType(NSButtonType::Switch);
        all.setState(if all_fans {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        let error = label(
            &view,
            mtm,
            "低于 0% 时交还系统；过热时安全保护始终生效。",
            rect(MARGIN, 52., inner, 36.),
            tokens::CAPTION,
        );
        let cancel = button(
            &view,
            delegate,
            "取消",
            sel!(cancelCurve:),
            rect(width - MARGIN - 250., 14., 100., 30.),
        );
        cancel.setKeyEquivalent(&raw_text("\u{1b}"));
        let save = button(
            &view,
            delegate,
            "保存并应用",
            sel!(saveCurve:),
            rect(width - MARGIN - 140., 14., 140., 30.),
        );
        save.setKeyEquivalent(&raw_text("\r"));
        self.editor = Some(Editor {
            window,
            fan,
            source,
            sources,
            hysteresis,
            rows,
            error,
            chart,
            undo,
            presets,
            all_fans: all,
            drag_recorded: false,
        });
        self.editor
            .as_ref()
            .unwrap()
            .window
            .makeKeyAndOrderFront(None);
    }
}
impl Editor {
    fn read(&self) -> Result<FanConfig, String> {
        let mut fan = self.read_draft()?;
        if let Some(curve) = fan.curve.as_mut() {
            curve
                .points
                .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
        }
        let config = Config {
            version: fan_core::CONFIG_VERSION,
            fans: vec![fan.clone()],
            ..Config::default()
        };
        config.validate().map_err(|e| e.to_string())?;
        Ok(fan)
    }
    fn read_draft(&self) -> Result<FanConfig, String> {
        self.read_values(None, None)
    }
    fn read_values(
        &self,
        skip: Option<usize>,
        source_override: Option<&str>,
    ) -> Result<FanConfig, String> {
        let mut fan = self.fan.clone();
        let curve = fan.curve.as_mut().ok_or("缺少曲线")?;
        let index = self.source.indexOfSelectedItem().max(0) as usize;
        let source = source_override
            .map(str::to_owned)
            .or_else(|| self.sources.get(index).cloned())
            .ok_or("选择控制源")?;
        // Switching input units requires a new preset rather than reinterpreting old coordinates.
        if (source == THERMAL_DEMAND_KEY) != (curve.sensor_key == THERMAL_DEMAND_KEY) {
            return Err("更换温度 / 热负荷单位前，请恢复对应默认曲线。".into());
        }
        curve.sensor_key = source;
        curve.hysteresis = self
            .hysteresis
            .stringValue()
            .to_string()
            .trim()
            .parse()
            .map_err(|_| "迟滞必须是数字")?;
        curve.points = self
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| skip != Some(*i))
            .map(|(_, (x, y))| {
                Ok(CurvePoint {
                    temperature: x
                        .stringValue()
                        .to_string()
                        .trim()
                        .parse()
                        .map_err(|_| "输入值必须是数字")?,
                    speed_percent: y
                        .stringValue()
                        .to_string()
                        .trim()
                        .parse()
                        .map_err(|_| "速度必须是数字")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        fan.mode = ControlMode::Curve {
            curve_id: curve.id.clone(),
        };
        Ok(fan)
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

fn source_title(key: &str, snapshot: &fan_core::Snapshot) -> String {
    match key {
        THERMAL_DEMAND_KEY => "热负荷 · 0–100%".into(),
        "Average CPU" => "CPU 平均温度 · °C".into(),
        "Hottest CPU" => "CPU 最高温度 · °C".into(),
        "Average GPU" => "GPU 平均温度 · °C".into(),
        "Hottest GPU" => "GPU 最高温度 · °C".into(),
        _ => snapshot
            .sensors
            .iter()
            .find(|sensor| sensor.key == key)
            .map(|sensor| format!("{} · {} · °C", sensor.name, key))
            .unwrap_or_else(|| format!("{key} · 暂不可用")),
    }
}
