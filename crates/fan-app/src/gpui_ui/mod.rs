//! GPUI + macOS Liquid Glass application lifecycle, thin AppKit `NSStatusItem` bridge,
//! floating menu bar panel, main split-view window, `--ui-smoke`, and `--dashboard-render`.
pub mod charts;
pub mod curve_editor;
pub mod glass;
pub mod main_window;
pub mod popover;
pub mod primitives;
pub mod state;
pub mod theme;

use self::{
    glass::GlassStyle,
    main_window::{MainWindowView, MAIN_HEIGHT, MAIN_WIDTH},
    popover::{PopoverView, POPOVER_HEIGHT, POPOVER_WIDTH},
    state::{AppState, Section},
};
use crate::status_bar::{MenuBarDisplayMode, StatusBarController};
use gpui::{
    px, size, App, AppContext, AssetSource, Bounds, Entity, Result as GpuiResult, SharedString,
    TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions,
};
use gpui_component::{Root, Theme};
use objc2::{define_class, msg_send, rc::Retained, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSUserDefaults};
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

gpui_kit_assets::icon_assets!(
    FanIconAssets,
    [
        Check,
        ChevronLeft,
        ChevronRight,
        ChevronsUpDown,
        CircleCheck,
        Fan,
        Flame,
        Gauge,
        Info,
        LayoutGrid,
        Power,
        RotateCcw,
        Settings,
        SlidersHorizontal,
        Sparkles,
        Thermometer,
    ]
);

/// Combines Fan Control's embedded Lucide icons with GPUI Kit's default component assets.
struct CombinedAssets;

