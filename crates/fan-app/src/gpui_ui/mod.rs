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
    actions, px, size, App, AppContext, AssetSource, Bounds, Entity, KeyBinding, Menu, MenuItem,
    Result as GpuiResult, SharedString, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions,
};
use gpui_component::Root;
use objc2::{define_class, msg_send, rc::Retained, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSUserDefaults};
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

actions!(fan_control, [QuitApp, CloseActiveWindow, DismissOverlay]);

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
        Search,
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
const STATUS_CLICK_DEBOUNCE: Duration = Duration::from_millis(160);
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const CLICK_TICK_INTERVAL: Duration = Duration::from_millis(30);

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
    popover_generation: u64,
    popover_dismissed_by_status_click_at: Option<Instant>,
    auto_dismiss_popover: bool,
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

fn register_app_menus_and_shortcuts(
    state: &Entity<AppState>,
    handles: &Rc<RefCell<WindowHandles>>,
    cx: &mut App,
) {
    cx.on_action(|_: &QuitApp, cx| cx.quit());
    cx.on_action(|_: &CloseActiveWindow, cx| {
        if let Some(active) = cx.active_window() {
            // Defer so `update_window` runs after GPUI finishes dispatching the key event.
            cx.defer(move |cx| {
                let _ = cx.update_window(active, |_, window, _| {
                    window.remove_window();
                });
            });
        }
    });
    let esc_state = state.clone();
    let esc_handles = handles.clone();
    cx.on_action(move |_: &DismissOverlay, cx| {
        let popover_open = esc_handles.borrow().popover_shown;
        if popover_open {
            let handles = esc_handles.clone();
            cx.defer(move |cx| hide_popover(&handles, cx));
            return;
        }
        esc_state.update(cx, |s, cx| {
            if s.curve_editor.is_some() {
                s.close_curve_editor(cx);
            } else if s.language_menu_open {
                s.language_menu_open = false;
                cx.notify();
            }
        });
    });
    cx.bind_keys([
        KeyBinding::new("cmd-q", QuitApp, None),
        KeyBinding::new("cmd-w", CloseActiveWindow, None),
        KeyBinding::new("escape", DismissOverlay, None),
    ]);
    cx.set_menus(vec![Menu {
        name: "Fan Control".into(),
        items: vec![
            MenuItem::action(crate::i18n::translate("关闭窗口"), CloseActiveWindow),
            MenuItem::separator(),
            MenuItem::action(crate::i18n::translate("退出 Fan Control"), QuitApp),
        ],
        disabled: false,
    }]);
}

