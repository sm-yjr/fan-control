//! Explicit isolated diagnostic. No Worker, SMC, helper, updater, user configuration or instance lock.
use crate::{
    presenter::{fan_cards, present},
    worker::UiSnapshot,
};
use fan_core::{
    Config, ControlMode, Fan, FanConfig, HardwareMode, Sensor, SensorGroup, Snapshot,
    ThermalPressure, ThermalReading,
};
use objc2::{MainThreadOnly, Message};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSDictionary, NSObject, NSPoint, NSRect};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};
fn fixture(count: usize, missing: bool, bias: i8) -> UiSnapshot {
    let fans = (0..count)
        .map(|id| Fan {
            id: id as u8,
            name: if count == 1 {
                "Mac mini Fan".into()
            } else {
                format!("Cooling Fan {}", id + 1)
            },
            min_rpm: if missing {
                None
            } else {
                Some(1200. + id as f64 * 300.)
            },
            max_rpm: if missing {
                None
            } else {
                Some(5000. + id as f64 * 700.)
            },
            current_rpm: if missing {
                None
            } else {
                Some(3083. + id as f64 * 7.)
            },
            mode: HardwareMode::Forced,
        })
        .collect();
    let config = Config {
        fans: (0..count)
            .map(|id| FanConfig {
                fan_id: id as u8,
                mode: ControlMode::Adaptive,
                curve: None,
            })
            .collect(),
        adaptive_tuning: fan_core::AdaptiveTuning { bias },
        ..Config::default()
    };
    UiSnapshot {
        snapshot: Snapshot {
            sampled_at: 0.,
            fan_count: Some(count as f64),
            thermal_pressure: ThermalPressure::Nominal,
            cpu_utilization_percent: Some(18.),
            machine_id: None,
            fans,
            sensors: vec![Sensor {
                key: "fixture-cpu".into(),
                name: "Simulated CPU".into(),
                group: SensorGroup::Cpu,
                value: if missing { None } else { Some(55.) },
            }],
        },
        thermal: ThermalReading::default(),
        config,
        helper_ready: true,
        installing: false,
        discovered: true,
        message: String::new(),
        fan_status: BTreeMap::new(),
        battery: None,
        configuration_notice: String::new(),
        safety_active: false,
        handback_pending: false,
        targets: BTreeMap::new(),
        confirmed_targets: BTreeMap::new(),
        sample_age_secs: 0.,
        published_at: Instant::now(),
    }
}
fn unlayer(view: &NSView) {
    view.setWantsLayer(false);
    for child in view.subviews() {
        unlayer(&child);
    }
}
pub fn run() {
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let target = NSObject::new();
    let output = std::env::var_os("FAN_CONTROL_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/fan-control-dashboard-render"));
    std::fs::create_dir_all(&output).expect("diagnostic output directory");
    let mut reports = Vec::new();
    for (language, lang) in [
        ("zh", crate::i18n::Lang::Chinese),
        ("en", crate::i18n::Lang::English),
    ] {
        for (theme, appearance) in [
            ("light", unsafe { NSAppearanceNameAqua }),
            ("dark", unsafe { NSAppearanceNameDarkAqua }),
        ] {
            for width in [400., 340.] {
                for (scene, count, missing) in [
                    ("single", 1, false),
                    ("dual", 2, false),
                    ("empty", 0, true),
                    ("missing", 1, true),
                    ("stale", 2, false),
                    ("stopped", 1, false),
                    ("unknown-range", 1, false),
                    ("hot", 2, false),
                ] {
                    crate::i18n::set_override(lang);
                    let mut panel = crate::popover::MenuPanel::with_width(mtm, &target, width);
                    let mut state = fixture(count, missing, 0);
                    if scene == "stopped" {
                        state.snapshot.fans[0].current_rpm = Some(0.);
                        state.snapshot.fans[0].mode = HardwareMode::Automatic;
                    }
                    if scene == "unknown-range" {
                        state.snapshot.fans[0].min_rpm = None;
                        state.snapshot.fans[0].max_rpm = None;
                    }
                    if scene == "hot" {
                        state.snapshot.sensors[0].value = Some(100.);
                        state.snapshot.thermal_pressure = ThermalPressure::Serious;
                        state.safety_active = true;
                    }
                    let fresh = scene != "stale";
                    let presentation = present(&state, fresh, false);
                    let cards = fan_cards(&state, fresh);
                    let now = Instant::now();
                    let mut history = crate::trend::History::default();
                    if !missing {
                        for age in (0..121).rev().map(|i| i * 5u64) {
                            history.record(crate::trend::Sample {
                                at: now - Duration::from_secs(age),
                                temperature: if scene == "stale" && age < 30 {
                                    None
                                } else {
                                    Some(if scene == "hot" {
                                        100. + (age as f64 / 42.).sin() * 1.3
                                    } else {
                                        55. + (age as f64 / 42.).sin() * 1.3
                                    })
                                },
                                fan_percent: None,
                            });
                        }
                    }
                    panel.mark_diagnostic();
                    panel.refresh(&presentation, None);
                    panel.refresh_dashboard(&presentation, &cards, &history, 0, None, false);
                    let view = panel.diagnostic_view().retain();
                    let appearance =
                        NSAppearance::appearanceNamed(appearance).expect("system appearance");
                    view.setAppearance(Some(&appearance));
                    let size = panel.popover.contentSize();
                    let window = unsafe {
                        NSWindow::initWithContentRect_styleMask_backing_defer(
                            NSWindow::alloc(mtm),
                            NSRect::new(NSPoint::new(0., 0.), size),
                            NSWindowStyleMask::Borderless,
                            NSBackingStoreType::Buffered,
                            false,
                        )
                    };
                    unsafe {
                        window.setReleasedWhenClosed(false);
                    }
                    window.setContentView(Some(&view));
                    window.setAppearance(Some(&appearance));
                    view.setFrame(NSRect::new(NSPoint::new(0., 0.), size));
                    view.layoutSubtreeIfNeeded();
                    window.displayIfNeeded();
                    panel.verify_layout(&cards);
                    // Exercise the full discrete preference range in the actual production controls.
                    for bias in -10..=10 {
                        panel.refresh_dashboard(&presentation, &cards, &history, bias, None, false);
                        view.layoutSubtreeIfNeeded();
                        panel.verify_layout(&cards);
                        assert_eq!(panel.preference.doubleValue(), bias as f64);
                    }
                    panel.refresh_dashboard(&presentation, &cards, &history, 0, None, false);
                    view.layoutSubtreeIfNeeded();
                    panel.refresh_dashboard(&presentation, &cards, &history, 0, Some(10), true);
                    assert!(!panel.preference.isEnabled());
                    assert_eq!(panel.preference.doubleValue(), 10.);
                    panel.preference_result(Err("simulated disk full".into()));
                    panel.refresh_dashboard(&presentation, &cards, &history, 0, None, false);
                    assert_eq!(panel.preference.doubleValue(), 0.);
                    assert!(panel.preference.isEnabled());
                    panel.clear_diagnostic_notice();
                    view.layoutSubtreeIfNeeded();
                    panel.verify_layout(&cards);
                    let final_size = panel.popover.contentSize();
                    window.setContentSize(final_size);
                    view.setFrame(NSRect::new(NSPoint::new(0., 0.), final_size));
                    view.layoutSubtreeIfNeeded();
                    let name = format!("{scene}-{theme}-{width:.0}-{language}");
                    unlayer(&view);
                    let bitmap = view
                        .bitmapImageRepForCachingDisplayInRect(view.bounds())
                        .expect("native bitmap");
                    appearance.performAsCurrentDrawingAppearance(&block2::RcBlock::new(|| {
                        if let Some(context) =
                            NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)
                        {
                            NSGraphicsContext::saveGraphicsState_class();
                            NSGraphicsContext::setCurrentContext(Some(&context));
                            NSColor::windowBackgroundColor().setFill();
                            NSBezierPath::bezierPathWithRect(view.bounds()).fill();
                            view.displayRectIgnoringOpacity_inContext(view.bounds(), &context);
                            NSGraphicsContext::restoreGraphicsState_class();
                        }
                    }));
                    let data = unsafe {
                        bitmap.representationUsingType_properties(
                            NSBitmapImageFileType::PNG,
                            &NSDictionary::new(),
                        )
                    }
                    .expect("PNG data");
                    let file = output.join(format!("{name}.png"));
                    std::fs::write(&file, data.to_vec()).expect("write PNG");
                    // PDF follows the same native view hierarchy and helps diagnose compositor limitations.
                    let pdf = view.dataWithPDFInsideRect(view.bounds());
                    std::fs::write(output.join(format!("{name}.pdf")), pdf.to_vec())
                        .expect("write PDF");
                    reports.push(serde_json::json!({"scene":scene,"theme":theme,"language":language,"width":width,"height":final_size.height,"fan_count":cards.len(),"all_21_preference_positions":true,"native_geometry_passed":true,"image":file,"fixture_data":true}));
                    // Diagnostic windows never ordered front or activated.
                }
            }
        }
    }
    let result = serde_json::json!({"hardware_access":false,"user_configuration_access":false,"desktop_activated":false,"scenes":reports});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .expect("write report");
    println!("Native dashboard rendering completed: {}", output.display());
}