impl AssetSource for CombinedAssets {
    fn load(&self, path: &str) -> GpuiResult<Option<Cow<'static, [u8]>>> {
        if let Some(found) = FanIconAssets.load(path)? {
            return Ok(Some(found));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> GpuiResult<Vec<SharedString>> {
        let mut items = FanIconAssets.list(path)?;
        items.extend(gpui_kit_assets::Assets.list(path)?);
        Ok(items)
    }
}

const ONBOARDED_KEY: &str = "FanControlPanelIntroduced";

struct StatusBridgeIvars {
    tx: mpsc::Sender<()>,
}

define_class!(
    /// Minimal Objective-C target that forwards `NSStatusItem` clicks to the GPUI loop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = StatusBridgeIvars]
    struct StatusBarTarget;
    unsafe impl NSObjectProtocol for StatusBarTarget {}
    impl StatusBarTarget {
        #[unsafe(method(togglePanel:))]
        fn toggle_panel(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            let _ = self.ivars().tx.send(());
        }
    }
);

impl StatusBarTarget {
    fn new(mtm: MainThreadMarker, tx: mpsc::Sender<()>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(StatusBridgeIvars { tx });
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Default)]
struct WindowHandles {
    main: Option<WindowHandle<Root>>,
    popover: Option<WindowHandle<Root>>,
    popover_shown: bool,
}

fn load_language(demo: bool) {
    let language = if demo {
        std::env::var("FAN_CONTROL_DEMO_LANGUAGE")
            .ok()
            .and_then(|v| crate::i18n::Lang::from_preference_key(&v))
            .unwrap_or_default()
    } else {
        NSUserDefaults::standardUserDefaults()
            .stringForKey(&crate::i18n::raw_nsstring("FanControlLanguage"))
            .and_then(|v| crate::i18n::Lang::from_preference_key(&v.to_string()))
            .unwrap_or_default()
    };
    crate::i18n::set_override(language);
}

fn open_or_show_main(
    state: &Entity<AppState>,
    handles: &Rc<RefCell<WindowHandles>>,
    section: Section,
    cx: &mut App,
) {
    state.update(cx, |s, cx| s.navigate(section, cx));
    if let Some(existing) = handles.borrow().main {
        let updated = cx.update_window(existing.into(), |_, window, _| {
            glass::activate_app();
            glass::order_front_window(window);
        });
        if updated.is_ok() {
            return;
        }
        handles.borrow_mut().main = None;
    }

    let bounds = Bounds::centered(
        None,
        size(px(MAIN_WIDTH as f32), px(MAIN_HEIGHT as f32)),
        cx,
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Fan Control".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(16.0), px(16.0))),
        }),
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: Some(size(px(720.0), px(520.0))),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let state_clone = state.clone();
    if let Ok(handle) = cx.open_window(options, move |window, cx| {
        Theme::sync_system_appearance(Some(window), cx);
        glass::attach_liquid_glass(window, 16.0, GlassStyle::Regular);
        glass::activate_app();
        let view = cx.new(|cx| MainWindowView::new(state_clone, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        handles.borrow_mut().main = Some(handle);
    }
}

fn hide_popover(handles: &Rc<RefCell<WindowHandles>>, cx: &mut App) {
    handles.borrow_mut().popover_shown = false;
    if let Some(existing) = handles.borrow().popover {
        let _ = cx.update_window(existing.into(), |_, window, _| {
            glass::order_out_window(window);
        });
    }
}

fn toggle_popover(state: &Entity<AppState>, handles: &Rc<RefCell<WindowHandles>>, cx: &mut App) {
    let currently_shown = handles.borrow().popover_shown;
    if currently_shown {
        hide_popover(handles, cx);
        return;
    }

    let anchor_rect = state
        .read(cx)
        .status_bar
        .as_ref()
        .and_then(|bar| glass::status_item_screen_rect(&bar.item));

    if let Some(existing) = handles.borrow().popover {
        let updated = cx.update_window(existing.into(), |_, window, _| {
            glass::position_popover_window(window, anchor_rect, POPOVER_WIDTH, POPOVER_HEIGHT);
            glass::activate_app();
            glass::order_front_window(window);
        });
        if updated.is_ok() {
            handles.borrow_mut().popover_shown = true;
            return;
        }
        handles.borrow_mut().popover = None;
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(POPOVER_WIDTH as f32), px(POPOVER_HEIGHT as f32)),
            cx,
        ))),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_resizable: false,
        ..Default::default()
    };

    let state_clone = state.clone();
    let state_open = state.clone();
    let handles_open = handles.clone();
    let handles_hide = handles.clone();
    if let Ok(handle) = cx.open_window(options, move |window, cx| {
        Theme::sync_system_appearance(Some(window), cx);
        glass::configure_popover_window(window, anchor_rect, POPOVER_WIDTH, POPOVER_HEIGHT);
        glass::activate_app();
        let view = cx.new(|cx| {
            PopoverView::new(
                state_clone,
                move |section, cx| {
                    open_or_show_main(&state_open, &handles_open, section, cx);
                },
                move |cx| {
                    hide_popover(&handles_hide, cx);
                },
                cx,
            )
        });
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        handles.borrow_mut().popover = Some(handle);
        handles.borrow_mut().popover_shown = true;
    }
}