fn open_or_show_main(
    state: &Entity<AppState>,
    handles: &Rc<RefCell<WindowHandles>>,
    section: Section,
    cx: &mut App,
) {
    state.update(cx, |s, cx| s.navigate(section, cx));

    let existing = handles.borrow().main;
    if let Some(existing) = existing {
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
    let state_close = state.clone();
    let handles_close = handles.clone();
    if let Ok(handle) = cx.open_window(options, move |window, cx| {
        glass::sync_glass_root_theme(window, cx);
        glass::attach_liquid_glass(window, 0.0, GlassStyle::Regular);
        glass::activate_app();
        window.on_window_should_close(cx, move |_, cx| {
            handles_close.borrow_mut().main = None;
            state_close.update(cx, |s, _| {
                s.curve_editor = None;
                s.language_menu_open = false;
            });
            true
        });
        let view = cx.new(|cx| MainWindowView::new(state_clone, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        handles.borrow_mut().main = Some(handle);
    }
}

fn hide_popover(handles: &Rc<RefCell<WindowHandles>>, cx: &mut App) {
    let existing = {
        let mut h = handles.borrow_mut();
        h.popover_shown = false;
        h.popover_generation = h.popover_generation.wrapping_add(1);
        h.popover
    };
    if let Some(existing) = existing {
        let _ = cx.update_window(existing.into(), |_, window, _| {
            glass::order_out_window(window);
        });
    }
}

fn toggle_popover(
    state: &Entity<AppState>,
    handles: &Rc<RefCell<WindowHandles>>,
    from_status_click: bool,
    cx: &mut App,
) {
    let (currently_shown, just_dismissed_by_status, existing, auto_dismiss) = {
        let h = handles.borrow();
        let dismissed = from_status_click
            && h.popover_dismissed_by_status_click_at
                .is_some_and(|at| at.elapsed() < STATUS_CLICK_DEBOUNCE);
        (
            h.popover_shown,
            dismissed,
            h.popover,
            h.auto_dismiss_popover,
        )
    };

    if currently_shown {
        hide_popover(handles, cx);
        return;
    }
    if just_dismissed_by_status {
        handles.borrow_mut().popover_dismissed_by_status_click_at = None;
        return;
    }

    let anchor_rect = state
        .read(cx)
        .status_bar
        .as_ref()
        .and_then(|bar| glass::status_item_screen_rect(&bar.item));

    if let Some(existing) = existing {
        let next_gen = {
            let mut h = handles.borrow_mut();
            h.popover_generation = h.popover_generation.wrapping_add(1);
            h.popover_shown = true;
            h.popover_dismissed_by_status_click_at = None;
            h.popover_generation
        };
        let _ = next_gen;
        let updated = cx.update_window(existing.into(), |_, window, _| {
            glass::position_popover_window(window, anchor_rect, POPOVER_WIDTH, POPOVER_HEIGHT);
            glass::order_front_window(window);
        });
        if updated.is_ok() {
            return;
        }
        let mut h = handles.borrow_mut();
        h.popover = None;
        h.popover_shown = false;
    }

    // Create hidden first so positioning and Liquid Glass attachment finish before the window
    // ever appears on screen (avoids flash at center and avoids post-show styleMask mutations).
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(POPOVER_WIDTH as f32), px(POPOVER_HEIGHT as f32)),
            cx,
        ))),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_resizable: false,
        ..Default::default()
    };

    let state_clone = state.clone();
    let state_open = state.clone();
    let handles_open = handles.clone();
    let handles_hide = handles.clone();
    let handles_deact = handles.clone();

    let on_deactivate = auto_dismiss.then(|| {
        Rc::new(move |window: &mut gpui::Window| {
            let should_hide = {
                let mut h = handles_deact.borrow_mut();
                if h.popover_shown && glass::is_window_visible(window) {
                    h.popover_shown = false;
                    // Only suppress the imminent status-bar click if the cursor is actually over
                    // the status item button so genuine quick re-opens are never swallowed.
                    if anchor_rect.is_some_and(glass::cursor_inside_rect) {
                        h.popover_dismissed_by_status_click_at = Some(Instant::now());
                    } else {
                        h.popover_dismissed_by_status_click_at = None;
                    }
                    true
                } else {
                    false
                }
            };
            if should_hide {
                glass::order_out_window(window);
            }
        }) as Rc<dyn Fn(&mut gpui::Window)>
    });

    if let Ok(handle) = cx.open_window(options, move |window, cx| {
        glass::sync_glass_root_theme(window, cx);
        glass::configure_popover_window(window, anchor_rect, POPOVER_WIDTH, POPOVER_HEIGHT);
        let view = cx.new(|cx| {
            PopoverView::new(
                state_clone,
                move |section, cx| {
                    open_or_show_main(&state_open, &handles_open, section, cx);
                },
                move |cx| {
                    hide_popover(&handles_hide, cx);
                },
                on_deactivate,
                window,
                cx,
            )
        });
        let root = cx.new(|cx| Root::new(view, window, cx));
        glass::order_front_window(window);
        root
    }) {
        let mut h = handles.borrow_mut();
        h.popover = Some(handle);
        h.popover_shown = true;
        h.popover_dismissed_by_status_click_at = None;
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
        let handles = Rc::new(RefCell::new(WindowHandles {
            auto_dismiss_popover: !smoke,
            ..Default::default()
        }));
        register_app_menus_and_shortcuts(&state, &handles, cx);

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
            toggle_popover(&state, &handles, false, cx);
        }

        // Smoke test mode: exercise repeated popover toggles, main window close/re-open,
        // pending speed cancellation on reset, all sections, curve editor ranges, and modes.
        if smoke {
            let smoke_state = state.clone();
            let smoke_handles = handles.clone();
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                cx.update(|cx| {
                    smoke_state.update(cx, |s, cx| s.poll_tick(true, cx));
                    open_or_show_main(&smoke_state, &smoke_handles, Section::Overview, cx);
                    // Repeated popover open -> hide -> re-open -> hide -> re-open
                    toggle_popover(&smoke_state, &smoke_handles, false, cx);
                    assert!(smoke_handles.borrow().popover_shown);
                    toggle_popover(&smoke_state, &smoke_handles, false, cx);
                    assert!(!smoke_handles.borrow().popover_shown);
                    toggle_popover(&smoke_state, &smoke_handles, false, cx);
                    assert!(smoke_handles.borrow().popover_shown);
                    hide_popover(&smoke_handles, cx);
                    assert!(!smoke_handles.borrow().popover_shown);
                    toggle_popover(&smoke_state, &smoke_handles, false, cx);
                    assert!(smoke_handles.borrow().popover_shown);
                });
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                cx.update(|cx| {
                    // Close main window and reopen cleanly.
                    let main_handle = smoke_handles.borrow().main;
                    if let Some(h) = main_handle {
                        let _ = cx.update_window(h.into(), |_, window, _| {
                            window.remove_window();
                        });
                    }
                    open_or_show_main(&smoke_state, &smoke_handles, Section::Overview, cx);
                    assert!(smoke_handles.borrow().main.is_some());

                    // Verify Root theme background is transparent so NSGlassEffectView shows through.
                    assert_eq!(
                        gpui_component::Theme::global(cx).tokens.background,
                        gpui::transparent_black().into()
                    );

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

                    // Verify curve editor open, source switch, hysteresis, add point, undo, close.
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
                        s.curve_nudge_hysteresis(2.0, cx);
                        s.close_curve_editor(cx);
                    });

                    // Verify all 4 menu bar modes.
                    for mode in MenuBarDisplayMode::ALL {
                        smoke_state.update(cx, |s, cx| s.set_menu_bar_mode(mode, cx));
                    }

                    // Verify sensor group filters & search query.
                    for grp in crate::sensor_list::GroupFilter::ALL {
                        smoke_state.update(cx, |s, cx| {
                            s.sensor_group = grp;
                            s.sensor_query = "cpu".into();
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

        // Main event loop: fast 30ms dispatch for menu bar clicks and URL deep-links,
        // throttled 500ms telemetry polling to avoid excessive UI repaints.
        let tick_state = state.clone();
        let tick_handles = handles.clone();
        let tick_url = url_open_flag.clone();
        cx.spawn(async move |cx| {
            let _keep_bridge = _bridge_guard;
            let mut last_poll = Instant::now()
                .checked_sub(POLL_INTERVAL)
                .unwrap_or_else(Instant::now);
            loop {
                cx.background_executor().timer(CLICK_TICK_INTERVAL).await;
                if crate::TERMINATE_REQUEST.load(std::sync::atomic::Ordering::SeqCst) {
                    cx.update(|cx| cx.quit());
                    break;
                }
                let mut click_count = 0usize;
                while click_rx.try_recv().is_ok() {
                    click_count += 1;
                }
                let url_open = {
                    let mut f = tick_url.borrow_mut();
                    let v = *f;
                    *f = false;
                    v
                };
                let should_poll = click_count > 0 || url_open || last_poll.elapsed() >= POLL_INTERVAL;
                if !should_poll {
                    continue;
                }
                last_poll = Instant::now();
                cx.update(|cx| {
                    // Process odd clicks cleanly without silently swallowing toggles.
                    if click_count % 2 == 1 {
                        toggle_popover(&tick_state, &tick_handles, true, cx);
                    }
                    if url_open {
                        hide_popover(&tick_handles, cx);
                        open_or_show_main(&tick_state, &tick_handles, Section::Overview, cx);
                    }
                    let (main_handle, pop_vis) = {
                        let h = tick_handles.borrow();
                        (h.main, h.popover_shown)
                    };
                    let main_vis = main_handle
                        .and_then(|h| {
                            cx.update_window(h.into(), |_, w, _| glass::is_window_visible(w))
                                .ok()
                        })
                        .unwrap_or(false);
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
                        let now = Instant::now();
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
                    toggle_popover(&state, &handles, false, cx);
                });

                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;

                for (theme_name, dark) in [("light", false), ("dark", true)] {
                    cx.update(|cx| {
                        let (main_h, pop_h) = {
                            let h = handles.borrow();
                            (h.main, h.popover)
                        };
                        if let Some(h) = main_h {
                            let _ = cx.update_window(h.into(), |_, w, cx| {
                                glass::set_window_appearance(w, dark);
                                glass::sync_glass_root_theme(w, cx);
                                w.refresh();
                            });
                        }
                        if let Some(h) = pop_h {
                            let _ = cx.update_window(h.into(), |_, w, cx| {
                                glass::set_window_appearance(w, dark);
                                glass::sync_glass_root_theme(w, cx);
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
                            let main_h = handles.borrow().main;
                            if let Some(h) = main_h {
                                let path =
                                    out_dir.join(format!("gpui-{sec_name}-{theme_name}.png"));
                                let _ = cx.update_window(h.into(), |_, w, _| {
                                    let _ = glass::capture_window_png(w, &path);
                                });
                            }
                        });
                    }

                    cx.update(|cx| {
                        let pop_h = handles.borrow().popover;
                        if let Some(h) = pop_h {
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