/// Primary GPUI 0.3.14 + GPUI Kit 0.11 + macOS Liquid Glass entry point.
pub fn run(demo: bool, smoke: bool) {
    load_language(demo);

    let (click_tx, click_rx) = mpsc::channel::<()>();
    let url_open_flag = Rc::new(RefCell::new(false));
    let url_flag_cb = url_open_flag.clone();

    let app = gpui_platform::application().with_assets(CombinedAssets);
    app.on_open_urls(move |urls| {
        if urls.iter().any(|u| u == "fancontrol://dashboard") {
            *url_flag_cb.borrow_mut() = true;
        }
    });

    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        // Stay a lightweight menu-bar accessory app (no Dock icon).
        glass::set_accessory_policy();

        let (status_bar, _bridge_guard) = if !smoke {
            if let Some(mtm) = MainThreadMarker::new() {
                let target = StatusBarTarget::new(mtm, click_tx.clone());
                let bar = StatusBarController::new(mtm, &target, demo);
                (Some(bar), Some(target))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let state = cx.new(|_| AppState::new(demo, status_bar));
        let handles = Rc::new(RefCell::new(WindowHandles::default()));

        // Safe fan handback on application quit.
        let quit_state = state.clone();
        cx.on_app_quit(move |cx| {
            quit_state.read(cx).shutdown_safely();
            async {}
        })
        .detach();

        if demo && !smoke {
            open_or_show_main(&state, &handles, Section::Settings, cx);
        } else if !smoke && !glass::get_bool_default(ONBOARDED_KEY) {
            glass::set_bool_default(ONBOARDED_KEY, true);
            toggle_popover(&state, &handles, cx);
        }

        // Smoke test mode: open main window + popover in isolated demo mode, exercise all
        // sections, curve editor, tuning preference, and menu bar modes, then exit cleanly.
        if smoke {
            let smoke_state = state.clone();
            let smoke_handles = handles.clone();
            cx.spawn(async move |cx| {
                // Allow worker demo discovery to populate fans.
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                cx.update(|cx| {
                    smoke_state.update(cx, |s, cx| s.poll_tick(true, cx));
                    open_or_show_main(&smoke_state, &smoke_handles, Section::Overview, cx);
                    toggle_popover(&smoke_state, &smoke_handles, cx);
                });
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                cx.update(|cx| {
                    // Verify all 5 sidebar sections + Back/Forward history navigation.
                    for section in Section::ALL {
                        smoke_state.update(cx, |s, cx| s.navigate(section, cx));
                    }
                    smoke_state.update(cx, |s, cx| {
                        s.go_back(cx);
                        assert_eq!(s.section, Section::Details);
                        s.go_forward(cx);
                        assert_eq!(s.section, Section::Settings);
                    });

                    // Verify curve editor open, add point, undo, close.
                    smoke_state.update(cx, |s, cx| {
                        s.open_curve_editor(None, cx);
                        let before = s.curve_editor.as_ref().unwrap().curve.points.len();
                        s.curve_add_point(cx);
                        assert_eq!(
                            s.curve_editor.as_ref().unwrap().curve.points.len(),
                            before + 1
                        );
                        s.curve_undo(cx);
                        assert_eq!(s.curve_editor.as_ref().unwrap().curve.points.len(), before);
                        s.close_curve_editor(cx);
                    });

                    // Verify all 4 menu bar modes.
                    for mode in MenuBarDisplayMode::ALL {
                        smoke_state.update(cx, |s, cx| s.set_menu_bar_mode(mode, cx));
                    }

                    // Verify sensor group filters & sort orders.
                    for grp in crate::sensor_list::GroupFilter::ALL {
                        smoke_state.update(cx, |s, cx| {
                            s.sensor_group = grp;
                            cx.notify();
                        });
                    }
                    println!(
                        "GPUI + Liquid Glass UI verified in isolated demo mode (native NSGlassEffectView={})",
                        glass::native_liquid_glass_available()
                    );
                    cx.quit();
                });
            })
            .detach();
            return;
        }

        // Main 500ms telemetry + status-item click + URL deep-link loop.
        let tick_state = state.clone();
        let tick_handles = handles.clone();
        let tick_url = url_open_flag.clone();
        cx.spawn(async move |cx| {
            // Keep the Objective-C status-bar click bridge alive for the entire app lifetime.
            let _keep_bridge = _bridge_guard;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                if crate::TERMINATE_REQUEST.load(std::sync::atomic::Ordering::SeqCst) {
                    cx.update(|cx| cx.quit());
                    break;
                }
                let clicked = click_rx.try_recv().is_ok();
                while click_rx.try_recv().is_ok() {}
                let url_open = {
                    let mut f = tick_url.borrow_mut();
                    let v = *f;
                    *f = false;
                    v
                };
                cx.update(|cx| {
                    if clicked {
                        toggle_popover(&tick_state, &tick_handles, cx);
                    }
                    if url_open {
                        hide_popover(&tick_handles, cx);
                        open_or_show_main(&tick_state, &tick_handles, Section::Overview, cx);
                    }
                    let main_vis = tick_handles
                        .borrow()
                        .main
                        .and_then(|h| {
                            cx.update_window(h.into(), |_, w, _| glass::is_window_visible(w))
                                .ok()
                        })
                        .unwrap_or(false);
                    let pop_vis = tick_handles.borrow().popover_shown;
                    tick_state.update(cx, |s, cx| s.poll_tick(main_vis || pop_vis, cx));
                });
            }
        })
        .detach();
    });
}

/// Renders GPUI + Liquid Glass screenshots (Light & Dark Settings + Overview + Popover)
/// into `FAN_CONTROL_RENDER_DIR` for automated visual inspection.
pub fn render_gpui_previews() {
    crate::i18n::set_override(crate::i18n::Lang::Chinese);
    let output = std::env::var_os("FAN_CONTROL_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/fan-control-gpui-render"));
    std::fs::create_dir_all(&output).expect("render directory");

    gpui_platform::application()
        .with_assets(CombinedAssets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            glass::set_accessory_policy();
            let state = cx.new(|_| AppState::new(true, None));
            let handles = Rc::new(RefCell::new(WindowHandles::default()));
            let out_dir = output.clone();

            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(350))
                    .await;
                cx.update(|cx| {
                    state.update(cx, |s, cx| {
                        s.poll_tick(true, cx);
                        // Seed synthetic 10-minute trend history for realistic charts.
                        let now = std::time::Instant::now();
                        for age in (0..60).rev().map(|i| i * 10u64) {
                            s.history.record(crate::trend::Sample {
                                at: now - Duration::from_secs(age),
                                temperature: Some(54.0 + (age as f64 / 38.0).sin() * 2.2),
                                fan_percent: Some(42.0),
                            });
                        }
                        s.set_menu_bar_mode(MenuBarDisplayMode::Temperature, cx);
                    });
                    open_or_show_main(&state, &handles, Section::Settings, cx);
                    toggle_popover(&state, &handles, cx);
                });

                // Wait for GPUI Metal first frame.
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;

                for (theme_name, dark) in [("light", false), ("dark", true)] {
                    cx.update(|cx| {
                        if let Some(h) = handles.borrow().main {
                            let _ = cx.update_window(h.into(), |_, w, _| {
                                glass::set_window_appearance(w, dark);
                                w.refresh();
                            });
                        }
                        if let Some(h) = handles.borrow().popover {
                            let _ = cx.update_window(h.into(), |_, w, _| {
                                glass::set_window_appearance(w, dark);
                                w.refresh();
                            });
                        }
                    });
                    cx.background_executor()
                        .timer(Duration::from_millis(220))
                        .await;

                    for (sec_name, sec) in [
                        ("settings", Section::Settings),
                        ("overview", Section::Overview),
                    ] {
                        cx.update(|cx| {
                            state.update(cx, |s, cx| s.navigate(sec, cx));
                        });
                        cx.background_executor()
                            .timer(Duration::from_millis(160))
                            .await;
                        cx.update(|cx| {
                            if let Some(h) = handles.borrow().main {
                                let path =
                                    out_dir.join(format!("gpui-{sec_name}-{theme_name}.png"));
                                let _ = cx.update_window(h.into(), |_, w, _| {
                                    let _ = glass::capture_window_png(w, &path);
                                });
                            }
                        });
                    }

                    cx.update(|cx| {
                        if let Some(h) = handles.borrow().popover {
                            let path = out_dir.join(format!("gpui-popover-{theme_name}.png"));
                            let _ = cx.update_window(h.into(), |_, w, _| {
                                let _ = glass::capture_window_png(w, &path);
                            });
                        }
                    });
                }

                cx.update(|cx| {
                    println!(
                        "GPUI Liquid Glass previews rendered to {}",
                        out_dir.display()
                    );
                    cx.quit();
                });
            })
            .detach();
        });
}
